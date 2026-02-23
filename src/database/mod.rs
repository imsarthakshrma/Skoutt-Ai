// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — database/mod.rs
// SQLite database layer using sqlx
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::path::Path;
use tracing::info;

use crate::{Company, Contact, EmailRecord, Exhibition, Participation, WeeklyMetrics};

/// Convert a sqlx NaiveDateTime (from SQLite) to DateTime<Utc>
fn naive_to_utc(dt: Option<NaiveDateTime>) -> DateTime<Utc> {
    dt.map(|n| n.and_utc()).unwrap_or_else(Utc::now)
}

/// Convert an optional NaiveDateTime to Option<DateTime<Utc>>
fn naive_to_utc_opt(dt: Option<NaiveDateTime>) -> Option<DateTime<Utc>> {
    dt.map(|n| n.and_utc())
}

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    /// Initialize database, create file if needed, run migrations
    pub async fn new(db_path: &str) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let url = format!("sqlite://{}?mode=rwc", db_path);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;

        let db = Self { pool };
        db.run_migrations().await?;
        Ok(db)
    }

    /// Run schema migrations from embedded SQL
    async fn run_migrations(&self) -> Result<()> {
        let schema = include_str!("schema.sql");
        sqlx::query(schema).execute(&self.pool).await?;
        info!("Database migrations applied");
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Exhibition operations
    // ─────────────────────────────────────────────────────────────────────

    pub async fn upsert_exhibition(&self, ex: &Exhibition) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO exhibitions (id, name, sector, region, start_date, end_date, location, city, country,
                organizer_name, organizer_contact, website_url, exhibitor_list_url, source_url, discovered_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                exhibitor_list_url = excluded.exhibitor_list_url,
                last_scraped_at = CURRENT_TIMESTAMP
            "#,
            ex.id, ex.name, ex.sector, ex.region,
            ex.start_date, ex.end_date, ex.location, ex.city, ex.country,
            ex.organizer_name, ex.organizer_contact, ex.website_url,
            ex.exhibitor_list_url, ex.source_url, ex.discovered_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_exhibitions_by_sector_region(&self, sector: &str, region: &str) -> Result<Vec<Exhibition>> {
        let rows = sqlx::query!(
            r#"SELECT id, name, sector, region, start_date, end_date, location, city, country,
               organizer_name, organizer_contact, website_url, exhibitor_list_url, source_url, discovered_at
               FROM exhibitions WHERE sector = ? AND region = ? ORDER BY start_date ASC"#,
            sector, region
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| Exhibition {
            id: r.id.unwrap_or_default(),
            name: r.name,
            sector: r.sector,
            region: r.region,
            start_date: r.start_date,
            end_date: r.end_date,
            location: r.location,
            city: r.city,
            country: r.country,
            organizer_name: r.organizer_name,
            organizer_contact: r.organizer_contact,
            website_url: r.website_url,
            exhibitor_list_url: r.exhibitor_list_url,
            source_url: r.source_url,
            discovered_at: naive_to_utc(r.discovered_at),
        }).collect())
    }

    /// Fetch a single exhibition by its ID.
    pub async fn get_exhibition(&self, exhibition_id: &str) -> Result<Option<Exhibition>> {
        let row = sqlx::query!(
            r#"SELECT id, name, sector, region, start_date, end_date, location, city, country,
               organizer_name, organizer_contact, website_url, exhibitor_list_url, source_url, discovered_at
               FROM exhibitions WHERE id = ?"#,
            exhibition_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Exhibition {
            id: r.id.unwrap_or_default(),
            name: r.name,
            sector: r.sector,
            region: r.region,
            start_date: r.start_date,
            end_date: r.end_date,
            location: r.location,
            city: r.city,
            country: r.country,
            organizer_name: r.organizer_name,
            organizer_contact: r.organizer_contact,
            website_url: r.website_url,
            exhibitor_list_url: r.exhibitor_list_url,
            source_url: r.source_url,
            discovered_at: naive_to_utc(r.discovered_at),
        }))
    }

    // ─────────────────────────────────────────────────────────────────────
    // Company operations
    // ─────────────────────────────────────────────────────────────────────

    pub async fn upsert_company(&self, company: &Company) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO companies (id, name, website, industry, size, location, country, description, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                website = COALESCE(excluded.website, website),
                industry = COALESCE(excluded.industry, industry),
                description = COALESCE(excluded.description, description)
            "#,
            company.id, company.name, company.website, company.industry,
            company.size, company.location, company.country, company.description, company.created_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_company(&self, id: &str) -> Result<Option<Company>> {
        let row = sqlx::query!(
            "SELECT id, name, website, industry, size, location, country, description, research_summary, enriched, enriched_at, created_at FROM companies WHERE id = ?",
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Company {
            id: r.id.unwrap_or_default(),
            name: r.name,
            website: r.website,
            industry: r.industry,
            size: r.size,
            location: r.location,
            country: r.country,
            description: r.description,
            research_summary: r.research_summary,
            enriched: r.enriched.unwrap_or(false),
            enriched_at: naive_to_utc_opt(r.enriched_at),
            created_at: naive_to_utc(r.created_at),
        }))
    }

    pub async fn get_unenriched_companies(&self, limit: i64) -> Result<Vec<Company>> {
        let rows = sqlx::query!(
            "SELECT id, name, website, industry, size, location, country, description, research_summary, enriched, enriched_at, created_at FROM companies WHERE enriched = 0 LIMIT ?",
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| Company {
            id: r.id.unwrap_or_default(),
            name: r.name,
            website: r.website,
            industry: r.industry,
            size: r.size,
            location: r.location,
            country: r.country,
            description: r.description,
            research_summary: r.research_summary,
            enriched: r.enriched.unwrap_or(false),
            enriched_at: naive_to_utc_opt(r.enriched_at),
            created_at: naive_to_utc(r.created_at),
        }).collect())
    }

    pub async fn mark_company_enriched(&self, id: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE companies SET enriched = 1, enriched_at = CURRENT_TIMESTAMP WHERE id = ?",
            id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_company_research(&self, id: &str, summary: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE companies SET research_summary = ? WHERE id = ?",
            summary, id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Contact operations
    // ─────────────────────────────────────────────────────────────────────

    pub async fn upsert_contact(&self, contact: &Contact) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO contacts (id, company_id, full_name, job_title, email, email_verified, email_confidence, linkedin_url, phone, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(email) DO UPDATE SET
                full_name = excluded.full_name,
                job_title = COALESCE(excluded.job_title, job_title),
                email_verified = excluded.email_verified,
                email_confidence = excluded.email_confidence,
                linkedin_url = COALESCE(excluded.linkedin_url, linkedin_url)
            "#,
            contact.id, contact.company_id, contact.full_name, contact.job_title,
            contact.email, contact.email_verified, contact.email_confidence,
            contact.linkedin_url, contact.phone, contact.created_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_contact_by_email(&self, email: &str) -> Result<Option<Contact>> {
        let row = sqlx::query!(
            "SELECT id, company_id, full_name, job_title, email, email_verified, email_confidence, linkedin_url, phone, do_not_contact, created_at FROM contacts WHERE email = ?",
            email
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Contact {
            id: r.id.unwrap_or_default(),
            company_id: r.company_id,
            full_name: r.full_name,
            job_title: r.job_title,
            email: r.email,
            email_verified: r.email_verified.unwrap_or(false),
            email_confidence: r.email_confidence.unwrap_or(0.0),
            linkedin_url: r.linkedin_url,
            phone: r.phone,
            do_not_contact: r.do_not_contact.unwrap_or(false),
            created_at: naive_to_utc(r.created_at),
        }))
    }

    pub async fn get_contacts_needing_research(&self, limit: i64) -> Result<Vec<Contact>> {
        // Contacts whose company has no research summary yet
        let rows = sqlx::query!(
            r#"
            SELECT c.id, c.company_id, c.full_name, c.job_title, c.email, c.email_verified,
                   c.email_confidence, c.linkedin_url, c.phone, c.do_not_contact, c.created_at
            FROM contacts c
            JOIN companies co ON c.company_id = co.id
            WHERE co.research_summary IS NULL
              AND c.do_not_contact = 0
              AND c.email_verified = 1
            LIMIT ?
            "#,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| Contact {
            id: r.id.unwrap_or_default(),
            company_id: r.company_id,
            full_name: r.full_name,
            job_title: r.job_title,
            email: r.email,
            email_verified: r.email_verified.unwrap_or(false),
            email_confidence: r.email_confidence.unwrap_or(0.0),
            linkedin_url: r.linkedin_url,
            phone: r.phone,
            do_not_contact: r.do_not_contact.unwrap_or(false),
            created_at: naive_to_utc(r.created_at),
        }).collect())
    }

    pub async fn get_contacts_ready_for_outreach(&self, limit: i64) -> Result<Vec<(Contact, Company, Option<Participation>)>> {
        // Contacts with verified email, research done, not yet emailed, not DNC
        let rows = sqlx::query!(
            r#"
            SELECT c.id as contact_id, c.company_id, c.full_name, c.job_title, c.email,
                   c.email_verified, c.email_confidence, c.linkedin_url, c.phone, c.do_not_contact, c.created_at as contact_created,
                   co.id as co_id, co.name as co_name, co.website, co.industry, co.size, co.location, co.country,
                   co.description, co.research_summary, co.enriched, co.enriched_at, co.created_at as co_created,
                   p.id as part_id, p.exhibition_id, p.booth_number, p.discovered_at as part_discovered
            FROM contacts c
            JOIN companies co ON c.company_id = co.id
            LEFT JOIN participations p ON p.company_id = co.id
            WHERE c.do_not_contact = 0
              AND c.email_verified = 1
              AND co.research_summary IS NOT NULL
              AND c.id NOT IN (SELECT contact_id FROM emails_sent)
            LIMIT ?
            "#,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| {
            let contact = Contact {
                id: r.contact_id.unwrap_or_default(),
                company_id: r.company_id.clone(),
                full_name: r.full_name,
                job_title: r.job_title,
                email: r.email,
                email_verified: r.email_verified.unwrap_or(false),
                email_confidence: r.email_confidence.unwrap_or(0.0),
                linkedin_url: r.linkedin_url,
                phone: r.phone,
                do_not_contact: r.do_not_contact.unwrap_or(false),
                created_at: naive_to_utc(r.contact_created),
            };
            let company = Company {
                id: r.co_id.unwrap_or_default(),
                name: r.co_name,
                website: r.website,
                industry: r.industry,
                size: r.size,
                location: r.location,
                country: r.country,
                description: r.description,
                research_summary: r.research_summary,
                enriched: r.enriched.unwrap_or(false),
                enriched_at: naive_to_utc_opt(r.enriched_at),
                created_at: naive_to_utc(r.co_created),
            };
            let participation = r.part_id.map(|pid| Participation {
                id: pid,
                exhibition_id: r.exhibition_id.unwrap_or_default(),
                company_id: r.company_id.clone(),
                booth_number: r.booth_number,
                discovered_at: naive_to_utc(r.part_discovered),
            });
            (contact, company, participation)
        }).collect())
    }

    pub async fn mark_do_not_contact(&self, email: &str) -> Result<()> {
        sqlx::query!("UPDATE contacts SET do_not_contact = 1 WHERE email = ?", email)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Email record operations
    // ─────────────────────────────────────────────────────────────────────

    pub async fn insert_email_record(&self, record: &EmailRecord) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO emails_sent (id, contact_id, participation_id, message_id, email_type, subject, body, sent_at, followup_count, followup_scheduled_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            record.id, record.contact_id, record.participation_id, record.message_id,
            record.email_type, record.subject, record.body, record.sent_at,
            record.followup_count, record.followup_scheduled_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_reply_analysis(&self, email_id: &str, analysis: &crate::intelligence::reply_analyzer::ReplyAnalysis) -> Result<()> {
        let signals_json = serde_json::to_string(&analysis.signals)?;
        let interest_level_str = analysis.interest_level_str();
        sqlx::query!(
            r#"
            UPDATE emails_sent SET
                replied_at = CURRENT_TIMESTAMP,
                reply_sentiment = ?,
                interest_level = ?,
                interest_signals = ?,
                next_step_recommendation = ?
            WHERE id = ?
            "#,
            analysis.sentiment, interest_level_str, signals_json,
            analysis.next_step, email_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_emails_for_followup(&self) -> Result<Vec<EmailRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, contact_id, participation_id, message_id, email_type, subject, body, sent_at,
                   bounced, replied_at, reply_body, reply_sentiment, interest_level, interest_signals,
                   next_step_recommendation, followup_count, followup_scheduled_at
            FROM emails_sent
            WHERE followup_scheduled_at <= CURRENT_TIMESTAMP
              AND replied_at IS NULL
              AND bounced = 0
              AND followup_count < 3
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| EmailRecord {
            id: r.id.unwrap_or_default(),
            contact_id: r.contact_id,
            participation_id: r.participation_id,
            message_id: r.message_id,
            email_type: r.email_type,
            subject: r.subject,
            body: r.body,
            sent_at: naive_to_utc(r.sent_at),
            bounced: r.bounced.unwrap_or(false),
            replied_at: naive_to_utc_opt(r.replied_at),
            reply_body: r.reply_body,
            reply_sentiment: r.reply_sentiment,
            interest_level: r.interest_level,
            interest_signals: r.interest_signals,
            next_step_recommendation: r.next_step_recommendation,
            followup_count: r.followup_count.unwrap_or(0),
            followup_scheduled_at: naive_to_utc_opt(r.followup_scheduled_at),
        }).collect())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Weekly metrics
    // ─────────────────────────────────────────────────────────────────────

    pub async fn get_or_create_weekly_metrics(&self, week_start: chrono::NaiveDate) -> Result<WeeklyMetrics> {
        sqlx::query!(
            "INSERT OR IGNORE INTO weekly_metrics (week_start) VALUES (?)",
            week_start
        )
        .execute(&self.pool)
        .await?;

        let row = sqlx::query!(
            "SELECT week_start, emails_sent, emails_opened, replies_received, interested_replies, not_interested_replies, neutral_replies, bounces, new_companies_found, new_contacts_enriched, survival_status FROM weekly_metrics WHERE week_start = ?",
            week_start
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(WeeklyMetrics {
            week_start: row.week_start.unwrap_or_default(),
            emails_sent: row.emails_sent.unwrap_or(0),
            emails_opened: row.emails_opened.unwrap_or(0),
            replies_received: row.replies_received.unwrap_or(0),
            interested_replies: row.interested_replies.unwrap_or(0),
            not_interested_replies: row.not_interested_replies.unwrap_or(0),
            neutral_replies: row.neutral_replies.unwrap_or(0),
            bounces: row.bounces.unwrap_or(0),
            new_companies_found: row.new_companies_found.unwrap_or(0),
            new_contacts_enriched: row.new_contacts_enriched.unwrap_or(0),
            survival_status: row.survival_status.unwrap_or_else(|| "safe".to_string()),
        })
    }

    pub async fn increment_weekly_metric(&self, week_start: chrono::NaiveDate, field: &str) -> Result<()> {
        // Safe field names only
        let sql = match field {
            "emails_sent" => "UPDATE weekly_metrics SET emails_sent = emails_sent + 1 WHERE week_start = ?",
            "interested_replies" => "UPDATE weekly_metrics SET interested_replies = interested_replies + 1 WHERE week_start = ?",
            "replies_received" => "UPDATE weekly_metrics SET replies_received = replies_received + 1 WHERE week_start = ?",
            "bounces" => "UPDATE weekly_metrics SET bounces = bounces + 1 WHERE week_start = ?",
            "new_companies_found" => "UPDATE weekly_metrics SET new_companies_found = new_companies_found + 1 WHERE week_start = ?",
            _ => return Err(anyhow::anyhow!("Unknown metric field: {}", field)),
        };
        sqlx::query(sql).bind(week_start).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn get_recent_weekly_metrics(&self, weeks: i64) -> Result<Vec<WeeklyMetrics>> {
        let rows = sqlx::query!(
            r#"
            SELECT week_start, emails_sent, emails_opened, replies_received, interested_replies,
                   not_interested_replies, neutral_replies, bounces, new_companies_found, new_contacts_enriched, survival_status
            FROM weekly_metrics
            ORDER BY week_start DESC
            LIMIT ?
            "#,
            weeks
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| WeeklyMetrics {
            week_start: r.week_start.unwrap_or_default(),
            emails_sent: r.emails_sent.unwrap_or(0),
            emails_opened: r.emails_opened.unwrap_or(0),
            replies_received: r.replies_received.unwrap_or(0),
            interested_replies: r.interested_replies.unwrap_or(0),
            not_interested_replies: r.not_interested_replies.unwrap_or(0),
            neutral_replies: r.neutral_replies.unwrap_or(0),
            bounces: r.bounces.unwrap_or(0),
            new_companies_found: r.new_companies_found.unwrap_or(0),
            new_contacts_enriched: r.new_contacts_enriched.unwrap_or(0),
            survival_status: r.survival_status.unwrap_or_else(|| "safe".to_string()),
        }).collect())
    }

    pub async fn update_survival_status(&self, week_start: chrono::NaiveDate, status: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE weekly_metrics SET survival_status = ? WHERE week_start = ?",
            status, week_start
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn log_survival_event(&self, event_type: &str, week_start: Option<chrono::NaiveDate>, details: &str) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query!(
            "INSERT INTO survival_log (id, event_type, week_start, details) VALUES (?, ?, ?, ?)",
            id, event_type, week_start, details
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Scrape cache
    // ─────────────────────────────────────────────────────────────────────

    pub async fn get_cached_page(&self, url: &str) -> Result<Option<String>> {
        let row = sqlx::query!(
            "SELECT content FROM scrape_cache WHERE url = ? AND expires_at > CURRENT_TIMESTAMP",
            url
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.content))
    }

    pub async fn cache_page(&self, url: &str, content: &str, ttl_hours: i64) -> Result<()> {
        let expires_at = Utc::now() + chrono::Duration::hours(ttl_hours);
        sqlx::query!(
            "INSERT OR REPLACE INTO scrape_cache (url, content, expires_at) VALUES (?, ?, ?)",
            url, content, expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn cleanup_expired_cache(&self) -> Result<u64> {
        let result = sqlx::query!("DELETE FROM scrape_cache WHERE expires_at <= CURRENT_TIMESTAMP")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Additional helpers
    // ─────────────────────────────────────────────────────────────────────

    /// Get exhibition name by ID (for email context)
    pub async fn get_exhibition_name(&self, exhibition_id: &str) -> Result<Option<String>> {
        let row = sqlx::query!(
            "SELECT name FROM exhibitions WHERE id = ?",
            exhibition_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.name))
    }

    /// Get contact by ID
    pub async fn get_contact_by_id(&self, id: &str) -> Result<Option<Contact>> {
        let row = sqlx::query!(
            "SELECT id, company_id, full_name, job_title, email, email_verified, email_confidence, linkedin_url, phone, do_not_contact, created_at FROM contacts WHERE id = ?",
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Contact {
            id: r.id.unwrap_or_default(),
            company_id: r.company_id,
            full_name: r.full_name,
            job_title: r.job_title,
            email: r.email,
            email_verified: r.email_verified.unwrap_or(false),
            email_confidence: r.email_confidence.unwrap_or(0.0),
            linkedin_url: r.linkedin_url,
            phone: r.phone,
            do_not_contact: r.do_not_contact.unwrap_or(false),
            created_at: naive_to_utc(r.created_at),
        }))
    }

    /// Get the most recent email sent to a contact
    pub async fn get_latest_email_for_contact(&self, contact_id: &str) -> Result<Option<EmailRecord>> {
        let row = sqlx::query!(
            r#"
            SELECT id, contact_id, participation_id, message_id, email_type, subject, body, sent_at,
                   bounced, replied_at, reply_body, reply_sentiment, interest_level, interest_signals,
                   next_step_recommendation, followup_count, followup_scheduled_at
            FROM emails_sent
            WHERE contact_id = ?
            ORDER BY sent_at DESC
            LIMIT 1
            "#,
            contact_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| EmailRecord {
            id: r.id.unwrap_or_default(),
            contact_id: r.contact_id,
            participation_id: r.participation_id,
            message_id: r.message_id,
            email_type: r.email_type,
            subject: r.subject,
            body: r.body,
            sent_at: naive_to_utc(r.sent_at),
            bounced: r.bounced.unwrap_or(false),
            replied_at: naive_to_utc_opt(r.replied_at),
            reply_body: r.reply_body,
            reply_sentiment: r.reply_sentiment,
            interest_level: r.interest_level,
            interest_signals: r.interest_signals,
            next_step_recommendation: r.next_step_recommendation,
            followup_count: r.followup_count.unwrap_or(0),
            followup_scheduled_at: naive_to_utc_opt(r.followup_scheduled_at),
        }))
    }

    /// Upsert a participation record
    pub async fn upsert_participation(&self, p: &Participation) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO participations (id, exhibition_id, company_id, booth_number, discovered_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(exhibition_id, company_id) DO UPDATE SET
                booth_number = COALESCE(excluded.booth_number, booth_number)
            "#,
            p.id, p.exhibition_id, p.company_id, p.booth_number, p.discovered_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Check if a permanent shutdown event has been logged
    pub async fn has_shutdown_event(&self) -> Result<bool> {
        let row = sqlx::query!(
            "SELECT COUNT(*) as count FROM survival_log WHERE event_type = 'shutdown_triggered'"
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.count > 0)
    }

    /// Update the next follow-up scheduled time for an email
    pub async fn update_followup_scheduled(&self, email_id: &str, next_date: Option<chrono::DateTime<Utc>>, new_count: i64) -> Result<()> {
        sqlx::query!(
            "UPDATE emails_sent SET followup_scheduled_at = ?, followup_count = ? WHERE id = ?",
            next_date, new_count, email_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Research report operations
    // ─────────────────────────────────────────────────────────────────────

    /// Store a completed research report (upsert by contact_id).
    pub async fn store_research_report(
        &self,
        report: &crate::intelligence::deep_researcher::ResearchReport,
        cache_days: i64,
    ) -> Result<()> {
        let news_json = serde_json::to_string(&report.recent_news).unwrap_or_default();
        let prev_ex_json = serde_json::to_string(&report.previous_exhibitions).unwrap_or_default();
        let pain_json = serde_json::to_string(&report.pain_points).unwrap_or_default();
        let hooks_json = serde_json::to_string(&report.personalization_hooks).unwrap_or_default();
        let sources_json = serde_json::to_string(&report.sources_used).unwrap_or_default();
        let cached_until = Utc::now() + chrono::Duration::days(cache_days);

        sqlx::query!(
            r#"
            INSERT INTO research_reports (
                id, contact_id, company_id, participation_id, researched_at,
                company_website_summary, recent_news, previous_exhibitions,
                company_overview, exhibition_strategy,
                pain_points, personalization_hooks, email_angle,
                research_quality_score, sources_used, cached_until
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                company_website_summary = excluded.company_website_summary,
                recent_news = excluded.recent_news,
                previous_exhibitions = excluded.previous_exhibitions,
                company_overview = excluded.company_overview,
                exhibition_strategy = excluded.exhibition_strategy,
                pain_points = excluded.pain_points,
                personalization_hooks = excluded.personalization_hooks,
                email_angle = excluded.email_angle,
                research_quality_score = excluded.research_quality_score,
                sources_used = excluded.sources_used,
                cached_until = excluded.cached_until,
                researched_at = excluded.researched_at
            "#,
            report.id,
            report.contact_id,
            report.company_id,
            report.participation_id,
            report.researched_at,
            report.company_website_summary,
            news_json,
            prev_ex_json,
            report.company_overview,
            report.exhibition_strategy,
            pain_json,
            hooks_json,
            report.email_angle,
            report.research_quality_score,
            sources_json,
            cached_until,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fetch cached research if still valid (cached_until > now).
    pub async fn get_cached_research(
        &self,
        contact_id: &str,
    ) -> Result<Option<crate::intelligence::deep_researcher::ResearchReport>> {
        let now = Utc::now().naive_utc();
        let row = sqlx::query!(
            r#"
            SELECT id, contact_id, company_id, participation_id, researched_at,
                   company_website_summary, recent_news, previous_exhibitions,
                   company_overview, exhibition_strategy,
                   pain_points, personalization_hooks, email_angle,
                   research_quality_score, sources_used
            FROM research_reports
            WHERE contact_id = ? AND cached_until > ?
            ORDER BY researched_at DESC
            LIMIT 1
            "#,
            contact_id,
            now,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            use crate::intelligence::deep_researcher::{NewsArticle, PreviousExhibition};
            crate::intelligence::deep_researcher::ResearchReport {
                id: r.id.unwrap_or_default(),
                contact_id: r.contact_id,
                company_id: r.company_id,
                participation_id: r.participation_id,
                researched_at: naive_to_utc(r.researched_at),
                company_website_summary: r.company_website_summary,
                recent_news: r.recent_news
                    .and_then(|s| serde_json::from_str::<Vec<NewsArticle>>(&s).ok())
                    .unwrap_or_default(),
                previous_exhibitions: r.previous_exhibitions
                    .and_then(|s| serde_json::from_str::<Vec<PreviousExhibition>>(&s).ok())
                    .unwrap_or_default(),
                company_overview: r.company_overview,
                exhibition_strategy: r.exhibition_strategy,
                pain_points: serde_json::from_str::<Vec<String>>(&r.pain_points)
                    .unwrap_or_default(),
                personalization_hooks: serde_json::from_str::<Vec<String>>(&r.personalization_hooks)
                    .unwrap_or_default(),
                email_angle: r.email_angle,
                research_quality_score: r.research_quality_score,
                sources_used: serde_json::from_str::<Vec<String>>(&r.sources_used)
                    .unwrap_or_default(),
            }
        }))
    }

    /// Fetch research report for a contact for CLI display (ignores cache expiry).
    pub async fn get_research_report(
        &self,
        contact_id: &str,
    ) -> Result<Option<crate::intelligence::deep_researcher::ResearchReport>> {
        let row = sqlx::query!(
            r#"
            SELECT id, contact_id, company_id, participation_id, researched_at,
                   company_website_summary, recent_news, previous_exhibitions,
                   company_overview, exhibition_strategy,
                   pain_points, personalization_hooks, email_angle,
                   research_quality_score, sources_used
            FROM research_reports
            WHERE contact_id = ?
            ORDER BY researched_at DESC
            LIMIT 1
            "#,
            contact_id,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            use crate::intelligence::deep_researcher::{NewsArticle, PreviousExhibition};
            crate::intelligence::deep_researcher::ResearchReport {
                id: r.id.unwrap_or_default(),
                contact_id: r.contact_id,
                company_id: r.company_id,
                participation_id: r.participation_id,
                researched_at: naive_to_utc(r.researched_at),
                company_website_summary: r.company_website_summary,
                recent_news: r.recent_news
                    .and_then(|s| serde_json::from_str::<Vec<NewsArticle>>(&s).ok())
                    .unwrap_or_default(),
                previous_exhibitions: r.previous_exhibitions
                    .and_then(|s| serde_json::from_str::<Vec<PreviousExhibition>>(&s).ok())
                    .unwrap_or_default(),
                company_overview: r.company_overview,
                exhibition_strategy: r.exhibition_strategy,
                pain_points: serde_json::from_str::<Vec<String>>(&r.pain_points)
                    .unwrap_or_default(),
                personalization_hooks: serde_json::from_str::<Vec<String>>(&r.personalization_hooks)
                    .unwrap_or_default(),
                email_angle: r.email_angle,
                research_quality_score: r.research_quality_score,
                sources_used: serde_json::from_str::<Vec<String>>(&r.sources_used)
                    .unwrap_or_default(),
            }
        }))
    }

    /// Previous exhibition participations for a company (for exhibition history).
    /// Returns (event_name, date, location) tuples.
    pub async fn get_past_participations(
        &self,
        company_id: &str,
    ) -> Result<Vec<(String, Option<chrono::NaiveDate>, String)>> {
        let rows = sqlx::query!(
            r#"
            SELECT e.name, e.start_date, e.location
            FROM participations p
            JOIN exhibitions e ON p.exhibition_id = e.id
            WHERE p.company_id = ?
            ORDER BY e.start_date DESC
            "#,
            company_id,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.name,
                    r.start_date,
                    r.location.unwrap_or_else(|| "Unknown".to_string()),
                )
            })
            .collect())
    }
}

