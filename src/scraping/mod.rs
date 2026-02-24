// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — scraping/mod.rs
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub mod company_scraper;
pub mod exhibitor_extractor;
pub mod exhibition_finder;
pub mod fallback_strategies;

use anyhow::Result;
use reqwest::Client;
use std::time::Duration;
use tracing::{info, warn};

use crate::ScrapingConfig;

/// Shared HTTP client with rate limiting and caching support
pub struct Scraper {
    pub client: Client,
    pub config: ScrapingConfig,
}

impl Scraper {
    pub fn new(config: ScrapingConfig) -> Result<Self> {
        let client = Client::builder()
            .user_agent(&config.user_agent)
            .timeout(Duration::from_secs(30))
            .cookie_store(true)
            .build()?;

        Ok(Self { client, config })
    }

    /// Fetch a URL with rate limiting and retry logic
    pub async fn fetch(&self, url: &str) -> Result<String> {
        let mut last_err = None;
        let mut got_403 = false;
        for attempt in 0..self.config.max_retries {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(
                    self.config.request_delay_ms * 2u64.pow(attempt),
                ))
                .await;
            }

            // Use browser-like headers to avoid 403 blocks from sites like 10times.com
            let request = self.client.get(url)
                .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8")
                .header("Accept-Language", "en-US,en;q=0.9")
                .header("Accept-Encoding", "gzip, deflate, br")
                .header("Sec-Fetch-Dest", "document")
                .header("Sec-Fetch-Mode", "navigate")
                .header("Sec-Fetch-Site", "none")
                .header("Sec-Fetch-User", "?1")
                .header("Upgrade-Insecure-Requests", "1")
                .header("Cache-Control", "max-age=0");

            match request.send().await {
                Ok(resp) => {
                    if resp.status() == 429 {
                        warn!("Rate limited by {url}, waiting 60s...");
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                    if resp.status() == 403 {
                        got_403 = true;
                        last_err = Some(anyhow::anyhow!("HTTP 403 Forbidden"));
                        break; // Don't retry 403s, fall through to Crawl4AI
                    }
                    if resp.status().is_success() {
                        let text = resp.text().await?;
                        // Respect delay between requests
                        tokio::time::sleep(Duration::from_millis(self.config.request_delay_ms)).await;
                        return Ok(text);
                    }
                    last_err = Some(anyhow::anyhow!("HTTP {}", resp.status()));
                }
                Err(e) => {
                    last_err = Some(e.into());
                }
            }
        }

        // Crawl4AI fallback for 403 (Cloudflare/anti-bot blocked sites)
        if got_403 {
            warn!("  reqwest got 403 for {url}, trying Crawl4AI headless browser...");
            match crate::python_bridge::crawl4ai_bridge::fetch_html_via_crawl4ai(url).await {
                Ok(html) => {
                    info!("  ✓ Crawl4AI fallback succeeded for {url}");
                    tokio::time::sleep(Duration::from_millis(self.config.request_delay_ms)).await;
                    return Ok(html);
                }
                Err(e) => {
                    warn!("  Crawl4AI fallback also failed for {url}: {e}");
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Max retries exceeded for {url}")))
    }
}
