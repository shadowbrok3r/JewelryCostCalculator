use std::time::Duration;

use surrealdb::{Surreal, engine::remote::ws::Client};
use surrealdb_types::SurrealValue;

use super::{DB, catalog, files, open, profiles};
use crate::database::files::ExportFormat;
use crate::database::profiles::NewWaxProfile;
use crate::pricing::{MetalPrices, WaxPricing};
use crate::report::CostReport;
use crate::ring_sizing::RingSize;

#[tokio::test]
async fn wss_connect_to_plain_tcp_errors_without_panicking() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            drop(socket);
        }
    });

    let db = Surreal::<Client>::init();
    let result = tokio::time::timeout(Duration::from_secs(20), open(&db, &format!("wss://{addr}")))
        .await
        .expect("wss connect to a closed TLS peer must fail fast");

    assert!(result.is_err(), "handshake against a plain TCP peer must fail");
    assert!(rustls::crypto::CryptoProvider::get_default().is_some());
}

#[derive(Debug, SurrealValue)]
struct JewelryRow {
    name: String,
    kind: String,
}

#[derive(Debug, SurrealValue)]
struct LinkedCost {
    design_name: String,
    ring_size: Option<String>,
    silver_usd: Option<f64>,
}

// Returns the URL only when its host is a loopback address.
fn local_url() -> String {
    let url = std::env::var("JCC_TEST_SURREAL_URL").expect("set JCC_TEST_SURREAL_URL=ws://127.0.0.1:<port>");
    let rest = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
    let host = if rest.starts_with('[') {
        rest.split(']').next().unwrap_or_default().trim_start_matches('[')
    } else {
        rest.split([':', '/']).next().unwrap_or_default()
    };
    assert!(
        matches!(host, "127.0.0.1" | "localhost" | "::1"),
        "refusing non-loopback SurrealDB host '{host}'"
    );
    url
}

#[tokio::test]
#[ignore = "needs a local SurrealDB with the app schema: JCC_TEST_SURREAL_URL=ws://127.0.0.1:<port>"]
async fn local_db_roundtrip() -> anyhow::Result<()> {
    let url = local_url();
    let user = std::env::var("JCC_TEST_SURREAL_USER").unwrap_or_else(|_| "root".into());
    let pass = std::env::var("JCC_TEST_SURREAL_PASS").unwrap_or_else(|_| "root".into());

    // Export cache files land under target/ instead of the repo's ./db/exports.
    let work = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/db-itest");
    std::fs::create_dir_all(&work)?;
    std::env::set_current_dir(&work)?;

    super::connect(&url, Some((user, pass))).await?;
    let tag = format!("itest{}", std::process::id());

    // Wax profiles: create, read back by id and name, update, delete.
    let seeded = profiles::get_all_profiles().await?;
    assert!(!seeded.is_empty(), "seeded wax_profiles expected");

    let created = profiles::create_profile(&NewWaxProfile {
        name: format!("{tag} wax"),
        density: 1.1,
        price_per_gram: 0.2,
        description: Some("integration test".into()),
    })
    .await?;
    assert_eq!(created.name, format!("{tag} wax"));
    assert_eq!(created.description.as_deref(), Some("integration test"));

    let by_id = profiles::get_profile(&created.id).await?.expect("profile by id");
    assert_eq!(by_id.name, created.name);
    let by_name = profiles::get_profile_by_name(&created.name).await?.expect("profile by name");
    assert_eq!(by_name.id, created.id);

    let updated = profiles::update_profile(
        &created.id,
        &NewWaxProfile {
            name: format!("{tag} wax v2"),
            density: 1.2,
            price_per_gram: 0.3,
            description: None,
        },
    )
    .await?;
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.name, format!("{tag} wax v2"));
    assert_eq!(updated.density, 1.2);
    assert_eq!(updated.description, None);
    assert_eq!(profiles::get_all_profiles().await?.len(), seeded.len() + 1);

    profiles::delete_profile(&created.id).await?;
    assert!(profiles::get_profile(&created.id).await?.is_none());

    // Catalog: jewelry upsert plus piece_costs rows linked through type::record.
    let report = CostReport::new_ring(
        format!("{tag}Ring.stl"),
        2.5,
        RingSize::new(7.0).inner_diameter_mm(),
        &RingSize::range(7.0, 8.0),
        &MetalPrices::default(),
        &WaxPricing::default(),
    );
    let (slug, name) = catalog::normalize(&format!("{tag}Ring"));
    assert_eq!(catalog::publish_report(&report).await?, 3);
    assert_eq!(catalog::publish_report(&report).await?, 3);

    let mut res = DB
        .query("SELECT name, kind FROM ONLY type::record('jewelry', $slug)")
        .bind(("slug", slug.clone()))
        .await?;
    let jewelry: Option<JewelryRow> = res.take(0)?;
    let jewelry = jewelry.expect("jewelry record");
    assert_eq!(jewelry.name, name);
    assert_eq!(jewelry.kind, "ring");

    let mut res = DB
        .query(
            "SELECT design_key.name AS design_name, ring_size, silver_usd FROM piece_costs \
             WHERE design_key = type::record('jewelry', $slug) ORDER BY ring_size",
        )
        .bind(("slug", slug.clone()))
        .await?;
    let costs: Vec<LinkedCost> = res.take(0)?;
    assert_eq!(costs.len(), 3, "duplicate publish must update in place");
    assert!(costs.iter().all(|c| c.design_name == name && c.silver_usd.is_some_and(|v| v > 0.0)));
    let sizes: Vec<_> = costs.iter().filter_map(|c| c.ring_size.clone()).collect();
    assert_eq!(sizes, ["US 7", "US 7.5", "US 8"]);

    // Export cache: write, look up, list, clear, and drop entries whose file vanished.
    let original = format!("{tag}-ring.stl");
    let data = b"solid itest\nendsolid itest\n";
    let path = files::cache_export(&original, 7.0, 1.0, ExportFormat::STL, data).await?;
    assert!(path.exists());
    assert_eq!(files::get_cached_export(&original, 7.0, ExportFormat::STL).await?, Some(path.clone()));
    assert_eq!(files::get_cached_export(&original, 7.0, ExportFormat::OBJ).await?, None);
    let cached = files::get_all_cached_exports(&original).await?;
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].format, "stl");
    assert_eq!(cached[0].scale_factor, 1.0);

    files::clear_export_cache(&original).await?;
    assert!(!path.exists());
    assert!(files::get_all_cached_exports(&original).await?.is_empty());

    let path = files::cache_export(&original, 8.0, 1.05, ExportFormat::OBJ, data).await?;
    std::fs::remove_file(&path)?;
    assert_eq!(files::get_cached_export(&original, 8.0, ExportFormat::OBJ).await?, None);
    assert!(files::get_all_cached_exports(&original).await?.is_empty());

    DB.query("DELETE piece_costs WHERE design_key = type::record('jewelry', $slug); DELETE type::record('jewelry', $slug)")
        .bind(("slug", slug))
        .await?
        .check()?;
    Ok(())
}
