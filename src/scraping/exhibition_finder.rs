// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — scraping/exhibition_finder.rs
// Discovers exhibitions from:
//   1. Seed list (config/seed_exhibitions.toml) — curated known exhibitions
//   2. Google search + Crawl4AI (when SerpAPI/Google API keys are available)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use chrono::{NaiveDate, Utc};
use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::{database::Database, Exhibition, ScrapingConfig, TargetingConfig};

// ── Seed exhibition TOML format ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SeedFile {
    exhibition: Vec<SeedExhibition>,
}

#[derive(Debug, Deserialize)]
struct SeedExhibition {
    name: String,
    sector: String,
    region: String,
    location: Option<String>,
    website: Option<String>,
    start_date: Option<String>,
}

// ── ExhibitionFinder ─────────────────────────────────────────────────────

pub struct ExhibitionFinder {
    targeting: TargetingConfig,
    db: Database,
    _config: ScrapingConfig,
}

impl ExhibitionFinder {
    pub fn new(config: ScrapingConfig, targeting: TargetingConfig, db: Database) -> Self {
        Self {
            targeting,
            db,
            _config: config,
        }
    }

    /// Main discovery method
    /// 1. Load seed exhibitions from config/seed_exhibitions.toml
    /// 2. (Future: Google search / SerpAPI for additional exhibitions)
    pub async fn discover_exhibitions(&self) -> Result<usize> {
        let mut total = 0;

        // ── Source 1: Seed exhibitions ────────────────────────────────
        match self.load_seed_exhibitions().await {
            Ok(count) => {
                total += count;
                info!("    {} exhibitions loaded from seed list", count);
            }
            Err(e) => warn!("    Failed to load seed exhibitions: {}", e),
        }

        // ── Source 2: Google search (future — needs SerpAPI key) ─────
        // When research.sources.serp_api_key is set, we can search for:
        //   "upcoming {sector} exhibition {region} {year}"
        // and parse results for additional exhibitions.

        Ok(total)
    }

    /// Load exhibitions from config/seed_exhibitions.toml
    async fn load_seed_exhibitions(&self) -> Result<usize> {
        let seed_path = "config/seed_exhibitions.toml";
        let content = tokio::fs::read_to_string(seed_path).await.map_err(|e| {
            anyhow::anyhow!(
                "Cannot read {}: {} — create it from the template",
                seed_path,
                e
            )
        })?;

        let seed_file: SeedFile =
            toml::from_str(&content).map_err(|e| anyhow::anyhow!("Invalid TOML in {}: {}", seed_path, e))?;

        let mut count = 0;

        for seed in &seed_file.exhibition {
            // Check if this sector+region matches our targeting config
            if !self.targeting.sectors.iter().any(|s| s == &seed.sector) {
                continue;
            }
            if !self.targeting.regions.iter().any(|r| r == &seed.region) {
                continue;
            }

            // Parse the start date
            let start_date = seed.start_date.as_deref().and_then(|d| {
                NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()
            });

            // Lead-time window filter
            if !self.is_in_lead_window(start_date) {
                continue;
            }

            let mut exhibition = Exhibition::new(
                seed.name.clone(),
                seed.sector.clone(),
                seed.region.clone(),
            );
            exhibition.start_date = start_date;
            exhibition.location = seed.location.clone();
            exhibition.website_url = seed.website.clone();
            exhibition.source_url = Some("seed_exhibitions.toml".to_string());

            match self.db.upsert_exhibition(&exhibition).await {
                Ok(_) => {
                    count += 1;
                    debug!("    ✓ {} ({}, {})", seed.name, seed.sector, seed.region);
                }
                Err(e) => warn!("    Failed to store '{}': {}", seed.name, e),
            }
        }

        Ok(count)
    }

    /// Check if an event's start date falls within the lead-time window.
    /// Events without a parsed date are kept (we err on the side of including).
    fn is_in_lead_window(&self, start_date: Option<NaiveDate>) -> bool {
        let Some(date) = start_date else {
            return true; // no date parsed — keep it
        };
        let today = Utc::now().date_naive();
        let days_until = (date - today).num_days();

        if days_until < self.targeting.lead_time_min_days {
            debug!(
                "    ⏭ {} — {} days away (min {}), too late",
                date, days_until, self.targeting.lead_time_min_days
            );
            return false;
        }
        if days_until > self.targeting.lead_time_max_days {
            debug!(
                "    ⏭ {} — {} days away (max {}), too far out",
                date, days_until, self.targeting.lead_time_max_days
            );
            return false;
        }
        true
    }
}
