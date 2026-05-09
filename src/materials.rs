//! Material properties and weight calculations for jewelry
//!
//! Densities are in g/cm³. Weight calculation uses the formula:
//! weight_g = volume_cm³ * density

use serde::{Deserialize, Serialize};

/// Material types supported by the calculator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Material {
    Silver,
    Gold24K,
    Gold22K,
    Gold18K,
    Gold14K,
    Gold10K,
    Bronze,
    Wax,
}

impl Material {
    /// Get all standard materials (excluding wax which is customizable)
    pub fn standard_metals() -> &'static [Material] {
        &[
            Material::Silver,
            Material::Gold24K,
            Material::Gold22K,
            Material::Gold18K,
            Material::Gold14K,
            Material::Gold10K,
            Material::Bronze,
        ]
    }

    /// Get all materials including wax
    pub fn all() -> &'static [Material] {
        &[
            Material::Silver,
            Material::Gold24K,
            Material::Gold22K,
            Material::Gold18K,
            Material::Gold14K,
            Material::Gold10K,
            Material::Bronze,
            Material::Wax,
        ]
    }

    /// Get the default density in g/cm³
    pub fn default_density(&self) -> f64 {
        match self {
            Material::Silver => 10.49,
            Material::Gold24K => 19.32,
            Material::Gold22K => 17.84, // 22/24 * 19.32 + 2/24 * ~10 (alloy)
            Material::Gold18K => 15.58, // 75% gold
            Material::Gold14K => 13.07, // 58.3% gold
            Material::Gold10K => 11.57, // 41.7% gold
            Material::Bronze => 8.73,
            Material::Wax => 1.08, // Defaults to Bluecast's X-WAX ($0.33 / g)
        }
    }

    /// Get a human-readable name
    pub fn display_name(&self) -> &'static str {
        match self {
            Material::Silver => "Silver (Sterling 925)",
            Material::Gold24K => "Gold 24K (Pure)",
            Material::Gold22K => "Gold 22K",
            Material::Gold18K => "Gold 18K",
            Material::Gold14K => "Gold 14K",
            Material::Gold10K => "Gold 10K",
            Material::Bronze => "Bronze",
            Material::Wax => "Casting Wax",
        }
    }

    /// Get a short name for JSON output
    pub fn short_name(&self) -> &'static str {
        match self {
            Material::Silver => "silver",
            Material::Gold24K => "gold_24k",
            Material::Gold22K => "gold_22k",
            Material::Gold18K => "gold_18k",
            Material::Gold14K => "gold_14k",
            Material::Gold10K => "gold_10k",
            Material::Bronze => "bronze",
            Material::Wax => "wax",
        }
    }

    /// Check if this is a precious metal (gold or silver)
    pub fn is_precious(&self) -> bool {
        matches!(
            self,
            Material::Silver
                | Material::Gold24K
                | Material::Gold22K
                | Material::Gold18K
                | Material::Gold14K
                | Material::Gold10K
        )
    }

    /// Get the gold content percentage (0.0 for non-gold materials)
    pub fn gold_content(&self) -> f64 {
        match self {
            Material::Gold24K => 1.0,
            Material::Gold22K => 22.0 / 24.0,
            Material::Gold18K => 18.0 / 24.0,
            Material::Gold14K => 14.0 / 24.0,
            Material::Gold10K => 10.0 / 24.0,
            _ => 0.0,
        }
    }
}

/// Weight calculation result for a material
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialWeight {
    pub material: Material,
    pub weight_grams: f64,
    pub density_used: f64,
}

/// Calculate the weight for a given volume and material
pub fn calculate_weight(volume_cm3: f64, material: Material, custom_density: Option<f64>) -> MaterialWeight {
    let density = custom_density.unwrap_or_else(|| material.default_density());
    let weight = volume_cm3 * density;

    MaterialWeight {
        material,
        weight_grams: weight,
        density_used: density,
    }
}

/// Calculate weights for all standard materials
pub fn calculate_all_weights(volume_cm3: f64, wax_density: Option<f64>) -> Vec<MaterialWeight> {
    let mut weights = Vec::with_capacity(Material::all().len());

    for &material in Material::all() {
        let custom_density = if material == Material::Wax {
            wax_density
        } else {
            None
        };
        weights.push(calculate_weight(volume_cm3, material, custom_density));
    }

    weights
}

/// Troy ounce to grams conversion
pub const TROY_OZ_TO_GRAMS: f64 = 31.1035;

/// Convert troy ounces to grams
pub fn troy_oz_to_grams(troy_oz: f64) -> f64 {
    troy_oz * TROY_OZ_TO_GRAMS
}

/// Convert grams to troy ounces
pub fn grams_to_troy_oz(grams: f64) -> f64 {
    grams / TROY_OZ_TO_GRAMS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_density_values() {
        assert!((Material::Silver.default_density() - 10.49).abs() < 0.01);
        assert!((Material::Gold24K.default_density() - 19.32).abs() < 0.01);
        assert!((Material::Bronze.default_density() - 8.73).abs() < 0.01);
    }

    #[test]
    fn test_weight_calculation() {
        // 1 cm³ of silver should weigh ~10.49g
        let weight = calculate_weight(1.0, Material::Silver, None);
        assert!((weight.weight_grams - 10.49).abs() < 0.01);
    }

    #[test]
    fn test_custom_wax_density() {
        let weight = calculate_weight(1.0, Material::Wax, Some(1.0));
        assert!((weight.weight_grams - 1.0).abs() < 0.001);
        assert!((weight.density_used - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_troy_oz_conversion() {
        assert!((troy_oz_to_grams(1.0) - 31.1035).abs() < 0.0001);
        assert!((grams_to_troy_oz(31.1035) - 1.0).abs() < 0.0001);
    }
}
