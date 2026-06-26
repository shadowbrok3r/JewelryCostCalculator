//! Publishes computed cost reports to the shared piece_costs catalog.

use std::collections::HashMap;

use jewelry_shared::PieceCostRow;

use super::DB;
use crate::report::{CostReport, MaterialCostEntry};

/// (weight_g, price_usd) for a material short_name, or (None, None) if absent.
fn pair(materials: &HashMap<String, MaterialCostEntry>, key: &str) -> (Option<f64>, Option<f64>) {
    match materials.get(key) {
        Some(m) => (Some(m.weight_g), Some(m.price_usd)),
        None => (None, None),
    }
}

/// Catalog key for a report: the mesh file stem.
fn design_key_from(file_name: &str) -> String {
    std::path::Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name)
        .trim()
        .to_string()
}

/// One PieceCostRow per report size. Maps 14K gold into the single gold_* columns.
pub fn report_to_rows(report: &CostReport) -> Vec<PieceCostRow> {
    let design_key = design_key_from(&report.file_name);
    report
        .sizes
        .iter()
        .map(|s| {
            let (silver_g, silver_usd) = pair(&s.materials, "silver");
            let (gold_g, gold_usd) = pair(&s.materials, "gold_14k");
            let (bronze_g, bronze_usd) = pair(&s.materials, "bronze");
            let wax_usd = s.materials.get("wax").map(|m| m.price_usd);
            PieceCostRow {
                design_key: design_key.clone(),
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

// Upsert keyed on the (design_key, ring_size) UNIQUE index; product_keys is left untouched.
const UPSERT: &str = "INSERT INTO piece_costs $rows ON DUPLICATE KEY UPDATE \
volume_cm3 = $input.volume_cm3, \
silver_g = $input.silver_g, silver_usd = $input.silver_usd, \
gold_g = $input.gold_g, gold_usd = $input.gold_usd, \
bronze_g = $input.bronze_g, bronze_usd = $input.bronze_usd, \
wax_usd = $input.wax_usd";

/// Upsert every size of a report into piece_costs; returns the row count written.
pub async fn publish_report(report: &CostReport) -> anyhow::Result<usize> {
    let rows = report_to_rows(report);
    if rows.is_empty() {
        return Ok(0);
    }
    let count = rows.len();
    DB.query(UPSERT).bind(("rows", rows)).await?.check()?;
    Ok(count)
}
