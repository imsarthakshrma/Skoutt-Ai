"""
Skoutt — python/enrichment/enricher.py
Orchestrates Apollo + Hunter enrichment for a company.
Called from Rust via PyO3 bridge.
"""

import logging
from typing import Optional
from urllib.parse import urlparse

from .apollo import ApolloClient
from .hunter import HunterClient

logger = logging.getLogger(__name__)


def enrich_company(
    company_name: str,
    company_website: str,
    company_id: str,
    apollo_key: str,
    hunter_key: str,
) -> list[dict]:
    """
    Main enrichment entry point called from Rust via PyO3.
    Returns a list of contact dicts.
    """
    apollo = ApolloClient(apollo_key)
    hunter = HunterClient(hunter_key)

    # Extract domain from website
    domain = _extract_domain(company_website)

    # Step 1: Find people via Apollo
    people = apollo.search_people(company_name, domain=domain)
    contacts = apollo.extract_contacts(people, company_id)

    # Step 2: If Apollo found nothing, try Hunter domain search
    if not contacts and domain:
        logger.info(f"Apollo found no contacts for {company_name}, trying Hunter...")
        hunter_emails = hunter.find_emails(domain)
        for entry in hunter_emails:
            contacts.append({
                "company_id": company_id,
                "full_name": "Unknown",
                "email": entry["email"],
                "job_title": None,
                "linkedin_url": None,
                "phone": None,
                "confidence": entry["confidence"],
                "email_verified": entry["confidence"] > 0.5,
            })

    # Step 3: Verify emails via Hunter (skip if already from Hunter)
    if apollo_key and hunter_key:
        for contact in contacts:
            if contact.get("confidence", 0) < 0.5:
                verification = hunter.verify_email(contact["email"])
                contact["confidence"] = verification["confidence"]
                contact["email_verified"] = verification["status"] == "valid"
                contact["is_generic"] = verification["is_generic"]

    # Filter out low-confidence and generic emails
    contacts = [
        c for c in contacts
        if c.get("confidence", 0) > 0.3 and not c.get("is_generic", False)
    ]

    logger.info(f"Enriched {company_name}: {len(contacts)} contacts found")
    return contacts


def _extract_domain(website: str) -> Optional[str]:
    if not website:
        return None
    try:
        parsed = urlparse(website if "://" in website else f"https://{website}")
        host = parsed.netloc or parsed.path
        return host.lstrip("www.").split("/")[0] or None
    except Exception:
        return None
