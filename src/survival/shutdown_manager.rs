// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — survival/shutdown_manager.rs
// Death rule enforcement — the heart of Skoutt's survival pressure
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use chrono::{Datelike, Utc};
use tracing::{error, info, warn};

use crate::{SurvivalConfig, database::Database};
use super::{SurvivalReport, SurvivalStatus, alert_system::AlertSystem};

pub struct ShutdownManager {
    config: SurvivalConfig,
    db: Database,
    alert_system: AlertSystem,
}

impl ShutdownManager {
    pub fn new(config: SurvivalConfig, db: Database, alert_system: AlertSystem) -> Self {
        Self { config, db, alert_system }
    }

    /// Check if Skoutt has been permanently shut down
    pub async fn check_status(&self) -> Result<SurvivalReport> {
        // Check if shutdown was already triggered
        let shutdown_logged = self.db.has_shutdown_event().await?;
        if shutdown_logged {
            return Ok(SurvivalReport {
                status: SurvivalStatus::Shutdown,
                weeks_active: 0,
                consecutive_zero_weeks: self.config.shutdown_threshold,
                interested_this_week: 0,
                total_emails_sent: 0,
                message: "Skoutt has been permanently shut down (death rule triggered).".to_string(),
            });
        }

        self.calculate_survival_status().await
    }

    /// Run the daily survival check and take action
    pub async fn run_survival_check(&self) -> Result<SurvivalReport> {
        let report = self.calculate_survival_status().await?;

        let week_start = current_week_start();
        let status_str = match &report.status {
            SurvivalStatus::GracePeriod => "safe",
            SurvivalStatus::Safe => "safe",
            SurvivalStatus::Warning => "warning",
            SurvivalStatus::Critical => "critical",
            SurvivalStatus::Shutdown => "shutdown",
        };

        self.db.update_survival_status(week_start, status_str).await?;

        match &report.status {
            SurvivalStatus::GracePeriod => {
                info!("  🌱 Grace period — building pipeline (week {} of {})",
                    report.weeks_active, self.config.grace_period_weeks);
            }
            SurvivalStatus::Safe => {
                info!("  ✅ Survival secure — {} interested replies this week", report.interested_this_week);
            }
            SurvivalStatus::Warning => {
                warn!("  ⚠️  WARNING: {} consecutive weeks with 0 interested replies", report.consecutive_zero_weeks);
                self.db.log_survival_event(
                    "warning_issued",
                    Some(week_start),
                    &serde_json::to_string(&report).unwrap_or_default(),
                ).await?;

                if self.alert_system.config.alert_on_warning {
                    self.alert_system.send_warning_alert(&report).await?;
                }
            }
            SurvivalStatus::Critical => {
                error!("  🔴 CRITICAL: {} consecutive weeks with 0 interested replies — attempting emergency pivots", report.consecutive_zero_weeks);
                self.db.log_survival_event(
                    "critical_status",
                    Some(week_start),
                    &serde_json::to_string(&report).unwrap_or_default(),
                ).await?;
            }
            SurvivalStatus::Shutdown => {
                error!("  💀 DEATH RULE TRIGGERED — {} consecutive weeks with 0 interested replies", report.consecutive_zero_weeks);
                self.db.log_survival_event(
                    "shutdown_triggered",
                    Some(week_start),
                    &serde_json::to_string(&report).unwrap_or_default(),
                ).await?;

                if self.alert_system.config.alert_on_shutdown {
                    self.alert_system.send_shutdown_alert(&report).await?;
                }
            }
        }

        Ok(report)
    }

    async fn calculate_survival_status(&self) -> Result<SurvivalReport> {
        let recent_metrics = self.db.get_recent_weekly_metrics(
            (self.config.shutdown_threshold + 2) as i64
        ).await?;

        let weeks_active = recent_metrics.len() as u32;
        let current_week = current_week_start();

        // Get this week's interested replies
        let interested_this_week = recent_metrics.first()
            .filter(|m| m.week_start == current_week)
            .map(|m| m.interested_replies)
            .unwrap_or(0);

        // Total emails sent ever
        let total_emails_sent: i64 = recent_metrics.iter().map(|m| m.emails_sent).sum();

        // Grace period check
        if weeks_active < self.config.grace_period_weeks {
            return Ok(SurvivalReport {
                status: SurvivalStatus::GracePeriod,
                weeks_active,
                consecutive_zero_weeks: 0,
                interested_this_week,
                total_emails_sent,
                message: format!("Grace period: week {} of {}", weeks_active, self.config.grace_period_weeks),
            });
        }

        // Count consecutive weeks with zero interested replies
        let mut consecutive_zero_weeks = 0u32;
        for metrics in &recent_metrics {
            if metrics.interested_replies == 0 {
                consecutive_zero_weeks += 1;
            } else {
                break; // Reset on any week with interested replies
            }
        }

        let status = if consecutive_zero_weeks >= self.config.shutdown_threshold {
            SurvivalStatus::Shutdown
        } else if consecutive_zero_weeks >= self.config.warning_threshold {
            SurvivalStatus::Critical
        } else if consecutive_zero_weeks >= 1 && weeks_active > self.config.grace_period_weeks {
            SurvivalStatus::Warning
        } else {
            SurvivalStatus::Safe
        };

        let message = match &status {
            SurvivalStatus::Safe => format!("{} interested replies this week — survival secured", interested_this_week),
            SurvivalStatus::Warning => format!("{} consecutive weeks with 0 interested replies — WARNING", consecutive_zero_weeks),
            SurvivalStatus::Critical => format!("{} consecutive weeks with 0 interested replies — CRITICAL", consecutive_zero_weeks),
            SurvivalStatus::Shutdown => format!("SHUTDOWN: {} consecutive weeks with 0 interested replies exceeded threshold of {}", consecutive_zero_weeks, self.config.shutdown_threshold),
            _ => String::new(),
        };

        Ok(SurvivalReport {
            status,
            weeks_active,
            consecutive_zero_weeks,
            interested_this_week,
            total_emails_sent,
            message,
        })
    }
}

fn current_week_start() -> chrono::NaiveDate {
    let today = Utc::now().date_naive();
    let days_since_monday = today.weekday().num_days_from_monday();
    today - chrono::Duration::days(days_since_monday as i64)
}
