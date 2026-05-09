//! File storage and export caching
//!
//! Uses local file storage to cache exported meshes with metadata in SurrealDB.

use serde::{Deserialize, Serialize};
use log::{info, debug, warn};
use surrealdb_types::{Datetime, RecordId, SurrealValue};
use std::path::PathBuf;

use super::{DB, tables};

/// Metadata for a cached export
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct ExportCache {
    pub id: RecordId,
    /// Original source file name
    pub original_file: String,
    /// Ring size this was exported for
    pub ring_size: f64,
    /// Scale factor applied
    pub scale_factor: f64,
    /// Export format (stl/obj)
    pub format: String,
    /// Path to cached file
    pub file_path: String,
    /// When the export was created
    pub created_at: Datetime,
}

/// Input struct for creating a new export cache entry
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct NewExportCache {
    pub original_file: String,
    pub ring_size: f64,
    pub scale_factor: f64,
    pub format: String,
    pub file_path: String,
}

/// Supported export formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    STL,
    OBJ,
}

impl ExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::STL => "stl",
            ExportFormat::OBJ => "obj",
        }
    }
    
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "stl" => Some(ExportFormat::STL),
            "obj" => Some(ExportFormat::OBJ),
            _ => None,
        }
    }
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.extension())
    }
}

/// Generate a filename for an exported ring
pub fn generate_export_filename(base_name: &str, ring_size: f64, format: ExportFormat) -> String {
    // Clean the base name (remove extension if present)
    let clean_name = base_name
        .trim_end_matches(".stl")
        .trim_end_matches(".STL")
        .trim_end_matches(".obj")
        .trim_end_matches(".OBJ");
    
    // Format ring size nicely
    let size_str = if ring_size.fract() == 0.0 {
        format!("{}", ring_size as i32)
    } else {
        format!("{:.1}", ring_size)
    };
    
    format!("{}-size{}.{}", clean_name, size_str, format.extension())
}

/// Get the cache directory path
pub fn get_cache_dir() -> PathBuf {
    let dir = PathBuf::from("./db/exports");
    if !dir.exists() {
        std::fs::create_dir_all(&dir).ok();
    }
    dir
}

/// Cache an exported mesh file
pub async fn cache_export(
    original_file: &str,
    ring_size: f64,
    scale_factor: f64,
    format: ExportFormat,
    data: &[u8],
) -> anyhow::Result<PathBuf> {
    let filename = generate_export_filename(original_file, ring_size, format);
    let cache_dir = get_cache_dir();
    let file_path = cache_dir.join(&filename);
    
    debug!("Caching export: {} ({} bytes)", file_path.display(), data.len());
    
    // Write file to disk
    tokio::fs::write(&file_path, data).await?;
    
    // Store metadata in database
    let new_cache = NewExportCache {
        original_file: original_file.to_string(),
        ring_size,
        scale_factor,
        format: format.extension().to_string(),
        file_path: file_path.to_string_lossy().to_string(),
    };
    
    let _: Option<ExportCache> = DB
        .create(tables::EXPORT_CACHE)
        .content(new_cache)
        .await?;
    
    info!("Cached export: {}", filename);
    Ok(file_path)
}

/// Check if an export is cached
pub async fn get_cached_export(
    original_file: &str,
    ring_size: f64,
    format: ExportFormat,
) -> anyhow::Result<Option<PathBuf>> {
    let mut result = DB
        .query(r#"
            SELECT * FROM export_cache 
            WHERE original_file == $original_file 
            AND ring_size == $ring_size 
            AND format == $format
            LIMIT 1
        "#)
        .bind(("original_file", original_file.to_string()))
        .bind(("ring_size", ring_size))
        .bind(("format", format.extension().to_string()))
        .await?;
    
    let cache: Option<ExportCache> = result.take(0)?;
    
    if let Some(entry) = cache {
        let path = PathBuf::from(&entry.file_path);
        if path.exists() {
            return Ok(Some(path));
        } else {
            // Cache entry exists but file is missing, clean up
            warn!("Cached file missing, cleaning up entry");
            DB.query("DELETE $id")
                .bind(("id", entry.id))
                .await?;
        }
    }
    
    Ok(None)
}

/// Get all cached exports for a file
pub async fn get_all_cached_exports(original_file: &str) -> anyhow::Result<Vec<ExportCache>> {
    let mut result = DB
        .query("SELECT * FROM export_cache WHERE original_file == $original_file ORDER BY ring_size")
        .bind(("original_file", original_file.to_string()))
        .await?;
    
    let exports: Vec<ExportCache> = result.take(0)?;
    Ok(exports)
}

/// Clear all cached exports for a file
pub async fn clear_export_cache(original_file: &str) -> anyhow::Result<()> {
    // Get all cached files first
    let exports = get_all_cached_exports(original_file).await?;
    
    // Delete files from disk
    for export in &exports {
        let path = PathBuf::from(&export.file_path);
        if path.exists() {
            tokio::fs::remove_file(&path).await.ok();
        }
    }
    
    // Delete from database
    DB.query("DELETE export_cache WHERE original_file == $original_file")
        .bind(("original_file", original_file.to_string()))
        .await?;
    
    info!("Cleared {} cached exports for {}", exports.len(), original_file);
    Ok(())
}

/// Clear all cached exports
pub async fn clear_all_export_cache() -> anyhow::Result<()> {
    let cache_dir = get_cache_dir();
    
    // Remove all files in cache directory
    if let Ok(mut entries) = tokio::fs::read_dir(&cache_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            tokio::fs::remove_file(entry.path()).await.ok();
        }
    }
    
    // Clear database
    DB.query("DELETE export_cache").await?;
    
    info!("Cleared all export cache");
    Ok(())
}
