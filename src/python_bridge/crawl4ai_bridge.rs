// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Skoutt — python_bridge/crawl4ai_bridge.rs
// Async Rust bridge to the Python crawl4ai_scraper.py subprocess.
// Uses tokio::process::Command with JSON in/out via stdin/stdout pipes.
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{debug, warn};

// ── Request / Response types ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ScrapeRequest {
    mode: String,
    urls: Vec<String>,
    max_chars_per_page: usize,
    timeout: u64,
}

#[derive(Debug, Serialize)]
struct NewsRequest {
    mode: String,
    company_name: String,
    max_articles: usize,
}

#[derive(Debug, Deserialize)]
struct ScrapeResponse {
    results: Option<Vec<ScrapedPage>>,
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScrapedPage {
    pub url: String,
    pub content: String,
    pub success: bool,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Deserialize)]
struct NewsResponse {
    articles: Option<Vec<NewsResult>>,
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewsResult {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub full_content: String,
}

// ── Public API ────────────────────────────────────────────────────────────

/// Scrape one or more URLs using Crawl4AI (JS-rendered).
/// Returns clean markdown content for each URL.
///
/// Falls back to empty results if:
/// - Python/crawl4ai not available
/// - Subprocess times out (60s total)
/// - Any parsing error occurs
pub async fn scrape_pages(
    urls: Vec<String>,
    max_chars_per_page: usize,
) -> Result<Vec<ScrapedPage>> {
    let request = ScrapeRequest {
        mode: "scrape".into(),
        urls: urls.clone(),
        max_chars_per_page,
        timeout: 30,
    };

    let input = serde_json::to_string(&request)?;
    
    match run_crawler_subprocess(&input, Duration::from_secs(90)).await {
        Ok(output) => {
            let resp: ScrapeResponse = serde_json::from_str(&output)
                .map_err(|e| {
                    warn!("  Crawl4AI response parse error: {e}");
                    anyhow::anyhow!("Parse error: {e}")
                })?;

            if let Some(err) = resp.error {
                warn!("  Crawl4AI error: {err}");
                return Ok(vec![]);
            }

            Ok(resp.results.unwrap_or_default())
        }
        Err(e) => {
            warn!("  Crawl4AI subprocess failed: {e}");
            Ok(vec![])
        }
    }
}

/// Search for company news using Crawl4AI (scrapes Google News).
/// Falls back to empty results on failure.
pub async fn search_news_via_crawl4ai(
    company_name: &str,
    max_articles: usize,
) -> Result<Vec<NewsResult>> {
    let request = NewsRequest {
        mode: "news".into(),
        company_name: company_name.to_string(),
        max_articles,
    };

    let input = serde_json::to_string(&request)?;

    match run_crawler_subprocess(&input, Duration::from_secs(90)).await {
        Ok(output) => {
            let resp: NewsResponse = serde_json::from_str(&output)
                .map_err(|e| {
                    warn!("  Crawl4AI news parse error: {e}");
                    anyhow::anyhow!("Parse error: {e}")
                })?;

            if let Some(err) = resp.error {
                warn!("  Crawl4AI news error: {err}");
                return Ok(vec![]);
            }

            Ok(resp.articles.unwrap_or_default())
        }
        Err(e) => {
            warn!("  Crawl4AI news subprocess failed: {e}");
            Ok(vec![])
        }
    }
}

// ── Fetch HTML (raw HTML for sites behind Cloudflare/JS) ─────────────────

#[derive(Debug, Serialize)]
struct FetchHtmlRequest {
    mode: String,
    url: String,
    timeout: u64,
}

#[derive(Debug, Deserialize)]
struct FetchHtmlResponse {
    html: Option<String>,
    success: bool,
    error: Option<String>,
}

/// Fetch raw rendered HTML from a URL using Crawl4AI (headless browser).
/// Used as fallback when reqwest gets 403 from Cloudflare-protected sites.
pub async fn fetch_html_via_crawl4ai(url: &str) -> Result<String> {
    let request = FetchHtmlRequest {
        mode: "fetch_html".into(),
        url: url.to_string(),
        timeout: 30,
    };

    let input = serde_json::to_string(&request)?;

    match run_crawler_subprocess(&input, Duration::from_secs(60)).await {
        Ok(output) => {
            let resp: FetchHtmlResponse = serde_json::from_str(&output)
                .map_err(|e| {
                    warn!("  Crawl4AI fetch_html parse error: {e}");
                    anyhow::anyhow!("Parse error: {e}")
                })?;

            if let Some(err) = resp.error {
                warn!("  Crawl4AI fetch_html error: {err}");
                return Err(anyhow::anyhow!("Crawl4AI: {err}"));
            }

            resp.html
                .filter(|h| !h.is_empty())
                .ok_or_else(|| anyhow::anyhow!("Crawl4AI returned empty HTML"))
        }
        Err(e) => {
            warn!("  Crawl4AI fetch_html subprocess failed: {e}");
            Err(e)
        }
    }
}

// ── Internal ──────────────────────────────────────────────────────────────

async fn run_crawler_subprocess(input_json: &str, timeout: Duration) -> Result<String> {
    debug!("  Calling crawl4ai_scraper.py …");

    let mut child = Command::new("python3")
        .arg("-m")
        .arg("python.intelligence.crawl4ai_scraper")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn crawl4ai subprocess: {e}"))?;

    // Write JSON to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input_json.as_bytes()).await?;
        stdin.shutdown().await?;
    }

    // Wait with timeout
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("Crawl4AI subprocess timed out after {:?}", timeout))?
        .map_err(|e| anyhow::anyhow!("Crawl4AI subprocess error: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("  Crawl4AI stderr: {}", stderr.chars().take(500).collect::<String>());
        return Err(anyhow::anyhow!(
            "Crawl4AI process exited with code {:?}",
            output.status.code()
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("Crawl4AI output not UTF-8: {e}"))?;

    // Crawl4AI prints "[INIT].... → Crawl4AI 0.8.0" to stdout before our JSON.
    // Extract only the JSON line (starts with '{').
    let json_line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .unwrap_or(&stdout);

    Ok(json_line.to_string())
}
