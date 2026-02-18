// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — intelligence/email_personalizer.rs
// Claude-powered email personalization
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{Company, CompanyConfig, Contact, Participation};
use crate::database::Database;

pub struct EmailPersonalizer {
    api_key: String,
    model: String,
    client: Client,
    company_config: CompanyConfig,
}

#[derive(Debug, Clone)]
pub struct EmailDraft {
    pub subject: String,
    pub body: String,
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

impl EmailPersonalizer {
    pub fn new(api_key: String, model: String, company_config: CompanyConfig) -> Self {
        Self { api_key, model, client: Client::new(), company_config }
    }

    pub async fn personalize_initial_email(
        &self,
        contact: &Contact,
        company: &Company,
        participation: Option<&Participation>,
        exhibition_name: Option<&str>,
        db: &Database,
    ) -> Result<EmailDraft> {
        let research = company.research_summary.as_deref().unwrap_or("No research available");
        let exhibition_info = if let Some(part) = participation {
            let booth = part.booth_number.as_deref().unwrap_or("TBD");
            let ex_name = exhibition_name.unwrap_or("the upcoming exhibition");
            format!("They are exhibiting at {} (Booth #{}).", ex_name, booth)
        } else {
            "They participate in trade shows in their sector.".to_string()
        };

        let prompt = format!(
            r#"Write a cold outreach email for Track Exhibits Pvt LTD.

TARGET:
- Name: {}
- Title: {}
- Company: {}
- Industry: {}
- Location: {}
- Exhibition context: {}

COMPANY RESEARCH:
{}

TRACK EXHIBITS INFO:
- Service: Exhibition booth fabrication with 3D design visualization and delivery
- Regions: Middle East, Europe, Asia Pacific, UK
- USP: Complete service (3D mockups → fabrication → on-site setup)
- One point of contact from concept to delivery

Write a professional B2B email that:
1. References their specific exhibition participation
2. Shows genuine research into their company
3. Mentions Track Exhibits' relevant experience for their sector
4. Clear value prop (3D design + fabrication + delivery)
5. Soft CTA (brief call to discuss requirements)
6. 150-200 words MAX
7. Professional B2B tone
8. NO mention of AI or automation
9. NO desperate or pushy language
10. NO generic phrases like "I hope this email finds you well"

Return ONLY in this format:
SUBJECT: [subject line]
BODY:
[email body]"#,
            contact.full_name,
            contact.job_title.as_deref().unwrap_or("Marketing"),
            company.name,
            company.industry.as_deref().unwrap_or(""),
            company.location.as_deref().unwrap_or(""),
            exhibition_info,
            research,
        );

        let request = ClaudeRequest {
            model: self.model.clone(),
            max_tokens: 600,
            system: "You write concise, professional B2B cold emails for Track Exhibits Pvt LTD. Your emails feel genuinely researched, never generic. You never mention AI, automation, or use desperate language. Every email is unique and personalized.".to_string(),
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
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Claude API error {}: {}", status, &text[..200.min(text.len())]));
        }

        let claude_response: ClaudeResponse = response.json().await?;
        let text = claude_response.content
            .into_iter()
            .next()
            .map(|c| c.text)
            .unwrap_or_default();

        self.parse_email_draft(&text)
    }

    fn parse_email_draft(&self, text: &str) -> Result<EmailDraft> {
        let lines: Vec<&str> = text.lines().collect();
        let mut subject = String::new();
        let mut body_lines = Vec::new();
        let mut in_body = false;

        for line in &lines {
            if line.starts_with("SUBJECT:") {
                subject = line.trim_start_matches("SUBJECT:").trim().to_string();
            } else if line.starts_with("BODY:") {
                in_body = true;
            } else if in_body {
                body_lines.push(*line);
            }
        }

        if subject.is_empty() {
            // Try to extract from first line
            subject = lines.first().copied().unwrap_or("Exhibition Booth Design").to_string();
        }

        let body = body_lines.join("\n").trim().to_string();

        if body.is_empty() {
            return Err(anyhow::anyhow!("Could not parse email body from Claude response"));
        }

        Ok(EmailDraft { subject, body })
    }
}
