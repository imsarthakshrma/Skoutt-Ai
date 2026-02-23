// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — outreach/email_drafter.rs
// Orchestrates Claude-powered email drafting.
// Uses ResearchReport when available; falls back to basic personalisation.
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use tracing::info;

use crate::{
    Company, CompanyConfig, Contact, EmailRecord, EmailType, Participation,
    database::Database,
    intelligence::{
        deep_researcher::ResearchReport,
        email_personalizer::{EmailDraft, EmailPersonalizer},
    },
};

pub struct EmailDrafter {
    personalizer: EmailPersonalizer,
}

impl EmailDrafter {
    pub fn new(api_key: String, model: String, company_config: CompanyConfig) -> Self {
        Self {
            personalizer: EmailPersonalizer::new(api_key, model, company_config),
        }
    }

    /// Draft an initial outreach email.
    /// When `research` is provided, uses the deep research-enhanced path for
    /// genuinely personalized output. Falls back to basic when research is None.
    pub async fn draft_initial_email(
        &self,
        contact: &Contact,
        company: &Company,
        participation: Option<&Participation>,
        db: &Database,
        research: Option<&ResearchReport>,
    ) -> Result<EmailDraft> {
        // Resolve exhibition name once
        let exhibition_name = if let Some(part) = participation {
            db.get_exhibition_name(&part.exhibition_id).await.ok().flatten()
        } else {
            None
        };

        if let Some(report) = research {
            info!("    ✨ Using deep research for {}", contact.email);
            self.personalizer.draft_researched_email(
                contact,
                company,
                participation,
                exhibition_name.as_deref(),
                report,
            ).await
        } else {
            info!("    ⚡ Using basic personalisation for {}", contact.email);
            self.personalizer.personalize_initial_email(
                contact,
                company,
                participation,
                exhibition_name.as_deref(),
                db,
            ).await
        }
    }

    /// Draft a follow-up email based on the original
    pub async fn draft_followup(
        &self,
        contact: &Contact,
        original: &EmailRecord,
        followup_type: &EmailType,
        db: &Database,
    ) -> Result<EmailDraft> {
        let company = db.get_company(&contact.company_id).await?
            .ok_or_else(|| anyhow::anyhow!("Company not found for contact {}", contact.id))?;

        let first_name = contact.full_name.split_whitespace().next().unwrap_or("there");
        let industry = company.industry.as_deref().unwrap_or("your industry");

        let (subject, body) = match followup_type {
            EmailType::Followup1 => {
                let subject = format!("Following up — {}", original.subject.trim_start_matches("Re: "));
                let body = format!(
                    "Hi {},\n\nJust wanted to circle back on my earlier note about your upcoming exhibition.\n\nHave you locked in a booth partner yet? We've been doing a lot of work with {} companies lately — design, fabrication, the whole setup on-site.\n\nWould love to share some ideas if you're still exploring. Happy to jump on a quick call this week.\n\nCheers,\nScott\nTrack Exhibits",
                    first_name, industry
                );
                (subject, body)
            }
            EmailType::Followup2 => {
                let subject = format!("Some booth ideas for {} companies", industry);
                let body = format!(
                    "Hey {},\n\nThought I'd share a few recent projects we've done for {} companies at trade shows — might spark some ideas for your booth.\n\nOur approach: we handle booth design, fabrication, and the full installation on-site. One person to talk to throughout, which keeps things simple.\n\nWorth a quick chat? I can pull up examples relevant to what you're working on.\n\nBest,\nScott\nTrack Exhibits",
                    first_name, industry
                );
                (subject, body)
            }
            EmailType::Followup3 => {
                let subject = "Last check-in before the exhibition".to_string();
                let body = format!(
                    "Hi {},\n\nLast follow-up from me — if your booth design is already sorted, totally understand!\n\nBut if you're still looking, we do quick turnarounds. Design to on-site install, even on tight timelines.\n\nEither way, best of luck at the show.\n\nTalk soon,\nScott\nTrack Exhibits",
                    first_name
                );
                (subject, body)
            }
            EmailType::Initial => {
                return self.draft_initial_email(contact, &company, None, db, None).await;
            }
        };

        Ok(EmailDraft { subject, body })
    }
}
