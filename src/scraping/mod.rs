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
use tracing::warn;

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
        for attempt in 0..self.config.max_retries {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(
                    self.config.request_delay_ms * 2u64.pow(attempt),
                ))
                .await;
            }

            match self.client.get(url).send().await {
                Ok(resp) => {
                    if resp.status() == 429 {
                        warn!("Rate limited by {url}, waiting 60s...");
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        continue;
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
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Max retries exceeded for {url}")))
    }
}
