//! `linkedin-jobs` — scrape public LinkedIn job listings for a profile.
//!
//! This uses LinkedIn's *public/guest* HTML endpoint (the same one an anonymous
//! browser gets) and does not log in. It is intended for personal, low-volume
//! use. Automated scraping may violate LinkedIn's User Agreement — review it
//! before use and prefer LinkedIn's official APIs for anything commercial.

mod client;
mod locations;
mod model;
mod output;
mod parse;
mod profile;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use client::{LinkedInClient, SearchParams};
use model::Job;
use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const PAGE_SIZE: u32 = 10;

#[derive(Parser, Debug)]
#[command(
    name = "linkedin-jobs",
    version,
    about = "Scrape public LinkedIn job listings for your profile (Tunisia, France, Europe / EMEA)",
    long_about = None,
)]
struct Cli {
    /// Search keywords, e.g. "rust developer" (required).
    #[arg(short, long)]
    keywords: String,

    /// Regions/presets to search. Repeat or comma-separate.
    #[arg(
        short,
        long,
        value_delimiter = ',',
        default_value = "tunisia,france,europe"
    )]
    region: Vec<String>,

    /// Additional literal LinkedIn locations, e.g. -l "Berlin, Germany".
    #[arg(short, long, value_delimiter = ',')]
    location: Vec<String>,

    /// TOML profile with `skills`, `titles` and `exclude` arrays.
    #[arg(short, long)]
    profile: Option<PathBuf>,

    /// Pages per region (10 results each). Default 3 => up to 30 per region.
    #[arg(long, default_value_t = 3)]
    pages: u32,

    /// Cap the number of results after de-duplication and sorting.
    #[arg(long)]
    limit: Option<usize>,

    /// How to order results.
    #[arg(long, value_enum, default_value_t = SortBy::Score)]
    sort: SortBy,

    /// Only jobs posted within this window.
    #[arg(long, value_enum)]
    date_posted: Option<DatePosted>,

    /// Workplace type (repeatable).
    #[arg(long, value_enum, value_delimiter = ',')]
    workplace: Vec<Workplace>,

    /// Experience level 1..6 (1 intern, 2 entry, 3 associate, 4 mid-senior,
    /// 5 director, 6 executive). Repeat or comma-separate.
    #[arg(long, value_delimiter = ',')]
    experience: Vec<u8>,

    /// Job type: F full-time, P part-time, C contract, T temporary, I internship.
    #[arg(long, value_delimiter = ',')]
    job_type: Vec<String>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Table)]
    format: Format,

    /// Write output to a file instead of stdout.
    #[arg(short, long)]
    out: Option<PathBuf>,

    /// Delay between HTTP requests in milliseconds.
    #[arg(long, default_value_t = 1500)]
    delay_ms: u64,

    /// Retry attempts for transient/rate-limited responses.
    #[arg(long, default_value_t = 3)]
    retries: u32,

    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = 20)]
    timeout: u64,

    /// Disable coloured output.
    #[arg(long)]
    no_color: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Format {
    Table,
    Json,
    Csv,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum SortBy {
    /// Most relevant to your profile first.
    Score,
    /// Most recently posted first.
    Date,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum DatePosted {
    #[value(name = "24h")]
    Day,
    Week,
    Month,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Workplace {
    Onsite,
    Remote,
    Hybrid,
}

impl DatePosted {
    fn code(self) -> String {
        match self {
            DatePosted::Day => "r86400",
            DatePosted::Week => "r604800",
            DatePosted::Month => "r2592000",
        }
        .to_string()
    }
}

impl Workplace {
    fn code(self) -> u8 {
        match self {
            Workplace::Onsite => 1,
            Workplace::Remote => 2,
            Workplace::Hybrid => 3,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    let regions = resolve_regions(cli);
    let profile = load_profile(cli)?;

    let keyword_list = cli.keywords.clone();

    let mut http = LinkedInClient::new(cli.delay_ms, cli.retries, cli.timeout)?;

    let mut seen: HashSet<String> = HashSet::new();
    let mut jobs: Vec<Job> = Vec::new();

    for region in &regions {
        eprintln!("· searching \"{}\" in {region}", cli.keywords);
        for page in 0..cli.pages {
            let params = build_params(cli, region, page * PAGE_SIZE);
            let html = match http.fetch_page(&params) {
                Ok(h) => h,
                Err(err) => {
                    eprintln!("  ! {region} page {page} failed: {err:#}");
                    break;
                }
            };
            if html.is_empty() {
                break; // no more results
            }

            let page_jobs = parse::parse_jobs(&html, region);
            let found = page_jobs.len();
            for job in page_jobs {
                let key = if job.id.is_empty() {
                    job.url.clone()
                } else {
                    job.id.clone()
                };
                if seen.insert(key) {
                    jobs.push(job);
                }
            }

            if found < PAGE_SIZE as usize {
                break; // short page => last page
            }
            http.polite_pause();
        }
    }

    // Apply the profile (drops excluded jobs, fills in scores).
    if !profile.is_empty() {
        jobs.retain_mut(|job| profile.evaluate(job));
    }

    sort_jobs(&mut jobs, cli.sort, !profile.is_empty());

    if let Some(limit) = cli.limit {
        jobs.truncate(limit);
    }

    emit(cli, &jobs, &keyword_list, &regions)
}

fn resolve_regions(cli: &Cli) -> Vec<String> {
    let mut regions: Vec<String> = Vec::new();
    for r in &cli.region {
        for loc in locations::expand(r) {
            if !regions.contains(&loc) {
                regions.push(loc);
            }
        }
    }
    for loc in &cli.location {
        if !regions.contains(loc) {
            regions.push(loc.clone());
        }
    }
    regions
}

fn load_profile(cli: &Cli) -> Result<profile::Profile> {
    match &cli.profile {
        Some(path) => profile::Profile::from_toml(path),
        None => Ok(profile::Profile::default()),
    }
}

fn build_params(cli: &Cli, region: &str, start: u32) -> SearchParams {
    SearchParams {
        keywords: cli.keywords.clone(),
        location: region.to_string(),
        start,
        date_posted: cli.date_posted.map(DatePosted::code),
        workplace: cli.workplace.iter().map(|w| w.code()).collect(),
        experience: cli
            .experience
            .iter()
            .copied()
            .filter(|e| (1..=6).contains(e))
            .collect(),
        job_type: cli.job_type.clone(),
        sort_by_date: cli.sort == SortBy::Date,
    }
}

fn sort_jobs(jobs: &mut [Job], sort: SortBy, has_profile: bool) {
    match sort {
        SortBy::Date => {
            jobs.sort_by(|a, b| b.posted.cmp(&a.posted).then(b.score.cmp(&a.score)));
        }
        SortBy::Score if has_profile => {
            jobs.sort_by(|a, b| b.score.cmp(&a.score).then(b.posted.cmp(&a.posted)));
        }
        SortBy::Score => {
            // No profile to score against: fall back to recency.
            jobs.sort_by(|a, b| b.posted.cmp(&a.posted));
        }
    }
}

fn emit(cli: &Cli, jobs: &[Job], keywords: &str, regions: &[String]) -> Result<()> {
    let mut buffer: Vec<u8> = Vec::new();
    let color = !cli.no_color && std::io::stdout().is_terminal() && cli.out.is_none();

    match cli.format {
        Format::Table => {
            output::print_summary(&mut buffer, jobs, keywords, regions, color)?;
            output::print_table(&mut buffer, jobs, color)?;
        }
        Format::Json => output::write_json(&mut buffer, jobs)?,
        Format::Csv => output::write_csv(&mut buffer, jobs)?,
    }

    match &cli.out {
        Some(path) => {
            std::fs::write(path, &buffer).with_context(|| format!("writing {}", path.display()))?;
            eprintln!("wrote {} result(s) to {}", jobs.len(), path.display());
        }
        None => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            lock.write_all(&buffer)?;
            lock.flush()?;
        }
    }
    Ok(())
}
