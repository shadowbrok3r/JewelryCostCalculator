//! Publishes computed cost reports to the shared catalog: a `jewelry` record per
//! design and the per-size rows in `piece_costs` (linked via `design_key`).

use std::collections::HashMap;

use jewelry_shared::PieceCostRow;

use super::DB;
use crate::report::{CostReport, JewelryType, MaterialCostEntry};

/// (weight_g, price_usd) for a material short_name, or (None, None) if absent.
fn pair(materials: &HashMap<String, MaterialCostEntry>, key: &str) -> (Option<f64>, Option<f64>) {
    match materials.get(key) {
        Some(m) => (Some(m.weight_g), Some(m.price_usd)),
        None => (None, None),
    }
}

/// Raw design key for a report: the mesh file stem.
fn design_key_from(file_name: &str) -> String {
    std::path::Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name)
        .trim()
        .to_string()
}

/// Strip a trailing case-insensitive suffix plus any dangling separators.
fn strip_suffix_ci(s: &str, suffix: &str) -> Option<String> {
    if s.len() > suffix.len()
        && s.is_char_boundary(s.len() - suffix.len())
        && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
    {
        Some(
            s[..s.len() - suffix.len()]
                .trim_end_matches(|c: char| c == '-' || c == '_' || c == ' ')
                .to_string(),
        )
    } else {
        None
    }
}

/// Title-case a single word (first char upper, rest unchanged).
fn title_word(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// "EgyptianKingRing" -> "Egyptian King": split camelCase + separators, title-case.
fn humanize(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut spaced = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '-' || c == '_' {
            spaced.push(' ');
            continue;
        }
        if i > 0 {
            let prev = chars[i - 1];
            if c.is_ascii_uppercase() && (prev.is_ascii_lowercase() || prev.is_ascii_digit()) {
                spaced.push(' ');
            }
        }
        spaced.push(c);
    }
    spaced.split_whitespace().map(title_word).collect::<Vec<_>>().join(" ")
}

/// Normalize a raw design key into (slug, display_name). Folds case and drops a
/// trailing Ring/Pendant so dupes merge onto one jewelry record.
pub fn normalize(raw: &str) -> (String, String) {
    let raw = raw.trim();
    let base = strip_suffix_ci(raw, "ring")
        .or_else(|| strip_suffix_ci(raw, "pendant"))
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| raw.to_string());
    let slug: String = base
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let slug = if slug.is_empty() {
        raw.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
    } else {
        slug
    };
    (slug, humanize(&base))
}

/// One PieceCostRow per report size; `design_key` carries the jewelry slug. Maps
/// 14K gold into the single gold_* columns.
pub fn report_to_rows(report: &CostReport) -> Vec<PieceCostRow> {
    let (slug, _) = normalize(&design_key_from(&report.file_name));
    report
        .sizes
        .iter()
        .map(|s| {
            let (silver_g, silver_usd) = pair(&s.materials, "silver");
            let (gold_g, gold_usd) = pair(&s.materials, "gold_14k");
            let (bronze_g, bronze_usd) = pair(&s.materials, "bronze");
            let wax_usd = s.materials.get("wax").map(|m| m.price_usd);
            PieceCostRow {
                design_key: slug.clone(),
                ring_size: Some(s.ring_size.clone()),
                volume_cm3: Some(s.volume_cm3),
                silver_g,
                silver_usd,
                gold_g,
                gold_usd,
                bronze_g,
                bronze_usd,
                wax_usd,
                product_keys: None,
            }
        })
        .collect()
}

// The slug in $rows.design_key is resolved to a record<jewelry> link via
// type::record; upsert keyed on the (design_key, ring_size) UNIQUE index.
const UPSERT: &str = "INSERT INTO piece_costs (SELECT type::record('jewelry', design_key) AS design_key, ring_size, volume_cm3, silver_g, silver_usd, gold_g, gold_usd, bronze_g, bronze_usd, wax_usd FROM $rows) ON DUPLICATE KEY UPDATE \
volume_cm3 = $input.volume_cm3, \
silver_g = $input.silver_g, silver_usd = $input.silver_usd, \
gold_g = $input.gold_g, gold_usd = $input.gold_usd, \
bronze_g = $input.bronze_g, bronze_usd = $input.bronze_usd, \
wax_usd = $input.wax_usd";

/// Upsert a batch of already-built rows in one query; returns the row count written.
pub async fn publish_rows(rows: Vec<PieceCostRow>) -> anyhow::Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }
    let count = rows.len();
    DB.query(UPSERT).bind(("rows", rows)).await?.check()?;
    Ok(count)
}

/// Upsert the jewelry record for a report, then every size into piece_costs.
pub async fn publish_report(report: &CostReport) -> anyhow::Result<usize> {
    let (slug, name) = normalize(&design_key_from(&report.file_name));
    let kind = match report.jewelry_type {
        JewelryType::Ring => "ring",
        _ => "pendant",
    };
    DB.query("UPSERT type::record('jewelry', $slug) SET name = $name, kind = $kind")
        .bind(("slug", slug))
        .bind(("name", name))
        .bind(("kind", kind.to_string()))
        .await?
        .check()?;
    publish_rows(report_to_rows(report)).await
}
