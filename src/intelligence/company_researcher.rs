// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — intelligence/company_researcher.rs
// Deep company research via Claude API
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{database::Database, Company};

pub struct CompanyResearcher {
    api_key: String,
    model: String,
    client: Client,
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

impl CompanyResearcher {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: Client::new(),
        }
    }

    /// Research a company and return a structured summary for email personalization
    pub async fn research_company(&self, company: &Company, db: &Database) -> Result<String> {
        let website_content = if let Some(website) = &company.website {
            // Check if we have cached content
            if let Ok(Some(cached)) = db.get_cached_page(website).await {
                cached
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let prompt = format!(
            r#"Research this company for B2B outreach purposes:

Company: {}
Website: {}
Industry: {}
Location: {}
Website Content: {}

Provide a concise research summary with:
1. What they do (1-2 sentences, specific products/services)
2. Why they likely exhibit at trade shows (market expansion? product launch? brand awareness?)
3. Estimated booth needs (size, likely requirements based on their industry/size)
4. Key pain points Track Exhibits can solve for them
5. Any notable recent news or growth signals

Keep it under 300 words. Be specific and actionable for email personalization.
Focus on what would make a cold email feel genuinely researched, not generic."#,
            company.name,
            company.website.as_deref().unwrap_or("unknown"),
            company.industry.as_deref().unwrap_or("unknown"),
            company.location.as_deref().unwrap_or("unknown"),
            if website_content.is_empty() {
                "Not available".to_string()
            } else {
                website_content.chars().take(2000).collect()
            }
        );

        let request = ClaudeRequest {
            model: self.model.clone(),
            max_tokens: 500,
            system: "You are a B2B researcher for Track Exhibits Pvt LTD, an exhibition booth fabrication company. Your research summaries help write personalized cold emails to companies exhibiting at trade shows. Be concise, specific, and focus on information that makes emails feel genuinely researched.".to_string(),
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
            return Err(anyhow::anyhow!("Claude API error {}: {}", status, &text[..text.len().min(200)]));
        }

        let claude_response: ClaudeResponse = response.json().await?;
        let summary = claude_response.content
            .into_iter()
            .next()
            .map(|c| c.text)
            .unwrap_or_default();

        info!("  Researched: {} ({} chars)", company.name, summary.len());
        Ok(summary)
    }
}
