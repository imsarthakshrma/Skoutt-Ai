// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — intelligence/reply_analyzer.rs
// Claude-powered reply analysis and interest classification
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::InterestLevel;

pub struct ReplyAnalyzer {
    api_key: String,
    model: String,
    client: Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingReply {
    pub email_id: String,
    pub from_email: String,
    pub from_name: Option<String>,
    pub subject: String,
    pub body: String,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyAnalysis {
    pub interest_level: InterestLevel,
    pub sentiment: String,
    pub signals: Vec<String>,
    pub next_step: String,
    pub summary: String,
}

impl ReplyAnalysis {
    pub fn interest_level_str(&self) -> &str {
        match self.interest_level {
            InterestLevel::High => "High",
            InterestLevel::Medium => "Medium",
            InterestLevel::Low => "Low",
            InterestLevel::None => "None",
        }
    }
}

#[derive(Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ClaudeMessage>,
    system: String,
}

#[derive(Serialize, Deserialize)]
struct ClaudeMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[derive(Deserialize)]
struct ClaudeContent {
    text: String,
}

impl ReplyAnalyzer {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: Client::new(),
        }
    }

    /// Analyze a reply email for interest level and next steps
    pub async fn analyze_reply(&self, reply: &IncomingReply, db: &crate::database::Database) -> Result<ReplyAnalysis> {
        // First do a quick keyword-based pre-filter
        let quick_sentiment = self.quick_sentiment_check(&reply.body);

        // If clearly not interested, skip Claude call to save API costs
        if quick_sentiment == "not_interested" && !self.has_ambiguous_signals(&reply.body) {
            return Ok(ReplyAnalysis {
                interest_level: InterestLevel::None,
                sentiment: "not_interested".to_string(),
                signals: vec!["Polite decline detected".to_string()],
                next_step: "Mark as not interested and stop outreach".to_string(),
                summary: "Recipient declined the outreach".to_string(),
            });
        }

        // Use Claude for nuanced analysis
        let prompt = format!(
            r#"Analyze this reply to our exhibition booth fabrication pitch:

REPLY:
{}

Provide analysis in this exact JSON format:
{{
  "interest_level": "High|Medium|Low|None",
  "sentiment": "interested|not_interested|neutral|needs_info",
  "signals": ["signal1", "signal2", "signal3"],
  "next_step": "specific recommended action",
  "summary": "1-2 sentence summary"
}}

Interest level definitions:
- High: Wants to schedule call, asks about pricing/timeline, ready to move forward
- Medium: Asks questions about services, wants more info, open to discussion
- Low: Acknowledges but non-committal, vague interest
- None: Clear decline, not relevant, wrong person, unsubscribe

Signals are specific things they said that indicate interest or lack thereof.
Be precise and actionable in next_step."#,
            reply.body.chars().take(2000).collect::<String>()
        );

        let request = ClaudeRequest {
            model: self.model.clone(),
            max_tokens: 400,
            system: "You are analyzing email replies for a B2B sales team at Track Exhibits Pvt LTD (exhibition booth fabrication). Classify replies accurately to help prioritize follow-up. Return ONLY valid JSON, no other text.".to_string(),
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: prompt,
            }],
        };

        let response = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            // Fall back to keyword analysis
            return Ok(self.keyword_analysis(reply));
        }

        let claude_response: ClaudeResponse = response.json().await?;
        let text = claude_response.content
            .into_iter()
            .next()
            .map(|c| c.text)
            .unwrap_or_default();

        // Parse JSON response
        self.parse_claude_analysis(&text, reply)
    }

    fn parse_claude_analysis(&self, text: &str, reply: &IncomingReply) -> Result<ReplyAnalysis> {
        // Extract JSON from response (Claude might add markdown code blocks)
        let json_str = if let Some(start) = text.find('{') {
            if let Some(end) = text.rfind('}') {
                &text[start..=end]
            } else {
                text
            }
        } else {
            text
        };

        #[derive(Deserialize)]
        struct RawAnalysis {
            interest_level: String,
            sentiment: String,
            signals: Vec<String>,
            next_step: String,
            summary: String,
        }

        let raw: RawAnalysis = serde_json::from_str(json_str)
            .unwrap_or_else(|_| RawAnalysis {
                interest_level: "Low".to_string(),
                sentiment: "neutral".to_string(),
                signals: vec!["Could not parse response".to_string()],
                next_step: "Manual review recommended".to_string(),
                summary: "Analysis failed, manual review needed".to_string(),
            });

        let interest_level = match raw.interest_level.as_str() {
            "High" => InterestLevel::High,
            "Medium" => InterestLevel::Medium,
            "Low" => InterestLevel::Low,
            _ => InterestLevel::None,
        };

        Ok(ReplyAnalysis {
            interest_level,
            sentiment: raw.sentiment,
            signals: raw.signals,
            next_step: raw.next_step,
            summary: raw.summary,
        })
    }

    /// Quick keyword-based sentiment check (saves Claude API calls)
    fn quick_sentiment_check(&self, body: &str) -> &'static str {
        let body_lower = body.to_lowercase();

        let not_interested = [
            "not interested", "no thank you", "no thanks", "not relevant",
            "wrong person", "please remove", "unsubscribe", "do not contact",
            "not looking for", "already have", "not in our budget",
            "not applicable", "not the right fit",
        ];

        let interested = [
            "interested", "would like to", "can we schedule", "let's talk",
            "please send", "tell me more", "what are your prices",
            "how much", "when can you", "sounds good", "yes please",
            "happy to discuss", "would love to",
        ];

        if not_interested.iter().any(|kw| body_lower.contains(kw)) {
            return "not_interested";
        }
        if interested.iter().any(|kw| body_lower.contains(kw)) {
            return "interested";
        }
        "neutral"
    }

    fn has_ambiguous_signals(&self, body: &str) -> bool {
        let body_lower = body.to_lowercase();
        let ambiguous = ["but", "however", "although", "maybe", "perhaps", "possibly", "if"];
        ambiguous.iter().any(|kw| body_lower.contains(kw))
    }

    fn keyword_analysis(&self, reply: &IncomingReply) -> ReplyAnalysis {
        let sentiment = self.quick_sentiment_check(&reply.body);
        let (interest_level, next_step) = match sentiment {
            "interested" => (InterestLevel::Medium, "Follow up with portfolio and pricing".to_string()),
            "not_interested" => (InterestLevel::None, "Mark as not interested and stop outreach".to_string()),
            _ => (InterestLevel::Low, "Send follow-up with more specific value proposition".to_string()),
        };

        ReplyAnalysis {
            interest_level,
            sentiment: sentiment.to_string(),
            signals: vec!["Keyword-based analysis (Claude unavailable)".to_string()],
            next_step,
            summary: "Analyzed via keyword matching".to_string(),
        }
    }
}
