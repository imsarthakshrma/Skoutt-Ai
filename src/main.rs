// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — main.rs
// Survival-driven lead generation agent for Track Exhibits Pvt LTD
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{error, info, warn};
use csv;

use skoutt::{
    database::Database,
    enrichment::{apollo_client::ApolloClient, hunter_client::HunterClient},
    intelligence::{
        company_researcher::CompanyResearcher,
        deep_researcher::DeepResearcher,
        reply_analyzer::ReplyAnalyzer,
    },
    load_config,
    outreach::{
        email_drafter::EmailDrafter, email_sender::EmailSender,
        followup_scheduler::FollowupScheduler, reply_monitor::ReplyMonitor,
    },
    scraping::{
        exhibition_finder::ExhibitionFinder,
        exhibitor_extractor::ExhibitorExtractor,
    },
    survival::{
        alert_system::AlertSystem, metrics_tracker::MetricsTracker,
        shutdown_manager::ShutdownManager,
    },
};


// ─────────────────────────────────────────────────────────────────────────
// CLI Definition
// ─────────────────────────────────────────────────────────────────────────

/// Skoutt — Survival-driven lead generation agent for Track Exhibits Pvt LTD.
///
/// Discovers companies exhibiting at trade shows, enriches their contacts,
/// researches them via Claude, and sends personalized cold emails.
/// Automatically shuts down after 3 consecutive weeks with zero interested replies.
#[derive(Parser, Debug)]
#[command(
    name = "skoutt",
    version = env!("CARGO_PKG_VERSION"),
    author = "Track Exhibits <scout@trackexhibits.com>",
    about = "Survival-driven B2B lead generation agent",
    long_about = None,
    after_help = "Environment variables can be used instead of config.toml.\nPrefix any config key with SKOUTT_ (e.g. SKOUTT_APIS__CLAUDE_API_KEY).\nOr place a .env file in the working directory.",
)]
struct Cli {
    /// Path to config file (default: config/config.toml)
    #[arg(short, long, default_value = "config/config.toml", env = "SKOUTT_CONFIG")]
    config: String,

    /// Path to .env file (default: .env)
    #[arg(long, default_value = ".env", env = "SKOUTT_ENV_FILE")]
    env_file: String,

    /// Suppress all output except errors
    #[arg(short, long)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the full daily cycle once and exit
    Run {
        /// Simulate all actions without sending any emails
        #[arg(long)]
        dry_run: bool,
    },

    /// Start the daemon — runs daily at 08:00 UTC automatically
    Daemon {
        /// Simulate all actions without sending any emails
        #[arg(long)]
        dry_run: bool,

        /// Override the daily run hour in UTC (default: 8)
        #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(0..24))]
        hour: u32,
    },

    /// Check current survival status and weekly metrics
    Status,

    /// Show database statistics (exhibitions, companies, contacts, emails)
    Stats,

    /// Manually trigger reply inbox check and analysis
    CheckReplies {
        /// Simulate analysis without writing to database
        #[arg(long)]
        dry_run: bool,
    },

    /// Validate configuration and API connectivity
    Validate,

    /// Show deep research report for a contact
    Research {
        /// Contact ID to look up research for
        contact_id: String,
    },

    /// Start an interactive chat session with Scott
    Chat,

    /// Export leads to a CSV file
    Export {
        /// Output file path (default: data/leads.csv)
        #[arg(short, long, default_value = "data/leads.csv")]
        output: String,
    },
}


// ─────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load .env file (silently ignore if missing)
    let _ = dotenvy::from_filename(&cli.env_file);

    // Load config (file + env vars)
    let config = load_config(&cli.config).map_err(|e| {
        eprintln!("❌  Config error: {e}");
        eprintln!("    → Copy config/config.example.toml to config/config.toml");
        eprintln!("    → Or set SKOUTT_* environment variables / create a .env file");
        e
    })?;

    // Setup logging
    if !cli.quiet {
        setup_logging(&config.logging.level, &config.logging.log_dir)?;
    } else {
        setup_logging("error", &config.logging.log_dir)?;
    }

    match cli.command {
        Commands::Run { dry_run } => {
            print_banner(dry_run);
            let ctx = build_context(&config, dry_run).await?;
            guard_shutdown(&ctx.shutdown_manager).await?;
            run_daily_cycle(&ctx).await?;
        }

        Commands::Status => {
            let db = Database::new(&config.database.path).await?;
            let status = skoutt::survival::shutdown_manager::ShutdownManager::new(
                config.survival.clone(),
                db.clone(),
                AlertSystem::new(config.email.clone(), config.alerts.clone(), true),
            ).run_survival_check().await?;
            
            println!("\n{}", "━".repeat(60));
            println!("  SKOUTT SURVIVAL STATUS");
            println!("{}", "━".repeat(60));
            println!("  Status:           {:?}", status.status);
            println!("  Weeks Active:     {}", status.weeks_active);
            println!("  Interested/Week:  {}", status.interested_this_week);
            println!("  Zero-Reply Weeks: {}", status.consecutive_zero_weeks);
            println!("{}", "━".repeat(60));
        }

        Commands::Stats => {
            let db = Database::new(&config.database.path).await?;
            print_database_stats(&db).await?;
        }

        Commands::Chat => {
            let ctx = build_context(&config, true).await?;
            let mut session = skoutt::intelligence::chat::ChatSession::new(
                config.apis.claude_api_key.clone(),
                config.apis.claude_model.clone(),
                ctx.db.clone(),
            );
            session.start().await?;
        }

        Commands::Export { output } => {
            let ctx = build_context(&config, true).await?;
            export_leads_csv(&ctx, &output).await?;
        }

        Commands::Daemon { dry_run, hour } => {
            print_banner(dry_run);
            let ctx = build_context(&config, dry_run).await?;
            guard_shutdown(&ctx.shutdown_manager).await?;

            info!("🚀 Daemon started — daily cycle at {:02}:00 UTC", hour);
            info!("   Use Ctrl+C to stop. Logs: {}", config.logging.log_dir);

            loop {
                let now = chrono::Utc::now();
                let next_run = next_daily_run_time(now, hour);
                let wait_duration = (next_run - now).to_std()?;

                info!(
                    "⏰  Next cycle: {} UTC  ({:.1}h from now)",
                    next_run.format("%Y-%m-%d %H:%M"),
                    wait_duration.as_secs_f64() / 3600.0
                );

                tokio::time::sleep(wait_duration).await;

                match run_daily_cycle(&ctx).await {
                    Ok(_) => info!("✅  Daily cycle complete"),
                    Err(e) => error!("❌  Daily cycle failed: {e}"),
                }

                // Re-check survival after each cycle
                let survival = ctx.shutdown_manager.check_status().await?;
                if survival.is_shutdown() {
                    error!("💀  Death rule triggered — Skoutt is shutting down permanently.");
                    std::process::exit(0);
                }
            }
        }

        Commands::CheckReplies { dry_run } => {
            let db = Database::new(&config.database.path).await?;
            let reply_monitor = ReplyMonitor::new(config.imap.clone());
            let reply_analyzer = ReplyAnalyzer::new(
                config.apis.claude_api_key.clone(),
                config.apis.claude_model.clone(),
            );
            let alert = AlertSystem::new(config.email.clone(), config.alerts.clone(), dry_run);
            let metrics = MetricsTracker::new(db.clone());

            println!("📬  Checking inbox...");
            let replies = reply_monitor.check_inbox(&db).await?;
            println!("    {} new replies found", replies.len());

            for reply in &replies {
                let analysis = reply_analyzer.analyze_reply(reply, &db).await?;
                println!(
                    "    {} <{}> → {} ({})",
                    reply.from_name.as_deref().unwrap_or("Unknown"),
                    reply.from_email,
                    analysis.interest_level_str(),
                    analysis.sentiment
                );

                if !dry_run {
                    let _ = db.update_reply_analysis(&reply.email_id, &analysis).await;
                    if analysis.interest_level.is_actionable() {
                        if let Ok(Some(contact)) = db.get_contact_by_email(&reply.from_email).await {
                            if let Ok(Some(company)) = db.get_company(&contact.company_id).await {
                                alert.send_interested_lead_alert(&contact, &company, reply, &analysis, None, None).await?;
                                metrics.record_interested_reply().await?;
                            }
                        }
                    }
                }
            }
        }

        Commands::Validate => {
            println!("\n{}", "━".repeat(60));
            println!("  SKOUTT CONFIGURATION VALIDATION");
            println!("{}", "━".repeat(60));

            // Config check
            println!("  ✅  Config loaded from: {}", cli.config);
            println!("  ✅  Database path:       {}", config.database.path);
            println!("  ✅  SMTP host:           {}", config.email.smtp_host);
            println!("  ✅  IMAP host:           {}", config.imap.host);
            println!("  ✅  Claude model:        {}", config.apis.claude_model);
            println!("  ✅  Daily email limit:   {}", config.email.daily_limit);
            println!("  ✅  Targeting regions:   {}", config.targeting.regions.join(", "));
            println!("  ✅  Targeting sectors:   {}", config.targeting.sectors.join(", "));

            // API key presence checks
            let claude_ok = !config.apis.claude_api_key.is_empty() && config.apis.claude_api_key != "sk-ant-REPLACE_ME";
            let apollo_ok = !config.apis.apollo_api_key.is_empty() && config.apis.apollo_api_key != "REPLACE_ME";
            let hunter_ok = !config.apis.hunter_api_key.is_empty() && config.apis.hunter_api_key != "REPLACE_ME";

            println!("  {}  Claude API key:     {}", if claude_ok { "✅" } else { "❌" }, if claude_ok { "set" } else { "MISSING" });
            println!("  {}  Apollo API key:     {}", if apollo_ok { "✅" } else { "⚠️ " }, if apollo_ok { "set" } else { "not set (optional)" });
            println!("  {}  Hunter API key:     {}", if hunter_ok { "✅" } else { "⚠️ " }, if hunter_ok { "set" } else { "not set (optional)" });

            // Database connectivity
            match Database::new(&config.database.path).await {
                Ok(_) => println!("  ✅  Database:           connected"),
                Err(e) => println!("  ❌  Database:           {e}"),
            }

            println!("{}", "━".repeat(60));
            if !claude_ok {
                println!("\n  ⚠️   Set SKOUTT_APIS__CLAUDE_API_KEY or fill config.toml");
            }
        }

        Commands::Research { contact_id } => {
            let db = Database::new(&config.database.path).await?;

            match db.get_research_report(&contact_id).await? {
                None => {
                    println!("\n  ℹ️  No research report found for contact: {}", contact_id);
                    println!("     Run `skoutt run` to trigger the research phase.");
                }
                Some(report) => {
                    println!("\n{}", "━".repeat(60));
                    println!("  RESEARCH REPORT  —  {}", contact_id);
                    println!("{}", "━".repeat(60));
                    println!("  Researched:  {}", report.researched_at.format("%Y-%m-%d %H:%M UTC"));
                    println!("  Quality:     {:.2} / 1.00", report.research_quality_score);
                    println!("  Sources:     {}", if report.sources_used.is_empty() {
                        "none".into()
                    } else {
                        report.sources_used.join(", ")
                    });

                    println!("\n── Company Overview ─────────────────────────────────────");
                    println!("  {}", report.company_overview);

                    println!("\n── Exhibition Strategy ──────────────────────────────────");
                    println!("  {}", report.exhibition_strategy);

                    if !report.pain_points.is_empty() {
                        println!("\n── Pain Points ──────────────────────────────────────────");
                        for (i, p) in report.pain_points.iter().enumerate() {
                            println!("  {}. {}", i + 1, p);
                        }
                    }

                    if !report.personalization_hooks.is_empty() {
                        println!("\n── Personalization Hooks ────────────────────────────────");
                        for (i, h) in report.personalization_hooks.iter().enumerate() {
                            println!("  {}. {}", i + 1, h);
                        }
                    }

                    println!("\n── Email Angle ──────────────────────────────────────────");
                    println!("  {}", report.email_angle);

                    if !report.previous_exhibitions.is_empty() {
                        println!("\n── Previous Exhibitions ─────────────────────────────────");
                        for ex in &report.previous_exhibitions {
                            println!(
                                "  • {} ({}) — {}",
                                ex.event_name,
                                ex.date.map(|d| d.format("%Y").to_string()).unwrap_or_else(|| "?".to_string()),
                                ex.location
                            );
                        }
                    }

                    if !report.recent_news.is_empty() {
                        println!("\n── Recent News ──────────────────────────────────────────");
                        for article in report.recent_news.iter().take(3) {
                            println!("  • [{}] {}", article.source, article.title);
                            println!("    {}", &article.summary.chars().take(120).collect::<String>());
                        }
                    }

                    println!("{}", "━".repeat(60));
                }
            }
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// Context — all initialized components
// ─────────────────────────────────────────────────────────────────────────

struct AppContext {
    db: Database,
    exhibition_finder: ExhibitionFinder,
    exhibitor_extractor: ExhibitorExtractor,
    apollo: ApolloClient,
    hunter: HunterClient,
    researcher: CompanyResearcher,
    deep_researcher: DeepResearcher,
    drafter: EmailDrafter,
    sender: EmailSender,
    reply_monitor: ReplyMonitor,
    reply_analyzer: ReplyAnalyzer,
    followup_scheduler: FollowupScheduler,
    metrics_tracker: MetricsTracker,
    alert_system: AlertSystem,
    shutdown_manager: ShutdownManager,
}

async fn build_context(config: &skoutt::AppConfig, dry_run: bool) -> Result<AppContext> {
    let db = Database::new(&config.database.path).await?;
    info!("✅  Database: {}", config.database.path);

    Ok(AppContext {
        exhibition_finder: ExhibitionFinder::new(config.scraping.clone(), config.targeting.clone(), db.clone()),
        exhibitor_extractor: ExhibitorExtractor::new(
            config.research.clone(),
            config.apis.claude_api_key.clone(),
            config.apis.claude_model.clone(),
            db.clone(),
        ),
        apollo: ApolloClient::new(config.apis.apollo_api_key.clone()),
        hunter: HunterClient::new(config.apis.hunter_api_key.clone()),
        researcher: CompanyResearcher::new(config.apis.claude_api_key.clone(), config.apis.claude_model.clone()),
        deep_researcher: DeepResearcher::new(
            config.research.clone(),
            config.apis.claude_api_key.clone(),
            config.apis.claude_model.clone(),
        ),
        drafter: EmailDrafter::new(config.apis.claude_api_key.clone(), config.apis.claude_model.clone(), config.company.clone()),
        sender: EmailSender::new(config.email.clone(), dry_run),
        reply_monitor: ReplyMonitor::new(config.imap.clone()),
        reply_analyzer: ReplyAnalyzer::new(config.apis.claude_api_key.clone(), config.apis.claude_model.clone()),
        followup_scheduler: FollowupScheduler::new(),
        metrics_tracker: MetricsTracker::new(db.clone()),
        alert_system: AlertSystem::new(config.email.clone(), config.alerts.clone(), dry_run),
        shutdown_manager: ShutdownManager::new(
            config.survival.clone(),
            db.clone(),
            AlertSystem::new(config.email.clone(), config.alerts.clone(), dry_run),
        ),
        db,
    })
}

async fn guard_shutdown(shutdown_manager: &ShutdownManager) -> Result<()> {
    let status = shutdown_manager.check_status().await?;
    if status.is_shutdown() {
        error!("💀  Skoutt has been permanently shut down (death rule triggered).");
        error!("    Run `skoutt status` to see the final report.");
        std::process::exit(1);
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// Daily cycle — all 7 phases
// ─────────────────────────────────────────────────────────────────────────

async fn run_daily_cycle(ctx: &AppContext) -> Result<()> {
    let cycle_start = chrono::Utc::now();
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  DAILY CYCLE — {}", cycle_start.format("%Y-%m-%d %H:%M UTC"));
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Phase 1: Discovery
    info!("📡  Phase 1: Exhibition Discovery");
    match ctx.exhibition_finder.discover_exhibitions().await {
        Ok(n) => info!("    {} new exhibitions found", n),
        Err(e) => warn!("    Discovery errors: {e}"),
    }

    // Phase 1b: Agentic Exhibitor Extraction
    info!("🕵️   Phase 1b: Agentic Exhibitor Extraction");
    let exhibitions = ctx.db.get_all_exhibitions().await?;
    let mut total_companies = 0;
    for exhibition in &exhibitions {
        match ctx.exhibitor_extractor.extract_exhibitors(exhibition).await {
            Ok(count) => total_companies += count,
            Err(e) => warn!("    Extraction failed for {}: {e}", exhibition.name),
        }
    }
    info!("    {} companies extracted from {} exhibitions", total_companies, exhibitions.len());

    // Phase 2: Enrichment
    info!("🔍  Phase 2: Lead Enrichment");
    let unenriched = ctx.db.get_unenriched_companies(50).await?;
    info!("    Enriching {} companies", unenriched.len());
    for company in &unenriched {
        match ctx.apollo.enrich_company(company, &ctx.db).await {
            Ok(contacts) => {
                for contact in contacts {
                    if let Ok(v) = ctx.hunter.verify_email(&contact.email).await {
                        if v.confidence > 0.5 {
                            let _ = ctx.db.upsert_contact(&contact).await;
                        }
                    }
                }
                let _ = ctx.db.mark_company_enriched(&company.id).await;
                info!("    ✓ {}", company.name);
            }
            Err(e) => warn!("    ✗ {} — {e}", company.name),
        }
    }

    // Phase 3: Research
    info!("🧠  Phase 3: Company Research");
    let to_research = ctx.db.get_contacts_needing_research(60).await?;
    info!("    Researching {} companies", to_research.len());
    for contact in &to_research {
        if let Some(company) = ctx.db.get_company(&contact.company_id).await? {
            match ctx.researcher.research_company(&company, &ctx.db).await {
                Ok(summary) => { let _ = ctx.db.update_company_research(&company.id, &summary).await; }
                Err(e) => warn!("    Research failed for {}: {e}", company.name),
            }
        }
    }

    // Phase 3a: Deep Research
    info!("🔬  Phase 3a: Deep Research");
    // Build research map: contact_id -> ResearchReport
    let mut research_map: std::collections::HashMap<String, skoutt::intelligence::deep_researcher::ResearchReport> = std::collections::HashMap::new();
    let research_targets = ctx.db.get_contacts_ready_for_outreach(60).await?;
    for (contact, company, participation) in &research_targets {
        if let Some(participation) = participation {
            if let Ok(Some(exhibition)) = ctx.db.get_exhibition(&participation.exhibition_id).await {
                match ctx.deep_researcher.research_contact(
                    contact, &company, participation, &exhibition, &ctx.db
                ).await {
                    Ok(Some(report)) => {
                        let _ = ctx.db.store_research_report(&report, 30).await;
                        research_map.insert(contact.id.clone(), report);
                    }
                    Ok(None) => {
                        info!("    ⏭  {} — skipping (below quality threshold)", contact.email);
                    }
                    Err(e) => warn!("    Research failed for {}: {e}", contact.email),
                }
            }
        }
    }
    info!("    {} contacts researched, {} passed quality gate", research_targets.len(), research_map.len());

    // Phase 4: Drafting
    info!("✍️   Phase 4: Email Drafting");
    let targets = ctx.db.get_contacts_ready_for_outreach(60).await?;
    info!("    Drafting {} emails", targets.len());
    let mut drafts = Vec::new();
    for (contact, company, participation) in &targets {
        // Only draft if we either have research (quality-gated) or research is disabled
        let research = research_map.get(&contact.id);
        if research.is_none() && !research_map.is_empty() {
            // Research ran but this contact didn't pass quality gate — skip
            continue;
        }
        match ctx.drafter.draft_initial_email(
            contact, company, participation.as_ref(), &ctx.db, research
        ).await {
            Ok(draft) => drafts.push((contact.clone(), draft)),
            Err(e) => warn!("    Draft failed for {}: {e}", contact.email),
        }
    }

    // Phase 5: Sending
    info!("📤  Phase 5: Email Sending");
    let mut sent = 0u32;
    for (contact, draft) in &drafts {
        match ctx.sender.send_email(contact, draft, &ctx.db).await {
            Ok(_) => { sent += 1; info!("    ✓ → {} <{}>", contact.full_name, contact.email); }
            Err(e) => warn!("    ✗ {}: {e}", contact.email),
        }
    }

    let followups = ctx.followup_scheduler.get_due_followups(&ctx.db).await?;
    info!("    {} follow-ups due", followups.len());
    for (contact, record, ftype) in &followups {
        if let Ok(draft) = ctx.drafter.draft_followup(contact, record, ftype, &ctx.db).await {
            if ctx.sender.send_email(contact, &draft, &ctx.db).await.is_ok() {
                sent += 1;
            }
        }
    }
    info!("    Total sent today: {}", sent);

    // Phase 6: Reply Monitoring
    info!("📬  Phase 6: Reply Monitoring");
    match ctx.reply_monitor.check_inbox(&ctx.db).await {
        Ok(replies) => {
            info!("    {} new replies", replies.len());
            for reply in &replies {
                if let Ok(analysis) = ctx.reply_analyzer.analyze_reply(reply, &ctx.db).await {
                    let _ = ctx.db.update_reply_analysis(&reply.email_id, &analysis).await;
                    if analysis.interest_level.is_actionable() {
                        info!("    🚨 INTERESTED: {} — {:?}", reply.from_email, analysis.interest_level);
                        if let Ok(Some(contact)) = ctx.db.get_contact_by_email(&reply.from_email).await {
                            if let Ok(Some(company)) = ctx.db.get_company(&contact.company_id).await {
                                // Build research briefing for the internal handoff
                                let briefing = match ctx.db.get_research_report(&contact.id).await {
                                    Ok(Some(report)) => {
                                        let mut b = format!("Company: {}\n", report.company_overview);
                                        b.push_str(&format!("Exhibition Strategy: {}\n", report.exhibition_strategy));
                                        if !report.pain_points.is_empty() {
                                            b.push_str("Pain Points:\n");
                                            for p in &report.pain_points {
                                                b.push_str(&format!("  • {}\n", p));
                                            }
                                        }
                                        if !report.personalization_hooks.is_empty() {
                                            b.push_str("Key Hooks:\n");
                                            for h in &report.personalization_hooks {
                                                b.push_str(&format!("  • {}\n", h));
                                            }
                                        }
                                        b.push_str(&format!("Suggested Angle: {}\n", report.email_angle));
                                        Some(b)
                                    }
                                    _ => None,
                                };

                                let _ = ctx.alert_system.send_interested_lead_alert(
                                    &contact, &company, reply, &analysis,
                                    briefing.as_deref(), None,
                                ).await;
                                let _ = ctx.metrics_tracker.record_interested_reply().await;
                            }
                        }
                    }
                }
            }
        }
        Err(e) => warn!("    Reply monitoring failed: {e}"),
    }

    // Phase 7: Survival Check
    info!("🧬  Phase 7: Survival Check");
    let survival = ctx.shutdown_manager.run_survival_check().await?;
    match survival.status {
        skoutt::survival::SurvivalStatus::GracePeriod =>
            info!("    🌱 Grace period — week {} of 2", survival.weeks_active),
        skoutt::survival::SurvivalStatus::Safe =>
            info!("    ✅ Healthy — {} interested replies this week", survival.interested_this_week),
        skoutt::survival::SurvivalStatus::Warning =>
            warn!("    ⚠️  WARNING: {} consecutive zero-reply weeks", survival.consecutive_zero_weeks),
        skoutt::survival::SurvivalStatus::Critical =>
            warn!("    🔴 CRITICAL: attempting emergency pivots"),
        skoutt::survival::SurvivalStatus::Shutdown => {
            error!("    💀 SHUTDOWN — death rule triggered after {} zero weeks", survival.consecutive_zero_weeks);
            std::process::exit(0);
        }
    }

    let elapsed = (chrono::Utc::now() - cycle_start).num_seconds();
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  Cycle complete in {}s", elapsed);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────

fn print_banner(dry_run: bool) {
    println!();
    println!("  ╔══════════════════════════════════════════════════════════╗");
    println!("  ║              SKOUTT  ·  Lead Generation Agent           ║");
    println!("  ║              Track Exhibits Pvt LTD  ·  v{}           ║", env!("CARGO_PKG_VERSION"));
    println!("  ╚══════════════════════════════════════════════════════════╝");
    if dry_run {
        println!("  ⚠️   DRY RUN MODE — no emails will be sent");
    }
    println!();
}

async fn export_leads_csv(ctx: &AppContext, path: &str) -> Result<()> {
    info!("📊  Exporting leads to {}...", path);
    
    let leads = sqlx::query!(
        r#"
        SELECT 
            c.name as company_name,
            c.website as "website?",
            co.full_name as contact_name,
            co.email as "contact_email?",
            co.job_title as "job_title?",
            rr.email_angle as "email_angle?",
            rr.exhibition_strategy as "strategy?",
            rr.company_overview as "overview?"
        FROM contacts co
        JOIN companies c ON co.company_id = c.id
        LEFT JOIN research_reports rr ON co.id = rr.contact_id
        WHERE co.email IS NOT NULL 
          AND co.do_not_contact = 0
        ORDER BY c.name ASC
        "#
    )
    .fetch_all(&ctx.db.pool)
    .await?;

    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record(&["Company", "Website", "Contact Name", "Contact Email", "Job Title", "Strategy", "Research Summary"])?;

    for lead in &leads {
        wtr.write_record(&[
            &lead.company_name,
            lead.website.as_deref().unwrap_or_default(),
            &lead.contact_name,
            lead.contact_email.as_deref().unwrap_or_default(),
            lead.job_title.as_deref().unwrap_or_default(),
            lead.strategy.as_deref().unwrap_or_default(),
            lead.overview.as_deref().unwrap_or_default(),
        ])?;
    }

    wtr.flush()?;
    info!("✅  Successfully exported {} leads", leads.len());
    Ok(())
}

async fn print_database_stats(db: &Database) -> Result<()> {
    // We use raw queries for stats since we don't have count methods
    let exhibitions = sqlx::query!("SELECT COUNT(*) as count FROM exhibitions")
        .fetch_one(&db.pool).await?.count;
    let companies = sqlx::query!("SELECT COUNT(*) as count FROM companies")
        .fetch_one(&db.pool).await?.count;
    let contacts = sqlx::query!("SELECT COUNT(*) as count FROM contacts")
        .fetch_one(&db.pool).await?.count;
    let emails = sqlx::query!("SELECT COUNT(*) as count FROM emails_sent")
        .fetch_one(&db.pool).await?.count;
    let interested = sqlx::query!("SELECT COUNT(*) as count FROM emails_sent WHERE interest_level IN ('High', 'Medium')")
        .fetch_one(&db.pool).await?.count;
    let dnc = sqlx::query!("SELECT COUNT(*) as count FROM contacts WHERE do_not_contact = 1")
        .fetch_one(&db.pool).await?.count;

    println!("\n{}", "━".repeat(60));
    println!("  SKOUTT DATABASE STATISTICS");
    println!("{}", "━".repeat(60));
    println!("  Exhibitions tracked:    {}", exhibitions);
    println!("  Companies found:        {}", companies);
    println!("  Contacts enriched:      {}", contacts);
    println!("  Emails sent:            {}", emails);
    println!("  Interested leads:       {}", interested);
    println!("  Do-not-contact:         {}", dnc);
    println!("{}", "━".repeat(60));
    Ok(())
}

fn next_daily_run_time(now: chrono::DateTime<chrono::Utc>, hour: u32) -> chrono::DateTime<chrono::Utc> {
    let today_run = now.date_naive().and_hms_opt(hour, 0, 0).unwrap();
    let today_run_utc = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(today_run, chrono::Utc);
    if now < today_run_utc { today_run_utc } else { today_run_utc + chrono::Duration::days(1) }
}

fn setup_logging(level: &str, log_dir: &str) -> Result<()> {
    std::fs::create_dir_all(log_dir)?;

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .with_thread_ids(false)
        .compact()
        .init();

    Ok(())
}
