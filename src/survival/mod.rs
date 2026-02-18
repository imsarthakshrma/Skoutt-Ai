// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — survival/mod.rs
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub mod alert_system;
pub mod metrics_tracker;
pub mod shutdown_manager;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SurvivalStatus {
    GracePeriod,
    Safe,
    Warning,
    Critical,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurvivalReport {
    pub status: SurvivalStatus,
    pub weeks_active: u32,
    pub consecutive_zero_weeks: u32,
    pub interested_this_week: i64,
    pub total_emails_sent: i64,
    pub message: String,
}

impl SurvivalReport {
    pub fn is_shutdown(&self) -> bool {
        self.status == SurvivalStatus::Shutdown
    }
}
