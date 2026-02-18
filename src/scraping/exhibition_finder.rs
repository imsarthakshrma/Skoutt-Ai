// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — scraping/exhibition_finder.rs
// Discovers exhibitions from aggregator sites
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use scraper::{Html, Selector};
use tracing::{info, warn};

use crate::{database::Database, Exhibition, ScrapingConfig, TargetingConfig};
use super::Scraper;

pub struct ExhibitionFinder {
    scraper: Scraper,
    targeting: TargetingConfig,
    db: Database,
}

impl ExhibitionFinder {
    pub fn new(config: ScrapingConfig, targeting: TargetingConfig, db: Database) -> Self {
        Self {
            scraper: Scraper::new(config).expect("Failed to create scraper"),
            targeting,
            db,
        }
    }

    /// Main discovery method — searches all configured sources
    pub async fn discover_exhibitions(&self) -> Result<usize> {
        let mut total = 0;

        for sector in &self.targeting.sectors {
            for region in &self.targeting.regions {
                info!("  Searching: {} exhibitions in {}", sector, region);

                // Try 10times.com
                match self.search_10times(sector, region).await {
                    Ok(count) => total += count,
                    Err(e) => warn!("  10times.com failed for {}/{}: {}", sector, region, e),
                }

                // Try Expodatabase
                match self.search_expodatabase(sector, region).await {
                    Ok(count) => total += count,
                    Err(e) => warn!("  Expodatabase failed for {}/{}: {}", sector, region, e),
                }
            }
        }

        Ok(total)
    }

    /// Search 10times.com for exhibitions
    async fn search_10times(&self, sector: &str, region: &str) -> Result<usize> {
        let sector_slug = self.sector_to_10times_slug(sector);
        let region_slug = self.region_to_10times_slug(region);
        let url = format!(
            "https://10times.com/{}/{}",
            region_slug, sector_slug
        );

        // Check cache first
        if let Ok(Some(cached)) = self.db.get_cached_page(&url).await {
            return self.parse_10times_results(&cached, sector, region).await;
        }

        let html = self.scraper.fetch(&url).await?;
        let _ = self.db.cache_page(&url, &html, self.scraper.config.cache_ttl_hours).await;
        self.parse_10times_results(&html, sector, region).await
    }

    async fn parse_10times_results(&self, html: &str, sector: &str, region: &str) -> Result<usize> {
        let document = Html::parse_document(html);
        let mut count = 0;

        // 10times event card selectors
        let event_sel = Selector::parse(".event-card, .event-item, article.event").unwrap_or_else(|_| Selector::parse("article").unwrap());
        let name_sel = Selector::parse("h2, h3, .event-name, .event-title").unwrap_or_else(|_| Selector::parse("h2").unwrap());
        let date_sel = Selector::parse(".event-date, .date, time").unwrap_or_else(|_| Selector::parse("time").unwrap());
        let location_sel = Selector::parse(".event-location, .location, .venue").unwrap_or_else(|_| Selector::parse(".location").unwrap());
        let link_sel = Selector::parse("a[href]").unwrap();

        for event_el in document.select(&event_sel) {
            let name = event_el
                .select(&name_sel)
                .next()
                .map(|el| el.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            if name.is_empty() {
                continue;
            }

            let location = event_el
                .select(&location_sel)
                .next()
                .map(|el| el.text().collect::<String>().trim().to_string());

            let website_url = event_el
                .select(&link_sel)
                .next()
                .and_then(|el| el.value().attr("href"))
                .map(|href| {
                    if href.starts_with("http") {
                        href.to_string()
                    } else {
                        format!("https://10times.com{}", href)
                    }
                });

            let mut exhibition = Exhibition::new(name, sector.to_string(), region.to_string());
            exhibition.location = location.clone();
            exhibition.website_url = website_url;
            exhibition.source_url = Some(format!("https://10times.com/{}/{}", region, sector));

            // Parse date if available
            if let Some(date_el) = event_el.select(&date_sel).next() {
                let date_text = date_el.text().collect::<String>();
                exhibition.start_date = parse_date_fuzzy(&date_text);
            }

            match self.db.upsert_exhibition(&exhibition).await {
                Ok(_) => count += 1,
                Err(e) => warn!("  Failed to store exhibition '{}': {}", exhibition.name, e),
            }
        }

        Ok(count)
    }

    /// Search Expodatabase.com
    async fn search_expodatabase(&self, sector: &str, region: &str) -> Result<usize> {
        let sector_slug = sector.to_lowercase().replace(' ', "-");
        let url = format!(
            "https://www.expodatabase.com/trade-shows/{}/",
            sector_slug
        );

        if let Ok(Some(cached)) = self.db.get_cached_page(&url).await {
            return self.parse_expodatabase_results(&cached, sector, region).await;
        }

        let html = self.scraper.fetch(&url).await?;
        let _ = self.db.cache_page(&url, &html, self.scraper.config.cache_ttl_hours).await;
        self.parse_expodatabase_results(&html, sector, region).await
    }

    async fn parse_expodatabase_results(&self, html: &str, sector: &str, region: &str) -> Result<usize> {
        let document = Html::parse_document(html);
        let mut count = 0;

        let row_sel = Selector::parse("tr.event-row, .trade-show-item, .event-listing").unwrap_or_else(|_| Selector::parse("tr").unwrap());
        let name_sel = Selector::parse("td.name a, .show-name, h3 a").unwrap_or_else(|_| Selector::parse("a").unwrap());

        for row in document.select(&row_sel) {
            let name = row
                .select(&name_sel)
                .next()
                .map(|el| el.text().collect::<String>().trim().to_string())
                .unwrap_or_default();

            if name.is_empty() || name.len() < 3 {
                continue;
            }

            // Filter by region keyword
            let row_text = row.text().collect::<String>().to_lowercase();
            let region_keywords = self.region_keywords(region);
            if !region_keywords.iter().any(|kw| row_text.contains(kw)) {
                continue;
            }

            let mut exhibition = Exhibition::new(name, sector.to_string(), region.to_string());
            exhibition.source_url = Some(format!("https://www.expodatabase.com/trade-shows/{}/", sector.to_lowercase()));

            match self.db.upsert_exhibition(&exhibition).await {
                Ok(_) => count += 1,
                Err(e) => warn!("  Failed to store exhibition: {}", e),
            }
        }

        Ok(count)
    }

    fn sector_to_10times_slug(&self, sector: &str) -> &'static str {
        match sector {
            "Tech" => "technology",
            "Medical" => "medical",
            "Pharma" => "pharmaceutical",
            "Auto" => "automotive",
            _ => "business",
        }
    }

    fn region_to_10times_slug(&self, region: &str) -> &'static str {
        match region {
            "Middle East" => "middle-east",
            "Europe" => "europe",
            "Asia Pacific" => "asia",
            "UK" => "united-kingdom",
            _ => "world",
        }
    }

    fn region_keywords(&self, region: &str) -> Vec<&'static str> {
        match region {
            "Middle East" => vec!["dubai", "abu dhabi", "riyadh", "doha", "kuwait", "middle east", "uae", "saudi"],
            "Europe" => vec!["germany", "france", "uk", "netherlands", "spain", "italy", "europe", "berlin", "paris", "amsterdam"],
            "Asia Pacific" => vec!["singapore", "hong kong", "tokyo", "shanghai", "sydney", "asia", "pacific", "china", "japan"],
            "UK" => vec!["london", "manchester", "birmingham", "uk", "united kingdom", "england"],
            _ => vec![],
        }
    }
}

/// Fuzzy date parser for exhibition dates
fn parse_date_fuzzy(text: &str) -> Option<chrono::NaiveDate> {
    use chrono::NaiveDate;

    // Try common formats
    let text = text.trim();
    let formats = [
        "%B %d, %Y",
        "%d %B %Y",
        "%Y-%m-%d",
        "%d/%m/%Y",
        "%m/%d/%Y",
        "%b %d, %Y",
        "%d %b %Y",
    ];

    for fmt in &formats {
        if let Ok(date) = NaiveDate::parse_from_str(text, fmt) {
            return Some(date);
        }
    }

    // Try to extract year-month-day from longer strings
    let re = regex::Regex::new(r"(\d{4})-(\d{2})-(\d{2})").ok()?;
    if let Some(caps) = re.captures(text) {
        let y: i32 = caps[1].parse().ok()?;
        let m: u32 = caps[2].parse().ok()?;
        let d: u32 = caps[3].parse().ok()?;
        return NaiveDate::from_ymd_opt(y, m, d);
    }

    None
}
