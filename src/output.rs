//! Renders results as a table, JSON or CSV into any writer.

use crate::model::Job;
use anyhow::Result;
use std::io::Write;

fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn colored(code: &str, s: &str, enabled: bool) -> String {
    if enabled {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn print_summary(
    out: &mut dyn Write,
    jobs: &[Job],
    keywords: &str,
    regions: &[String],
    color: bool,
) -> Result<()> {
    writeln!(out, "{}", colored("1", "LinkedIn job results", color))?;
    writeln!(
        out,
        "{} listing(s) · keywords: \"{}\" · regions: {}",
        jobs.len(),
        keywords,
        regions.join(", ")
    )?;
    writeln!(out)?;
    Ok(())
}

pub fn print_table(out: &mut dyn Write, jobs: &[Job], color: bool) -> Result<()> {
    if jobs.is_empty() {
        writeln!(out, "No listings found.")?;
        return Ok(());
    }

    let has_score = jobs.iter().any(|j| j.score != 0);
    writeln!(
        out,
        "{:<6} {:<40} {:<24} {:<28} {:<12}",
        "SCORE", "TITLE", "COMPANY", "LOCATION", "POSTED"
    )?;

    for job in jobs {
        let score = if has_score {
            let plain = format!("{:>5}", job.score);
            colored(score_color(job.score), &plain, color)
        } else {
            "     ".to_string()
        };
        writeln!(
            out,
            "{} {:<40} {:<24} {:<28} {:<12}",
            score,
            truncate(&job.title, 40),
            truncate(&job.company, 24),
            truncate(&job.location, 28),
            job.posted.as_deref().unwrap_or("-"),
        )?;
        writeln!(
            out,
            "      {}",
            colored("2", &job.url, color)
        )?;
    }
    writeln!(out)?;
    Ok(())
}

fn score_color(score: i32) -> &'static str {
    match score {
        s if s >= 6 => "32", // green
        s if s >= 3 => "33", // yellow
        s if s > 0 => "36",  // cyan
        _ => "2",
    }
}

pub fn write_json(out: &mut dyn Write, jobs: &[Job]) -> Result<()> {
    writeln!(out, "{}", serde_json::to_string_pretty(jobs)?)?;
    Ok(())
}

pub fn write_csv(out: &mut dyn Write, jobs: &[Job]) -> Result<()> {
    let mut wtr = csv::Writer::from_writer(out);
    wtr.write_record([
        "id",
        "score",
        "title",
        "company",
        "location",
        "posted",
        "posted_ago",
        "region",
        "matched",
        "url",
    ])?;
    for j in jobs {
        wtr.write_record([
            j.id.as_str(),
            &j.score.to_string(),
            j.title.as_str(),
            j.company.as_str(),
            j.location.as_str(),
            j.posted.as_deref().unwrap_or(""),
            j.posted_ago.as_deref().unwrap_or(""),
            j.source_region.as_str(),
            &j.matched.join(";"),
            j.url.as_str(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}
