-- ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
-- Skoutt Database Schema
-- SQLite database for Track Exhibits lead generation agent
-- ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- ─────────────────────────────────────────────────────────────────────────
-- Exhibitions discovered by the scraper
-- ─────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS exhibitions (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    sector              TEXT NOT NULL,       -- Tech, Medical, Pharma, Auto
    region              TEXT NOT NULL,       -- Middle East, Europe, Asia Pacific, UK
    start_date          DATE,
    end_date            DATE,
    location            TEXT,
    city                TEXT,
    country             TEXT,
    organizer_name      TEXT,
    organizer_contact   TEXT,
    website_url         TEXT,
    exhibitor_list_url  TEXT,
    source_url          TEXT,               -- Where we found this exhibition
    discovered_at       TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_scraped_at     TIMESTAMP
);

-- ─────────────────────────────────────────────────────────────────────────
-- Companies (exhibitors)
-- ─────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS companies (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    website         TEXT,
    industry        TEXT,
    size            TEXT,                   -- Small (<50), Medium (50-500), Large (500+)
    location        TEXT,
    country         TEXT,
    description     TEXT,                   -- Scraped from their website
    research_summary TEXT,                  -- Claude's research summary
    enriched        BOOLEAN DEFAULT FALSE,
    enriched_at     TIMESTAMP,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ─────────────────────────────────────────────────────────────────────────
-- Exhibition participation (which company is at which show)
-- ─────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS participations (
    id              TEXT PRIMARY KEY,
    exhibition_id   TEXT NOT NULL,
    company_id      TEXT NOT NULL,
    booth_number    TEXT,
    discovered_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (exhibition_id) REFERENCES exhibitions(id),
    FOREIGN KEY (company_id) REFERENCES companies(id),
    UNIQUE(exhibition_id, company_id)
);

-- ─────────────────────────────────────────────────────────────────────────
-- Contacts (decision makers at companies)
-- ─────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS contacts (
    id              TEXT PRIMARY KEY,
    company_id      TEXT NOT NULL,
    full_name       TEXT NOT NULL,
    job_title       TEXT,
    email           TEXT NOT NULL,
    email_verified  BOOLEAN DEFAULT FALSE,
    email_confidence REAL DEFAULT 0.0,      -- Hunter.io confidence score (0-1)
    linkedin_url    TEXT,
    phone           TEXT,
    do_not_contact  BOOLEAN DEFAULT FALSE,  -- Unsubscribed or bounced
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (company_id) REFERENCES companies(id),
    UNIQUE(email)
);

-- ─────────────────────────────────────────────────────────────────────────
-- Email campaigns (sent emails and their outcomes)
-- ─────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS emails_sent (
    id                  TEXT PRIMARY KEY,
    contact_id          TEXT NOT NULL,
    participation_id    TEXT,
    message_id          TEXT UNIQUE,        -- Email Message-ID header for reply matching
    email_type          TEXT NOT NULL,      -- initial, followup_1, followup_2, followup_3
    subject             TEXT NOT NULL,
    body                TEXT NOT NULL,
    sent_at             TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    opened_at           TIMESTAMP,
    clicked_at          TIMESTAMP,
    bounced             BOOLEAN DEFAULT FALSE,
    replied_at          TIMESTAMP,
    reply_body          TEXT,
    reply_sentiment     TEXT,               -- interested, not_interested, neutral, needs_info
    interest_level      TEXT,              -- High, Medium, Low, None (from Claude analysis)
    interest_signals    TEXT,              -- JSON array of signals
    next_step_recommendation TEXT,         -- Claude's recommendation
    followup_count      INTEGER DEFAULT 0,
    followup_scheduled_at TIMESTAMP,
    FOREIGN KEY (contact_id) REFERENCES contacts(id),
    FOREIGN KEY (participation_id) REFERENCES participations(id)
);

-- ─────────────────────────────────────────────────────────────────────────
-- Weekly survival metrics
-- ─────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS weekly_metrics (
    week_start              DATE PRIMARY KEY,
    emails_sent             INTEGER DEFAULT 0,
    emails_opened           INTEGER DEFAULT 0,
    replies_received        INTEGER DEFAULT 0,
    interested_replies      INTEGER DEFAULT 0,  -- KEY SURVIVAL METRIC
    not_interested_replies  INTEGER DEFAULT 0,
    neutral_replies         INTEGER DEFAULT 0,
    bounces                 INTEGER DEFAULT 0,
    new_companies_found     INTEGER DEFAULT 0,
    new_contacts_enriched   INTEGER DEFAULT 0,
    survival_status         TEXT DEFAULT 'safe', -- safe, warning, critical, shutdown
    updated_at              TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ─────────────────────────────────────────────────────────────────────────
-- Survival / death rule event log
-- ─────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS survival_log (
    id          TEXT PRIMARY KEY,
    timestamp   TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    event_type  TEXT NOT NULL,  -- warning_issued, shutdown_triggered, lead_generated, grace_period, strategy_pivot
    week_start  DATE,
    details     TEXT            -- JSON with full context
);

-- ─────────────────────────────────────────────────────────────────────────
-- Scraping cache (avoid re-scraping same URLs)
-- ─────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS scrape_cache (
    url         TEXT PRIMARY KEY,
    content     TEXT NOT NULL,
    scraped_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    expires_at  TIMESTAMP NOT NULL
);

-- ─────────────────────────────────────────────────────────────────────────
-- Research reports (deep intelligence before email drafting)
-- ─────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS research_reports (
    id                      TEXT PRIMARY KEY,
    contact_id              TEXT NOT NULL,
    company_id              TEXT NOT NULL,
    participation_id        TEXT NOT NULL,
    researched_at           TIMESTAMP DEFAULT CURRENT_TIMESTAMP,

    -- Raw research sources
    company_website_summary TEXT NOT NULL DEFAULT '',
    recent_news             TEXT,          -- JSON array of NewsArticle
    previous_exhibitions    TEXT,          -- JSON array of PreviousExhibition

    -- Claude-synthesized intelligence
    company_overview        TEXT NOT NULL DEFAULT '',
    exhibition_strategy     TEXT NOT NULL DEFAULT '',
    pain_points             TEXT NOT NULL DEFAULT '[]',  -- JSON array
    personalization_hooks   TEXT NOT NULL DEFAULT '[]',  -- JSON array
    email_angle             TEXT NOT NULL DEFAULT '',

    -- Quality & caching
    research_quality_score  REAL NOT NULL DEFAULT 0.0,
    sources_used            TEXT NOT NULL DEFAULT '[]',  -- JSON array of source names
    cached_until            TIMESTAMP,                   -- Valid for 30 days

    FOREIGN KEY (contact_id)      REFERENCES contacts(id),
    FOREIGN KEY (company_id)      REFERENCES companies(id),
    FOREIGN KEY (participation_id) REFERENCES participations(id)
);

-- ─────────────────────────────────────────────────────────────────────────
-- Indexes for performance
-- ─────────────────────────────────────────────────────────────────────────
CREATE INDEX IF NOT EXISTS idx_companies_enriched ON companies(enriched);
CREATE INDEX IF NOT EXISTS idx_contacts_company ON contacts(company_id);
CREATE INDEX IF NOT EXISTS idx_contacts_dnc ON contacts(do_not_contact);
CREATE INDEX IF NOT EXISTS idx_emails_contact ON emails_sent(contact_id);
CREATE INDEX IF NOT EXISTS idx_emails_sent_at ON emails_sent(sent_at);
CREATE INDEX IF NOT EXISTS idx_emails_followup ON emails_sent(followup_scheduled_at);
CREATE INDEX IF NOT EXISTS idx_emails_message_id ON emails_sent(message_id);
CREATE INDEX IF NOT EXISTS idx_participations_exhibition ON participations(exhibition_id);
CREATE INDEX IF NOT EXISTS idx_exhibitions_sector_region ON exhibitions(sector, region);
CREATE INDEX IF NOT EXISTS idx_scrape_cache_expires ON scrape_cache(expires_at);
CREATE INDEX IF NOT EXISTS idx_research_contact ON research_reports(contact_id);
CREATE INDEX IF NOT EXISTS idx_research_cached ON research_reports(cached_until);
