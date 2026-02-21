# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Skoutt — python/analytics/visualizer.py
# Generates weekly performance reports and optional charts.
# Primary output is a text-based report suitable for email delivery.
# Matplotlib charts are generated only if matplotlib is available.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

from datetime import datetime
from typing import Optional
import json
import os

try:
    import matplotlib
    matplotlib.use("Agg")  # Non-interactive backend
    import matplotlib.pyplot as plt
    import matplotlib.dates as mdates
    HAS_MATPLOTLIB = True
except ImportError:
    HAS_MATPLOTLIB = False


# ── Survival status symbols ─────────────────────────────────────────────

STATUS_SYMBOLS = {
    "safe": "🟢",
    "warning": "🟡",
    "critical": "🔴",
    "shutdown": "💀",
}


class WeeklyReportGenerator:
    """Generates weekly performance reports from metric data."""

    def __init__(self, company_name: str = "Track Exhibits"):
        self.company_name = company_name

    def generate_text_report(self, metrics: list[dict]) -> str:
        """Generate a formatted text report from weekly metrics.

        Args:
            metrics: List of dicts, each with keys:
                - week_start: ISO date string (YYYY-MM-DD)
                - emails_sent, replies_received, interested_replies,
                  bounces, new_companies_found, new_contacts_enriched,
                  survival_status

        Returns:
            Formatted multi-line text report
        """
        if not metrics:
            return "No metrics data available."

        lines = []
        lines.append("━" * 60)
        lines.append(f"  SKOUTT WEEKLY PERFORMANCE REPORT")
        lines.append(f"  {self.company_name}")
        lines.append(f"  Generated: {datetime.utcnow().strftime('%Y-%m-%d %H:%M UTC')}")
        lines.append("━" * 60)
        lines.append("")

        # Latest week summary
        latest = metrics[0]
        status = latest.get("survival_status", "safe")
        symbol = STATUS_SYMBOLS.get(status, "⚪")

        lines.append(f"  Current Status: {symbol} {status.upper()}")
        lines.append("")
        lines.append("  This Week:")
        lines.append(f"    Emails sent:          {latest.get('emails_sent', 0)}")
        lines.append(f"    Replies received:     {latest.get('replies_received', 0)}")
        lines.append(f"    Interested leads:     {latest.get('interested_replies', 0)}")
        lines.append(f"    Bounces:              {latest.get('bounces', 0)}")
        lines.append(f"    New companies found:  {latest.get('new_companies_found', 0)}")
        lines.append(f"    Contacts enriched:    {latest.get('new_contacts_enriched', 0)}")
        lines.append("")

        # Rates
        sent = latest.get("emails_sent", 0)
        if sent > 0:
            reply_rate = (latest.get("replies_received", 0) / sent) * 100
            interest_rate = (latest.get("interested_replies", 0) / sent) * 100
            bounce_rate = (latest.get("bounces", 0) / sent) * 100
            lines.append(f"    Reply rate:           {reply_rate:.1f}%")
            lines.append(f"    Interest rate:        {interest_rate:.1f}%")
            lines.append(f"    Bounce rate:          {bounce_rate:.1f}%")
            lines.append("")

        # Trend (if multiple weeks)
        if len(metrics) > 1:
            lines.append("─" * 60)
            lines.append("  WEEKLY TREND")
            lines.append("")
            lines.append(f"  {'Week':<14} {'Sent':>6} {'Replies':>8} {'Interest':>9} {'Status':>8}")
            lines.append(f"  {'─'*14} {'─'*6} {'─'*8} {'─'*9} {'─'*8}")

            for m in metrics:
                week = m.get("week_start", "?")[:10]
                s_status = m.get("survival_status", "safe")
                s_symbol = STATUS_SYMBOLS.get(s_status, "⚪")
                lines.append(
                    f"  {week:<14} {m.get('emails_sent', 0):>6} "
                    f"{m.get('replies_received', 0):>8} "
                    f"{m.get('interested_replies', 0):>9} "
                    f"{s_symbol:>5} {s_status}"
                )
            lines.append("")

        # Survival warning
        if status == "warning":
            lines.append("─" * 60)
            lines.append("  ⚠️  WARNING: Low interest rate detected.")
            lines.append("  Skoutt needs at least 1 interested reply per week to survive.")
            lines.append("  If this continues, Skoutt will shut down permanently.")
            lines.append("")
        elif status == "critical":
            lines.append("─" * 60)
            lines.append("  🚨  CRITICAL: Shutdown imminent!")
            lines.append("  This is the final warning before permanent shutdown.")
            lines.append("")
        elif status == "shutdown":
            lines.append("─" * 60)
            lines.append("  💀  SHUTDOWN: Skoutt has been permanently disabled.")
            lines.append("  Death rule triggered: 3 consecutive weeks with 0 interest.")
            lines.append("")

        lines.append("━" * 60)
        return "\n".join(lines)

    def generate_chart(
        self,
        metrics: list[dict],
        output_path: str = "data/weekly_report.png",
    ) -> Optional[str]:
        """Generate a matplotlib chart of weekly performance.

        Args:
            metrics: List of weekly metric dicts (same format as text report)
            output_path: File path for the saved PNG chart

        Returns:
            Absolute path to the generated chart, or None if matplotlib unavailable
        """
        if not HAS_MATPLOTLIB:
            return None

        if len(metrics) < 2:
            return None

        # Reverse so oldest is first (for chronological x-axis)
        data = list(reversed(metrics))

        weeks = []
        sent_vals = []
        reply_vals = []
        interest_vals = []
        bounce_vals = []

        for m in data:
            try:
                weeks.append(datetime.strptime(m["week_start"][:10], "%Y-%m-%d"))
            except (KeyError, ValueError):
                continue
            sent_vals.append(m.get("emails_sent", 0))
            reply_vals.append(m.get("replies_received", 0))
            interest_vals.append(m.get("interested_replies", 0))
            bounce_vals.append(m.get("bounces", 0))

        if not weeks:
            return None

        fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(10, 7), sharex=True)
        fig.suptitle("Skoutt Weekly Performance", fontsize=14, fontweight="bold")

        # Top chart: volume
        ax1.bar(weeks, sent_vals, width=4, alpha=0.3, color="#3498db", label="Sent")
        ax1.plot(weeks, reply_vals, "o-", color="#2ecc71", linewidth=2, label="Replies")
        ax1.plot(weeks, interest_vals, "s-", color="#e74c3c", linewidth=2, label="Interested")
        ax1.set_ylabel("Count")
        ax1.legend(loc="upper left")
        ax1.grid(True, alpha=0.3)

        # Bottom chart: rates
        reply_rates = [
            (r / s * 100) if s > 0 else 0
            for s, r in zip(sent_vals, reply_vals)
        ]
        interest_rates = [
            (i / s * 100) if s > 0 else 0
            for s, i in zip(sent_vals, interest_vals)
        ]
        bounce_rates = [
            (b / s * 100) if s > 0 else 0
            for s, b in zip(sent_vals, bounce_vals)
        ]

        ax2.plot(weeks, reply_rates, "o-", color="#2ecc71", linewidth=2, label="Reply %")
        ax2.plot(weeks, interest_rates, "s-", color="#e74c3c", linewidth=2, label="Interest %")
        ax2.plot(weeks, bounce_rates, "^-", color="#95a5a6", linewidth=1, label="Bounce %")
        ax2.set_ylabel("Rate (%)")
        ax2.set_xlabel("Week")
        ax2.legend(loc="upper left")
        ax2.grid(True, alpha=0.3)
        ax2.xaxis.set_major_formatter(mdates.DateFormatter("%b %d"))

        plt.tight_layout()
        os.makedirs(os.path.dirname(output_path) or ".", exist_ok=True)
        plt.savefig(output_path, dpi=150, bbox_inches="tight")
        plt.close(fig)

        return os.path.abspath(output_path)


# ── Module-level convenience functions (called from PyO3 bridge) ────────

def generate_weekly_report(metrics_json: str, company_name: str = "Track Exhibits") -> str:
    """Generate a text-based weekly report.

    Args:
        metrics_json: JSON array of weekly metric records
        company_name: Company name for the report header

    Returns:
        Formatted text report string
    """
    try:
        metrics = json.loads(metrics_json)
    except json.JSONDecodeError:
        return "Error: Invalid metrics JSON"

    generator = WeeklyReportGenerator(company_name)
    return generator.generate_text_report(metrics)


def generate_weekly_chart(
    metrics_json: str,
    output_path: str = "data/weekly_report.png",
) -> str:
    """Generate a chart PNG and return the file path.

    Args:
        metrics_json: JSON array of weekly metric records
        output_path: Where to save the chart

    Returns:
        Absolute path to chart file, or empty string if unavailable
    """
    try:
        metrics = json.loads(metrics_json)
    except json.JSONDecodeError:
        return ""

    generator = WeeklyReportGenerator()
    result = generator.generate_chart(metrics, output_path)
    return result or ""
