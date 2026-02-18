// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — enrichment/apollo_client.rs
// Apollo.io API integration for finding decision makers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{database::Database, Company, Contact};

const APOLLO_BASE_URL: &str = "https://api.apollo.io/v1";

/// Target job titles in priority order
const TARGET_TITLES: &[&str] = &[
    "Marketing Director",
    "Director of Marketing",
    "Events Manager",
    "Event Manager",
    "Trade Show Manager",
    "Brand Manager",
    "Head of Marketing",
    "VP Marketing",
    "VP of Marketing",
    "Chief Marketing Officer",
    "CMO",
    "CEO",
    "Managing Director",
    "General Manager",
];

pub struct ApolloClient {
    api_key: String,
    client: Client,
}

#[derive(Debug, Deserialize)]
struct ApolloSearchResponse {
    people: Option<Vec<ApolloPerson>>,
    organizations: Option<Vec<ApolloOrganization>>,
}

#[derive(Debug, Deserialize)]
struct ApolloPerson {
    id: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    name: Option<String>,
    title: Option<String>,
    email: Option<String>,
    linkedin_url: Option<String>,
    phone_numbers: Option<Vec<ApolloPhone>>,
    organization: Option<ApolloOrganization>,
}

#[derive(Debug, Deserialize)]
struct ApolloOrganization {
    id: Option<String>,
    name: Option<String>,
    website_url: Option<String>,
    num_employees: Option<u32>,
    estimated_num_employees: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ApolloPhone {
    raw_number: Option<String>,
}

impl ApolloClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }

    /// Find decision makers at a company via Apollo.io
    pub async fn enrich_company(&self, company: &Company, db: &Database) -> Result<Vec<Contact>> {
        let domain = self.extract_domain(company.website.as_deref());

        // Search for people at this company
        let people = self.search_people(company, domain.as_deref()).await?;

        let mut contacts = Vec::new();
        for person in people {
            if let Some(email) = &person.email {
                if email.is_empty() || email.contains("@example") {
                    continue;
                }

                let full_name = person.name.clone()
                    .or_else(|| {
                        let first = person.first_name.as_deref().unwrap_or("");
                        let last = person.last_name.as_deref().unwrap_or("");
                        if first.is_empty() && last.is_empty() {
                            None
                        } else {
                            Some(format!("{} {}", first, last).trim().to_string())
                        }
                    })
                    .unwrap_or_else(|| "Unknown".to_string());

                let mut contact = Contact::new(
                    company.id.clone(),
                    full_name,
                    email.clone(),
                );
                contact.job_title = person.title.clone();
                contact.linkedin_url = person.linkedin_url.clone();
                contact.phone = person.phone_numbers
                    .as_ref()
                    .and_then(|phones| phones.first())
                    .and_then(|p| p.raw_number.clone());

                contacts.push(contact);
            }
        }

        info!("  Apollo: {} contacts found for {}", contacts.len(), company.name);
        Ok(contacts)
    }

    async fn search_people(&self, company: &Company, domain: Option<&str>) -> Result<Vec<ApolloPerson>> {
        let mut body = serde_json::json!({
            "api_key": self.api_key,
            "per_page": 10,
            "person_titles": TARGET_TITLES,
        });

        if let Some(domain) = domain {
            body["q_organization_domains"] = serde_json::json!([domain]);
        } else {
            body["q_organization_name"] = serde_json::json!(company.name);
        }

        let response = self.client
            .post(format!("{}/mixed_people/search", APOLLO_BASE_URL))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            warn!("Apollo API error {}: {}", status, &text[..text.len().min(200)]);
            return Ok(vec![]);
        }

        let data: ApolloSearchResponse = response.json().await?;
        Ok(data.people.unwrap_or_default())
    }

    fn extract_domain(&self, website: Option<&str>) -> Option<String> {
        let website = website?;
        let url = url::Url::parse(website).ok()?;
        let host = url.host_str()?;
        // Remove www. prefix
        Some(host.trim_start_matches("www.").to_string())
    }
}
