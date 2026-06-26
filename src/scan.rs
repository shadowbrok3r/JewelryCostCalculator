//! Headless batch scan: cost every STL/OBJ in a folder and publish to the catalog.
//!
//! Invoked as a subcommand so the GUI binary doubles as a CLI:
//!   jewelry_cost_calculator scan <DIR> [--kind ring|pendant|auto] [--sizes 5-10]
//!   [--recursive] [--dry-run] [--offline] [--wax-cost 0.10] [--report out.json]
//!
//! Ring size is read from the filename (authoritative); its known inner diameter
//! anchors the size-range scaling. Ring files with no parseable size are skipped
//! and listed in the report for manual review.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand};
use regex::Regex;
use serde::Serialize;

use crate::materials::calculate_all_weights;
use crate::mesh::{load_mesh, volume::calculate_volume_cm3};
use crate::pricing::{api::fetch_metal_prices, calculate_all_costs, MetalPrices, WaxPricing};
use crate::report::CostReport;
use crate::ring_sizing::RingSize;

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
    /// Ring size range to publish, e.g. "5-10"
    #[arg(long, default_value = "5-10")]
    sizes: String,
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
    kind: String,
    ring_size: Option<String>,
    sizes_published: Vec<String>,
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
    let (lo, hi) = parse_size_range(&args.sizes)?;

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
        let item = process_file(path, &kind, lo, hi, &prices, &wax, args.dry_run).await;
        println!(
            "{:<40} {:<8} {:>10} {}",
            truncate(&item.file, 40),
            item.kind,
            format!("{:.3}cm3", item.volume_cm3),
            item.status
        );
        items.push(item);
    }

    let published_rows: usize = items.iter().map(|i| i.rows).sum();
    let skipped = items.iter().filter(|i| i.status.starts_with("skipped")).count();
    let errors = items.iter().filter(|i| i.status.starts_with("error")).count();

    println!(
        "\nscanned {} file(s): {} row(s) {}, {} skipped, {} error(s)",
        items.len(),
        published_rows,
        if args.dry_run { "computed (dry-run)" } else { "published" },
        skipped,
        errors
    );

    let report = ScanReport {
        dir: args.dir.display().to_string(),
        scanned: items.len(),
        published_rows,
        skipped,
        errors,
        prices_live: prices.is_live,
        dry_run: args.dry_run,
        items,
    };
    if let Some(path) = &args.report {
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(path, json)?;
        eprintln!("wrote scan report to {}", path.display());
    }

    Ok(if report.errors > 0 { 1 } else { 0 })
}

#[allow(clippy::too_many_arguments)]
async fn process_file(
    path: &Path,
    kind: &str,
    lo: f64,
    hi: f64,
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
        kind: "?".into(),
        ring_size: None,
        sizes_published: Vec::new(),
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
        item.status = "skipped: zero/invalid volume".into();
        item.kind = "skipped".into();
        return item;
    }

    let size_from_name = parse_ring_size_from_filename(stem);
    let as_ring = match kind {
        "ring" => true,
        "pendant" => false,
        _ => size_from_name.is_some(),
    };

    let report = if as_ring {
        let Some(size) = size_from_name else {
            item.status = "skipped: no ring size in filename".into();
            item.kind = "skipped".into();
            return item;
        };
        item.kind = "ring".into();
        item.ring_size = Some(RingSize::new(size).display());
        let anchor = RingSize::new(size);
        let range = RingSize::range(lo, hi);
        CostReport::new_ring(file.clone(), volume_cm3, anchor.inner_diameter_mm(), &range, prices, wax)
    } else {
        item.kind = "pendant".into();
        let weights = calculate_all_weights(volume_cm3, None);
        let costs = calculate_all_costs(&weights, prices, wax);
        CostReport::new_pendant(file.clone(), volume_cm3, &costs, prices)
    };

    item.sizes_published = report.sizes.iter().map(|s| s.ring_size.clone()).collect();
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
        Err(e) => {
            item.status = format!("error: publish failed ({e})");
        }
    }
    item
}

/// Parse "lo-hi" into an inclusive ring-size range (e.g. "5-10").
fn parse_size_range(s: &str) -> Result<(f64, f64)> {
    let (a, b) = s
        .split_once('-')
        .ok_or_else(|| anyhow!("--sizes must look like 5-10 (got '{}')", s))?;
    let lo: f64 = a.trim().parse().map_err(|_| anyhow!("bad low size in '{}'", s))?;
    let hi: f64 = b.trim().parse().map_err(|_| anyhow!("bad high size in '{}'", s))?;
    if lo > hi {
        return Err(anyhow!("--sizes low {} is greater than high {}", lo, hi));
    }
    Ok((lo, hi))
}

/// Extract a US ring size from a filename stem, preferring labeled tokens.
fn parse_ring_size_from_filename(stem: &str) -> Option<f64> {
    static LABELED: OnceLock<Regex> = OnceLock::new();
    static BARE: OnceLock<Regex> = OnceLock::new();
    let labeled = LABELED
        .get_or_init(|| Regex::new(r"(?i)(?:size|sz|us)[\s_\-\.]*([0-9]{1,2}(?:\.5|\.0)?)").unwrap());
    if let Some(c) = labeled.captures(stem) {
        if let Ok(v) = c[1].parse::<f64>() {
            if (2.0..=16.0).contains(&v) {
                return Some(round_half(v));
            }
        }
    }
    let bare = BARE
        .get_or_init(|| Regex::new(r"(?:^|[\s_\-\(\[])([0-9]{1,2}(?:\.5)?)(?:[\s_\-\)\]]|$)").unwrap());
    for c in bare.captures_iter(stem) {
        if let Ok(v) = c[1].parse::<f64>() {
            if (3.0..=15.0).contains(&v) {
                return Some(round_half(v));
            }
        }
    }
    None
}

fn round_half(v: f64) -> f64 {
    (v * 2.0).round() / 2.0
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
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());
            if matches!(ext.as_deref(), Some("stl") | Some("obj")) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_labeled_sizes() {
        assert_eq!(parse_ring_size_from_filename("celtic-knot-size7"), Some(7.0));
        assert_eq!(parse_ring_size_from_filename("celtic_knot_size_7.5"), Some(7.5));
        assert_eq!(parse_ring_size_from_filename("dragon sz9"), Some(9.0));
        assert_eq!(parse_ring_size_from_filename("band US 10"), Some(10.0));
        assert_eq!(parse_ring_size_from_filename("band-us10.5"), Some(10.5));
    }

    #[test]
    fn parses_bare_size_token() {
        assert_eq!(parse_ring_size_from_filename("celtic-knot-7"), Some(7.0));
        assert_eq!(parse_ring_size_from_filename("ring_8.5_final"), Some(8.5));
    }

    #[test]
    fn rejects_when_no_size() {
        assert_eq!(parse_ring_size_from_filename("celtic-knot-pendant"), None);
        assert_eq!(parse_ring_size_from_filename("dragon"), None);
    }

    #[test]
    fn size_range_parsing() {
        assert_eq!(parse_size_range("5-10").unwrap(), (5.0, 10.0));
        assert!(parse_size_range("10-5").is_err());
        assert!(parse_size_range("abc").is_err());
    }
}
