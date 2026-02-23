// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — intelligence/deep_researcher.rs
// Multi-source company intelligence gathering before email drafting.
// Sources: company website, Google News / SerpAPI, exhibition history.
// LinkedIn is a future stub — not implemented.
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{Company, Contact, Exhibition, Participation, ResearchConfig};
use crate::database::Database;
use crate::intelligence::agentic_researcher::AgenticResearcher;
use crate::intelligence::research_synthesizer::ResearchSynthesizer;
use crate::python_bridge::crawl4ai_bridge;

// ── Public data types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchReport {
    pub id: String,
    pub contact_id: String,
    pub company_id: String,
    pub participation_id: String,
    pub researched_at: DateTime<Utc>,

    // Raw gathered sources
    pub company_website_summary: String,
    pub recent_news: Vec<NewsArticle>,
    pub previous_exhibitions: Vec<PreviousExhibition>,

    // Claude synthesis
    pub company_overview: String,
    pub exhibition_strategy: String,
    pub pain_points: Vec<String>,
    pub personalization_hooks: Vec<String>,
    pub email_angle: String,

    // Quality
    pub research_quality_score: f64,
    pub sources_used: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsArticle {
    pub title: String,
    pub source: String,
    pub url: String,
    pub summary: String,
    pub published_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviousExhibition {
    pub event_name: String,
    pub date: Option<NaiveDate>,
    pub location: String,
    pub booth_size: Option<String>,
}

// ── SerpAPI response shapes ────────────────────────────────────────────────

#[derive(Deserialize)]
struct SerpResult {
    organic_results: Option<Vec<SerpOrganic>>,
    news_results: Option<Vec<SerpNews>>,
}

#[derive(Deserialize)]
struct SerpOrganic {
    title: Option<String>,
    link: Option<String>,
    snippet: Option<String>,
    displayed_link: Option<String>,
}

#[derive(Deserialize)]
struct SerpNews {
    title: Option<String>,
    link: Option<String>,
    snippet: Option<String>,
    source: Option<SerpNewsSource>,
    date: Option<String>,
}

#[derive(Deserialize)]
struct SerpNewsSource {
    name: Option<String>,
}

// ── Google Custom Search response shapes ───────────────────────────────────

#[derive(Deserialize)]
struct GoogleSearchResponse {
    items: Option<Vec<GoogleItem>>,
}

#[derive(Deserialize)]
struct GoogleItem {
    title: Option<String>,
    link: Option<String>,
    snippet: Option<String>,
    #[serde(rename = "displayLink")]
    display_link: Option<String>,
}

// ── DeepResearcher ─────────────────────────────────────────────────────────

pub struct DeepResearcher {
    config: ResearchConfig,
    http: Client,
    synthesizer: ResearchSynthesizer,
    agentic: Option<AgenticResearcher>,
}

impl DeepResearcher {
    pub fn new(config: ResearchConfig, claude_api_key: String, claude_model: String) -> Self {
        let agentic = if config.use_agentic {
            Some(AgenticResearcher::new(
                config.clone(),
                claude_api_key.clone(),
                claude_model.clone(),
            ))
        } else {
            None
        };
        Self {
            synthesizer: ResearchSynthesizer::new(claude_api_key, claude_model),
            config,
            http: Client::builder()
                .user_agent("SkouTT-Research/1.0")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("HTTP client construction failed"),
            agentic,
        }
    }

    /// Full research pipeline for one contact.
    /// Returns None if quality is below the configured threshold.
    pub async fn research_contact(
        &self,
        contact: &Contact,
        company: &Company,
        participation: &Participation,
        exhibition: &Exhibition,
        db: &Database,
    ) -> Result<Option<ResearchReport>> {
        info!("🔍  Deep research: {} at {}", contact.full_name, company.name);

        // ── 1. Check DB cache ────────────────────────────────────────────
        if let Ok(Some(cached)) = db.get_cached_research(&contact.id).await {
            debug!("  Using cached research (quality {:.2})", cached.research_quality_score);
            return Ok(Some(cached));
        }

        // ── 1b. Agentic path (if enabled) ────────────────────────────────
        if let Some(ref agentic) = self.agentic {
            info!("  Using agentic tool-calling research");
            let result = agentic.research(
                contact, company, participation, exhibition, db,
            ).await?;
            if let Some(ref report) = result {
                let _ = db.store_research_report(report, self.config.cache_duration_days).await;
            }
            return Ok(result);
        }

        let mut sources_used: Vec<String> = Vec::new();

        // ── 2. Company website deep scrape ───────────────────────────────
        let website_summary = self
            .scrape_company_website(company)
            .await
            .unwrap_or_else(|e| {
                warn!("  Website scrape failed for {}: {e}", company.name);
                String::new()
            });
        if !website_summary.is_empty() {
            sources_used.push("company_website".into());
        }

        // ── 3. News search (SerpAPI → Google → skip) ─────────────────────
        let news = self
            .search_company_news(&company.name)
            .await
            .unwrap_or_else(|e| {
                warn!("  News search failed for {}: {e}", company.name);
                vec![]
            });
        if !news.is_empty() {
            sources_used.push(if self.config.sources.serp_api_key.is_some() {
                "serpapi_news".into()
            } else {
                "google_news".into()
            });
        }

        // ── 4. Exhibition history ─────────────────────────────────────────
        let prev_exhibitions = self
            .find_previous_exhibitions(company, db)
            .await
            .unwrap_or_else(|e| {
                warn!("  Exhibition history lookup failed for {}: {e}", company.name);
                vec![]
            });
        if !prev_exhibitions.is_empty() {
            sources_used.push("exhibition_history".into());
        }

        // ── 5. Calculate preliminary quality score ────────────────────────
        let raw_quality = self.calculate_raw_quality(
            &website_summary,
            &news,
            &prev_exhibitions,
        );

        if raw_quality < 0.3 {
            // Not enough raw material for Claude to synthesise anything useful
            warn!(
                "  ⚠️  {} — insufficient research data ({:.2}), skipping",
                company.name, raw_quality
            );
            return Ok(None);
        }

        // ── 6. Claude synthesis ───────────────────────────────────────────
        let synthesis = self.synthesizer.synthesize(
            company,
            contact,
            exhibition,
            participation,
            &website_summary,
            &news,
            &prev_exhibitions,
        ).await?;

        // ── 7. Final quality score ─────────────────────────────────────────
        let quality_score = self.calculate_final_quality(
            &website_summary,
            &news,
            &prev_exhibitions,
            &synthesis.company_overview,
        );

        if quality_score < self.config.quality_threshold {
            warn!(
                "  ⚠️  {} — below quality threshold ({:.2} < {:.2}), skipping",
                company.name, quality_score, self.config.quality_threshold
            );
            return Ok(None);
        }

        info!(
            "  ✓  {} — quality {:.2} | sources: {}",
            company.name,
            quality_score,
            sources_used.join(", ")
        );

        Ok(Some(ResearchReport {
            id: Uuid::new_v4().to_string(),
            contact_id: contact.id.clone(),
            company_id: company.id.clone(),
            participation_id: participation.id.clone(),
            researched_at: Utc::now(),
            company_website_summary: website_summary,
            recent_news: news,
            previous_exhibitions: prev_exhibitions,
            company_overview: synthesis.company_overview,
            exhibition_strategy: synthesis.exhibition_strategy,
            pain_points: synthesis.pain_points,
            personalization_hooks: synthesis.personalization_hooks,
            email_angle: synthesis.email_angle,
            research_quality_score: quality_score,
            sources_used,
        }))
    }

    // ── Company website scraping ──────────────────────────────────────────

    async fn scrape_company_website(&self, company: &Company) -> Result<String> {
        let website = match &company.website {
            Some(w) if !w.is_empty() => w.clone(),
            _ => return Ok(String::new()),
        };

        // Normalise base URL
        let base = website.trim_end_matches('/').to_string();

        // Pages to scrape
        let pages = vec![
            base.clone(),
            format!("{}/about", base),
            format!("{}/about-us", base),
            format!("{}/products", base),
            format!("{}/services", base),
        ];

        // ── Try Crawl4AI first (JS-rendered) ──────────────────────────────
        if self.config.crawl4ai_enabled {
            debug!("  Trying Crawl4AI for {} …", company.name);
            let crawl_pages: Vec<String> = pages.iter().take(3).cloned().collect();
            let results = crawl4ai_bridge::scrape_pages(crawl_pages, 2000).await?;

            let mut combined = String::new();
            for page in &results {
                if page.success && !page.content.is_empty() {
                    combined.push_str(&page.content);
                    combined.push('\n');
                }
            }

            if !combined.trim().is_empty() {
                debug!("  Crawl4AI returned {} chars for {}", combined.len(), company.name);
                return Ok(combined.trim().to_string());
            }
            debug!("  Crawl4AI returned no content, falling back to reqwest");
        }

        // ── Fallback: basic reqwest (no JS rendering) ─────────────────────
        let mut combined = String::new();
        for page in pages.iter().take(3) {
            if let Ok(resp) = self.http.get(page).send().await {
                if resp.status().is_success() {
                    if let Ok(html) = resp.text().await {
                        let text = html_to_text(&html);
                        combined.push_str(&text.chars().take(1500).collect::<String>());
                        combined.push('\n');
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        Ok(combined.trim().to_string())
    }

    // ── News search ───────────────────────────────────────────────────────

    async fn search_company_news(&self, company_name: &str) -> Result<Vec<NewsArticle>> {
        let limit = self.config.limits.max_news_articles;

        // ── Try Crawl4AI first (scrapes Google News directly) ─────────────
        if self.config.crawl4ai_enabled {
            debug!("  Trying Crawl4AI news search for {} …", company_name);
            let crawl_results = crawl4ai_bridge::search_news_via_crawl4ai(
                company_name,
                limit,
            ).await?;

            if !crawl_results.is_empty() {
                let articles: Vec<NewsArticle> = crawl_results
                    .into_iter()
                    .map(|r| NewsArticle {
                        title: r.title,
                        source: r.source,
                        url: r.url,
                        summary: if r.full_content.is_empty() { r.snippet } else { r.full_content },
                        published_date: None,
                    })
                    .collect();
                debug!("  Crawl4AI found {} news articles for {}", articles.len(), company_name);
                return Ok(articles);
            }
            debug!("  Crawl4AI news returned no results, trying API fallback");
        }

        // ── Fallback: SerpAPI (preferred API) ─────────────────────────────
        if let Some(key) = &self.config.sources.serp_api_key {
            return self.search_via_serp(company_name, key, limit).await;
        }

        // ── Fallback: Google Custom Search ─────────────────────────────────
        if let (Some(key), Some(cx)) = (
            &self.config.sources.google_api_key,
            &self.config.sources.google_cx,
        ) {
            return self.search_via_google(company_name, key, cx, limit).await;
        }

        // No search method available — silent skip
        debug!("  No news search method available, skipping");
        Ok(vec![])
    }

    async fn search_via_serp(
        &self,
        company_name: &str,
        api_key: &str,
        limit: usize,
    ) -> Result<Vec<NewsArticle>> {
        let url = "https://serpapi.com/search.json";
        let query = format!("{} news", company_name);

        let resp: SerpResult = self.http
            .get(url)
            .query(&[
                ("api_key", api_key),
                ("engine", "google"),
                ("q", query.as_str()),
                ("tbm", "nws"),
                ("tbs", "qdr:m6"),
                ("num", "10"),
            ])
            .send()
            .await?
            .json()
            .await?;

        // Prefer dedicated news_results, fall back to organic_results
        let articles: Vec<NewsArticle> = if let Some(news) = resp.news_results {
            news.into_iter()
                .take(limit)
                .map(|item| NewsArticle {
                    title: item.title.unwrap_or_default(),
                    source: item.source.and_then(|s| s.name).unwrap_or_default(),
                    url: item.link.unwrap_or_default(),
                    summary: item.snippet.unwrap_or_default(),
                    published_date: item.date,
                })
                .collect()
        } else if let Some(organic) = resp.organic_results {
            organic.into_iter()
                .take(limit)
                .map(|item| NewsArticle {
                    title: item.title.unwrap_or_default(),
                    source: item.displayed_link.unwrap_or_default(),
                    url: item.link.unwrap_or_default(),
                    summary: item.snippet.unwrap_or_default(),
                    published_date: None,
                })
                .collect()
        } else {
            vec![]
        };

        Ok(articles)
    }

    async fn search_via_google(
        &self,
        company_name: &str,
        api_key: &str,
        cx: &str,
        limit: usize,
    ) -> Result<Vec<NewsArticle>> {
        let url = "https://www.googleapis.com/customsearch/v1";
        let query = format!("{} news", company_name);

        let resp: GoogleSearchResponse = self.http
            .get(url)
            .query(&[
                ("key", api_key),
                ("cx", cx),
                ("q", &query),
                ("dateRestrict", "m6"),
                ("sort", "date"),
                ("num", "10"),
            ])
            .send()
            .await?
            .json()
            .await?;

        let articles = resp.items.unwrap_or_default()
            .into_iter()
            .take(limit)
            .map(|item| NewsArticle {
                title: item.title.unwrap_or_default(),
                source: item.display_link.unwrap_or_default(),
                url: item.link.unwrap_or_default(),
                summary: item.snippet.unwrap_or_default(),
                published_date: None,
            })
            .collect();

        Ok(articles)
    }

    // ── Exhibition history ────────────────────────────────────────────────

    async fn find_previous_exhibitions(
        &self,
        company: &Company,
        db: &Database,
    ) -> Result<Vec<PreviousExhibition>> {
        let limit = self.config.limits.max_previous_exhibitions;

        // 1. Internal DB (most reliable)
        let internal = db.get_past_participations(&company.id).await?;

        let mut exhibitions: Vec<PreviousExhibition> = internal
            .into_iter()
            .map(|(name, date, location)| PreviousExhibition {
                event_name: name,
                date,
                location,
                booth_size: None,
            })
            .collect();

        // 2. Company news mentions of exhibitions (extracted from news above)
        // This is done during synthesis — no separate call needed

        exhibitions.sort_by(|a, b| b.date.cmp(&a.date));
        exhibitions.dedup_by(|a, b| a.event_name == b.event_name);
        exhibitions.truncate(limit);

        Ok(exhibitions)
    }

    // ── Quality scoring ───────────────────────────────────────────────────

    fn calculate_raw_quality(
        &self,
        website: &str,
        news: &[NewsArticle],
        exhibitions: &[PreviousExhibition],
    ) -> f64 {
        let mut score = 0.0_f64;
        if website.len() > 200       { score += 0.3; }
        if !news.is_empty()          { score += 0.3; }
        if !exhibitions.is_empty()   { score += 0.2; }
        if website.len() > 1000      { score += 0.1; } // richer website
        if news.len() >= 3           { score += 0.1; } // substantial news
        score.min(1.0)
    }

    fn calculate_final_quality(
        &self,
        website: &str,
        news: &[NewsArticle],
        exhibitions: &[PreviousExhibition],
        overview: &str,
    ) -> f64 {
        let mut score = self.calculate_raw_quality(website, news, exhibitions);
        // Claude produced a meaningful overview
        if overview.len() > 100 { score = (score + 0.2).min(1.0); }
        score
    }
}

// ── Utility: strip HTML tags to plain text ────────────────────────────────

fn html_to_text(html: &str) -> String {
    // Lightweight: strip tags with a simple state machine, collapse whitespace
    let mut result = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut last_was_space = true;

    for ch in html.chars() {
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
