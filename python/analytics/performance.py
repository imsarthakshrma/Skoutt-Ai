"""
Skoutt — python/analytics/performance.py
Weekly performance report generation.
"""

import sqlite3
import logging
from datetime import date, timedelta
from typing import Optional

logger = logging.getLogger(__name__)


def generate_report(db_path: str) -> str:
    """
    Generate a weekly performance report from the SQLite database.
    Called from Rust via PyO3.
    """
    try:
        conn = sqlite3.connect(db_path)
        conn.row_factory = sqlite3.Row
        cursor = conn.cursor()

        # Get last 4 weeks of metrics
        cursor.execute("""
            SELECT week_start, emails_sent, emails_opened, replies_received,
                   interested_replies, survival_status
            FROM weekly_metrics
            ORDER BY week_start DESC
            LIMIT 4
        """)
        weeks = cursor.fetchall()

        # Get total companies and contacts
        cursor.execute("SELECT COUNT(*) as count FROM companies")
        total_companies = cursor.fetchone()["count"]

        cursor.execute("SELECT COUNT(*) as count FROM contacts")
        total_contacts = cursor.fetchone()["count"]

        cursor.execute("SELECT COUNT(*) as count FROM exhibitions")
        total_exhibitions = cursor.fetchone()["count"]

        cursor.execute("""
            SELECT COUNT(*) as count FROM emails_sent
            WHERE interest_level IN ('High', 'Medium')
        """)
        total_interested = cursor.fetchone()["count"]

        conn.close()

        # Build report
        lines = [
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
            "SKOUTT WEEKLY PERFORMANCE REPORT",
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
            "",
            f"DATABASE TOTALS:",
            f"  Exhibitions tracked:  {total_exhibitions}",
            f"  Companies found:      {total_companies}",
            f"  Contacts enriched:    {total_contacts}",
            f"  Total interested:     {total_interested}",
            "",
            "WEEKLY BREAKDOWN:",
        ]

        for week in weeks:
            sent = week["emails_sent"] or 0
            opened = week["emails_opened"] or 0
            replies = week["replies_received"] or 0
            interested = week["interested_replies"] or 0
            status = week["survival_status"] or "unknown"

            open_rate = f"{opened/sent*100:.1f}%" if sent > 0 else "N/A"
            reply_rate = f"{replies/sent*100:.1f}%" if sent > 0 else "N/A"

            lines.extend([
                f"",
                f"  Week of {week['week_start']}:",
                f"    Sent:       {sent}",
                f"    Open rate:  {open_rate}",
                f"    Reply rate: {reply_rate}",
                f"    Interested: {interested}",
                f"    Status:     {status.upper()}",
            ])

        lines.extend([
            "",
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
        ])

        return "\n".join(lines)

    except Exception as e:
        logger.error(f"Failed to generate report: {e}")
        return f"Report generation failed: {e}"
