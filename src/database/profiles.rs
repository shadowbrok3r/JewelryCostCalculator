//! Wax/Resin profile management

use serde::{Deserialize, Serialize};
use log::{info, debug};
use surrealdb_types::{Datetime, RecordId, SurrealValue};

use super::{DB, tables};

/// A wax or resin material profile
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct WaxProfile {
    /// Database ID
    pub id: RecordId,
    /// Display name for the profile
    pub name: String,
    /// Density in g/cm³
    pub density: f64,
    /// Price per gram in USD
    pub price_per_gram: f64,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// When the profile was created
    pub created_at: Datetime,
    /// When the profile was last updated
    pub updated_at: Datetime,
}

/// Input struct for creating a new profile (without ID - SurrealDB assigns it)
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct NewWaxProfile {
    pub name: String,
    pub density: f64,
    pub price_per_gram: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl WaxProfile {
    /// Get the ID key as a string
    pub fn id_key(&self) -> String {
        format!("{:?}", self.id.key)
    }
}

impl From<&WaxProfile> for NewWaxProfile {
    fn from(profile: &WaxProfile) -> Self {
        Self {
            name: profile.name.clone(),
            density: profile.density,
            price_per_gram: profile.price_per_gram,
            description: profile.description.clone(),
        }
    }
}

impl Default for NewWaxProfile {
    fn default() -> Self {
        Self {
            name: "New Profile".to_string(),
            density: 1.08,
            price_per_gram: 0.15,
            description: None,
        }
    }
}

/// Get all wax profiles from the database
pub async fn get_all_profiles() -> anyhow::Result<Vec<WaxProfile>> {
    debug!("Fetching all wax profiles");
    
    let mut result = DB
        .query("SELECT * FROM wax_profiles ORDER BY name")
        .await?;
    
    let profiles: Vec<WaxProfile> = result.take(0)?;
    
    debug!("Found {} profiles", profiles.len());
    Ok(profiles)
}

/// Get a single profile by ID
pub async fn get_profile(id: &RecordId) -> anyhow::Result<Option<WaxProfile>> {
    debug!("Fetching profile: {:?}", id);
    
    let mut result = DB
        .query("SELECT * FROM wax_profiles WHERE id == $id")
        .bind(("id", id.clone()))
        .await?;
    
    let profile: Option<WaxProfile> = result.take(0)?;
    Ok(profile)
}

/// Get a profile by name
pub async fn get_profile_by_name(name: &str) -> anyhow::Result<Option<WaxProfile>> {
    debug!("Fetching profile by name: {}", name);
    
    let mut result = DB
        .query("SELECT * FROM wax_profiles WHERE name == $name LIMIT 1")
        .bind(("name", name.to_string()))
        .await?;
    
    let profile: Option<WaxProfile> = result.take(0)?;
    Ok(profile)
}

/// Create a new profile
pub async fn create_profile(profile: &NewWaxProfile) -> anyhow::Result<WaxProfile> {
    info!("Creating profile: {}", profile.name);
    
    let created: Option<WaxProfile> = DB
        .create(tables::WAX_PROFILES)
        .content(profile.clone())
        .await?;
    
    created.ok_or_else(|| anyhow::anyhow!("Failed to create profile"))
}

/// Update an existing profile
pub async fn update_profile(id: &RecordId, profile: &NewWaxProfile) -> anyhow::Result<WaxProfile> {
    info!("Updating profile: {} ({:?})", profile.name, id);
    
    let mut result = DB
        .query(r#"
            UPDATE $id SET
                name = $name,
                density = $density,
                price_per_gram = $price_per_gram,
                description = $description,
                updated_at = time::now()
        "#)
        .bind(("id", id.clone()))
        .bind(("name", profile.name.clone()))
        .bind(("density", profile.density))
        .bind(("price_per_gram", profile.price_per_gram))
        .bind(("description", profile.description.clone()))
        .await?;
    
    let updated: Option<WaxProfile> = result.take(0)?;
    updated.ok_or_else(|| anyhow::anyhow!("Profile not found"))
}

/// Delete a profile by ID
pub async fn delete_profile(id: &RecordId) -> anyhow::Result<()> {
    info!("Deleting profile: {:?}", id);
    
    DB.query("DELETE $id")
        .bind(("id", id.clone()))
        .await?;
    
    Ok(())
}
