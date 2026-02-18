"""
Skoutt — python/intelligence/sentiment.py
Lightweight keyword-based sentiment analysis for email replies.
No heavy ML dependencies required — pure Python.
"""

import re
import logging
from typing import NamedTuple

logger = logging.getLogger(__name__)

# Positive signals — indicates interest
INTERESTED_SIGNALS = [
    r"\binterested\b",
    r"\bwould like to\b",
    r"\bcan we schedule\b",
    r"\blet'?s talk\b",
    r"\bplease send\b",
    r"\btell me more\b",
    r"\bhow much\b",
    r"\bwhat are your prices?\b",
    r"\bwhen can you\b",
    r"\bsounds good\b",
    r"\byes please\b",
    r"\bhappy to discuss\b",
    r"\bwould love to\b",
    r"\bcan you send\b",
    r"\bmore information\b",
    r"\bget in touch\b",
    r"\bschedule a call\b",
    r"\bbook a meeting\b",
    r"\bquote\b",
    r"\bproposal\b",
    r"\bportfolio\b",
    r"\bprevious work\b",
    r"\bexamples\b",
]

# Negative signals — not interested
NOT_INTERESTED_SIGNALS = [
    r"\bnot interested\b",
    r"\bno thank you\b",
    r"\bno thanks\b",
    r"\bnot relevant\b",
    r"\bwrong person\b",
    r"\bplease remove\b",
    r"\bunsubscribe\b",
    r"\bdo not contact\b",
    r"\bnot looking for\b",
    r"\balready have\b",
    r"\bnot in our budget\b",
    r"\bnot applicable\b",
    r"\bnot the right fit\b",
    r"\bnot at this time\b",
    r"\bpassed\b",
    r"\bdecline\b",
    r"\bno need\b",
    r"\bsorry\b.*\bnot\b",
]

# Neutral / needs info signals
NEEDS_INFO_SIGNALS = [
    r"\bmore details\b",
    r"\bwhat exactly\b",
    r"\bcan you clarify\b",
    r"\bwhat services\b",
    r"\bwhat regions\b",
    r"\bwhat countries\b",
    r"\bdo you cover\b",
    r"\bdo you work in\b",
]


class SentimentResult(NamedTuple):
    sentiment: str  # "interested" | "not_interested" | "needs_info" | "neutral"
    confidence: float
    matched_signals: list[str]


def analyze(text: str) -> str:
    """
    Analyze reply text and return sentiment string.
    Called from Rust via PyO3.
    """
    result = analyze_detailed(text)
    return result.sentiment


def analyze_detailed(text: str) -> SentimentResult:
    """Full sentiment analysis with confidence and matched signals."""
    text_lower = text.lower()

    interested_matches = _match_signals(text_lower, INTERESTED_SIGNALS)
    not_interested_matches = _match_signals(text_lower, NOT_INTERESTED_SIGNALS)
    needs_info_matches = _match_signals(text_lower, NEEDS_INFO_SIGNALS)

    interested_score = len(interested_matches)
    not_interested_score = len(not_interested_matches)
    needs_info_score = len(needs_info_matches)

    # Not interested takes priority — check first and decisively
    if not_interested_score > 0 and not_interested_score >= interested_score:
        confidence = min(0.9, 0.5 + not_interested_score * 0.2)
        return SentimentResult("not_interested", confidence, not_interested_matches)

    # Strong interest signals
    if interested_score >= 2:
        confidence = min(0.95, 0.6 + interested_score * 0.1)
        return SentimentResult("interested", confidence, interested_matches)

    # Needs more info
    if needs_info_score > 0 and not_interested_score == 0:
        return SentimentResult("needs_info", 0.7, needs_info_matches)

    # Single interest signal
    if interested_score == 1:
        return SentimentResult("interested", 0.55, interested_matches)

    # Mixed or ambiguous
    return SentimentResult("neutral", 0.4, [])


def _match_signals(text: str, patterns: list[str]) -> list[str]:
    """Return list of matched signal patterns."""
    matched = []
    for pattern in patterns:
        if re.search(pattern, text, re.IGNORECASE):
            matched.append(pattern.replace(r"\b", "").strip())
    return matched
