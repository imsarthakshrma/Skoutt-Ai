// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — survival/alert_system.rs
// Email alerts for interested leads, warnings, and shutdown
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use lettre::{
    message::header,
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use tracing::info;

use crate::{AlertsConfig, Company, Contact, EmailConfig};
use crate::intelligence::reply_analyzer::{IncomingReply, ReplyAnalysis};
use super::SurvivalReport;

#[derive(Clone)]
pub struct AlertSystem {
    email_config: EmailConfig,
    pub config: AlertsConfig,
    dry_run: bool,
}

impl AlertSystem {
    pub fn new(email_config: EmailConfig, config: AlertsConfig, dry_run: bool) -> Self {
        Self { email_config, config, dry_run }
    }

    /// Send immediate alert when an interested lead is detected.
    /// Includes full deep research briefing + conversation thread for internal handoff.
    pub async fn send_interested_lead_alert(
        &self,
        contact: &Contact,
        company: &Company,
        reply: &IncomingReply,
        analysis: &ReplyAnalysis,
        research_briefing: Option<&str>,
        conversation_thread: Option<&str>,
    ) -> Result<()> {
        let subject = format!("🚨 HOT LEAD — {} at {}", contact.full_name, company.name);

        let signals_text = analysis.signals
            .iter()
            .map(|s| format!("  • {}", s))
            .collect::<Vec<_>>()
            .join("\n");

        let research_section = if let Some(briefing) = research_briefing {
            format!(
                r#"━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
DEEP RESEARCH BRIEFING:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

{}
"#,
                briefing
            )
        } else {
            format!(
                "COMPANY BACKGROUND:\n{}\n",
                company.research_summary.as_deref().unwrap_or("No research available")
            )
        };

        let thread_section = if let Some(thread) = conversation_thread {
            format!(
                r#"━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
FULL CONVERSATION THREAD:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

{}
"#,
                thread
            )
        } else {
            String::new()
        };

        let body = format!(
            r#"━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
🚨  INTERESTED LEAD — ACTION REQUIRED
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Company:   {}
Contact:   {}
Title:     {}
Email:     {}
Phone:     {}

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
THEIR REPLY:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

{}

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
INTEREST ANALYSIS:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Interest Level: {}
Key Signals:
{}

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
RECOMMENDED NEXT STEP:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

{}

{}
{}
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

⚠️  Scott has paused automated follow-ups to this contact.
📧  Reply directly to: {}
📞  Their phone: {}

— Skoutt Agent (automated internal alert)"#,
            company.name,
            contact.full_name,
            contact.job_title.as_deref().unwrap_or("Unknown"),
            contact.email,
            contact.phone.as_deref().unwrap_or("Not available"),
            reply.body,
            analysis.interest_level_str(),
            signals_text,
            analysis.next_step,
            research_section,
            thread_section,
            contact.email,
            contact.phone.as_deref().unwrap_or("Not available"),
        );

        self.send_alert_email(&subject, &body).await?;
        info!("  🚨 Interested lead alert sent for {} at {}", contact.full_name, company.name);
        Ok(())
    }

    /// Send warning alert when approaching death rule threshold
    pub async fn send_warning_alert(&self, report: &SurvivalReport) -> Result<()> {
        let subject = format!(
            "⚠️ Skoutt Warning: {} consecutive weeks with 0 interested leads",
            report.consecutive_zero_weeks
        );

        let body = format!(
            r#"━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
SKOUTT SURVIVAL WARNING
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Skoutt has gone {} consecutive weeks without generating an interested lead.

Current Status: WARNING
Shutdown Threshold: {} consecutive weeks

Total emails sent: {}
Weeks active: {}

Skoutt is automatically:
  • Increasing daily email volume to maximum (60/day)
  • Testing different subject lines
  • Trying partnership outreach angle

If no interested leads are generated in {} more week(s), Skoutt will shut down permanently.

— Skoutt Agent"#,
            report.consecutive_zero_weeks,
            3, // shutdown threshold
            report.total_emails_sent,
            report.weeks_active,
            3u32.saturating_sub(report.consecutive_zero_weeks),
        );

        self.send_alert_email(&subject, &body).await
    }

    /// Send final shutdown notification
    pub async fn send_shutdown_alert(&self, report: &SurvivalReport) -> Result<()> {
        let subject = "💀 Skoutt Agent — Shutdown Triggered (Death Rule)".to_string();

        let body = format!(
            r#"━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
SKOUTT AGENT SHUTDOWN
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Skoutt has been permanently shut down after {} consecutive weeks with zero interested leads.

FINAL STATISTICS:
  Total emails sent:     {}
  Weeks active:          {}
  Interested replies:    0 (for {} weeks)

DEATH RULE: {} consecutive weeks with 0 interested replies = permanent shutdown.

This is final. Skoutt will not restart automatically.

To restart, you must manually re-initialize the agent after reviewing what went wrong.

Check data/logs/ for detailed activity logs.

— Skoutt Agent (Final Message)"#,
            report.consecutive_zero_weeks,
            report.total_emails_sent,
            report.weeks_active,
            report.consecutive_zero_weeks,
            3, // shutdown threshold
        );

        self.send_alert_email(&subject, &body).await
    }

    async fn send_alert_email(&self, subject: &str, body: &str) -> Result<()> {
        if self.dry_run {
            info!("  [DRY RUN] Alert: {}", subject);
            return Ok(());
        }

        let from: lettre::message::Mailbox = format!("Skoutt Agent <{}>", self.email_config.from_email)
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid from: {}", e))?;

        let to: lettre::message::Mailbox = self.config.user_email
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid to: {}", e))?;

        let email = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .header(header::ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .map_err(|e| anyhow::anyhow!("Failed to build alert email: {}", e))?;

        let creds = Credentials::new(
            self.email_config.smtp_user.clone(),
            self.email_config.smtp_password.clone(),
        );

        let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.email_config.smtp_host)?
            .port(self.email_config.smtp_port)
            .credentials(creds)
            .build();

        mailer.send(email).await
            .map_err(|e| anyhow::anyhow!("Alert email send failed: {}", e))?;

        Ok(())
    }
}
