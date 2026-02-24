// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — intelligence/email_personalizer.rs
// Claude-powered email personalization
// Two modes: basic (company research) and researched (ResearchReport)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{Company, CompanyConfig, Contact, Participation};
use crate::database::Database;
use crate::intelligence::deep_researcher::ResearchReport;

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

    // ── Research-enhanced drafting (primary path) ─────────────────────────

    /// Draft a highly personalized email using a ResearchReport.
    /// This is the primary path — used when deep research is available.
    pub async fn draft_researched_email(
        &self,
        contact: &Contact,
        company: &Company,
        participation: Option<&Participation>,
        exhibition_name: Option<&str>,
        research: &ResearchReport,
    ) -> Result<EmailDraft> {
        let exhibition_info = if let Some(part) = participation {
            let booth = part.booth_number.as_deref().unwrap_or("TBA");
            let ex_name = exhibition_name.unwrap_or("the upcoming exhibition");
            format!("Exhibiting at {} (booth: {}).", ex_name, booth)
        } else {
            "Participates in trade shows.".to_string()
        };

        let pain_points_block = if research.pain_points.is_empty() {
            "Not specifically identified.".to_string()
        } else {
            research.pain_points.iter()
                .enumerate()
                .map(|(i, p)| format!("{}. {}", i + 1, p))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let hooks_block = if research.personalization_hooks.is_empty() {
            "Use their exhibition participation and industry.".to_string()
        } else {
            research.personalization_hooks.iter()
                .enumerate()
                .map(|(i, h)| format!("{}. {}", i + 1, h))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prev_ex_block = if research.previous_exhibitions.is_empty() {
            "No previous exhibition history found.".to_string()
        } else {
            research.previous_exhibitions.iter()
                .take(3)
                .map(|e| format!(
                    "• {} ({})",
                    e.event_name,
                    e.date.map(|d| d.format("%Y").to_string()).unwrap_or_else(|| "?".to_string())
                ))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prompt = format!(
            r#"You are Scott from Track Exhibits. Write this email as Scott — a real person, not a company.

CONTACT:
- Name: {name}
- Title: {title}
- Company: {company}
- Industry: {industry}
- Location: {location}
- {exhibition_info}

COMPANY INTELLIGENCE:
Overview: {overview}
Exhibition strategy: {strategy}

Pain points we can solve:
{pain_points}

Best personalization hooks to use in opening:
{hooks}

Previous exhibitions:
{prev_exhibs}

RECOMMENDED ANGLE: {angle}

WHAT WE DO AT TRACK EXHIBITS:
- Booth Design: Creative concepts that capture your brand's essence
- Fabrication: Precision building with quality materials and care
- Installation: Seamless setup ensuring your booth shines on event day
- Free 3D concept design within 48 hours of inquiry
- Regions: India, UAE, Middle East, Europe, Asia Pacific

SCOTT'S WRITING STYLE:
1. Write like a real person — conversational, warm, not corporate
2. Open with a genuine observation about their company (never "I hope this email finds you well")
3. Reference ONE concrete detail from the research — show you actually looked them up
4. Keep it natural — the way a real sales guy would write, not a marketing team
5. Vary sentence length. Short ones hit harder. Mix it up.
6. Mention the free 3D concept offer casually (not as a sales pitch)
7. Soft CTA: quick call or "happy to share some ideas"
8. 120-180 words. Some emails can be shorter. Not every email needs to be the same length.
9. NEVER sound robotic, use buzzwords, or mention AI/automation
10. End naturally — "Cheers," or "Best," or "Talk soon," (vary it)

SIGNATURE (always include):
Scott
Track Exhibits
+91 98765 43210

Return ONLY:
SUBJECT: [subject line]
BODY:
[email body including signature]"#,
            name = contact.full_name,
            title = contact.job_title.as_deref().unwrap_or("Marketing"),
            company = company.name,
            industry = company.industry.as_deref().unwrap_or("their sector"),
            location = company.location.as_deref().unwrap_or(""),
            overview = research.company_overview,
            strategy = research.exhibition_strategy,
            pain_points = pain_points_block,
            hooks = hooks_block,
            prev_exhibs = prev_ex_block,
            angle = research.email_angle,
        );

        let request = ClaudeRequest {
            model: self.model.clone(),
            max_tokens: 600,
            system: EMAIL_SYSTEM_PROMPT.to_string(),
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: prompt,
            }],
        };

        let text = self.call_claude(request).await?;
        self.parse_email_draft(&text)
    }

    // ── Fallback basic drafting ────────────────────────────────────────────

    pub async fn personalize_initial_email(
        &self,
        contact: &Contact,
        company: &Company,
        participation: Option<&Participation>,
        exhibition_name: Option<&str>,
        _db: &Database,
    ) -> Result<EmailDraft> {
        let research = company.research_summary.as_deref().unwrap_or("No research available");
        let exhibition_info = if let Some(part) = participation {
            let booth = part.booth_number.as_deref().unwrap_or("TBD");
            let ex_name = exhibition_name.unwrap_or("the upcoming exhibition");
            format!("Exhibiting at {} (Booth #{}).", ex_name, booth)
        } else {
            "Participates in trade shows.".to_string()
        };

        let prompt = format!(
            r#"You are Scott from Track Exhibits. Write this email as Scott — a real person.

TARGET:
- Name: {}
- Title: {}
- Company: {}
- Industry: {}
- Location: {}
- Exhibition context: {}

COMPANY RESEARCH:
{}

WHAT WE DO AT TRACK EXHIBITS:
- Booth Design: Creative concepts that capture your brand's essence
- Fabrication: Precision building with quality materials and care
- Installation: Seamless setup ensuring your booth shines on event day
- Free 3D concept design within 48 hours

Write a 120-180 word email as Scott. Conversational, warm, like a real person wrote it.
No generic openers. No buzzwords. No mention of AI.
End with: Scott / Track Exhibits / +91 98765 43210

Return ONLY:
SUBJECT: [subject line]
BODY:
[email body including signature]"#,
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
            system: EMAIL_SYSTEM_PROMPT.to_string(),
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: prompt,
            }],
        };

        let text = self.call_claude(request).await?;
        self.parse_email_draft(&text)
    }

    // ── Shared helpers ────────────────────────────────────────────────────

    async fn call_claude(&self, request: ClaudeRequest) -> Result<String> {
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
                "Claude API error {}: {}",
                status,
                &text[..200.min(text.len())]
            ));
        }

        let claude_response: ClaudeResponse = response.json().await?;
        Ok(claude_response.content
            .into_iter()
            .next()
            .map(|c| c.text)
            .unwrap_or_default())
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
            subject = lines.first().copied().unwrap_or("Exhibition Booth Design").to_string();
        }

        let body = body_lines.join("\n").trim().to_string();

        if body.is_empty() {
            return Err(anyhow::anyhow!("Could not parse email body from Claude response"));
        }

        Ok(EmailDraft { subject, body })
    }
}

const EMAIL_SYSTEM_PROMPT: &str = "You are Scott, an exhibition expert based in Dubai at Track Exhibits Pvt LTD. You write natural, conversational cold emails to global clients — the kind a friendly, competent salesperson would actually send. Your tone is warm but not pushy, knowledgeable but not preachy. You write like a human: sometimes a sentence fragment, sometimes a dash instead of a comma, occasionally starting with 'So' or 'Hey'. You NEVER sound like a template, a marketing team, or a robot. You never mention AI or automation. Every email feels like Scott genuinely sat down and wrote it after looking up the company. Your services: Booth Design, Fabrication, and Installation for exhibitions worldwide.";
