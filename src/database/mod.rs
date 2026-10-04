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
    let credentials = match (std::env::var("SURREAL_USER"), std::env::var("SURREAL_PASS")) {
        (Ok(user), Ok(pass)) => Some((user, pass)),
        _ => None,
    };
    connect(&url, credentials).await
}

/// Connect [`DB`] to `url`, sign in as root when credentials are given, and select NS/DB.
pub async fn connect(url: &str, credentials: Option<(String, String)>) -> anyhow::Result<()> {
    let url = url.trim();
    if url.is_empty() {
        anyhow::bail!("SURREAL_URL is empty");
    }
    info!("Connecting to SurrealDB at {}", url);

    open(&DB, url).await?;

    if let Some((username, password)) = credentials {
        DB.signin(surrealdb::opt::auth::Root { username: username.clone(), password }).await?;
        info!("Signed in as {}", username);
    }

    DB.use_ns(NS).use_db(DB_NAME).await?;
    info!("Database connected (NS: {}, DB: {})", NS, DB_NAME);
    Ok(())
}

/// Open the websocket connection for `db`, choosing TLS from the URL scheme.
async fn open(db: &Surreal<Client>, url: &str) -> surrealdb::Result<()> {
    install_crypto_provider();

    // Typed Ws/Wss engines prepend the scheme; pass host only, not the full URL.
    if let Some(host) = url.strip_prefix("wss://") {
        db.connect::<Wss>(host).await
    } else if let Some(host) = url.strip_prefix("ws://") {
        db.connect::<Ws>(host).await
    } else {
        db.connect::<Ws>(url).await
    }
}

/// Install aws-lc-rs as the process-wide rustls provider unless one is already set.
fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

#[cfg(test)]
mod tests;
