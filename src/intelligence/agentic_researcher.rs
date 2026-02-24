// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — intelligence/agentic_researcher.rs
// Claude-driven research agent using Anthropic Tool Use.
// Claude decides what to research by calling tools (scrape, news, exhibitions)
// and produces a structured ResearchReport when satisfied.
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, info, warn};

use crate::{Company, Contact, Exhibition, Participation, ResearchConfig};
use crate::database::Database;
use crate::intelligence::deep_researcher::{NewsArticle, PreviousExhibition, ResearchReport};
use crate::python_bridge::crawl4ai_bridge;

// ── Claude API types (with tool use support) ──────────────────────────────

#[derive(Debug, Serialize)]
struct ToolUseRequest {
    model: String,
    max_tokens: u32,
    system: String,
    tools: Vec<ToolDef>,
    messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
struct ToolDef {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolUseResponse {
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

// ── AgenticResearcher ─────────────────────────────────────────────────────

pub struct AgenticResearcher {
    config: ResearchConfig,
    api_key: String,
    model: String,
    http: Client,
}

impl AgenticResearcher {
    pub fn new(config: ResearchConfig, api_key: String, model: String) -> Self {
        Self {
            config,
            api_key,
            model,
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("HTTP client build failed"),
        }
    }

    /// Run the agentic research loop for one contact.
    /// Claude decides what tools to call and produces a structured report.
    pub async fn research(
        &self,
        contact: &Contact,
        company: &Company,
        participation: &Participation,
        exhibition: &Exhibition,
        db: &Database,
    ) -> Result<Option<ResearchReport>> {
        info!("🤖  Agentic research: {} at {}", contact.full_name, company.name);

        let system_prompt = self.build_system_prompt(contact, company, participation, exhibition);
        let tools = self.build_tool_definitions();

        let initial_message = format!(
            r#"Research this lead for Track Exhibits (exhibition booth services company).

Contact: {} ({})
Company: {} — {}
Industry: {}
Exhibition: {} ({})

Your goal: Gather enough intelligence to draft a highly personalized cold email.
Use the tools to learn about the company, their exhibition strategy, and any relevant news.
When you have enough information, output your final analysis as JSON."#,
            contact.full_name,
            contact.job_title.as_deref().unwrap_or("Unknown title"),
            company.name,
            company.website.as_deref().unwrap_or("no website"),
            company.industry.as_deref().unwrap_or("Unknown"),
            exhibition.name,
            exhibition.location.as_deref().unwrap_or(""),
        );

        let mut messages = vec![Message {
            role: "user".into(),
            content: json!(initial_message),
        }];

        let max_rounds = self.config.max_tool_rounds;

        // ── Tool use loop ────────────────────────────────────────────────
        for round in 0..max_rounds {
            debug!("  Tool-use round {}/{}", round + 1, max_rounds);

            let request = ToolUseRequest {
                model: self.model.clone(),
                max_tokens: 2048,
                system: system_prompt.clone(),
                tools: tools.clone(),
                messages: messages.clone(),
            };

            let response = self.call_claude_tools(request).await?;

            // Collect text and tool_use blocks from the response
            let mut has_tool_use = false;
            let mut final_text = String::new();
            let mut assistant_content: Vec<serde_json::Value> = Vec::new();

            for block in &response.content {
                match block {
                    ContentBlock::Text { text } => {
                        final_text.push_str(text);
                        assistant_content.push(json!({
                            "type": "text",
                            "text": text,
                        }));
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        has_tool_use = true;
                        assistant_content.push(json!({
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": input,
                        }));
                    }
                }
            }

            // Add assistant response to messages
            messages.push(Message {
                role: "assistant".into(),
                content: json!(assistant_content),
            });

            // If no tool calls, Claude is done
            if !has_tool_use {
                return self.parse_final_report(
                    &final_text, contact, company, participation,
                );
            }

            // Execute each tool call and add results
            let mut tool_results: Vec<serde_json::Value> = Vec::new();

            for block in &response.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    let result = self.execute_tool(name, input, company, db).await;
                    debug!("  Tool {} → {} chars", name, result.len());
                    tool_results.push(json!({
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": result,
                    }));
                }
            }

            // Add tool results as a user message
            messages.push(Message {
                role: "user".into(),
                content: json!(tool_results),
            });
        }

        // Max rounds exceeded — try to parse whatever we have
        warn!("  ⚠️  Agentic research hit max rounds for {}", company.name);

        // Ask Claude for final output without tools
        messages.push(Message {
            role: "user".into(),
            content: json!("You've used all your research rounds. Please output your final analysis now as JSON based on what you've gathered so far."),
        });

        let final_request = ToolUseRequest {
            model: self.model.clone(),
            max_tokens: 2048,
            system: system_prompt,
            tools: vec![], // No more tools
            messages,
        };

        let response = self.call_claude_tools(final_request).await?;
        let text: String = response.content.iter()
            .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
            .collect();

        self.parse_final_report(&text, contact, company, participation)
    }

    // ── Tool definitions ──────────────────────────────────────────────────

    fn build_tool_definitions(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "scrape_page".into(),
                description: "Fetch and read the content of a web page. Returns the page content as clean markdown. Use this to learn about the company from their website (about page, products page, news page, etc).".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The full URL to scrape (must start with http:// or https://)"
                        }
                    },
                    "required": ["url"]
                }),
            },
            ToolDef {
                name: "search_news".into(),
                description: "Search for recent news articles about a company or topic. Returns a list of news articles with titles, sources, and summaries. Use this to find recent press coverage, funding announcements, exhibition participation, partnerships, or any noteworthy company developments.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query (e.g. 'Acme Corp exhibition' or 'Acme Corp news 2024')"
                        }
                    },
                    "required": ["query"]
                }),
            },
            ToolDef {
                name: "check_exhibitions".into(),
                description: "Look up a company's past exhibition participation history from the internal database and 10times.com. Returns a list of exhibitions the company has previously participated in, including event names, dates, and locations. This is fully automatic — no manual input needed.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "company_name": {
                            "type": "string",
                            "description": "Name of the company to look up"
                        }
                    },
                    "required": ["company_name"]
                }),
            },
        ]
    }

    // ── Tool execution ────────────────────────────────────────────────────

    async fn execute_tool(
        &self,
        name: &str,
        input: &serde_json::Value,
        company: &Company,
        db: &Database,
    ) -> String {
        match name {
            "scrape_page" => {
                let url = input.get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if url.is_empty() {
                    return "Error: no URL provided".into();
                }

                // Try Crawl4AI first, then reqwest fallback
                if self.config.crawl4ai_enabled {
                    match crawl4ai_bridge::scrape_pages(vec![url.clone()], 3000).await {
                        Ok(results) => {
                            if let Some(page) = results.first() {
                                if page.success && !page.content.is_empty() {
                                    return page.content.clone();
                                }
                            }
                        }
                        Err(e) => {
                            debug!("  Crawl4AI failed for {}: {}", url, e);
                        }
                    }
                }

                // Reqwest fallback
                match self.http.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        match resp.text().await {
                            Ok(html) => {
                                let text = html_to_text_limited(&html, 3000);
                                if text.is_empty() {
                                    "Page returned no readable content.".into()
                                } else {
                                    text
                                }
                            }
                            Err(_) => "Failed to read page content.".into(),
                        }
                    }
                    Ok(resp) => format!("HTTP {} — page not accessible.", resp.status()),
                    Err(e) => format!("Failed to fetch URL: {}", e),
                }
            }

            "search_news" => {
                let query = input.get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if query.is_empty() {
                    return "Error: no search query provided".into();
                }

                // Try Crawl4AI news first
                if self.config.crawl4ai_enabled {
                    match crawl4ai_bridge::search_news_via_crawl4ai(&query, 5).await {
                        Ok(results) if !results.is_empty() => {
                            let formatted: Vec<String> = results.iter().map(|r| {
                                format!("• {} ({})\n  {}\n  URL: {}",
                                    r.title, r.source,
                                    if r.full_content.is_empty() { &r.snippet } else { &r.full_content },
                                    r.url
                                )
                            }).collect();
                            return formatted.join("\n\n");
                        }
                        _ => {}
                    }
                }

                // Fallback to SerpAPI/Google
                if let Some(key) = &self.config.sources.serp_api_key {
                    match self.search_serp(&query, key).await {
                        Ok(text) => return text,
                        Err(e) => debug!("  SerpAPI failed: {}", e),
                    }
                }

                "No news results found for this query.".into()
            }

            "check_exhibitions" => {
                let company_name = input.get("company_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&company.name);

                // Query internal DB for past participations
                match db.get_past_participations(&company.id).await {
                    Ok(exhibitions) if !exhibitions.is_empty() => {
                        let formatted: Vec<String> = exhibitions.iter().map(|(name, date, location)| {
                            let date_str = date
                                .map(|d| d.format("%Y-%m-%d").to_string())
                                .unwrap_or_else(|| "Unknown date".into());
                            format!("• {} — {} ({})", name, date_str, location)
                        }).collect();
                        format!(
                            "Found {} past exhibitions for {} (from internal database):\n{}",
                            exhibitions.len(), company_name, formatted.join("\n")
                        )
                    }
                    Ok(_) => format!("No past exhibition records found for {} in internal database.", company_name),
                    Err(e) => format!("Database lookup failed: {}", e),
                }
            }

            _ => format!("Unknown tool: {}", name),
        }
    }

    // ── SerpAPI fallback for news ─────────────────────────────────────────

    async fn search_serp(&self, query: &str, api_key: &str) -> Result<String> {
        let resp: serde_json::Value = self.http
            .get("https://serpapi.com/search.json")
            .query(&[
                ("api_key", api_key),
                ("engine", "google"),
                ("q", query),
                ("tbm", "nws"),
                ("num", "5"),
            ])
            .send()
            .await?
            .json()
            .await?;

        let articles = resp.get("news_results")
            .or_else(|| resp.get("organic_results"))
            .and_then(|v| v.as_array());

        match articles {
            Some(items) if !items.is_empty() => {
                let formatted: Vec<String> = items.iter().take(5).map(|item| {
                    format!("• {} ({})\n  {}",
                        item.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled"),
                        item.get("source").and_then(|v| v.as_str())
                            .or_else(|| item.get("displayed_link").and_then(|v| v.as_str()))
                            .unwrap_or(""),
                        item.get("snippet").and_then(|v| v.as_str()).unwrap_or(""),
                    )
                }).collect();
                Ok(formatted.join("\n\n"))
            }
            _ => Ok("No news results found.".into()),
        }
    }

    // ── Claude API call ───────────────────────────────────────────────────

    async fn call_claude_tools(&self, request: ToolUseRequest) -> Result<ToolUseResponse> {
        let resp = self.http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Claude API error {}: {}",
                status,
                body.chars().take(500).collect::<String>()
            ));
        }

        let response: ToolUseResponse = resp.json().await?;
        Ok(response)
    }

    // ── System prompt ─────────────────────────────────────────────────────

    fn build_system_prompt(
        &self,
        contact: &Contact,
        company: &Company,
        _participation: &Participation,
        exhibition: &Exhibition,
    ) -> String {
        format!(
            r#"You are Scott, a high-stakes research agent for Track Exhibits, based in Dubai (GST/UTC+4). Track Exhibits provides premium exhibition booth services (Design, Fabrication, Installation) to a global clientele.

Your job: Research a lead thoroughly so we can draft a personalized, high-conversion cold email.

LEAD:
- Contact: {} ({})
- Company: {} | Industry: {} | Location: {}
- Exhibition: {} in {}

AVAILABLE TOOLS:
1. scrape_page — Read any web page (company website, LinkedIn, articles)
2. search_news — Search for recent news about the company
3. check_exhibitions — Look up their past exhibition history (automatic, uses internal DB + 10times.com)

RESEARCH STRATEGY:
1. Start by scraping their website (home, about, products/services pages)
2. Search for recent news (press releases, funding, exhibitions, partnerships)
3. Check their exhibition history to understand their trade show strategy
4. If you find interesting leads in initial research, dig deeper with follow-up scrapes

WHEN DONE, output your analysis as JSON with exactly these fields:
{{
    "company_overview": "2-3 sentence overview of the company",
    "exhibition_strategy": "Their trade show / exhibition approach",
    "pain_points": ["specific pain point 1", "specific pain point 2"],
    "personalization_hooks": ["hook for email opening 1", "hook 2"],
    "email_angle": "The recommended angle for our cold email"
}}

Output ONLY the JSON when you're done researching. No markdown fences, no preamble."#,
            contact.full_name,
            contact.job_title.as_deref().unwrap_or("Unknown"),
            company.name,
            company.industry.as_deref().unwrap_or("Unknown"),
            company.location.as_deref().unwrap_or(""),
            exhibition.name,
            exhibition.location.as_deref().unwrap_or(""),
        )
    }

    // ── Parse final report ────────────────────────────────────────────────

    fn parse_final_report(
        &self,
        text: &str,
        contact: &Contact,
        company: &Company,
        participation: &Participation,
    ) -> Result<Option<ResearchReport>> {
        // Try to extract JSON from the text
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
        struct AgenticSynthesis {
            company_overview: Option<String>,
            exhibition_strategy: Option<String>,
            pain_points: Option<Vec<String>>,
            personalization_hooks: Option<Vec<String>>,
            email_angle: Option<String>,
        }

        match serde_json::from_str::<AgenticSynthesis>(json_str) {
            Ok(synthesis) => {
                let overview = synthesis.company_overview.unwrap_or_default();
                if overview.len() < 20 {
                    warn!("  Agentic research produced insufficient overview for {}", company.name);
                    return Ok(None);
                }

                Ok(Some(ResearchReport {
                    id: uuid::Uuid::new_v4().to_string(),
                    contact_id: contact.id.clone(),
                    company_id: company.id.clone(),
                    participation_id: participation.id.clone(),
                    researched_at: chrono::Utc::now(),
                    company_website_summary: String::new(), // Gathered by tools
                    recent_news: vec![],                    // Gathered by tools
                    previous_exhibitions: vec![],           // Gathered by tools
                    company_overview: overview,
                    exhibition_strategy: synthesis.exhibition_strategy.unwrap_or_default(),
                    pain_points: synthesis.pain_points.unwrap_or_default(),
                    personalization_hooks: synthesis.personalization_hooks.unwrap_or_default(),
                    email_angle: synthesis.email_angle.unwrap_or_default(),
                    research_quality_score: 0.7, // Agentic research is generally good quality
                    sources_used: vec!["agentic_tool_use".into()],
                }))
            }
            Err(e) => {
                warn!("  Failed to parse agentic research output: {}", e);
                warn!("  Raw text: {}", text.chars().take(500).collect::<String>());
                Ok(None)
            }
        }
    }
}

// ── Utility ───────────────────────────────────────────────────────────────

fn html_to_text_limited(html: &str, max_chars: usize) -> String {
    let mut result = String::with_capacity(max_chars);
    let mut in_tag = false;
    let mut last_was_space = true;

    for ch in html.chars() {
        if result.len() >= max_chars {
            break;
        }
        match ch {
            '<' => { in_tag = true; }
            '>' => {
                in_tag = false;
                if !last_was_space {
                    result.push(' ');
                    last_was_space = true;
                }
            }
            _ if in_tag => {}
            c if c.is_whitespace() => {
                if !last_was_space {
                    result.push(' ');
                    last_was_space = true;
                }
            }
            c => {
                result.push(c);
                last_was_space = false;
            }
        }
    }

    result.trim().to_string()
}
