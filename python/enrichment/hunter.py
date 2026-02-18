"""
Skoutt — python/enrichment/hunter.py
Hunter.io email verification wrapper.
"""

import requests
import logging
from typing import Optional
from urllib.parse import quote

logger = logging.getLogger(__name__)

HUNTER_BASE_URL = "https://api.hunter.io/v2"

GENERIC_PREFIXES = [
    "info@", "contact@", "hello@", "admin@", "support@",
    "sales@", "office@", "enquiries@", "enquiry@", "general@",
]


class HunterClient:
    def __init__(self, api_key: str):
        self.api_key = api_key
        self.session = requests.Session()

    def verify_email(self, email: str) -> dict:
        """
        Verify an email address.
        Returns: {email, status, confidence, is_generic}
        """
        try:
            response = self.session.get(
                f"{HUNTER_BASE_URL}/email-verifier",
                params={"email": email, "api_key": self.api_key},
                timeout=15,
            )
            response.raise_for_status()
            data = response.json().get("data", {})

            result = data.get("result", "unknown")
            score = data.get("score", 0)

            status_map = {
                "deliverable": "valid",
                "undeliverable": "invalid",
                "risky": "risky",
                "unknown": "unknown",
            }

            return {
                "email": email,
                "status": status_map.get(result, "unknown"),
                "confidence": score / 100.0,
                "is_generic": self._is_generic(email),
            }
        except requests.RequestException as e:
            logger.warning(f"Hunter verification failed for {email}: {e}")
            return {
                "email": email,
                "status": "unknown",
                "confidence": 0.0,
                "is_generic": self._is_generic(email),
            }

    def find_emails(self, domain: str, limit: int = 10) -> list[dict]:
        """Find email addresses for a domain."""
        try:
            response = self.session.get(
                f"{HUNTER_BASE_URL}/domain-search",
                params={"domain": domain, "api_key": self.api_key, "limit": limit},
                timeout=15,
            )
            response.raise_for_status()
            data = response.json().get("data", {})
            emails = data.get("emails", [])
            return [
                {"email": e["value"], "confidence": e.get("confidence", 0) / 100.0}
                for e in emails
                if e.get("confidence", 0) > 50
            ]
        except requests.RequestException as e:
            logger.warning(f"Hunter domain search failed for {domain}: {e}")
            return []

    def _is_generic(self, email: str) -> bool:
        email_lower = email.lower()
        return any(email_lower.startswith(prefix) for prefix in GENERIC_PREFIXES)
