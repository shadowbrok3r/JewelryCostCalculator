//! Database module for SurrealDB integration
//!
//! Connects to the shared SurrealDB server. Schema and seed data are owned by
//! surrealkit (see ../surrealdb-server/database), not defined here.

pub mod profiles;
pub mod files;
pub mod catalog;

use surrealdb::{Surreal, engine::remote::ws::{Client, Ws, Wss}};
use std::sync::LazyLock;
use log::info;

pub static DB: LazyLock<Surreal<Client>> = LazyLock::new(Surreal::init);

pub const NS: &str = "jewelry_calculator";
pub const DB_NAME: &str = "jewelry_calculator";
pub const EXPORTS_BUCKET: &str = "exports";

/// Table names
pub mod tables {
    pub const WAX_PROFILES: &str = "wax_profiles";
    pub const EXPORT_CACHE: &str = "export_cache";
    pub const PIECE_COSTS: &str = "piece_costs";
}

/// Connect to the shared SurrealDB server.
/// Reads SURREAL_URL (required) and SURREAL_USER/SURREAL_PASS (optional signin).
pub async fn init() -> anyhow::Result<()> {
    let url = std::env::var("SURREAL_URL")
        .map_err(|_| anyhow::anyhow!("SURREAL_URL not set"))?;
    let url = url.trim();
    if url.is_empty() {
        anyhow::bail!("SURREAL_URL is empty");
    }
    info!("Connecting to SurrealDB at {}", url);

    if url.starts_with("wss") {
        DB.connect::<Wss>(url).await?;
    } else {
        DB.connect::<Ws>(url).await?;
    }

    if let (Ok(user), Ok(pass)) =
        (std::env::var("SURREAL_USER"), std::env::var("SURREAL_PASS"))
    {
        DB.signin(surrealdb::opt::auth::Root { username: user.clone(), password: pass }).await?;
        info!("Signed in as {}", user);
    }

    DB.use_ns(NS).use_db(DB_NAME).await?;
    info!("Database connected (NS: {}, DB: {})", NS, DB_NAME);
    Ok(())
}
