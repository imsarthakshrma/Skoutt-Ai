// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — lib.rs
// Module declarations and shared types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub mod database;
pub mod enrichment;
pub mod intelligence;
pub mod outreach;
pub mod python_bridge;
pub mod scraping;
pub mod survival;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────
// Configuration types (mirrors config/config.toml)
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub company: CompanyConfig,
    pub email: EmailConfig,
    pub imap: ImapConfig,
    pub apis: ApiConfig,
    pub targeting: TargetingConfig,
    pub survival: SurvivalConfig,
    pub alerts: AlertsConfig,
    pub scraping: ScrapingConfig,
    pub database: DatabaseConfig,
    pub logging: LoggingConfig,
    #[serde(default)]
    pub research: ResearchConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResearchConfig {
    /// Master switch — set false to skip deep research entirely
    pub enabled: bool,
    /// Contacts below this score are skipped (no email drafted)
    pub quality_threshold: f64,
    /// How many days to cache research before re-running
    pub cache_duration_days: i64,
    pub sources: ResearchSourcesConfig,
    pub limits: ResearchLimitsConfig,
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            quality_threshold: 0.5,
            cache_duration_days: 30,
            sources: ResearchSourcesConfig::default(),
            limits: ResearchLimitsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResearchSourcesConfig {
    /// SerpAPI key (https://serpapi.com) — optional, enables news search
    pub serp_api_key: Option<String>,
    /// Google Custom Search JSON API key — fallback if SerpAPI not set
    pub google_api_key: Option<String>,
    /// Google Custom Search Engine ID (cx parameter)
    pub google_cx: Option<String>,
    /// LinkedIn integration — stub for future use
    pub linkedin_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResearchLimitsConfig {
    pub max_news_articles: usize,
    pub max_previous_exhibitions: usize,
    /// Seconds before research attempt times out
    pub research_timeout_seconds: u64,
    /// Seconds between research requests (rate limiting)
    pub request_delay_seconds: u64,
}

impl Default for ResearchLimitsConfig {
    fn default() -> Self {
        Self {
            max_news_articles: 5,
            max_previous_exhibitions: 10,
            research_timeout_seconds: 30,
            request_delay_seconds: 2,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompanyConfig {
    pub name: String,
    pub website: String,
    pub services: String,
    pub tagline: String,
    pub regions_served: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmailConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_password: String,
    pub from_name: String,
    pub from_email: String,
    pub daily_limit: u32,
    pub min_send_interval_seconds: u64,
    pub max_per_hour_per_domain: u32,
    pub max_bounce_rate_percent: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub mailbox: String,
    pub poll_interval_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    pub claude_api_key: String,
    pub claude_model: String,
    pub apollo_api_key: String,
    pub hunter_api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetingConfig {
    pub regions: Vec<String>,
    pub sectors: Vec<String>,
    pub target_titles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SurvivalConfig {
    pub grace_period_weeks: u32,
    pub min_interested_per_week: u32,
    pub warning_threshold: u32,
    pub shutdown_threshold: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AlertsConfig {
    pub user_email: String,
    pub alert_on_interested: bool,
    pub alert_on_warning: bool,
    pub alert_on_shutdown: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScrapingConfig {
    pub request_delay_ms: u64,
    pub max_retries: u32,
    pub user_agent: String,
    pub respect_robots_txt: bool,
    pub cache_responses: bool,
    pub cache_ttl_hours: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub log_dir: String,
}

// ─────────────────────────────────────────────────────────────────────────
// Domain types
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exhibition {
    pub id: String,
    pub name: String,
    pub sector: String,
    pub region: String,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
    pub location: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub organizer_name: Option<String>,
    pub organizer_contact: Option<String>,
    pub website_url: Option<String>,
    pub exhibitor_list_url: Option<String>,
    pub source_url: Option<String>,
    pub discovered_at: DateTime<Utc>,
}

impl Exhibition {
    pub fn new(name: String, sector: String, region: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            sector,
            region,
            start_date: None,
            end_date: None,
            location: None,
            city: None,
            country: None,
            organizer_name: None,
            organizer_contact: None,
            website_url: None,
            exhibitor_list_url: None,
            source_url: None,
            discovered_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Company {
    pub id: String,
    pub name: String,
    pub website: Option<String>,
    pub industry: Option<String>,
    pub size: Option<String>,
    pub location: Option<String>,
    pub country: Option<String>,
    pub description: Option<String>,
    pub research_summary: Option<String>,
    pub enriched: bool,
    pub enriched_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl Company {
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            website: None,
            industry: None,
            size: None,
            location: None,
            country: None,
            description: None,
            research_summary: None,
            enriched: false,
            enriched_at: None,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub company_id: String,
    pub full_name: String,
    pub job_title: Option<String>,
    pub email: String,
    pub email_verified: bool,
    pub email_confidence: f64,
    pub linkedin_url: Option<String>,
    pub phone: Option<String>,
    pub do_not_contact: bool,
    pub created_at: DateTime<Utc>,
}

impl Contact {
    pub fn new(company_id: String, full_name: String, email: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            company_id,
            full_name,
            job_title: None,
            email,
            email_verified: false,
            email_confidence: 0.0,
            linkedin_url: None,
            phone: None,
            do_not_contact: false,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participation {
    pub id: String,
    pub exhibition_id: String,
    pub company_id: String,
    pub booth_number: Option<String>,
    pub discovered_at: DateTime<Utc>,
}

impl Participation {
    pub fn new(exhibition_id: String, company_id: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            exhibition_id,
            company_id,
            booth_number: None,
            discovered_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EmailType {
    Initial,
    Followup1,
    Followup2,
    Followup3,
}

impl EmailType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EmailType::Initial => "initial",
            EmailType::Followup1 => "followup_1",
            EmailType::Followup2 => "followup_2",
            EmailType::Followup3 => "followup_3",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Replysentiment {
    Interested,
    NotInterested,
    Neutral,
    NeedsInfo,
}

impl std::fmt::Display for Replysentiment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Replysentiment::Interested => write!(f, "interested"),
            Replysentiment::NotInterested => write!(f, "not_interested"),
            Replysentiment::Neutral => write!(f, "neutral"),
            Replysentiment::NeedsInfo => write!(f, "needs_info"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InterestLevel {
    High,
    Medium,
    Low,
    None,
}

impl std::fmt::Display for InterestLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterestLevel::High => write!(f, "High"),
            InterestLevel::Medium => write!(f, "Medium"),
            InterestLevel::Low => write!(f, "Low"),
            InterestLevel::None => write!(f, "None"),
        }
    }
}

impl InterestLevel {
    pub fn is_actionable(&self) -> bool {
        matches!(self, InterestLevel::High | InterestLevel::Medium)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailRecord {
    pub id: String,
    pub contact_id: String,
    pub participation_id: Option<String>,
    pub message_id: Option<String>,
    pub email_type: String,
    pub subject: String,
    pub body: String,
    pub sent_at: DateTime<Utc>,
    pub bounced: bool,
    pub replied_at: Option<DateTime<Utc>>,
    pub reply_body: Option<String>,
    pub reply_sentiment: Option<String>,
    pub interest_level: Option<String>,
    pub interest_signals: Option<String>,
    pub next_step_recommendation: Option<String>,
    pub followup_count: i64,
    pub followup_scheduled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyMetrics {
    pub week_start: chrono::NaiveDate,
    pub emails_sent: i64,
    pub emails_opened: i64,
    pub replies_received: i64,
    pub interested_replies: i64,
    pub not_interested_replies: i64,
    pub neutral_replies: i64,
    pub bounces: i64,
    pub new_companies_found: i64,
    pub new_contacts_enriched: i64,
    pub survival_status: String,
}

/// Load configuration from the given TOML file path.
/// Environment variables prefixed with SKOUTT_ override file values.
/// Use double underscores for nested keys: SKOUTT_APIS__CLAUDE_API_KEY
pub fn load_config(path: &str) -> Result<AppConfig> {
    // Strip .toml extension if present — config crate adds it automatically
    let path_stripped = path.trim_end_matches(".toml");

    let settings = config::Config::builder()
        .add_source(config::File::with_name(path_stripped).required(false))
        .add_source(
            config::Environment::with_prefix("SKOUTT")
                .separator("__")
                .try_parsing(true),
        )
        .build()?;

    Ok(settings.try_deserialize::<AppConfig>()?)
}
