//! Headless batch scan: cost every STL/OBJ in a folder and publish to the catalog.
//!
//! Invoked as a subcommand so the GUI binary doubles as a CLI:
//!   jewelry_cost_calculator scan <DIR> [--kind ring|pendant|auto] [--recursive]
//!   [--dry-run] [--offline] [--wax-cost 0.10] [--report out.json]
//!
//! Ring files are named with the size appended to the design (e.g. `Hades9.stl`,
//! `Kamon-11.25.stl`, `AthenaRing8.obj`). The size becomes the row's ring_size and
//! the remaining name becomes the catalog design_key, so every size groups under
//! one design. Each file is costed at its native volume. Files with no parseable
//! ring size are skipped and listed in the report for manual review.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand};
use regex::Regex;
use serde::Serialize;

use crate::materials::calculate_all_weights;
use crate::mesh::{load_mesh, volume::calculate_volume_cm3};
use crate::pricing::{api::fetch_metal_prices, calculate_all_costs, MetalPrices, WaxPricing};
use crate::report::{CostReport, JewelryType};
use crate::ring_sizing::{calculate_scale_factor, calculate_scaled_volume, RingSize};
use jewelry_shared::PieceCostRow;

/// Plausible US ring sizes; bounds reject part numbers and version tags.
const RING_SIZE_MIN: f64 = 3.0;
const RING_SIZE_MAX: f64 = 16.0;

#[derive(Parser)]
#[command(name = "jewelry_cost_calculator", disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Cost every STL/OBJ in a folder and publish to the shared catalog
    Scan(ScanArgs),
}

#[derive(Args)]
struct ScanArgs {
    /// Directory containing STL/OBJ files
    dir: PathBuf,
    /// Treat files as: ring | pendant | auto
    #[arg(long, default_value = "auto")]
    kind: String,
    /// Recurse into subdirectories
    #[arg(long)]
    recursive: bool,
    /// Compute costs without writing to the database
    #[arg(long)]
    dry_run: bool,
    /// Skip the live price fetch and use built-in defaults
    #[arg(long)]
    offline: bool,
    /// Wax cost per gram in USD
    #[arg(long, default_value_t = 0.10)]
    wax_cost: f64,
    /// Write a JSON scan report to this path
    #[arg(long)]
    report: Option<PathBuf>,
    /// Fill in missing ring sizes per design by scaling the nearest modeled size (cube law)
    #[arg(long)]
    scale_sizes: bool,
    /// Lowest ring size to generate with --scale-sizes
    #[arg(long, default_value_t = 5.0)]
    size_min: f64,
    /// Highest ring size to generate with --scale-sizes
    #[arg(long, default_value_t = 15.0)]
    size_max: f64,
}

/// Handle the `scan` subcommand; returns Some(exit_code) when handled, None for GUI.
pub async fn dispatch() -> Option<i32> {
    if std::env::args().nth(1).as_deref() != Some("scan") {
        return None;
    }
    let cli = Cli::parse();
    match cli.command {
        Commands::Scan(args) => Some(run(args).await),
    }
}

async fn run(args: ScanArgs) -> i32 {
    match run_inner(args).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("scan failed: {e:#}");
            1
        }
    }
}

#[derive(Serialize)]
struct ScanItem {
    file: String,
    design_key: Option<String>,
    kind: String,
    ring_size: Option<String>,
    volume_cm3: f64,
    silver_usd: Option<f64>,
    rows: usize,
    status: String,
}

#[derive(Serialize)]
struct ScanReport {
    dir: String,
    scanned: usize,
    published_rows: usize,
    skipped: usize,
    errors: usize,
    prices_live: bool,
    dry_run: bool,
    items: Vec<ScanItem>,
}

async fn run_inner(args: ScanArgs) -> Result<i32> {
    let prices = if args.offline {
        eprintln!("offline: using default metal prices");
        MetalPrices::default()
    } else {
        match fetch_metal_prices(crate::METAL_API_KEY).await {
            Ok(p) => {
                eprintln!(
                    "live prices: gold ${:.2}/ozt, silver ${:.2}/ozt",
                    p.gold_per_troy_oz, p.silver_per_troy_oz
                );
                p
            }
            Err(e) => {
                eprintln!("price fetch failed ({e}); using defaults");
                MetalPrices::default()
            }
        }
    };
    let wax = WaxPricing { cost_per_gram: args.wax_cost };

    if !args.dry_run {
        crate::database::init().await.map_err(|e| {
            anyhow!("DB connect failed: {e} (set SURREAL_URL in .env, or pass --dry-run)")
        })?;
    }

    let kind = args.kind.to_lowercase();
    if !matches!(kind.as_str(), "ring" | "pendant" | "auto") {
        return Err(anyhow!("--kind must be ring, pendant, or auto (got '{}')", kind));
    }

    let files = collect_mesh_files(&args.dir, args.recursive)?;
    if files.is_empty() {
        eprintln!("no .stl/.obj files found under {}", args.dir.display());
    }

    let mut items: Vec<ScanItem> = Vec::with_capacity(files.len());
    for path in &files {
        let item = process_file(path, &kind, &prices, &wax, args.dry_run).await;
        println!(
            "{:<42} {:<7} {:<7} {:>10}  {}",
            truncate(&item.file, 42),
            item.kind,
            item.ring_size.as_deref().unwrap_or("-"),
            format!("{:.3}cm3", item.volume_cm3),
            item.status
        );
        items.push(item);
    }

    if args.scale_sizes {
        let scaled = generate_scaled_sizes(&items, args.size_min, args.size_max, &prices, &wax);
        if !args.dry_run {
            let rows: Vec<PieceCostRow> = scaled.iter().map(|(_, r)| r.clone()).collect();
            for chunk in rows.chunks(256) {
                crate::database::catalog::publish_rows(chunk.to_vec()).await?;
            }
        }
        for (item, _) in &scaled {
            println!(
                "{:<42} {:<7} {:<7} {:>10}  {}",
                truncate(&item.file, 42),
                item.kind,
                item.ring_size.as_deref().unwrap_or("-"),
                format!("{:.3}cm3", item.volume_cm3),
                item.status
            );
        }
        items.extend(scaled.into_iter().map(|(it, _)| it));
    }

    let published_rows: usize = items.iter().map(|i| i.rows).sum();
    let skipped = items.iter().filter(|i| i.status.starts_with("skipped")).count();
    let errors = items.iter().filter(|i| i.status.starts_with("error")).count();
    let scaled_rows = items.iter().filter(|i| i.kind == "scaled").count();

    println!(
        "\nscanned {} file(s): {} row(s) {} ({} scaled), {} skipped, {} error(s)",
        files.len(),
        published_rows,
        if args.dry_run { "computed (dry-run)" } else { "published" },
        scaled_rows,
        skipped,
        errors
    );

    let report = ScanReport {
        dir: args.dir.display().to_string(),
        scanned: files.len(),
        published_rows,
        skipped,
        errors,
        prices_live: prices.is_live,
        dry_run: args.dry_run,
        items,
    };
    if let Some(path) = &args.report {
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
        eprintln!("wrote scan report to {}", path.display());
    }

    Ok(if report.errors > 0 { 1 } else { 0 })
}

async fn process_file(
    path: &Path,
    kind: &str,
    prices: &MetalPrices,
    wax: &WaxPricing,
    dry_run: bool,
) -> ScanItem {
    let file = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(&file);

    let mut item = ScanItem {
        file: file.clone(),
        design_key: None,
        kind: "?".into(),
        ring_size: None,
        volume_cm3: 0.0,
        silver_usd: None,
        rows: 0,
        status: String::new(),
    };

    let mesh = match load_mesh(path) {
        Ok(m) => m,
        Err(e) => {
            item.status = format!("error: load failed ({e})");
            return item;
        }
    };
    let volume_cm3 = calculate_volume_cm3(&mesh);
    item.volume_cm3 = volume_cm3;
    if !(volume_cm3 > 0.0) {
        item.kind = "skipped".into();
        item.status = "skipped: zero/invalid volume".into();
        return item;
    }

    let ring = parse_ring(stem);
    let as_ring = match kind {
        "ring" => true,
        "pendant" => false,
        _ => ring.is_some(),
    };

    let weights = calculate_all_weights(volume_cm3, None);
    let costs = calculate_all_costs(&weights, prices, wax);

    let report = if as_ring {
        let Some((design_key, size)) = ring else {
            item.kind = "skipped".into();
            item.status = "skipped: no ring size in filename".into();
            return item;
        };
        let size_label = format!("US {}", fmt_size(size));
        item.kind = "ring".into();
        item.design_key = Some(design_key.clone());
        item.ring_size = Some(size_label.clone());
        let mut r = CostReport::new_pendant(design_key, volume_cm3, &costs, prices);
        r.jewelry_type = JewelryType::Ring;
        r.sizes[0].ring_size = size_label;
        r
    } else {
        let design_key = stem.trim().to_string();
        item.kind = "pendant".into();
        item.design_key = Some(design_key);
        CostReport::new_pendant(file.clone(), volume_cm3, &costs, prices)
    };

    item.silver_usd = report
        .sizes
        .first()
        .and_then(|s| s.materials.get("silver"))
        .map(|m| m.price_usd);

    if dry_run {
        item.rows = report.sizes.len();
        item.status = "computed (dry-run)".into();
        return item;
    }

    match crate::database::catalog::publish_report(&report).await {
        Ok(count) => {
            item.rows = count;
            item.status = "published".into();
        }
        Err(e) => item.status = format!("error: publish failed ({e})"),
    }
    item
}

/// Split a ring filename stem into (design_key, size). The size is the last
/// number in the plausible ring range; the design_key is the text before it.
fn parse_ring(stem: &str) -> Option<(String, f64)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\d{1,2}(?:\.\d{1,2})?").unwrap());

    let mut hit: Option<(usize, f64)> = None;
    for m in re.find_iter(stem) {
        if let Ok(v) = m.as_str().parse::<f64>() {
            if (RING_SIZE_MIN..=RING_SIZE_MAX).contains(&v) {
                hit = Some((m.start(), v)); // keep the last in-range match
            }
        }
    }
    let (start, size) = hit?;
    let design = stem[..start]
        .trim_end_matches(|c: char| c == ' ' || c == '-' || c == '_' || c == '.')
        .trim();
    if design.is_empty() {
        return None;
    }
    Some((design.to_string(), size))
}

/// Format a ring size without trailing zeros: 9.0 -> "9", 8.5 -> "8.5", 8.75 -> "8.75".
fn fmt_size(s: f64) -> String {
    if s.fract().abs() < 1e-9 {
        format!("{}", s as i64)
    } else {
        let mut t = format!("{:.2}", s);
        while t.ends_with('0') {
            t.pop();
        }
        if t.ends_with('.') {
            t.pop();
        }
        t
    }
}

/// Parse the numeric size from a "US 9" / "US 8.75" label.
fn parse_size_label(label: &str) -> Option<f64> {
    label.trim().strip_prefix("US ").and_then(|s| s.trim().parse::<f64>().ok())
}

/// For each ring design, generate rows for the missing sizes in [size_min, size_max]
/// (0.5 steps) by scaling the nearest modeled size's volume by the cube of the
/// inner-diameter ratio. Real modeled sizes are left untouched.
fn generate_scaled_sizes(
    real_items: &[ScanItem],
    size_min: f64,
    size_max: f64,
    prices: &MetalPrices,
    wax: &WaxPricing,
) -> Vec<(ScanItem, PieceCostRow)> {
    use std::collections::BTreeMap;
    let mut by_design: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    for it in real_items {
        if it.kind != "ring" {
            continue;
        }
        let (Some(dk), Some(rs)) = (it.design_key.as_ref(), it.ring_size.as_ref()) else {
            continue;
        };
        if let Some(sz) = parse_size_label(rs) {
            by_design.entry(dk.clone()).or_default().push((sz, it.volume_cm3));
        }
    }

    let targets = RingSize::range(size_min, size_max);
    let mut out = Vec::new();
    for (design_key, reals) in &by_design {
        if reals.is_empty() {
            continue;
        }
        for t in &targets {
            let tsz = t.0;
            if reals.iter().any(|(s, _)| (s - tsz).abs() < 1e-6) {
                continue;
            }
            let (s0, v0) = reals
                .iter()
                .cloned()
                .min_by(|a, b| (a.0 - tsz).abs().partial_cmp(&(b.0 - tsz).abs()).unwrap())
                .unwrap();
            let scale = calculate_scale_factor(RingSize::new(s0).inner_diameter_mm(), *t);
            let vol = calculate_scaled_volume(v0, scale);
            let weights = calculate_all_weights(vol, None);
            let costs = calculate_all_costs(&weights, prices, wax);
            let label = format!("US {}", fmt_size(tsz));
            let mut r = CostReport::new_pendant(design_key.clone(), vol, &costs, prices);
            r.jewelry_type = JewelryType::Ring;
            r.sizes[0].ring_size = label.clone();
            let Some(row) = crate::database::catalog::report_to_rows(&r).into_iter().next() else {
                continue;
            };
            let silver_usd = r
                .sizes
                .first()
                .and_then(|s| s.materials.get("silver"))
                .map(|m| m.price_usd);
            let item = ScanItem {
                file: format!("{design_key} \u{2192} {label}"),
                design_key: Some(design_key.clone()),
                kind: "scaled".into(),
                ring_size: Some(label),
                volume_cm3: vol,
                silver_usd,
                rows: 1,
                status: format!("scaled from US {}", fmt_size(s0)),
            };
            out.push((item, row));
        }
    }
    out
}

/// Collect *.stl / *.obj files under `dir`, sorted, optionally recursing.
fn collect_mesh_files(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Err(anyhow!("{} is not a directory", dir.display()));
    }
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let path = entry?.path();
            if path.is_dir() {
                if recursive {
                    stack.push(path);
                }
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase());
            if matches!(ext.as_deref(), Some("stl") | Some("obj")) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_size_appended_to_design() {
        assert_eq!(parse_ring("Hades9"), Some(("Hades".into(), 9.0)));
        assert_eq!(parse_ring("AthenaRing8"), Some(("AthenaRing".into(), 8.0)));
        assert_eq!(parse_ring("Kamon-8"), Some(("Kamon".into(), 8.0)));
        assert_eq!(parse_ring("Kamon.stl is a stem? no"), None); // sanity: no in-range number
    }

    #[test]
    fn parses_quarter_and_half_sizes() {
        assert_eq!(parse_ring("Hades8.75"), Some(("Hades".into(), 8.75)));
        assert_eq!(parse_ring("Kamon-11.25"), Some(("Kamon".into(), 11.25)));
        assert_eq!(parse_ring("Kamon-11.5"), Some(("Kamon".into(), 11.5)));
        assert_eq!(parse_ring("Hades12.5"), Some(("Hades".into(), 12.5)));
    }

    #[test]
    fn strips_trailing_material_word() {
        assert_eq!(parse_ring("Hades11Bronze"), Some(("Hades".into(), 11.0)));
    }

    #[test]
    fn keeps_distinct_blank_design() {
        assert_eq!(parse_ring("BlankKamon-13"), Some(("BlankKamon".into(), 13.0)));
    }

    #[test]
    fn rejects_when_no_ring_size() {
        assert_eq!(parse_ring("UMesh_AthenaRing"), None);
        assert_eq!(parse_ring("Kamon"), None);
        assert_eq!(parse_ring("AthenaRing"), None);
        assert_eq!(parse_ring("BlankSignet"), None);
        assert_eq!(parse_ring("1-NordicAxe"), None); // 1 is below the ring range
    }

    #[test]
    fn size_label_formatting() {
        assert_eq!(fmt_size(9.0), "9");
        assert_eq!(fmt_size(8.5), "8.5");
        assert_eq!(fmt_size(8.75), "8.75");
        assert_eq!(fmt_size(11.25), "11.25");
    }
}
