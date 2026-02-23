#!/usr/bin/env python3
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Skoutt — python/intelligence/crawl4ai_scraper.py
# Standalone CLI for JS-rendered web scraping via Crawl4AI (v0.8.x).
# Called from Rust as a subprocess: JSON in via stdin, JSON out via stdout.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

import asyncio
import json
import os
import sys
import traceback
from typing import Optional


# Suppress Crawl4AI's [INIT] banner that gets printed to stdout
# (would break JSON parsing in the Rust bridge)
os.environ["CRAWL4AI_QUIET"] = "1"


def _get_raw_markdown(result) -> str:
    """Extract raw markdown from a CrawlResult, handling both v0.8.x
    (MarkdownGenerationResult object) and older versions (plain string)."""
    md = result.markdown
    if md is None:
        return ""
    # v0.8.x: md is a MarkdownGenerationResult with .raw_markdown
    if hasattr(md, "raw_markdown"):
        return md.raw_markdown or ""
    # Older versions: md is a plain string
    if isinstance(md, str):
        return md
    return str(md)


async def scrape_urls(urls: list[str], max_chars: int = 3000, timeout: int = 30) -> list[dict]:
    """Scrape a list of URLs using Crawl4AI's AsyncWebCrawler.
    
    Returns clean markdown content for each URL.
    Falls back gracefully if crawl4ai is not installed.
    """
    try:
        from crawl4ai import AsyncWebCrawler, BrowserConfig, CrawlerRunConfig
    except ImportError:
        return [{"url": u, "content": "", "success": False, "error": "crawl4ai not installed"} for u in urls]

    results = []
    
    browser_config = BrowserConfig(
        headless=True,
    )
    
    crawler_config = CrawlerRunConfig(
        word_count_threshold=50,
        verbose=False,
    )

    try:
        async with AsyncWebCrawler(config=browser_config) as crawler:
            for url in urls:
                try:
                    result = await asyncio.wait_for(
                        crawler.arun(url=url, config=crawler_config),
                        timeout=timeout,
                    )
                    
                    if result.success:
                        content = _get_raw_markdown(result)
                        if not content:
                            content = result.cleaned_html or ""
                        
                        # Truncate to max chars
                        if len(content) > max_chars:
                            content = content[:max_chars] + "\n...[truncated]"
                        
                        results.append({
                            "url": url,
                            "content": content,
                            "success": True,
                            "title": getattr(result, 'title', '') or '',
                        })
                    else:
                        results.append({
                            "url": url,
                            "content": "",
                            "success": False,
                            "error": f"Crawl failed: {getattr(result, 'error_message', 'unknown')}",
                        })
                        
                except asyncio.TimeoutError:
                    results.append({
                        "url": url,
                        "content": "",
                        "success": False,
                        "error": f"Timeout after {timeout}s",
                    })
                except Exception as e:
                    results.append({
                        "url": url,
                        "content": "",
                        "success": False,
                        "error": str(e),
                    })

    except Exception as e:
        return [{"url": u, "content": "", "success": False, "error": f"Browser init failed: {e}"} for u in urls]

    return results


async def search_news_via_crawl4ai(
    company_name: str,
    max_articles: int = 5,
    max_chars_per_article: int = 1500,
    timeout: int = 30,
) -> list[dict]:
    """Scrape Google News search results for a company using Crawl4AI.
    
    This replaces the SerpAPI/Google Custom Search approach by directly
    scraping Google News. Falls back gracefully if it fails.
    """
    try:
        from crawl4ai import AsyncWebCrawler, BrowserConfig, CrawlerRunConfig
    except ImportError:
        return []

    import urllib.parse
    
    # Google News search URL — sort by date (recent first)
    query = urllib.parse.quote(f"{company_name} exhibition booth trade show")
    google_news_url = f"https://news.google.com/search?q={query}&hl=en&gl=US&ceid=US:en"
    
    # Also try regular Google with news tab
    google_search_url = f"https://www.google.com/search?q={urllib.parse.quote(company_name + ' news')}&tbm=nws"
    
    browser_config = BrowserConfig(
        headless=True,
    )
    
    crawler_config = CrawlerRunConfig(
        word_count_threshold=30,
        verbose=False,
    )

    articles = []

    try:
        async with AsyncWebCrawler(config=browser_config) as crawler:
            # Try Google search news tab first
            try:
                result = await asyncio.wait_for(
                    crawler.arun(url=google_search_url, config=crawler_config),
                    timeout=timeout,
                )
                
                if result.success:
                    content = _get_raw_markdown(result)
                    if content:
                        articles = _parse_news_from_markdown(
                            content,
                            max_articles=max_articles,
                        )
            except (asyncio.TimeoutError, Exception) as e:
                # Google search failed, try Google News
                try:
                    result = await asyncio.wait_for(
                        crawler.arun(url=google_news_url, config=crawler_config),
                        timeout=timeout,
                    )
                    if result.success:
                        content = _get_raw_markdown(result)
                        if content:
                            articles = _parse_news_from_markdown(
                                content,
                                max_articles=max_articles,
                            )
                except Exception:
                    pass
                    
            # If we got article URLs, try to scrape a few for richer content
            enriched = []
            for article in articles[:3]:  # Only enrich top 3
                if article.get("url") and article["url"].startswith("http"):
                    try:
                        detail = await asyncio.wait_for(
                            crawler.arun(url=article["url"], config=crawler_config),
                            timeout=15,
                        )
                        if detail.success:
                            detail_md = _get_raw_markdown(detail)
                            if detail_md:
                                article["full_content"] = detail_md[:max_chars_per_article]
                    except Exception:
                        pass
                enriched.append(article)
            
            # Add remaining non-enriched articles
            enriched.extend(articles[3:])
            return enriched[:max_articles]

    except Exception:
        return []


def _parse_news_from_markdown(markdown: str, max_articles: int = 5) -> list[dict]:
    """Extract news articles from Google search results markdown.
    
    Looks for patterns like: [Title](url) ... snippet text
    """
    import re
    
    articles = []
    
    # Find markdown links that look like article titles
    # Pattern: [Title text](https://url)
    link_pattern = re.compile(r'\[([^\]]{10,})\]\((https?://[^\)]+)\)')
    
    lines = markdown.split('\n')
    
    i = 0
    while i < len(lines) and len(articles) < max_articles:
        line = lines[i]
        matches = link_pattern.findall(line)
        
        for title, url in matches:
            # Skip Google's own links, navigation, etc
            if any(skip in url.lower() for skip in [
                'google.com', 'accounts.google', 'support.google',
                'policies.google', 'maps.google', '#', 'javascript:',
            ]):
                continue
            
            # Skip navigation-like titles
            if len(title) < 15 or title.lower() in ['sign in', 'help', 'settings']:
                continue
            
            # Grab snippet from surrounding lines
            snippet = ""
            for j in range(i + 1, min(i + 4, len(lines))):
                candidate = lines[j].strip()
                if candidate and not candidate.startswith('[') and not candidate.startswith('#'):
                    snippet = candidate[:300]
                    break
            
            articles.append({
                "title": title.strip(),
                "url": url.strip(),
                "snippet": snippet,
                "source": _extract_domain(url),
                "full_content": "",
            })
            
            if len(articles) >= max_articles:
                break
        
        i += 1
    
    return articles


def _extract_domain(url: str) -> str:
    """Extract clean domain name from URL."""
    try:
        from urllib.parse import urlparse
        parsed = urlparse(url)
        domain = parsed.netloc
        if domain.startswith('www.'):
            domain = domain[4:]
        return domain
    except Exception:
        return ""


# ── CLI entry point ────────────────────────────────────────────────────────

async def main():
    """Read JSON from stdin, scrape, write JSON to stdout."""
    try:
        raw = sys.stdin.read()
        request = json.loads(raw)
    except (json.JSONDecodeError, Exception) as e:
        print(json.dumps({"error": f"Invalid input: {e}"}), file=sys.stdout)
        sys.exit(1)

    mode = request.get("mode", "scrape")
    
    if mode == "scrape":
        urls = request.get("urls", [])
        max_chars = request.get("max_chars_per_page", 3000)
        timeout = request.get("timeout", 30)
        
        results = await scrape_urls(urls, max_chars=max_chars, timeout=timeout)
        print(json.dumps({"results": results}), file=sys.stdout)

    elif mode == "news":
        company_name = request.get("company_name", "")
        max_articles = request.get("max_articles", 5)
        
        articles = await search_news_via_crawl4ai(
            company_name,
            max_articles=max_articles,
        )
        print(json.dumps({"articles": articles}), file=sys.stdout)
    
    else:
        print(json.dumps({"error": f"Unknown mode: {mode}"}), file=sys.stdout)
        sys.exit(1)


if __name__ == "__main__":
    asyncio.run(main())
