//! Data model shared across the scraper.

use serde::Serialize;

/// A single job listing scraped from LinkedIn's public search results.
#[derive(Debug, Clone, Serialize)]
pub struct Job {
    /// LinkedIn job posting id (numeric).
    pub id: String,
    pub title: String,
    pub company: String,
    pub location: String,
    pub url: String,
    /// ISO date (YYYY-MM-DD) the posting was listed, when available.
    pub posted: Option<String>,
    /// Human relative age as shown by LinkedIn (e.g. "2 months ago").
    pub posted_ago: Option<String>,
    /// The location query that produced this listing.
    pub source_region: String,
    /// Relevance score against the user's profile skills/keywords.
    pub score: i32,
    /// Which profile terms matched this listing.
    pub matched: Vec<String>,
}

impl Job {
    /// Trims and collapses runs of whitespace, as LinkedIn HTML is heavily indented.
    pub fn clean(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}
