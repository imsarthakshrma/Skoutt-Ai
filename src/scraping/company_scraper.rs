// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — scraping/company_scraper.rs
// Scrapes company websites for research context
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use scraper::{Html, Selector};

use crate::Company;
use super::Scraper;

pub struct CompanyScraper {
    scraper: Scraper,
}

impl CompanyScraper {
    pub fn new(scraper: Scraper) -> Self {
        Self { scraper }
    }

    /// Scrape a company website and return a text summary of what they do
    pub async fn scrape_company(&self, company: &Company) -> Result<String> {
        let website = company.website.as_deref().ok_or_else(|| {
            anyhow::anyhow!("No website for company {}", company.name)
        })?;

        let html = self.scraper.fetch(website).await?;
        let document = Html::parse_document(&html);

        let mut content_parts = Vec::new();

        // Extract meta description
        if let Ok(meta_sel) = Selector::parse("meta[name='description']") {
            if let Some(meta) = document.select(&meta_sel).next() {
                if let Some(content) = meta.value().attr("content") {
                    content_parts.push(format!("Description: {}", content.trim()));
                }
            }
        }

        // Extract title
        if let Ok(title_sel) = Selector::parse("title") {
            if let Some(title) = document.select(&title_sel).next() {
                let title_text = title.text().collect::<String>();
                content_parts.push(format!("Title: {}", title_text.trim()));
            }
        }

        // Extract main headings (h1, h2)
        for tag in &["h1", "h2"] {
            if let Ok(sel) = Selector::parse(tag) {
                let headings: Vec<String> = document
                    .select(&sel)
                    .take(5)
                    .map(|el| el.text().collect::<String>().trim().to_string())
                    .filter(|s| !s.is_empty() && s.len() < 200)
                    .collect();
                if !headings.is_empty() {
                    content_parts.push(format!("{}: {}", tag.to_uppercase(), headings.join(" | ")));
                }
            }
        }

        // Extract about/hero section text
        let content_selectors = [
            ".about-us p",
            ".hero-text p",
            ".intro p",
            "main p",
            "section p",
            ".content p",
        ];

        let mut paragraphs = Vec::new();
        for sel_str in &content_selectors {
            if let Ok(sel) = Selector::parse(sel_str) {
                for p in document.select(&sel).take(3) {
                    let text = p.text().collect::<String>().trim().to_string();
                    if text.len() > 50 && text.len() < 500 {
                        paragraphs.push(text);
                    }
                }
                if !paragraphs.is_empty() {
                    break;
                }
            }
        }

        if !paragraphs.is_empty() {
            content_parts.push(format!("Content: {}", paragraphs.join(" ")));
        }

        // Also try to find "About" page
        if content_parts.len() < 3 {
            if let Ok(about_content) = self.scrape_about_page(website).await {
                content_parts.push(about_content);
            }
        }

        if content_parts.is_empty() {
            return Err(anyhow::anyhow!("Could not extract meaningful content from {}", website));
        }

        Ok(content_parts.join("\n").chars().take(3000).collect())
    }

    async fn scrape_about_page(&self, base_url: &str) -> Result<String> {
        let about_paths = ["/about", "/about-us", "/company", "/who-we-are"];

        let base = url::Url::parse(base_url)?;
        let origin = format!("{}://{}", base.scheme(), base.host_str().unwrap_or(""));

        for path in &about_paths {
            let url = format!("{}{}", origin, path);
            if let Ok(html) = self.scraper.fetch(&url).await {
                let document = Html::parse_document(&html);
                if let Ok(p_sel) = Selector::parse("main p, .about p, article p") {
                    let text: String = document
                        .select(&p_sel)
                        .take(3)
                        .map(|el| el.text().collect::<String>())
                        .collect::<Vec<_>>()
                        .join(" ");
                    if text.len() > 100 {
                        return Ok(format!("About: {}", &text[..text.len().min(1000)]));
                    }
                }
            }
        }

        Err(anyhow::anyhow!("No about page found"))
    }
}
