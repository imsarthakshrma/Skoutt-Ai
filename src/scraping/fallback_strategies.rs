// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — scraping/fallback_strategies.rs
// Fallback lead discovery when direct scraping fails
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use chrono::Datelike;
use tracing::{info, warn};

use crate::{database::Database, Exhibition};
use super::Scraper;

pub struct FallbackStrategies {
    scraper: Scraper,
    db: Database,
}

impl FallbackStrategies {
    pub fn new(scraper: Scraper, db: Database) -> Self {
        Self { scraper, db }
    }

    /// Try all fallback strategies for an exhibition with no exhibitor list
    pub async fn try_fallbacks(&self, exhibition: &Exhibition) -> Result<usize> {
        let mut total = 0;

        // Strategy 1: Check previous year's exhibitors (same exhibition, last year)
        match self.find_previous_year_exhibitors(exhibition).await {
            Ok(n) => {
                if n > 0 {
                    info!("  Fallback: Found {} previous-year exhibitors for {}", n, exhibition.name);
                    total += n;
                }
            }
            Err(e) => warn!("  Previous year fallback failed: {}", e),
        }

        // Strategy 2: Search Google for "[exhibition name] exhibitors [year]"
        // Note: Direct Google scraping is against ToS; we use a search-friendly approach
        match self.search_for_exhibitor_mentions(exhibition).await {
            Ok(n) => {
                if n > 0 {
                    info!("  Fallback: Found {} exhibitors via web search", n);
                    total += n;
                }
            }
            Err(e) => warn!("  Web search fallback failed: {}", e),
        }

        Ok(total)
    }

    /// Look for previous year's exhibitor list (same show, -1 year in URL)
    async fn find_previous_year_exhibitors(&self, exhibition: &Exhibition) -> Result<usize> {
        let current_year = chrono::Utc::now().format("%Y").to_string();
        let prev_year = (chrono::Utc::now().year() - 1).to_string();

        if let Some(website) = &exhibition.website_url {
            // Try replacing year in URL
            let prev_url = website.replace(&current_year, &prev_year);
            if prev_url != *website {
                if let Ok(html) = self.scraper.fetch(&prev_url).await {
                    // Look for exhibitor names in the previous year's page
                    return self.extract_company_names_from_html(&html, exhibition).await;
                }
            }
        }

        Ok(0)
    }

    /// Search for press releases and news mentioning exhibitors
    async fn search_for_exhibitor_mentions(&self, exhibition: &Exhibition) -> Result<usize> {
        // Use DuckDuckGo HTML search (more scraping-friendly than Google)
        let query = format!(
            "{} exhibitors participants 2025 2026",
            exhibition.name
        );
        let encoded = urlencoding::encode(&query);
        let url = format!("https://html.duckduckgo.com/html/?q={}", encoded);

        let html = self.scraper.fetch(&url).await?;
        let count = self.extract_company_names_from_html(&html, exhibition).await?;
        Ok(count)
    }

    /// Generic company name extractor from arbitrary HTML
    async fn extract_company_names_from_html(&self, html: &str, exhibition: &Exhibition) -> Result<usize> {
        use scraper::{Html, Selector};

        let document = Html::parse_document(html);
        let mut count = 0;

        // Look for company-like patterns in list items and table cells
        let selectors = ["li", "td", ".result__title", ".result-title"];

        for sel_str in &selectors {
            if let Ok(sel) = Selector::parse(sel_str) {
                for el in document.select(&sel).take(50) {
                    let text = el.text().collect::<String>().trim().to_string();
                    // Heuristic: company names are 2-100 chars, contain letters, not just numbers
                    if text.len() >= 3
                        && text.len() <= 100
                        && text.chars().any(|c| c.is_alphabetic())
                        && !text.starts_with("http")
                        && !text.contains('\n')
                    {
                        let mut company = crate::Company::new(text);
                        company.industry = Some(exhibition.sector.clone());
                        if let Ok(_) = self.db.upsert_company(&company).await {
                            let participation = crate::Participation::new(
                                exhibition.id.clone(),
                                company.id.clone(),
                            );
                            let _ = self.db.upsert_participation(&participation).await;
                            count += 1;
                        }
                    }
                }
                if count > 5 {
                    break;
                }
            }
        }

        Ok(count)
    }
}

// Simple URL encoding helper
mod urlencoding {
    pub fn encode(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                ' ' => '+'.to_string(),
                _ => format!("%{:02X}", c as u32),
            })
            .collect()
    }
}
