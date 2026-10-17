//! Scores listings against a candidate profile so the most relevant jobs float
//! to the top. The profile is a small TOML file or a set of CLI flags.

use crate::model::Job;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Profile {
    /// Terms worth one point each when found in the listing.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Strong signals worth three points (usually title fragments).
    #[serde(default)]
    pub titles: Vec<String>,
    /// Any match drops the listing entirely (e.g. "unpaid", "internship").
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl Profile {
    pub fn from_toml(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading profile {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing profile {}", path.display()))
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty() && self.titles.is_empty() && self.exclude.is_empty()
    }

    /// Returns `true` if the job should be kept, computing its score in place.
    pub fn evaluate(&self, job: &mut Job) -> bool {
        let haystack_title = job.title.to_lowercase();
        let haystack_all = format!("{} {} {}", job.title, job.company, job.location).to_lowercase();

        if self
            .exclude
            .iter()
            .any(|t| haystack_all.contains(&t.to_lowercase()))
        {
            return false;
        }

        let mut score = 0i32;
        let mut matched = Vec::new();

        for term in &self.titles {
            let term = term.to_lowercase();
            if !term.is_empty() && haystack_title.contains(&term) {
                score += 3;
                matched.push(term);
            }
        }
        for term in &self.skills {
            let term = term.to_lowercase();
            if term.is_empty() {
                continue;
            }
            if haystack_title.contains(&term) {
                score += 2;
                matched.push(term);
            } else if haystack_all.contains(&term) {
                score += 1;
                matched.push(term);
            }
        }

        job.score = score;
        job.matched = matched;
        true
    }
}
