<div align="center">

```
 ╔═══════════════════════════════════════════════════════════════╗
 ║                                                               ║
 ║    ███████╗██╗  ██╗ ██████╗ ██╗   ██╗████████╗████████╗      ║
 ║    ██╔════╝██║ ██╔╝██╔═══██╗██║   ██║╚══██╔══╝╚══██╔══╝      ║
 ║    ███████╗█████╔╝ ██║   ██║██║   ██║   ██║      ██║         ║
 ║    ╚════██║██╔═██╗ ██║   ██║██║   ██║   ██║      ██║         ║
 ║    ███████║██║  ██╗╚██████╔╝╚██████╔╝   ██║      ██║         ║
 ║    ╚══════╝╚═╝  ╚═╝ ╚═════╝  ╚═════╝    ╚═╝      ╚═╝         ║
 ║                                                               ║
 ║         Survival-Driven B2B Lead Generation Agent             ║
 ║                  Track Exhibits Pvt LTD                       ║
 ╚═══════════════════════════════════════════════════════════════╝
```

**Skoutt** is a fully autonomous lead generation agent that discovers companies exhibiting at trade shows, enriches their contacts, researches them using Claude AI, and sends personalized cold emails — with a hard survival constraint that shuts it down permanently if it stops producing results.

[![Rust](https://img.shields.io/badge/Rust-1.75+-CE422B?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Python](https://img.shields.io/badge/Python-3.11+-3776AB?style=flat-square&logo=python&logoColor=white)](https://python.org)
[![SQLite](https://img.shields.io/badge/SQLite-3-003B57?style=flat-square&logo=sqlite&logoColor=white)](https://sqlite.org)
[![License](https://img.shields.io/badge/License-Proprietary-red?style=flat-square)](./LICENSE)
[![Status](https://img.shields.io/badge/Status-Internal%20Use%20Only-orange?style=flat-square)]()

</div>

---

## ◈ What Skoutt Does

Every day at 08:00 UTC, Skoutt runs a 7-phase autonomous cycle:

```
  Phase 1 ──▶  Exhibition Discovery   (10times.com, Expodatabase)
  Phase 2 ──▶  Lead Enrichment        (Apollo.io + Hunter.io)
  Phase 3 ──▶  Company Research       (Claude AI — structured summaries)
  Phase 4 ──▶  Email Drafting         (Claude AI — personalized, 150–200 words)
  Phase 5 ──▶  Email Sending          (SMTP, rate-limited, DNC-aware)
  Phase 6 ──▶  Reply Monitoring       (IMAP, sentiment analysis, auto-unsubscribe)
  Phase 7 ──▶  Survival Check         (death rule enforcement)
```

> **The Death Rule:** If Skoutt receives zero interested replies for **3 consecutive weeks** (after a 2-week grace period), it permanently shuts itself down and sends a final alert. No manual intervention needed.

---

## ◈ Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         SKOUTT CORE (Rust)                      │
│                                                                 │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────┐   │
│  │ Scraping │  │Enrichment│  │Intelligence│  │   Outreach   │   │
│  │          │  │          │  │            │  │              │   │
│  │10times   │  │Apollo.io │  │ Claude AI  │  │SMTP + IMAP   │   │
│  │Expodb    │  │Hunter.io │  │ Research   │  │Rate limiting │   │
│  └────┬─────┘  └────┬─────┘  └─────┬──────┘  └──────┬───────┘   │
│       │              │              │                 │           │
│       └──────────────┴──────────────┴─────────────────┘           │
│                              │                                   │
│                     ┌────────▼────────┐                          │
│                     │   SQLite DB     │                          │
│                     └────────┬────────┘                          │
│                              │                                   │
│  ┌──────────────────────────▼──────────────────────────────┐    │
│  │                    Survival System                       │    │
│  │   MetricsTracker → ShutdownManager → AlertSystem        │    │
│  └──────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
                              │ PyO3
┌─────────────────────────────▼─────────────────────────────────┐
│                      PYTHON MODULES                            │
│   enrichment/enricher.py  ·  intelligence/sentiment.py        │
│   analytics/performance.py                                     │
└────────────────────────────────────────────────────────────────┘
```

**Why Rust + Python?**
- **Rust** handles everything performance-critical: HTTP scraping, SMTP/IMAP, SQLite, rate limiting, the daily loop
- **Python** handles AI/ML tasks where ecosystem richness matters: enrichment orchestration, sentiment analysis, analytics reporting
- **PyO3** bridges them with zero-copy FFI

---

## ◈ Prerequisites

| Requirement | Version | Purpose |
|---|---|---|
| Rust | ≥ 1.75 | Core binary |
| Python | ≥ 3.11 | Intelligence & analytics modules |
| SQLite | ≥ 3.35 | Local database |
| Claude API key | — | Company research + email drafting |
| Apollo.io API key | — | Decision-maker discovery (optional) |
| Hunter.io API key | — | Email verification (optional) |
| SMTP credentials | — | Sending emails |
| IMAP credentials | — | Reply monitoring |

---

## ◈ Installation

### 1. Clone the repository

```bash
git clone <repo-url> Skoutt-Ai
cd Skoutt-Ai
```

### 2. Install Python dependencies

```bash
pip install -r python/requirements.txt
```

### 3. Configure credentials

**Option A — Environment file (recommended):**

```bash
cp .env.example .env
# Edit .env with your credentials
nano .env
```

**Option B — Config file:**

```bash
cp config/config.example.toml config/config.toml
# Edit config.toml with your credentials
nano config/config.toml
```

**Option C — Shell environment variables (CI/CD, Docker):**

```bash
export SKOUTT_APIS__CLAUDE_API_KEY="sk-ant-..."
export SKOUTT_EMAIL__SMTP_HOST="smtp.gmail.com"
# ... etc
```

> Environment variables always override the config file. See [Environment Variables](#-environment-variables) for the full reference.

### 4. Build

```bash
cargo build --release
```

The binary will be at `./target/release/skoutt`.

---

## ◈ Usage

### Validate your setup first

```bash
./target/release/skoutt validate
```

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  SKOUTT CONFIGURATION VALIDATION
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✅  Config loaded from: config/config.toml
  ✅  Database path:       data/skoutt.db
  ✅  SMTP host:           smtp.gmail.com
  ✅  Claude model:        claude-3-5-haiku-20241022
  ✅  Claude API key:      set
  ✅  Database:            connected
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### Run a single cycle (test)

```bash
# Dry run — logs everything, sends nothing
./target/release/skoutt run --dry-run

# Live run — actually sends emails
./target/release/skoutt run
```

### Start the daemon (production)

```bash
# Runs daily at 08:00 UTC
./target/release/skoutt daemon

# Custom hour (e.g. 06:00 UTC)
./target/release/skoutt daemon --hour 6

# Dry-run daemon (useful for staging)
./target/release/skoutt daemon --dry-run
```

### Monitor status

```bash
# Survival status and weekly metrics
./target/release/skoutt status

# Database statistics
./target/release/skoutt stats
```

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  SKOUTT STATUS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Status:           Safe
  Weeks active:     4
  Zero-reply weeks: 0
  Interested/week:  3
  Total sent:       240
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### Manually check replies

```bash
# Check inbox and analyze replies
./target/release/skoutt check-replies

# Analyze without writing to database
./target/release/skoutt check-replies --dry-run
```

### Start an interactive chat session with Scott
```bash
./target/release/skoutt chat
```

### Export leads to CSV
```bash
# Default path (data/leads.csv)
./target/release/skoutt export

# Custom path
./target/release/skoutt export --output my_leads.csv
```

### Full CLI reference
```bash
./target/release/skoutt --help
./target/release/skoutt daemon --help
```

---

## ◈ Environment Variables

All config values can be set via environment variables. The prefix is `SKOUTT_` and nested keys use double underscores (`__`).

| Variable | Description | Example |
|---|---|---|
| `SKOUTT_CONFIG` | Path to config file | `config/config.toml` |
| `SKOUTT_APIS__CLAUDE_API_KEY` | Anthropic Claude API key | `sk-ant-...` |
| `SKOUTT_APIS__CLAUDE_MODEL` | Claude model to use | `claude-3-5-haiku-20241022` |
| `SKOUTT_APIS__APOLLO_API_KEY` | Apollo.io API key | `...` |
| `SKOUTT_APIS__HUNTER_API_KEY` | Hunter.io API key | `...` |
| `SKOUTT_EMAIL__SMTP_HOST` | SMTP server hostname | `smtp.gmail.com` |
| `SKOUTT_EMAIL__SMTP_PORT` | SMTP port | `587` |
| `SKOUTT_EMAIL__SMTP_USER` | SMTP username | `you@gmail.com` |
| `SKOUTT_EMAIL__SMTP_PASSWORD` | SMTP password / app password | `...` |
| `SKOUTT_EMAIL__FROM_NAME` | Sender display name | `Track Exhibits` |
| `SKOUTT_EMAIL__FROM_EMAIL` | Sender email address | `you@gmail.com` |
| `SKOUTT_EMAIL__DAILY_LIMIT` | Max emails per day | `60` |
| `SKOUTT_IMAP__HOST` | IMAP server hostname | `imap.gmail.com` |
| `SKOUTT_IMAP__PORT` | IMAP port | `993` |
| `SKOUTT_IMAP__USERNAME` | IMAP username | `you@gmail.com` |
| `SKOUTT_IMAP__PASSWORD` | IMAP password / app password | `...` |
| `SKOUTT_ALERTS__USER_EMAIL` | Where to send alert emails | `you@personal.com` |
| `SKOUTT_DATABASE__PATH` | SQLite database file path | `data/skoutt.db` |
| `SKOUTT_SURVIVAL__SHUTDOWN_THRESHOLD` | Consecutive zero-reply weeks before shutdown | `3` |

> **Gmail users:** Generate an [App Password](https://support.google.com/accounts/answer/185833) — do not use your main account password.

---

## ◈ Survival System

The survival system is the core constraint that keeps Skoutt accountable.

```
Week 1–2   ──▶  Grace Period    (no survival pressure, pipeline building)
Week 3+    ──▶  Survival Mode   (must get ≥1 interested reply per week)

0 interested replies for 1 week  ──▶  ⚠️  WARNING  (alert sent, volume increased)
0 interested replies for 2 weeks ──▶  🔴 CRITICAL  (emergency pivots attempted)
0 interested replies for 3 weeks ──▶  💀 SHUTDOWN  (permanent, irreversible)
```

**What counts as "interested"?**
- Replies asking for pricing, portfolio, or a meeting
- Replies with positive sentiment detected by Claude or keyword analysis
- Manually classified via the database

**Survival alerts** are sent to `SKOUTT_ALERTS__USER_EMAIL` at each threshold.

---

## ◈ Project Structure

```
Skoutt-Ai/
├── src/
│   ├── main.rs                    CLI entry point (clap)
│   ├── lib.rs                     Types, config, module declarations
│   ├── database/
│   │   ├── mod.rs                 SQLite layer (sqlx)
│   │   └── schema.sql             Database schema
│   ├── scraping/
│   │   ├── exhibition_finder.rs   10times.com + Expodatabase discovery
│   │   ├── exhibitor_extractor.rs Exhibitor list parsing
│   │   ├── company_scraper.rs     Company website content extraction
│   │   └── fallback_strategies.rs Previous-year URLs + web search
│   ├── enrichment/
│   │   ├── apollo_client.rs       Apollo.io people search
│   │   └── hunter_client.rs       Hunter.io email verification
│   ├── intelligence/
│   │   ├── company_researcher.rs  Claude-powered company research
│   │   ├── email_personalizer.rs  Claude-powered email drafting
│   │   └── reply_analyzer.rs      Claude reply classification
│   ├── outreach/
│   │   ├── email_drafter.rs       Initial + follow-up email orchestration
│   │   ├── email_sender.rs        SMTP with rate limiting + DNC
│   │   ├── reply_monitor.rs       IMAP inbox monitoring
│   │   └── followup_scheduler.rs  Day 3/7/14 follow-up scheduling
│   ├── survival/
│   │   ├── metrics_tracker.rs     Weekly metrics aggregation
│   │   ├── shutdown_manager.rs    Death rule enforcement
│   │   └── alert_system.rs        Email alerts
│   └── python_bridge/
│       ├── enrichment_bridge.rs   PyO3 → Python enricher
│       └── analytics_bridge.rs    PyO3 → Python analytics
├── python/
│   ├── enrichment/
│   │   ├── apollo.py              Apollo.io Python client
│   │   ├── hunter.py              Hunter.io Python client
│   │   └── enricher.py            Enrichment orchestrator
│   ├── intelligence/
│   │   └── sentiment.py           Keyword-based sentiment analysis
│   └── analytics/
│       └── performance.py         Weekly performance reports
├── config/
│   └── config.example.toml        Configuration template
├── data/                           SQLite database (auto-created)
├── logs/                           Daily log files (auto-created)
├── .env.example                    Environment variable template
├── Cargo.toml
├── pyproject.toml
└── LICENSE
```

---

## ◈ Configuration Reference

The full configuration lives in `config/config.toml`. Every value can be overridden by an environment variable.

```toml
[company]
name = "Track Exhibits"
website = "https://trackexhibits.com"
services = "Custom exhibition stands, booth design, fabrication"
tagline = "We build stands that get noticed"
regions_served = ["India", "UAE", "Europe", "USA"]

[email]
smtp_host = "smtp.gmail.com"
smtp_port = 587
daily_limit = 60                    # Max emails per day
min_send_interval_seconds = 120     # Min gap between sends
max_per_hour_per_domain = 2         # Sender reputation protection

[survival]
grace_period_weeks = 2              # No pressure for first 2 weeks
shutdown_threshold = 3              # Shutdown after 3 consecutive zero weeks
warning_threshold = 1               # Warn after 1 zero week

[targeting]
regions = ["India", "UAE", "Singapore", "UK"]
sectors = ["Manufacturing", "Technology", "Healthcare", "Retail"]
target_titles = ["Marketing Director", "CEO", "Head of Events"]
```

---

## ◈ Running in Production

### As a systemd service

```ini
# /etc/systemd/system/skoutt.service
[Unit]
Description=Skoutt Lead Generation Agent
After=network.target

[Service]
Type=simple
User=skoutt
WorkingDirectory=/opt/skoutt
EnvironmentFile=/opt/skoutt/.env
ExecStart=/opt/skoutt/skoutt daemon
Restart=on-failure
RestartSec=30

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable skoutt
sudo systemctl start skoutt
sudo journalctl -u skoutt -f
```

### With Docker

```dockerfile
FROM rust:1.75-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y python3 python3-pip ca-certificates
COPY --from=builder /app/target/release/skoutt /usr/local/bin/
COPY python/ /app/python/
COPY config/ /app/config/
RUN pip3 install -r /app/python/requirements.txt
WORKDIR /app
CMD ["skoutt", "daemon"]
```

```bash
docker build -t skoutt .
docker run -d \
  -e SKOUTT_APIS__CLAUDE_API_KEY="sk-ant-..." \
  -e SKOUTT_EMAIL__SMTP_PASSWORD="..." \
  -v skoutt-data:/app/data \
  skoutt
```

---

## ◈ Logs

Skoutt writes structured logs to `logs/skoutt-YYYY-MM-DD.log`.

```
2026-02-18 08:00:01  INFO  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
2026-02-18 08:00:01  INFO  DAILY CYCLE — 2026-02-18 08:00 UTC
2026-02-18 08:00:01  INFO  📡  Phase 1: Exhibition Discovery
2026-02-18 08:00:04  INFO      3 new exhibitions found
2026-02-18 08:00:04  INFO  🔍  Phase 2: Lead Enrichment
2026-02-18 08:00:12  INFO      ✓ Acme Displays Ltd
2026-02-18 08:00:15  INFO  🧠  Phase 3: Company Research
2026-02-18 08:00:45  INFO  ✍️   Phase 4: Email Drafting
2026-02-18 08:01:10  INFO  📤  Phase 5: Email Sending
2026-02-18 08:01:10  INFO      ✓ → Sarah Chen <s.chen@acmedisplays.com>
2026-02-18 08:01:12  INFO  📬  Phase 6: Reply Monitoring
2026-02-18 08:01:12  INFO      🚨 INTERESTED: john@exhibitco.com — High
2026-02-18 08:01:13  INFO  🧬  Phase 7: Survival Check
2026-02-18 08:01:13  INFO      ✅ Healthy — 1 interested replies this week
2026-02-18 08:01:13  INFO  Cycle complete in 72s
```

Control log verbosity with `RUST_LOG`:

```bash
RUST_LOG=debug ./target/release/skoutt run --dry-run   # verbose
RUST_LOG=warn  ./target/release/skoutt daemon           # warnings only
```

---

## ◈ Safety Features

| Feature | Description |
|---|---|
| **Daily email limit** | Hard cap (default: 60/day) — never exceeded |
| **Min send interval** | Minimum gap between sends (default: 120s) |
| **Per-domain limit** | Max 2 emails/hour to any single domain |
| **DNC list** | Contacts who unsubscribe are permanently blocked |
| **Unsubscribe detection** | IMAP monitor auto-detects and processes unsubscribes |
| **Dry-run mode** | `--dry-run` on any command — zero side effects |
| **Bounce tracking** | Bounced emails are tracked and excluded from follow-ups |
| **Scrape cache** | Pages cached for 24h — avoids hammering sites |

---

## ◈ License

This software is **proprietary and confidential**. It is the exclusive property of **Track Exhibits Pvt LTD** and is not licensed for public use, redistribution, or modification.

See [LICENSE](./LICENSE) for full terms.

---

<div align="center">

**Track Exhibits Pvt LTD** · Built with Rust + Python + Claude AI

*"If it doesn't generate leads, it shuts itself down."*

</div>
