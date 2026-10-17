//! Named location presets. Every location is queried independently, so more
//! locations means more HTTP requests (and more rate-limit pressure).

/// Expand `--region`/preset names into concrete LinkedIn location strings.
pub fn expand(region: &str) -> Vec<String> {
    match region.trim().to_ascii_lowercase().as_str() {
        "tunisia" | "tn" => vec!["Tunisia".into()],
        "france" | "fr" => vec!["France".into()],
        "europe" | "eu" => vec!["Europe".into()],
        "emea" => vec![
            "Tunisia".into(),
            "France".into(),
            "Europe".into(),
            "United Arab Emirates".into(),
            "Saudi Arabia".into(),
            "Qatar".into(),
            "Morocco".into(),
            "Algeria".into(),
            "Egypt".into(),
        ],
        // Unknown names are treated as a literal LinkedIn location string so
        // `--region "Berlin, Germany"` keeps working.
        _ => vec![region.trim().to_string()],
    }
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect()
}
