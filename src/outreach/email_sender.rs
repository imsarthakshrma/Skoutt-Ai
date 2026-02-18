// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — outreach/email_sender.rs
// SMTP email sending with rate limiting and safety checks
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use chrono::Utc;
use lettre::{
    message::{header, Mailbox, MultiPart, SinglePart},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    Contact, EmailConfig, EmailRecord, EmailType,
    database::Database,
    intelligence::email_personalizer::EmailDraft,
};

pub struct EmailSender {
    config: EmailConfig,
    dry_run: bool,
    daily_sent: Arc<Mutex<u32>>,
    last_sent: Arc<Mutex<Option<chrono::DateTime<Utc>>>>,
}

impl EmailSender {
    pub fn new(config: EmailConfig, dry_run: bool) -> Self {
        Self {
            config,
            dry_run,
            daily_sent: Arc::new(Mutex::new(0)),
            last_sent: Arc::new(Mutex::new(None)),
        }
    }

    /// Send an email to a contact, respecting all rate limits
    pub async fn send_email(
        &self,
        contact: &Contact,
        draft: &EmailDraft,
        db: &Database,
    ) -> Result<()> {
        // Check DNC
        if contact.do_not_contact {
            return Err(anyhow::anyhow!("Contact {} is on DNC list", contact.email));
        }

        // Check daily limit
        {
            let sent = self.daily_sent.lock().await;
            if *sent >= self.config.daily_limit {
                return Err(anyhow::anyhow!("Daily email limit ({}) reached", self.config.daily_limit));
            }
        }

        // Enforce minimum interval between sends
        {
            let last = self.last_sent.lock().await;
            if let Some(last_time) = *last {
                let elapsed = (Utc::now() - last_time).num_seconds() as u64;
                if elapsed < self.config.min_send_interval_seconds {
                    let wait = self.config.min_send_interval_seconds - elapsed;
                    info!("  Rate limit: waiting {}s before next send", wait);
                    tokio::time::sleep(tokio::time::Duration::from_secs(wait)).await;
                }
            }
        }

        let message_id = format!("<{}.skoutt@trackexhibits.com>", Uuid::new_v4());

        // Add unsubscribe footer
        let body_with_footer = format!(
            "{}\n\n---\nTo unsubscribe from Track Exhibits outreach, reply with 'unsubscribe' in the subject.",
            draft.body
        );

        if self.dry_run {
            info!("  [DRY RUN] Would send to {} <{}>", contact.full_name, contact.email);
            info!("  [DRY RUN] Subject: {}", draft.subject);
        } else {
            // Build email message
            let from: Mailbox = format!("{} <{}>", self.config.from_name, self.config.from_email)
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid from address: {}", e))?;

            let to: Mailbox = format!("{} <{}>", contact.full_name, contact.email)
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid to address: {}", e))?;

            let email = Message::builder()
                .from(from)
                .to(to)
                .subject(&draft.subject)
                .header(header::ContentType::TEXT_PLAIN)
                .body(body_with_footer.clone())
                .map_err(|e| anyhow::anyhow!("Failed to build email: {}", e))?;

            // Send via SMTP
            let creds = Credentials::new(
                self.config.smtp_user.clone(),
                self.config.smtp_password.clone(),
            );

            let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.config.smtp_host)?
                .port(self.config.smtp_port)
                .credentials(creds)
                .build();

            mailer.send(email).await
                .map_err(|e| anyhow::anyhow!("SMTP send failed: {}", e))?;
        }

        // Record in database
        let record = EmailRecord {
            id: Uuid::new_v4().to_string(),
            contact_id: contact.id.clone(),
            participation_id: None,
            message_id: Some(message_id),
            email_type: EmailType::Initial.as_str().to_string(),
            subject: draft.subject.clone(),
            body: body_with_footer,
            sent_at: Utc::now(),
            bounced: false,
            replied_at: None,
            reply_body: None,
            reply_sentiment: None,
            interest_level: None,
            interest_signals: None,
            next_step_recommendation: None,
            followup_count: 0,
            followup_scheduled_at: Some(Utc::now() + chrono::Duration::days(3)),
        };

        db.insert_email_record(&record).await?;

        // Update counters
        {
            let mut sent = self.daily_sent.lock().await;
            *sent += 1;
        }
        {
            let mut last = self.last_sent.lock().await;
            *last = Some(Utc::now());
        }

        // Update weekly metrics
        let week_start = current_week_start();
        db.get_or_create_weekly_metrics(week_start).await?;
        db.increment_weekly_metric(week_start, "emails_sent").await?;

        Ok(())
    }

    /// Reset daily counter (called at start of each day)
    pub async fn reset_daily_counter(&self) {
        let mut sent = self.daily_sent.lock().await;
        *sent = 0;
    }
}

fn current_week_start() -> chrono::NaiveDate {
    use chrono::Datelike;
    let today = Utc::now().date_naive();
    let days_since_monday = today.weekday().num_days_from_monday();
    today - chrono::Duration::days(days_since_monday as i64)
}
