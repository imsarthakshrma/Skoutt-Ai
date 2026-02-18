// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — enrichment/hunter_client.rs
// Hunter.io email verification
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use tracing::warn;

const HUNTER_BASE_URL: &str = "https://api.hunter.io/v2";

pub struct HunterClient {
    api_key: String,
    client: Client,
}

#[derive(Debug)]
pub struct EmailVerification {
    pub email: String,
    pub status: VerificationStatus,
    pub confidence: f64,
    pub is_generic: bool,
}

#[derive(Debug, PartialEq)]
pub enum VerificationStatus {
    Valid,
    Invalid,
    Risky,
    Unknown,
}

#[derive(Debug, Deserialize)]
struct HunterVerifyResponse {
    data: Option<HunterVerifyData>,
    errors: Option<Vec<HunterError>>,
}

#[derive(Debug, Deserialize)]
struct HunterVerifyData {
    result: Option<String>,    // "deliverable", "undeliverable", "risky", "unknown"
    score: Option<f64>,
    regexp: Option<bool>,
    gibberish: Option<bool>,
    disposable: Option<bool>,
    webmail: Option<bool>,
    mx_records: Option<bool>,
    smtp_server: Option<bool>,
    smtp_check: Option<bool>,
    accept_all: Option<bool>,
    block: Option<bool>,
    sources: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct HunterError {
    details: Option<String>,
}

impl HunterClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }

    /// Verify an email address via Hunter.io
    pub async fn verify_email(&self, email: &str) -> Result<EmailVerification> {
        let url = format!(
            "{}/email-verifier?email={}&api_key={}",
            HUNTER_BASE_URL,
            urlencoding::encode(email),
            self.api_key
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            warn!("Hunter API error {} for {}", status, email);
            return Ok(EmailVerification {
                email: email.to_string(),
                status: VerificationStatus::Unknown,
                confidence: 0.0,
                is_generic: self.is_generic_email(email),
            });
        }

        let data: HunterVerifyResponse = response.json().await?;

        if let Some(errors) = data.errors {
            if !errors.is_empty() {
                warn!("Hunter errors for {}: {:?}", email, errors.first().and_then(|e| e.details.as_ref()));
            }
        }

        let verification = if let Some(d) = data.data {
            let status = match d.result.as_deref() {
                Some("deliverable") => VerificationStatus::Valid,
                Some("undeliverable") => VerificationStatus::Invalid,
                Some("risky") => VerificationStatus::Risky,
                _ => VerificationStatus::Unknown,
            };

            let confidence = d.score.unwrap_or(0.0) / 100.0; // Hunter returns 0-100

            EmailVerification {
                email: email.to_string(),
                status,
                confidence,
                is_generic: self.is_generic_email(email),
            }
        } else {
            EmailVerification {
                email: email.to_string(),
                status: VerificationStatus::Unknown,
                confidence: 0.0,
                is_generic: self.is_generic_email(email),
            }
        };

        Ok(verification)
    }

    /// Find email addresses for a domain
    pub async fn find_emails(&self, domain: &str) -> Result<Vec<String>> {
        let url = format!(
            "{}/domain-search?domain={}&api_key={}&limit=10",
            HUNTER_BASE_URL, domain, self.api_key
        );

        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Ok(vec![]);
        }

        #[derive(Deserialize)]
        struct DomainResponse {
            data: Option<DomainData>,
        }
        #[derive(Deserialize)]
        struct DomainData {
            emails: Option<Vec<EmailEntry>>,
        }
        #[derive(Deserialize)]
        struct EmailEntry {
            value: Option<String>,
            confidence: Option<u32>,
        }

        let data: DomainResponse = response.json().await?;
        let emails = data.data
            .and_then(|d| d.emails)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.confidence.unwrap_or(0) > 50)
            .filter_map(|e| e.value)
            .collect();

        Ok(emails)
    }

    fn is_generic_email(&self, email: &str) -> bool {
        let generic_prefixes = [
            "info@", "contact@", "hello@", "admin@", "support@",
            "sales@", "office@", "enquiries@", "enquiry@", "general@",
        ];
        let email_lower = email.to_lowercase();
        generic_prefixes.iter().any(|prefix| email_lower.starts_with(prefix))
    }
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                '@' => "%40".to_string(),
                '+' => "%2B".to_string(),
                _ => format!("%{:02X}", c as u32),
            })
            .collect()
    }
}
