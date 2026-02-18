// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — outreach/email_drafter.rs
// Orchestrates Claude-powered email drafting
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use tracing::info;

use crate::{
    Company, CompanyConfig, Contact, EmailRecord, EmailType, Participation,
    database::Database,
    intelligence::email_personalizer::{EmailDraft, EmailPersonalizer},
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

    /// Draft an initial outreach email for a contact
    pub async fn draft_initial_email(
        &self,
        contact: &Contact,
        company: &Company,
        participation: Option<&Participation>,
        db: &Database,
    ) -> Result<EmailDraft> {
        // Get exhibition name if we have a participation
        let exhibition_name = if let Some(part) = participation {
            db.get_exhibition_name(&part.exhibition_id).await.ok().flatten()
        } else {
            None
        };

        self.personalizer.personalize_initial_email(
            contact,
            company,
            participation,
            exhibition_name.as_deref(),
            db,
        ).await
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

        let (subject, body) = match followup_type {
            EmailType::Followup1 => {
                let subject = format!("Following up — {}", original.subject.trim_start_matches("Re: "));
                let body = format!(
                    "Hi {},\n\nWanted to follow up on my message about your upcoming exhibition booth.\n\nHave you had a chance to finalize your design partner? We'd love to show you what we've done for similar {} companies.\n\nHappy to jump on a quick call this week if useful.\n\nBest regards,\nTrack Exhibits",
                    contact.full_name.split_whitespace().next().unwrap_or("there"),
                    company.industry.as_deref().unwrap_or("industry")
                );
                (subject, body)
            }
            EmailType::Followup2 => {
                let subject = format!("Portfolio examples for {} companies", company.industry.as_deref().unwrap_or("your sector"));
                let body = format!(
                    "Hi {},\n\nSharing some recent work we've done for {} companies at trade shows — thought it might be relevant for your upcoming exhibition.\n\nOur process: 3D design mockup → fabrication → on-site setup. One point of contact throughout.\n\nWould a brief call make sense? I can share specific examples relevant to your booth requirements.\n\nBest,\nTrack Exhibits",
                    contact.full_name.split_whitespace().next().unwrap_or("there"),
                    company.industry.as_deref().unwrap_or("your sector")
                );
                (subject, body)
            }
            EmailType::Followup3 => {
                let subject = "Last check-in before the exhibition".to_string();
                let body = format!(
                    "Hi {},\n\nFinal follow-up from Track Exhibits. If your booth design is already sorted, no worries at all!\n\nIf you're still exploring options, we specialize in quick turnarounds for companies that need quality booths on tight timelines.\n\nEither way, best of luck at the exhibition.\n\nBest,\nTrack Exhibits",
                    contact.full_name.split_whitespace().next().unwrap_or("there")
                );
                (subject, body)
            }
            EmailType::Initial => {
                return self.draft_initial_email(contact, &company, None, db).await;
            }
        };

        Ok(EmailDraft { subject, body })
    }
}
