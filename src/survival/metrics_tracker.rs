// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — survival/metrics_tracker.rs
// Weekly metrics aggregation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use chrono::{Datelike, Utc};
use tracing::info;

use crate::database::Database;

pub struct MetricsTracker {
    db: Database,
}

impl MetricsTracker {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub async fn record_interested_reply(&self) -> Result<()> {
        let week_start = current_week_start();
        self.db.get_or_create_weekly_metrics(week_start).await?;
        self.db.increment_weekly_metric(week_start, "interested_replies").await?;
        self.db.log_survival_event(
            "lead_generated",
            Some(week_start),
            r#"{"message": "Interested reply received — survival secured for this week"}"#,
        ).await?;
        info!("  📊 Interested reply recorded — survival counter reset");
        Ok(())
    }

    pub async fn record_reply(&self, sentiment: &str) -> Result<()> {
        let week_start = current_week_start();
        self.db.get_or_create_weekly_metrics(week_start).await?;
        self.db.increment_weekly_metric(week_start, "replies_received").await?;
        Ok(())
    }

    pub async fn get_summary(&self) -> Result<String> {
        let metrics = self.db.get_recent_weekly_metrics(4).await?;
        let mut lines = vec!["Weekly Performance Summary:".to_string()];

        for m in &metrics {
            let open_rate = if m.emails_sent > 0 {
                (m.emails_opened as f64 / m.emails_sent as f64 * 100.0) as u32
            } else { 0 };

            let reply_rate = if m.emails_sent > 0 {
                (m.replies_received as f64 / m.emails_sent as f64 * 100.0) as u32
            } else { 0 };

            lines.push(format!(
                "  Week {}: {} sent | {}% open | {}% reply | {} interested | Status: {}",
                m.week_start, m.emails_sent, open_rate, reply_rate,
                m.interested_replies, m.survival_status
            ));
        }

        Ok(lines.join("\n"))
    }
}

fn current_week_start() -> chrono::NaiveDate {
    let today = Utc::now().date_naive();
    let days_since_monday = today.weekday().num_days_from_monday();
    today - chrono::Duration::days(days_since_monday as i64)
}
