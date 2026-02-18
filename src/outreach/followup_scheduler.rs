// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — outreach/followup_scheduler.rs
// Determines which contacts need follow-up emails
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use tracing::info;

use crate::{Contact, EmailRecord, EmailType, database::Database};

pub struct FollowupScheduler;

impl FollowupScheduler {
    pub fn new() -> Self {
        Self
    }

    /// Get all contacts due for follow-up today
    pub async fn get_due_followups(
        &self,
        db: &Database,
    ) -> Result<Vec<(Contact, EmailRecord, EmailType)>> {
        let due_emails = db.get_emails_for_followup().await?;
        let mut result = Vec::new();

        for email_record in due_emails {
            if let Ok(Some(contact)) = db.get_contact_by_id(&email_record.contact_id).await {
                if contact.do_not_contact {
                    continue;
                }

                let followup_type = match email_record.followup_count {
                    0 => EmailType::Followup1,
                    1 => EmailType::Followup2,
                    2 => EmailType::Followup3,
                    _ => continue, // Max 3 follow-ups
                };

                result.push((contact, email_record, followup_type));
            }
        }

        info!("  {} follow-ups due today", result.len());
        Ok(result)
    }

    /// Schedule next follow-up after sending
    pub fn next_followup_date(current_followup_count: i64) -> Option<chrono::DateTime<chrono::Utc>> {
        let days = match current_followup_count {
            0 => 3,  // Day 3 after initial
            1 => 4,  // Day 7 total (4 more days after day 3)
            2 => 7,  // Day 14 total (7 more days after day 7)
            _ => return None, // No more follow-ups
        };
        Some(chrono::Utc::now() + chrono::Duration::days(days))
    }
}

impl Default for FollowupScheduler {
    fn default() -> Self {
        Self::new()
    }
}
