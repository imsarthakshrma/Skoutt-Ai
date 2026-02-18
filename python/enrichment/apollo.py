"""
Skoutt — python/enrichment/apollo.py
Apollo.io API wrapper for finding decision makers.
"""

import requests
import logging
from typing import Optional

logger = logging.getLogger(__name__)

APOLLO_BASE_URL = "https://api.apollo.io/v1"

TARGET_TITLES = [
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
]


class ApolloClient:
    def __init__(self, api_key: str):
        self.api_key = api_key
        self.session = requests.Session()
        self.session.headers.update({"Content-Type": "application/json"})

    def search_people(
        self,
        company_name: str,
        domain: Optional[str] = None,
        per_page: int = 10,
    ) -> list[dict]:
        """Find decision makers at a company."""
        payload = {
            "api_key": self.api_key,
            "per_page": per_page,
            "person_titles": TARGET_TITLES,
        }

        if domain:
            payload["q_organization_domains"] = [domain]
        else:
            payload["q_organization_name"] = company_name

        try:
            response = self.session.post(
                f"{APOLLO_BASE_URL}/mixed_people/search",
                json=payload,
                timeout=30,
            )
            response.raise_for_status()
            data = response.json()
            return data.get("people", [])
        except requests.RequestException as e:
            logger.error(f"Apollo API error for {company_name}: {e}")
            return []

    def extract_contacts(self, people: list[dict], company_id: str) -> list[dict]:
        """Convert Apollo people to Skoutt contact format."""
        contacts = []
        for person in people:
            email = person.get("email")
            if not email or "@example" in email:
                continue

            first = person.get("first_name", "")
            last = person.get("last_name", "")
            full_name = person.get("name") or f"{first} {last}".strip() or "Unknown"

            phones = person.get("phone_numbers", [])
            phone = phones[0].get("raw_number") if phones else None

            contacts.append({
                "company_id": company_id,
                "full_name": full_name,
                "email": email,
                "job_title": person.get("title"),
                "linkedin_url": person.get("linkedin_url"),
                "phone": phone,
                "confidence": 0.7,  # Apollo emails are generally reliable
                "email_verified": True,
            })

        return contacts
