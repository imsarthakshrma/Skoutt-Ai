// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — scraping/exhibitor_extractor.rs
// Agentic exhibitor extraction using Claude tool-calling.
//
// Claude is given tools to:
//   1. scrape_page  — Crawl4AI headless browser (renders JS, bypasses blocks)
//   2. google_search — Google Custom Search API or Crawl4AI Google fallback
//   3. extract_companies — Parse text/HTML → structured company list
//   4. save_companies — Store extracted companies to DB
//   5. email_organizer — Draft email to exhibition organizer requesting list
//
// Claude autonomously decides the best strategy per exhibition:
//   - Scrape exhibition website for exhibitor/participant pages
//   - Google for "[Exhibition] exhibitor list"
//   - Find organizer contact and queue a requesting email
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

use crate::{
    database::Database, Company, Exhibition, Participation, ResearchConfig,
};
use crate::python_bridge::crawl4ai_bridge;

// ── Claude API types ─────────────────────────────────────────────────────

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
    #[allow(dead_code)]
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

// ── Extracted company from Claude ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExtractedCompany {
    name: String,
    booth: Option<String>,
    website: Option<String>,
    description: Option<String>,
}

// ── ExhibitorExtractor ───────────────────────────────────────────────────

pub struct ExhibitorExtractor {
    config: ResearchConfig,
    api_key: String,
    model: String,
    http: Client,
    db: Database,
}

impl ExhibitorExtractor {
    pub fn new(
        config: ResearchConfig,
        api_key: String,
        model: String,
        db: Database,
    ) -> Self {
        Self {
            config,
            api_key,
            model,
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("HTTP client build failed"),
            db,
        }
    }

    /// Extract exhibitors for a given exhibition using Claude agentic tool-calling.
    /// Claude decides the best strategy to find exhibitor lists.
    pub async fn extract_exhibitors(&self, exhibition: &Exhibition) -> Result<usize> {
        info!(
            "🤖  Agentic exhibitor extraction: {} ({})",
            exhibition.name,
            exhibition.location.as_deref().unwrap_or("unknown location")
        );

        let now = chrono::Utc::now();
        let system_prompt = self.build_system_prompt(exhibition, &now);
        let tools = self.build_tool_definitions();

        let initial_message = format!(
            r#"Find the exhibitor list for this exhibition and extract every company name.

EXHIBITION: {}
SECTOR: {}
REGION: {}
LOCATION: {}
DATE: {}
WEBSITE: {}
KNOWN EXHIBITOR LIST URL: {}

Start by scraping the exhibition website. Look for pages labeled "exhibitors", "participants", "directory", "companies", "stand holders", or similar.

If you find an exhibitor list page, use extract_companies to parse it.
If you can't find it on the website, use google_search to find it.
If still nothing, try to find the organizer's contact info and use email_organizer.

Always call save_companies with whatever companies you find before finishing."#,
            exhibition.name,
            exhibition.sector,
            exhibition.region,
            exhibition.location.as_deref().unwrap_or("Unknown"),
            exhibition
                .start_date
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "Unknown".into()),
            exhibition.website_url.as_deref().unwrap_or("None"),
            exhibition.exhibitor_list_url.as_deref().unwrap_or("None"),
        );

        let mut messages = vec![Message {
            role: "user".into(),
            content: json!(initial_message),
        }];

        // Accumulate extracted companies across tool rounds
        let companies: Arc<Mutex<Vec<ExtractedCompany>>> =
            Arc::new(Mutex::new(Vec::new()));

        let max_rounds = self.config.max_tool_rounds.max(5); // At least 5 rounds

        // ── Tool use loop ────────────────────────────────────────────
        for round in 0..max_rounds {
            debug!("  Exhibitor extraction round {}/{}", round + 1, max_rounds);

            let request = ToolUseRequest {
                model: self.model.clone(),
                max_tokens: 4096,
                system: system_prompt.clone(),
                tools: tools.clone(),
                messages: messages.clone(),
            };

            let response = match self.call_claude(request).await {
                Ok(r) => r,
                Err(e) => {
                    warn!("  Claude API error in round {}: {}", round + 1, e);
                    break;
                }
            };

            let mut has_tool_use = false;
            let mut assistant_content: Vec<serde_json::Value> = Vec::new();

            for block in &response.content {
                match block {
                    ContentBlock::Text { text } => {
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

            messages.push(Message {
                role: "assistant".into(),
                content: json!(assistant_content),
            });

            // No more tool calls — Claude is done
            if !has_tool_use {
                break;
            }

            // Execute each tool call
            let mut tool_results: Vec<serde_json::Value> = Vec::new();

            for block in &response.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    let result = self
                        .execute_tool(name, input, exhibition, &companies)
                        .await;
                    debug!("  Tool {} → {} chars", name, result.len());
                    tool_results.push(json!({
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": result,
                    }));
                }
            }

            messages.push(Message {
                role: "user".into(),
                content: json!(tool_results),
            });
        }

        // ── Store all accumulated companies ──────────────────────────
        let extracted = companies.lock().unwrap().clone();
        let mut stored = 0;

        for ec in &extracted {
            match self.store_company(ec, exhibition).await {
                Ok(_) => stored += 1,
                Err(e) => warn!("  Failed to store '{}': {}", ec.name, e),
            }
        }

        info!(
            "    ✓ {} companies extracted, {} stored for {}",
            extracted.len(),
            stored,
            exhibition.name
        );

        Ok(stored)
    }

    // ── Tool definitions ─────────────────────────────────────────────────

    fn build_tool_definitions(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "scrape_page".into(),
                description: "Fetch and read the content of any web page using a headless browser (Crawl4AI). \
                    Returns the page content as clean markdown. Handles JavaScript rendering and many \
                    anti-bot protections. Use this to browse exhibition websites, exhibitor directories, \
                    and any other web pages.".into(),
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
                name: "google_search".into(),
                description: "Search Google for information. Returns search result snippets with titles, \
                    URLs, and descriptions. Use this to find exhibitor lists, company directories, \
                    or exhibition information that isn't on the homepage.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Google search query (e.g. 'Middle East Electricity 2026 exhibitor list')"
                        }
                    },
                    "required": ["query"]
                }),
            },
            ToolDef {
                name: "extract_companies".into(),
                description: "Parse text content (from a scraped page) and extract company names. \
                    Returns a structured list of companies found in the text. Use this after scraping \
                    an exhibitor list page to get clean company names.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "The raw text/markdown content to extract company names from"
                        },
                        "context": {
                            "type": "string",
                            "description": "Brief context about what this content is (e.g. 'exhibitor list page from MEE website')"
                        }
                    },
                    "required": ["content", "context"]
                }),
            },
            ToolDef {
                name: "save_companies".into(),
                description: "Save a list of discovered companies to the database. Call this once you \
                    have extracted company names from exhibitor lists. You MUST call this before finishing.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "companies": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": {
                                        "type": "string",
                                        "description": "Company name"
                                    },
                                    "booth": {
                                        "type": "string",
                                        "description": "Booth/stand number if available"
                                    },
                                    "website": {
                                        "type": "string",
                                        "description": "Company website URL if found"
                                    },
                                    "description": {
                                        "type": "string",
                                        "description": "Brief description of what the company does"
                                    }
                                },
                                "required": ["name"]
                            },
                            "description": "List of companies to save"
                        }
                    },
                    "required": ["companies"]
                }),
            },
            ToolDef {
                name: "email_organizer".into(),
                description: "Draft and queue an email to the exhibition organizer requesting their \
                    exhibitor/participant list. Use this as a last resort when you cannot find the \
                    exhibitor list online. Provide the organizer's email and a professional request.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "organizer_email": {
                            "type": "string",
                            "description": "The organizer's email address"
                        },
                        "organizer_name": {
                            "type": "string",
                            "description": "The organizer's name or company"
                        },
                        "exhibition_name": {
                            "type": "string",
                            "description": "The exhibition name"
                        },
                        "message": {
                            "type": "string",
                            "description": "Professional email body requesting the exhibitor list"
                        }
                    },
                    "required": ["organizer_email", "exhibition_name", "message"]
                }),
            },
        ]
    }

    // ── Tool execution ───────────────────────────────────────────────────

    async fn execute_tool(
        &self,
        name: &str,
        input: &serde_json::Value,
        exhibition: &Exhibition,
        companies: &Arc<Mutex<Vec<ExtractedCompany>>>,
    ) -> String {
        match name {
            "scrape_page" => self.tool_scrape_page(input).await,
            "google_search" => self.tool_google_search(input).await,
            "extract_companies" => self.tool_extract_companies(input, companies).await,
            "save_companies" => self.tool_save_companies(input, companies).await,
            "email_organizer" => self.tool_email_organizer(input, exhibition).await,
            _ => format!("Unknown tool: {}", name),
        }
    }

    /// Tool: Scrape a web page via Crawl4AI
    async fn tool_scrape_page(&self, input: &serde_json::Value) -> String {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if url.is_empty() {
            return "Error: no URL provided".into();
        }

        info!("    🌐  Scraping: {}", url);

        // Use Crawl4AI for JS rendering
        if self.config.crawl4ai_enabled {
            match crawl4ai_bridge::scrape_pages(vec![url.clone()], 10000).await {
                Ok(results) => {
                    if let Some(page) = results.first() {
                        if page.success && !page.content.is_empty() {
                            return truncate(&page.content, 10000);
                        }
                    }
                }
                Err(e) => {
                    debug!("  Crawl4AI failed for {}: {}", url, e);
                }
            }
        }

        // Reqwest fallback
        match self.http.get(&url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.text().await {
                    Ok(html) => {
                        let text = html_to_text_limited(&html, 10000);
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

    /// Tool: Google search via Crawl4AI rendering of Google results
    async fn tool_google_search(&self, input: &serde_json::Value) -> String {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if query.is_empty() {
            return "Error: no search query provided".into();
        }

        info!("    🔍  Searching: {}", query);

        // Try Google Custom Search API first (if configured)
        if let Some(api_key) = &self.config.sources.google_api_key {
            if !api_key.is_empty() {
                if let Some(cx) = &self.config.sources.google_cx {
                    if !cx.is_empty() {
                        match self.google_api_search(&query, api_key, cx).await {
                            Ok(results) if !results.is_empty() => return results,
                            Ok(_) => debug!("  Google API returned no results"),
                            Err(e) => debug!("  Google API failed: {}", e),
                        }
                    }
                }
            }
        }

        // Fallback: Google via Crawl4AI
        let google_url = format!(
            "https://www.google.com/search?q={}&num=10",
            urlencoding::encode(&query)
        );

        match crawl4ai_bridge::scrape_pages(vec![google_url], 5000).await {
            Ok(results) => {
                if let Some(page) = results.first() {
                    if page.success && !page.content.is_empty() {
                        return truncate(&page.content, 5000);
                    }
                }
                "Google search returned no results. Try a different query or use scrape_page with a direct URL.".into()
            }
            Err(e) => format!("Google search failed: {}. Try scrape_page with a direct URL instead.", e),
        }
    }

    /// Google Custom Search API
    async fn google_api_search(
        &self,
        query: &str,
        api_key: &str,
        cx: &str,
    ) -> Result<String> {
        let resp: serde_json::Value = self
            .http
            .get("https://www.googleapis.com/customsearch/v1")
            .query(&[("key", api_key), ("cx", cx), ("q", query), ("num", "10")])
            .send()
            .await?
            .json()
            .await?;

        let items = resp
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if items.is_empty() {
            return Ok(String::new());
        }

        let formatted: Vec<String> = items
            .iter()
            .take(10)
            .map(|item| {
                format!(
                    "• {} — {}\n  URL: {}\n  {}",
                    item.get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Untitled"),
                    item.get("displayLink")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    item.get("link")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    item.get("snippet")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                )
            })
            .collect();

        Ok(formatted.join("\n\n"))
    }

    /// Tool: Extract companies from text
    async fn tool_extract_companies(
        &self,
        input: &serde_json::Value,
        companies: &Arc<Mutex<Vec<ExtractedCompany>>>,
    ) -> String {
        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let context = input
            .get("context")
            .and_then(|v| v.as_str())
            .unwrap_or("exhibitor list");

        if content.is_empty() {
            return "Error: no content provided to extract from".into();
        }

        // Use Claude to extract company names from the raw content
        let extraction_prompt = format!(
            r#"Extract all company/organization names from this {} content.

Return a JSON array of objects with "name" and optionally "booth", "website", "description".

Rules:
- Only include actual company/organization names (not section headers, navigation items, etc.)
- Clean up names (remove extra whitespace, fix casing)
- Minimum 3 characters per name
- If booth/stand numbers are visible, include them

Content:
{}

Return ONLY the JSON array, no markdown fences."#,
            context,
            truncate(&content, 6000),
        );

        let request = ToolUseRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            system: "You are a data extraction assistant. Extract company names accurately.".into(),
            tools: vec![],
            messages: vec![Message {
                role: "user".into(),
                content: json!(extraction_prompt),
            }],
        };

        match self.call_claude(request).await {
            Ok(response) => {
                let text: String = response
                    .content
                    .iter()
                    .filter_map(|b| {
                        if let ContentBlock::Text { text } = b {
                            Some(text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();

                // Parse the JSON array
                let json_str = if let Some(start) = text.find('[') {
                    if let Some(end) = text.rfind(']') {
                        &text[start..=end]
                    } else {
                        &text
                    }
                } else {
                    &text
                };

                match serde_json::from_str::<Vec<ExtractedCompany>>(json_str) {
                    Ok(extracted) => {
                        let count = extracted.len();
                        companies.lock().unwrap().extend(extracted);
                        format!(
                            "Successfully extracted {} companies. Call save_companies when ready to store them.",
                            count
                        )
                    }
                    Err(e) => {
                        format!(
                            "Failed to parse extracted companies: {}. Raw output: {}",
                            e,
                            text.chars().take(500).collect::<String>()
                        )
                    }
                }
            }
            Err(e) => format!("Extraction failed: {}", e),
        }
    }

    /// Tool: Save companies to database
    async fn tool_save_companies(
        &self,
        input: &serde_json::Value,
        companies: &Arc<Mutex<Vec<ExtractedCompany>>>,
    ) -> String {
        // Parse the companies from the tool input
        if let Some(company_arr) = input.get("companies").and_then(|v| v.as_array()) {
            let new_companies: Vec<ExtractedCompany> = company_arr
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect();

            let count = new_companies.len();
            companies.lock().unwrap().extend(new_companies);
            format!(
                "Queued {} companies for saving. They will be stored to the database when the extraction completes.",
                count
            )
        } else {
            // If no explicit input, confirm what we already have
            let count = companies.lock().unwrap().len();
            format!(
                "{} companies are queued for saving from previous extract_companies calls.",
                count
            )
        }
    }

    /// Tool: Email the exhibition organizer
    async fn tool_email_organizer(
        &self,
        input: &serde_json::Value,
        exhibition: &Exhibition,
    ) -> String {
        let organizer_email = input
            .get("organizer_email")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let organizer_name = input
            .get("organizer_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Exhibition Team");
        let message = input
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if organizer_email.is_empty() || message.is_empty() {
            return "Error: organizer_email and message are required".into();
        }

        // Store the organizer contact on the exhibition for future reference
        info!(
            "    📧  Queuing organizer email: {} → {}",
            exhibition.name, organizer_email
        );

        // Store to database as a special "organizer_request" record
        match self
            .db
            .store_organizer_request(
                &exhibition.id,
                organizer_email,
                organizer_name,
                message,
            )
            .await
        {
            Ok(_) => format!(
                "Email queued to {} ({}). It will be sent in the next email sending phase.",
                organizer_name, organizer_email
            ),
            Err(e) => {
                // Even if DB fails, log it so the user sees it
                warn!("  Failed to queue organizer email: {}", e);
                format!(
                    "Note: Could not queue to DB ({}), but organizer contact recorded: {} <{}>",
                    e, organizer_name, organizer_email
                )
            }
        }
    }

    // ── Store a company and its participation ────────────────────────────

    async fn store_company(
        &self,
        ec: &ExtractedCompany,
        exhibition: &Exhibition,
    ) -> Result<()> {
        let mut company = Company::new(ec.name.clone());
        company.industry = Some(exhibition.sector.clone());
        company.location = exhibition.city.clone().or(exhibition.location.clone());
        company.country = exhibition.country.clone();
        company.website = ec.website.clone();
        company.description = ec.description.clone();

        self.db.upsert_company(&company).await?;

        let mut participation = Participation::new(exhibition.id.clone(), company.id.clone());
        participation.booth_number = ec.booth.clone();
        self.db.upsert_participation(&participation).await?;

        Ok(())
    }

    // ── Claude API call ──────────────────────────────────────────────────

    async fn call_claude(&self, request: ToolUseRequest) -> Result<ToolUseResponse> {
        let resp = self
            .http
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

    // ── System prompt ────────────────────────────────────────────────────

    fn build_system_prompt(
        &self,
        exhibition: &Exhibition,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> String {
        let gst_time = now
            .with_timezone(&chrono::FixedOffset::east_opt(4 * 3600).unwrap());

        format!(
            r#"You are Scott, an autonomous lead generation agent for Track Exhibits Pvt LTD — a premium exhibition booth design, fabrication, and installation company based in Dubai (GST, UTC+4).

CURRENT TIME: {} UTC | {} GST

YOUR TASK: Find the complete list of companies exhibiting at "{}" and extract their names.

EXHIBITION DETAILS:
- Name: {}
- Sector: {} | Region: {} | Location: {}
- Date: {}
- Website: {}

AVAILABLE TOOLS:
1. scrape_page — Fetch any web page using a headless browser (handles JS, Cloudflare, etc.)
2. google_search — Search Google for information
3. extract_companies — Parse scraped text and extract structured company names
4. save_companies — Store discovered companies to the database (MUST call before finishing)
5. email_organizer — As a last resort, queue an email to the exhibition organizer

STRATEGY:
1. If a website URL is given, scrape it first
2. Navigate to the exhibitor/participant directory page — look for links containing words like "exhibitors", "participants", "directory", "companies", "stand holders", "floorplan"
3. If the exhibitor list is paginated, try scraping multiple pages
4. If the website doesn't have a list, google_search for "[Exhibition Name] exhibitor list [year]"
5. If Google finds direct links to exhibitor directories, scrape those
6. As a last resort, find the organizer's contact email and use email_organizer to request the list
7. ALWAYS call save_companies with whatever companies you find before finishing

IMPORTANT:
- Extract REAL company names only (not headers, navigation, categories)
- Be thorough — try multiple approaches if the first doesn't work
- Maximum 10 tool calls — be efficient"#,
            now.format("%Y-%m-%d %H:%M"),
            ist_time.format("%Y-%m-%d %H:%M"),
            exhibition.name,
            exhibition.name,
            exhibition.sector,
            exhibition.region,
            exhibition.location.as_deref().unwrap_or("Unknown"),
            exhibition
                .start_date
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or("Unknown".into()),
            exhibition.website_url.as_deref().unwrap_or("None"),
        )
    }
}

// ── Utilities ────────────────────────────────────────────────────────────

fn truncate(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        format!("{}... [truncated at {} chars]", &s[..max_chars], max_chars)
    }
}

fn html_to_text_limited(html: &str, max_chars: usize) -> String {
    let mut result = String::with_capacity(max_chars);
    let mut in_tag = false;
    let mut last_was_space = true;

    for ch in html.chars() {
        if result.len() >= max_chars {
            break;
        }
        match ch {
            '<' => in_tag = true,
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
