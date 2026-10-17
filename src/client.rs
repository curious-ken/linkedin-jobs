//! Blocking HTTP client for LinkedIn's public ("guest") job search endpoint.
//!
//! No authentication is used: this hits the same public HTML fragment that an
//! anonymous browser sees. Be polite — the caller spaces requests out.

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client as HttpClient;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, REFERER};
use reqwest::StatusCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SEARCH_URL: &str = "https://www.linkedin.com/jobs-guest/jobs/api/seeMoreJobPostings/search";

const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15",
];

/// Everything needed to build one search request.
#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub keywords: String,
    pub location: String,
    pub start: u32,
    /// `f_TPR`: r86400 (24h), r604800 (week), r2592000 (month).
    pub date_posted: Option<String>,
    /// `f_WT`: 1 on-site, 2 remote, 3 hybrid.
    pub workplace: Vec<u8>,
    /// `f_E`: 1 internship .. 6 executive.
    pub experience: Vec<u8>,
    /// `f_JT`: F full-time, P part-time, C contract, T temporary, I internship.
    pub job_type: Vec<String>,
    /// Sort most recent first (`DD`) instead of relevance (`R`).
    pub sort_by_date: bool,
}

pub struct LinkedInClient {
    http: HttpClient,
    delay: Duration,
    retries: u32,
    ua_index: usize,
}

impl LinkedInClient {
    pub fn new(delay_ms: u64, retries: u32, timeout_secs: u64) -> Result<Self> {
        let http = HttpClient::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .gzip(true)
            .build()
            .context("failed to build HTTP client")?;
        Ok(LinkedInClient {
            http,
            delay: Duration::from_millis(delay_ms),
            retries,
            ua_index: 0,
        })
    }

    fn next_user_agent(&mut self) -> &'static str {
        let ua = USER_AGENTS[self.ua_index % USER_AGENTS.len()];
        self.ua_index += 1;
        ua
    }

    fn headers(&mut self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
        headers.insert(
            REFERER,
            HeaderValue::from_static("https://www.linkedin.com/jobs/search/"),
        );
        let ua = self.next_user_agent();
        headers.insert(
            reqwest::header::USER_AGENT,
            HeaderValue::from_str(ua).context("invalid user agent")?,
        );
        Ok(headers)
    }

    /// Fetch one page of results. Retries transient failures with exponential
    /// backoff and returns the raw HTML fragment.
    pub fn fetch_page(&mut self, params: &SearchParams) -> Result<String> {
        let url = self.build_url(params);
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let headers = self.headers()?;
            let result = self.http.get(&url).headers(headers).send();

            match result {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let body = resp.text().unwrap_or_default();
                        // A normal page is tens of KB; the end-of-results page
                        // is a tiny skeleton. Treat tiny bodies as "no more".
                        if body.len() < 512 && !body.contains("base-search-card") {
                            return Ok(String::new());
                        }
                        return Ok(body);
                    }

                    if is_transient(status) && attempt <= self.retries {
                        self.backoff(attempt, &format!("HTTP {}", status.as_u16()));
                        continue;
                    }
                    bail!("LinkedIn returned HTTP {} for {}", status.as_u16(), url);
                }
                Err(err) => {
                    if attempt <= self.retries {
                        self.backoff(attempt, &format!("request error: {err}"));
                        continue;
                    }
                    return Err(err).with_context(|| format!("request failed for {url}"));
                }
            }
        }
    }

    /// Sleep between pages so we do not hammer the endpoint.
    pub fn polite_pause(&self) {
        std::thread::sleep(self.delay + jitter(self.delay));
    }

    fn backoff(&self, attempt: u32, reason: &str) {
        let base = self.delay.as_millis().max(500) as u64;
        let backoff = base * (1 << attempt.min(6));
        eprintln!(
            "  ! {reason}; backing off {} ms (attempt {}/{})",
            backoff, attempt, self.retries
        );
        std::thread::sleep(Duration::from_millis(backoff));
    }

    fn build_url(&self, params: &SearchParams) -> String {
        let mut url = format!("{}?keywords={}", SEARCH_URL, urlencode(&params.keywords));
        if !params.location.is_empty() {
            url.push_str(&format!("&location={}", urlencode(&params.location)));
        }
        url.push_str(&format!("&start={}", params.start));
        if let Some(tpr) = &params.date_posted {
            url.push_str(&format!("&f_TPR={}", tpr));
        }
        if !params.workplace.is_empty() {
            url.push_str(&format!("&f_WT={}", join_u8(&params.workplace)));
        }
        if !params.experience.is_empty() {
            url.push_str(&format!("&f_E={}", join_u8(&params.experience)));
        }
        if !params.job_type.is_empty() {
            url.push_str(&format!("&f_JT={}", params.job_type.join(",")));
        }
        url.push_str(if params.sort_by_date {
            "&sortBy=DD"
        } else {
            "&sortBy=R"
        });
        url
    }
}

fn is_transient(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::REQUEST_TIMEOUT
        || status.as_u16() == 999 // LinkedIn's unofficial "blocked" code
        || status.is_server_error()
}

fn jitter(d: Duration) -> Duration {
    let span = d.as_millis().max(1) as u64;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    Duration::from_millis(nanos % span)
}

fn join_u8(values: &[u8]) -> String {
    values
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Minimal percent-encoding for query values (RFC 3986 unreserved kept).
fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}
