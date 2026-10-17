//! Parses LinkedIn's public search-result HTML fragments into [`Job`] values.

use crate::model::Job;
use scraper::{ElementRef, Html, Selector};
use std::sync::OnceLock;

// LinkedIn has used a few different class names over time; selector groups keep
// this resilient to cosmetic markup changes.
fn card_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("div.base-search-card, li > div.base-card").unwrap())
}

fn sel(selector: &str) -> Selector {
    Selector::parse(selector).expect("valid selector")
}

fn text_of(el: &ElementRef, selector: &Selector) -> Option<String> {
    el.select(selector)
        .next()
        .map(|n| Job::clean(&n.text().collect::<String>()))
        .filter(|s| !s.is_empty())
}

/// Parse the HTML fragment and return every job card found.
pub fn parse_jobs(html: &str, source_region: &str) -> Vec<Job> {
    let document = Html::parse_fragment(html);
    let title_sel = sel("h3.base-search-card__title");
    let company_sel = sel("h4.base-search-card__subtitle a, h4.base-search-card__subtitle");
    let location_sel = sel("span.job-search-card__location");
    let link_sel = sel("a.base-card__full-link");
    let time_sel = sel("time.job-search-card__listdate, time.job-search-card__listdate--new");

    let mut jobs = Vec::new();
    for card in document.select(card_selector()) {
        let title = match text_of(&card, &title_sel) {
            Some(t) => t,
            None => continue, // not a real job card
        };

        let url = card
            .select(&link_sel)
            .next()
            .and_then(|a| a.value().attr("href"))
            .map(strip_query)
            .unwrap_or_default();

        let id = card
            .value()
            .attr("data-entity-urn")
            .and_then(|urn| urn.rsplit(':').next())
            .filter(|s| s.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_string)
            .or_else(|| id_from_url(&url))
            .unwrap_or_default();

        let (posted, posted_ago) = card
            .select(&time_sel)
            .next()
            .map(|t| {
                let date = t.value().attr("datetime").map(str::to_string);
                let ago = {
                    let s = Job::clean(&t.text().collect::<String>());
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                };
                (date, ago)
            })
            .unwrap_or((None, None));

        jobs.push(Job {
            id,
            title,
            company: text_of(&card, &company_sel).unwrap_or_default(),
            location: text_of(&card, &location_sel).unwrap_or_default(),
            url,
            posted,
            posted_ago,
            source_region: source_region.to_string(),
            score: 0,
            matched: Vec::new(),
        });
    }
    jobs
}

/// Drop tracking query string: `.../view/foo-123?position=1` -> `.../view/foo-123`.
fn strip_query(url: &str) -> String {
    url.split('?')
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_string()
}

/// LinkedIn slugs end with `-<jobId>`, e.g. `.../at-keyrock-4451488708`.
fn id_from_url(url: &str) -> Option<String> {
    url.rsplit('-')
        .next()
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
}
