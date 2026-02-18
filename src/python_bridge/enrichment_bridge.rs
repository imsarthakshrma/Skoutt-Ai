// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — python_bridge/enrichment_bridge.rs
// PyO3 bridge to Python enrichment modules
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use tracing::warn;

use crate::{Company, Contact};

/// Call Python enrichment module to enrich a company
pub fn enrich_company_via_python(company: &Company, apollo_key: &str, hunter_key: &str) -> Result<Vec<Contact>> {
    Python::with_gil(|py| {
        // Import the Python enricher module
        let enricher = py.import("python.enrichment.enricher")
            .map_err(|e| {
                warn!("Python enricher not available: {}", e);
                anyhow::anyhow!("Python enricher import failed: {}", e)
            })?;

        let kwargs = PyDict::new(py);
        kwargs.set_item("company_name", &company.name)?;
        kwargs.set_item("company_website", company.website.as_deref().unwrap_or(""))?;
        kwargs.set_item("company_id", &company.id)?;
        kwargs.set_item("apollo_key", apollo_key)?;
        kwargs.set_item("hunter_key", hunter_key)?;

        let result = enricher
            .call_method("enrich_company", (), Some(kwargs))
            .map_err(|e| anyhow::anyhow!("Python enrichment failed: {}", e))?;

        // Parse result as list of contact dicts
        let contacts_list: Vec<&PyDict> = result.extract()
            .map_err(|e| anyhow::anyhow!("Failed to parse Python result: {}", e))?;

        let contacts = contacts_list.iter().filter_map(|d| {
            let email: String = d.get_item("email").ok()??.extract().ok()?;
            let full_name: String = d.get_item("full_name").ok()??.extract().ok()?;

            let mut contact = Contact::new(company.id.clone(), full_name, email);
            contact.job_title = d.get_item("job_title").ok()?.and_then(|v| v.extract().ok());
            contact.linkedin_url = d.get_item("linkedin_url").ok()?.and_then(|v| v.extract().ok());
            contact.email_confidence = d.get_item("confidence").ok()?
                .and_then(|v| v.extract::<f64>().ok())
                .unwrap_or(0.0);
            contact.email_verified = contact.email_confidence > 0.5;

            Some(contact)
        }).collect();

        Ok(contacts)
    })
}

/// Call Python sentiment analysis for a reply
pub fn analyze_sentiment_via_python(reply_text: &str) -> Result<String> {
    Python::with_gil(|py| {
        let sentiment_module = py.import("python.intelligence.sentiment")
            .map_err(|e| anyhow::anyhow!("Python sentiment module not available: {}", e))?;

        let result = sentiment_module
            .call_method1("analyze", (reply_text,))
            .map_err(|e| anyhow::anyhow!("Sentiment analysis failed: {}", e))?;

        let sentiment: String = result.extract()
            .map_err(|e| anyhow::anyhow!("Failed to extract sentiment: {}", e))?;

        Ok(sentiment)
    })
}
