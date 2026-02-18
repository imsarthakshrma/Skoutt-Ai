// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — scraping/exhibitor_extractor.rs
// Extracts exhibitor lists from exhibition websites
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use scraper::{Html, Selector};
use tracing::{info, warn};

use crate::{database::Database, Company, Exhibition, Participation};
use super::Scraper;

pub struct ExhibitorExtractor {
    scraper: Scraper,
    db: Database,
}

impl ExhibitorExtractor {
    pub fn new(scraper: Scraper, db: Database) -> Self {
        Self { scraper, db }
    }

    /// Extract exhibitors for a given exhibition
    pub async fn extract_exhibitors(&self, exhibition: &Exhibition) -> Result<usize> {
        let mut count = 0;

        // Priority 1: Direct exhibitor list URL
        if let Some(list_url) = &exhibition.exhibitor_list_url {
            match self.scrape_exhibitor_list(list_url, exhibition).await {
                Ok(n) => {
                    info!("  Extracted {} exhibitors from direct list", n);
                    return Ok(n);
                }
                Err(e) => warn!("  Direct list failed: {}", e),
            }
        }

        // Priority 2: Find exhibitor page on exhibition website
        if let Some(website) = &exhibition.website_url {
            match self.find_exhibitor_page(website, exhibition).await {
                Ok(n) => {
                    count += n;
                    if n > 0 {
                        return Ok(count);
                    }
                }
                Err(e) => warn!("  Exhibitor page search failed: {}", e),
            }
        }

        Ok(count)
    }

    /// Scrape a known exhibitor list URL
    async fn scrape_exhibitor_list(&self, url: &str, exhibition: &Exhibition) -> Result<usize> {
        let html = self.scraper.fetch(url).await?;
        let document = Html::parse_document(&html);
        let mut count = 0;

        // Common exhibitor list patterns
        let selectors = [
            ".exhibitor-name",
            ".company-name",
            ".exhibitor-item h3",
            ".exhibitor-item h2",
            "td.company",
            ".exhibitor-list li",
            ".participant-name",
            "article.exhibitor h2",
            ".booth-company",
        ];

        for sel_str in &selectors {
            if let Ok(sel) = Selector::parse(sel_str) {
                let items: Vec<_> = document.select(&sel).collect();
                if items.len() > 3 {
                    for item in items {
                        let name = item.text().collect::<String>().trim().to_string();
                        if name.len() > 2 && name.len() < 200 {
                            count += self.store_exhibitor(&name, exhibition, None).await.unwrap_or(0);
                        }
                    }
                    if count > 0 {
                        break;
                    }
                }
            }
        }

        // Fallback: look for booth number + company name patterns
        if count == 0 {
            count += self.extract_booth_table(&document, exhibition).await?;
        }

        Ok(count)
    }

    /// Look for exhibitor/participant pages on the exhibition website
    async fn find_exhibitor_page(&self, base_url: &str, exhibition: &Exhibition) -> Result<usize> {
        let html = self.scraper.fetch(base_url).await?;
        let document = Html::parse_document(&html);

        // Find links that might be exhibitor lists
        let link_sel = Selector::parse("a[href]").unwrap();
        let exhibitor_keywords = ["exhibitor", "participant", "company", "booth", "stand", "directory"];

        for link in document.select(&link_sel) {
            let href = link.value().attr("href").unwrap_or("");
            let text = link.text().collect::<String>().to_lowercase();

            if exhibitor_keywords.iter().any(|kw| href.to_lowercase().contains(kw) || text.contains(kw)) {
                let full_url = if href.starts_with("http") {
                    href.to_string()
                } else if href.starts_with('/') {
                    let base = url::Url::parse(base_url).ok();
                    if let Some(base) = base {
                        format!("{}://{}{}", base.scheme(), base.host_str().unwrap_or(""), href)
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };

                if let Ok(count) = self.scrape_exhibitor_list(&full_url, exhibition).await {
                    if count > 0 {
                        return Ok(count);
                    }
                }
            }
        }

        Ok(0)
    }

    /// Extract booth table (booth# | company name format)
    async fn extract_booth_table(&self, document: &Html, exhibition: &Exhibition) -> Result<usize> {
        let row_sel = Selector::parse("tr").unwrap();
        let cell_sel = Selector::parse("td").unwrap();
        let mut count = 0;

        for row in document.select(&row_sel) {
            let cells: Vec<String> = row
                .select(&cell_sel)
                .map(|c| c.text().collect::<String>().trim().to_string())
                .collect();

            if cells.len() >= 2 {
                // Heuristic: first cell is booth number (short), second is company name
                let booth = &cells[0];
                let company = &cells[1];

                if booth.len() <= 10 && company.len() > 2 && company.len() < 200 {
                    let booth_num = if booth.chars().any(|c| c.is_alphanumeric()) {
                        Some(booth.clone())
                    } else {
                        None
                    };
                    count += self.store_exhibitor(company, exhibition, booth_num.as_deref()).await.unwrap_or(0);
                }
            }
        }

        Ok(count)
    }

    async fn store_exhibitor(&self, name: &str, exhibition: &Exhibition, booth_number: Option<&str>) -> Result<usize> {
        let mut company = Company::new(name.to_string());
        company.industry = Some(exhibition.sector.clone());
        company.location = exhibition.city.clone().or(exhibition.location.clone());
        company.country = exhibition.country.clone();

        self.db.upsert_company(&company).await?;

        let mut participation = Participation::new(exhibition.id.clone(), company.id.clone());
        participation.booth_number = booth_number.map(String::from);
        self.db.upsert_participation(&participation).await?;

        Ok(1)
    }
}
