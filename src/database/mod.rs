//! Database module for SurrealDB integration
//! 
//! Handles wax/resin profiles and file storage for exported meshes.

pub mod profiles;
pub mod files;

use surrealdb::{Surreal, engine::local::Db};
use std::sync::LazyLock;
use log::{info, error};

pub static DB: LazyLock<Surreal<Db>> = LazyLock::new(Surreal::init);

pub const NS: &str = "jewelry_calculator";
pub const DB_NAME: &str = "jewelry_calculator";
pub const DB_DEFAULT_PATH: &str = "./db/jewelry_calculator";
pub const EXPORTS_BUCKET: &str = "exports";

/// Table names
pub mod tables {
    pub const WAX_PROFILES: &str = "wax_profiles";
    pub const EXPORT_CACHE: &str = "export_cache";
}

/// Initialize the database connection and schema
pub async fn init() -> anyhow::Result<()> {
    info!("Initializing database at {}", DB_DEFAULT_PATH);
    
    DB.connect::<surrealdb::engine::local::SurrealKv>(DB_DEFAULT_PATH).await?;
    DB.use_ns(NS).use_db(DB_NAME).await?;
    
    // Define schema
    let schema = r#"
        BEGIN;
        
        -- Wax/Resin profiles table
        DEFINE TABLE IF NOT EXISTS wax_profiles TYPE NORMAL SCHEMAFULL PERMISSIONS FULL;
        DEFINE FIELD IF NOT EXISTS name ON wax_profiles TYPE string;
        DEFINE FIELD IF NOT EXISTS density ON wax_profiles TYPE float;
        DEFINE FIELD IF NOT EXISTS price_per_gram ON wax_profiles TYPE float;
        DEFINE FIELD IF NOT EXISTS description ON wax_profiles TYPE option<string>;
        DEFINE FIELD IF NOT EXISTS created_at ON wax_profiles TYPE datetime DEFAULT time::now();
        DEFINE FIELD IF NOT EXISTS updated_at ON wax_profiles TYPE datetime DEFAULT time::now();
        DEFINE INDEX IF NOT EXISTS unique_name ON wax_profiles COLUMNS name UNIQUE;
        
        -- Export cache table (metadata for cached exports)
        DEFINE TABLE IF NOT EXISTS export_cache TYPE NORMAL SCHEMAFULL PERMISSIONS FULL;
        DEFINE FIELD IF NOT EXISTS original_file ON export_cache TYPE string;
        DEFINE FIELD IF NOT EXISTS ring_size ON export_cache TYPE float;
        DEFINE FIELD IF NOT EXISTS scale_factor ON export_cache TYPE float;
        DEFINE FIELD IF NOT EXISTS format ON export_cache TYPE string;
        DEFINE FIELD IF NOT EXISTS file_path ON export_cache TYPE string;
        DEFINE FIELD IF NOT EXISTS created_at ON export_cache TYPE datetime DEFAULT time::now();
        
        COMMIT;
    "#;
    
    let response = DB.query(schema).await?;
    response.check()?;
    
    // Seed default profiles if none exist
    let count: Option<i64> = DB.query("SELECT count() FROM wax_profiles GROUP ALL")
        .await?
        .take("count")?;
    
    if count.unwrap_or(0) == 0 {
        info!("Seeding default wax profiles");
        seed_default_profiles().await?;
    }
    
    info!("Database initialized successfully");
    Ok(())
}

/// Seed default wax/resin profiles
async fn seed_default_profiles() -> anyhow::Result<()> {
    use profiles::NewWaxProfile;
    
    let defaults = vec![
        NewWaxProfile {
            name: "Standard Casting Wax".to_string(),
            density: 1.08,
            price_per_gram: 0.15,
            description: Some("Standard jewelry casting wax".to_string()),
        },
        NewWaxProfile {
            name: "Hard Carving Wax".to_string(),
            density: 0.96,
            price_per_gram: 0.20,
            description: Some("Hard wax for detailed carving".to_string()),
        },
        NewWaxProfile {
            name: "Castable Resin (Standard)".to_string(),
            density: 1.10,
            price_per_gram: 0.25,
            description: Some("Standard castable 3D printing resin".to_string()),
        },
        NewWaxProfile {
            name: "Castable Resin (High Detail)".to_string(),
            density: 1.12,
            price_per_gram: 0.35,
            description: Some("High-detail castable resin for fine jewelry".to_string()),
        },
        NewWaxProfile {
            name: "Injection Wax".to_string(),
            density: 0.92,
            price_per_gram: 0.08,
            description: Some("Wax for rubber mold injection".to_string()),
        },
    ];
    
    for profile in defaults {
        if let Err(e) = profiles::create_profile(&profile).await {
            error!("Failed to create default profile '{}': {}", profile.name, e);
        }
    }
    
    Ok(())
}
