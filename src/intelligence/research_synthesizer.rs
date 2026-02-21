// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — intelligence/research_synthesizer.rs
// Claude-powered synthesis of raw research data into actionable
// intelligence for writing genuinely personalized outreach emails.
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{Company, Contact, Exhibition, Participation};
use crate::intelligence::deep_researcher::{NewsArticle, PreviousExhibition};

// ── Public types ──────────────────────────────────────────────────────────

/// The actionable intelligence Claude synthesises from raw research.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSynthesis {
    /// 2-3 sentences: what they do, size, recent developments
    pub company_overview: String,
    /// 2-3 sentences: why they exhibit, their goals, experience level
    pub exhibition_strategy: String,
    /// Specific problems Track Exhibits can solve for this company
    pub pain_points: Vec<String>,
    /// Concrete hooks to reference in the email (news, specifics, our experience)
    pub personalization_hooks: Vec<String>,
    /// One sentence: best angle for THIS contact at THIS company
    pub email_angle: String,
}

// ── Claude wire types ─────────────────────────────────────────────────────

#[derive(Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    temperature: f32,
    system: String,
    messages: Vec<ClaudeMessage>,
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

// ── ResearchSynthesizer ───────────────────────────────────────────────────

pub struct ResearchSynthesizer {
    api_key: String,
    model: String,
    client: Client,
}

impl ResearchSynthesizer {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: Client::new(),
        }
    }

    pub async fn synthesize(
        &self,
        company: &Company,
        contact: &Contact,
        exhibition: &Exhibition,
        participation: &Participation,
        website_summary: &str,
        news: &[NewsArticle],
        prev_exhibitions: &[PreviousExhibition],
    ) -> Result<ResearchSynthesis> {
        let prompt = self.build_prompt(
            company,
            contact,
            exhibition,
            participation,
            website_summary,
            news,
            prev_exhibitions,
        );

        let request = ClaudeRequest {
            model: self.model.clone(),
            max_tokens: 1500,
            temperature: 0.3,  // Factual, not creative
            system: SYSTEM_PROMPT.to_string(),
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
            return Err(anyhow::anyhow!(
                "Claude synthesis error {}: {}",
                status,
                &text[..text.len().min(300)]
            ));
        }

        let claude: ClaudeResponse = response.json().await?;
        let raw = claude.content.into_iter().next().map(|c| c.text).unwrap_or_default();

        debug!("Synthesis raw response ({} chars)", raw.len());
        self.parse_synthesis(&raw)
    }

    fn build_prompt(
        &self,
        company: &Company,
        contact: &Contact,
        exhibition: &Exhibition,
        participation: &Participation,
        website_summary: &str,
        news: &[NewsArticle],
        prev_exhibitions: &[PreviousExhibition],
    ) -> String {
        let news_block = if news.is_empty() {
            "No recent news found.".to_string()
        } else {
            news.iter()
                .map(|a| format!(
                    "• [{}] {} — {} ({})",
                    a.source,
                    a.title,
                    a.summary.chars().take(200).collect::<String>(),
                    a.published_date.as_deref().unwrap_or("recent")
                ))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prev_ex_block = if prev_exhibitions.is_empty() {
            "No previous exhibition history found.".to_string()
        } else {
            prev_exhibitions.iter()
                .map(|e| format!(
                    "• {} ({}) — {}",
                    e.event_name,
                    e.date.map(|d| d.format("%Y").to_string()).unwrap_or_else(|| "?".to_string()),
                    e.location
                ))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let website_block = if website_summary.is_empty() {
            "Website content not available.".to_string()
        } else {
            website_summary.chars().take(2000).collect::<String>()
        };

        format!(
            r#"Research data for B2B lead generation synthesis:

COMPANY:
  Name:     {company_name}
  Industry: {industry}
  Website:  {website}
  Size:     {size}
  Location: {location}

COMPANY WEBSITE INTELLIGENCE:
{website_block}

RECENT NEWS (past 6 months):
{news_block}

EXHIBITION CONTEXT:
  Exhibiting at: {exhibition_name}
  Date:          {exhibition_date}
  Location:      {exhibition_location}
  Booth:         {booth}

PREVIOUS EXHIBITIONS:
{prev_ex_block}

CONTACT:
  Name:  {contact_name}
  Title: {job_title}

Synthesize this research into actionable sales intelligence.
Output ONLY valid JSON (no markdown, no code fences):

{{
  "company_overview": "2-3 sentences: what they make/do, company scale, any recent developments (funding, launches, expansions)",
  "exhibition_strategy": "2-3 sentences: why this company exhibits at trade shows, their likely goals, first-timer or repeat exhibitor?",
  "pain_points": [
    "Specific pain point Track Exhibits can solve directly",
    "Another concrete pain point",
    "Third pain point if applicable"
  ],
  "personalization_hooks": [
    "Specific fact, news item, or detail to reference in opening line",
    "Second hook — different angle",
    "Third hook — Track Exhibits relevant experience for this sector"
  ],
  "email_angle": "One sentence: the single strongest approach for this contact given their role and company situation"
}}

Rules:
- Use ONLY information from the research above, not assumptions
- If research is limited, say so honestly (affects quality score)
- Pain points must be solvable by exhibition booth fabrication
- Personalization hooks must be specific, not generic
- email_angle must reference the contact's specific title"#,
            company_name = company.name,
            industry = company.industry.as_deref().unwrap_or("Unknown"),
            website = company.website.as_deref().unwrap_or("unknown"),
            size = company.size.as_deref().unwrap_or("Unknown"),
            location = company.location.as_deref().unwrap_or("Unknown"),
            exhibition_name = exhibition.name,
            exhibition_date = exhibition.start_date
                .map(|d| d.format("%B %Y").to_string())
                .unwrap_or_else(|| "upcoming".to_string()),
            exhibition_location = exhibition.location.as_deref().unwrap_or("TBD"),
            booth = participation.booth_number.as_deref().unwrap_or("TBA"),
            contact_name = contact.full_name,
            job_title = contact.job_title.as_deref().unwrap_or("Marketing"),
        )
    }

    fn parse_synthesis(&self, raw: &str) -> Result<ResearchSynthesis> {
        // Strip markdown code fences if Claude wrapped it
        let cleaned = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        // Find JSON object boundaries
        let json_start = cleaned.find('{').unwrap_or(0);
        let json_end = cleaned.rfind('}').map(|i| i + 1).unwrap_or(cleaned.len());
        let json_str = &cleaned[json_start..json_end];

        #[derive(Deserialize)]
        struct RawSynthesis {
            company_overview: Option<String>,
            exhibition_strategy: Option<String>,
            pain_points: Option<Vec<String>>,
            personalization_hooks: Option<Vec<String>>,
            email_angle: Option<String>,
        }

        let parsed: RawSynthesis = serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse synthesis JSON: {e}\nRaw: {}", &raw[..raw.len().min(500)]))?;

        Ok(ResearchSynthesis {
            company_overview: parsed.company_overview.unwrap_or_default(),
            exhibition_strategy: parsed.exhibition_strategy.unwrap_or_default(),
            pain_points: parsed.pain_points.unwrap_or_default(),
            personalization_hooks: parsed.personalization_hooks.unwrap_or_default(),
            email_angle: parsed.email_angle.unwrap_or_default(),
        })
    }
}

// ── System prompt ─────────────────────────────────────────────────────────

const SYSTEM_PROMPT: &str = r#"You are a B2B research analyst for Track Exhibits Pvt LTD, an exhibition booth fabrication company.

Track Exhibits capabilities:
- Custom exhibition booth design with free 3D visualization concept in 48 hours
- Complete fabrication and on-site delivery/setup/dismantling
- Regions: India, UAE, Middle East, Europe, Asia Pacific
- Sectors served: Tech, Medical, Pharma, Auto, Manufacturing, Consumer Goods
- Typical turnaround: 3-6 weeks from sign-off

Your role: Synthesize raw research into actionable intelligence that enables a sales rep to write an email that feels genuinely researched — not generic AI spam.

Quality principle: If research data is sparse, say so concisely. A short, honest synthesis is better than a padded generic one. The quality score will reflect data richness.

Output ONLY valid JSON. No markdown, no preamble, no code fences."#;
