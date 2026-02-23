# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Skoutt — python/intelligence/timing.py
# Predicts optimal email send times based on recipient timezone
# and historical open/reply rate data.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

from datetime import datetime, timedelta, timezone
import math
import json
import random
from typing import Optional


# ── Default send-time windows by region ──────────────────────────────────
# These represent the "business morning" window in each region's
# dominant timezone. Emails sent during 09:00–11:00 local time
# historically achieve the highest open rates in B2B outreach.

REGION_WINDOWS = {
    "Middle East": {
        "tz_offset_hours": 3,       # UTC+3 (Gulf Standard Time)
        "best_hours": (9, 11),      # 09:00–11:00 local
        "best_days": [0, 1, 2, 3],  # Mon–Thu (Sun is weekend in ME)
    },
    "Europe": {
        "tz_offset_hours": 1,       # UTC+1 (CET avg)
        "best_hours": (9, 11),
        "best_days": [0, 1, 2, 3],  # Mon–Thu
    },
    "Asia Pacific": {
        "tz_offset_hours": 8,       # UTC+8 (SGT/HKT avg)
        "best_hours": (9, 11),
        "best_days": [0, 1, 2, 3, 4],  # Mon–Fri
    },
    "UK": {
        "tz_offset_hours": 0,       # UTC+0 (GMT)
        "best_hours": (9, 11),
        "best_days": [1, 2, 3],     # Tue–Thu (best in UK)
    },
}

# Fallback for unknown regions
DEFAULT_WINDOW = {
    "tz_offset_hours": 0,
    "best_hours": (9, 11),
    "best_days": [1, 2, 3],
}


class SendTimePredictor:
    """Predicts the optimal UTC send time for an email based on the
    recipient's region and historical engagement data."""

    def __init__(self, historical_data: Optional[list] = None):
        """
        Args:
            historical_data: Optional list of dicts with keys:
                - sent_at: ISO 8601 datetime string (UTC)
                - region: recipient region string
                - opened: bool
                - replied: bool
        """
        self.region_windows = dict(REGION_WINDOWS)
        self._engagement_scores: dict[int, float] = {}

        if historical_data:
            self._learn_from_history(historical_data)

    def _learn_from_history(self, data: list) -> None:
        """Build hour-of-day engagement scores from historical send data."""
        hour_stats: dict[int, dict] = {h: {"sent": 0, "engaged": 0} for h in range(24)}

        for record in data:
            try:
                sent_at = datetime.fromisoformat(record["sent_at"])
                hour = sent_at.hour
                hour_stats[hour]["sent"] += 1
                if record.get("replied") or record.get("opened"):
                    hour_stats[hour]["engaged"] += 1
            except (KeyError, ValueError):
                continue

        # Compute engagement rate per hour (with Laplace smoothing)
        for hour, stats in hour_stats.items():
            total = stats["sent"] + 2  # Laplace smoothing
            engaged = stats["engaged"] + 1
            self._engagement_scores[hour] = engaged / total

    def predict_send_time(
        self,
        region: str,
        now: Optional[datetime] = None,
        min_delay_minutes: int = 60,
    ) -> datetime:
        """Predict the next optimal send time in UTC.

        Args:
            region: Recipient region (e.g., "Middle East", "Europe")
            now: Current UTC time (defaults to utcnow)
            min_delay_minutes: Minimum minutes from now before sending

        Returns:
            datetime: Recommended send time in UTC
        """
        if now is None:
            now = datetime.now(timezone.utc)

        window = self.region_windows.get(region, DEFAULT_WINDOW)
        tz_offset = timedelta(hours=window["tz_offset_hours"])
        best_start, best_end = window["best_hours"]
        best_days = set(window["best_days"])

        earliest = now + timedelta(minutes=min_delay_minutes)

        # Search up to 7 days ahead for the best slot
        for day_offset in range(8):
            candidate_date = (earliest + timedelta(days=day_offset)).date()

            for hour in range(best_start, best_end + 1):
                # Build candidate time in recipient's local timezone, convert to UTC
                local_time = datetime(
                    candidate_date.year,
                    candidate_date.month,
                    candidate_date.day,
                    hour,
                    0,
                    0,
                    tzinfo=timezone.utc,
                )
                utc_time = local_time - tz_offset

                # Must be in the future and on a good day
                if utc_time <= earliest:
                    continue
                if utc_time.weekday() not in best_days:
                    continue

                # Add human-like jitter: ±15 min + random seconds
                # So emails don't arrive at machine-precise xx:00:00
                jitter_minutes = random.randint(-15, 15)
                jitter_seconds = random.randint(0, 59)
                jittered = utc_time + timedelta(minutes=jitter_minutes, seconds=jitter_seconds)

                # Ensure jitter didn't push us before earliest
                if jittered <= earliest:
                    jittered = earliest + timedelta(minutes=random.randint(1, 10), seconds=jitter_seconds)

                return jittered

        # Fallback: send at the minimum delay
        return earliest

    def predict_best_hour(self, region: str) -> int:
        """Return the single best hour (UTC) for a region.

        If we have engagement data, weight by historical performance.
        Otherwise, return the midpoint of the region's best window.
        """
        window = self.region_windows.get(region, DEFAULT_WINDOW)
        tz_offset = window["tz_offset_hours"]
        best_start, best_end = window["best_hours"]

        if self._engagement_scores:
            # Find the hour in the window with the best engagement
            best_hour = best_start
            best_score = -1.0
            for local_hour in range(best_start, best_end + 1):
                utc_hour = (local_hour - tz_offset) % 24
                score = self._engagement_scores.get(utc_hour, 0.0)
                if score > best_score:
                    best_score = score
                    best_hour = local_hour

            return (best_hour - tz_offset) % 24
        else:
            # Default: midpoint of the window
            mid = (best_start + best_end) // 2
            return (mid - tz_offset) % 24

    def get_region_info(self, region: str) -> dict:
        """Return send-time metadata for a region."""
        window = self.region_windows.get(region, DEFAULT_WINDOW)
        tz_offset = window["tz_offset_hours"]
        best_start, best_end = window["best_hours"]

        day_names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        best_day_names = [day_names[d] for d in window["best_days"]]

        return {
            "region": region,
            "tz_offset_utc": f"UTC{'+' if tz_offset >= 0 else ''}{tz_offset}",
            "best_local_hours": f"{best_start:02d}:00–{best_end:02d}:00",
            "best_utc_hours": f"{(best_start - tz_offset) % 24:02d}:00–{(best_end - tz_offset) % 24:02d}:00",
            "best_days": best_day_names,
        }


# ── Module-level convenience function (called from PyO3 bridge) ─────────

def predict_optimal_send_time(
    region: str,
    historical_json: Optional[str] = None,
) -> str:
    """Convenience function for PyO3 bridge.

    Args:
        region: Recipient region
        historical_json: Optional JSON array of historical send records

    Returns:
        ISO 8601 UTC datetime string of the recommended send time
    """
    history = None
    if historical_json:
        try:
            history = json.loads(historical_json)
        except json.JSONDecodeError:
            pass

    predictor = SendTimePredictor(historical_data=history)
    send_time = predictor.predict_send_time(region)
    return send_time.isoformat()


def get_all_region_info() -> str:
    """Return JSON with send-time info for all known regions."""
    predictor = SendTimePredictor()
    info = {r: predictor.get_region_info(r) for r in REGION_WINDOWS}
    return json.dumps(info, indent=2)
