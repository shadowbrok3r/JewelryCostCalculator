//! Pricing module for metal prices and cost calculations

pub mod api;

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::materials::{Material, MaterialWeight, TROY_OZ_TO_GRAMS};

/// Cached prices for storage persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPrices {
    pub prices: MetalPrices,
    pub fetched_at: DateTime<Utc>,
}

/// Current metal prices (per troy ounce for precious metals)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetalPrices {
    /// Gold price per troy ounce in USD
    pub gold_per_troy_oz: f64,
    /// Silver price per troy ounce in USD
    pub silver_per_troy_oz: f64,
    /// Bronze price per kg in USD (non-precious)
    pub bronze_per_kg: f64,
    /// Timestamp when prices were fetched
    pub fetched_at: DateTime<Utc>,
    /// Whether these are live prices or manual/default
    pub is_live: bool,
}

impl Default for MetalPrices {
    fn default() -> Self {
        Self {
            // Default prices (approximate as of 2025)
            gold_per_troy_oz: 2000.0,
            silver_per_troy_oz: 25.0,
            bronze_per_kg: 8.0,
            fetched_at: Utc::now(),
            is_live: false,
        }
    }
}

impl MetalPrices {
    /// Calculate price per gram for a given material
    pub fn price_per_gram(&self, material: Material) -> f64 {
        match material {
            Material::Silver => self.silver_per_troy_oz / TROY_OZ_TO_GRAMS,
            Material::Gold24K => self.gold_per_troy_oz / TROY_OZ_TO_GRAMS,
            Material::Gold22K => (self.gold_per_troy_oz / TROY_OZ_TO_GRAMS) * material.gold_content(),
            Material::Gold18K => (self.gold_per_troy_oz / TROY_OZ_TO_GRAMS) * material.gold_content(),
            Material::Gold14K => (self.gold_per_troy_oz / TROY_OZ_TO_GRAMS) * material.gold_content(),
            Material::Gold10K => (self.gold_per_troy_oz / TROY_OZ_TO_GRAMS) * material.gold_content(),
            Material::Bronze => self.bronze_per_kg / 1000.0, // per kg to per gram
            Material::Wax => 0.0, // Wax price is user-defined separately
        }
    }

    /// Get a formatted age string for the prices
    pub fn age_string(&self) -> String {
        let duration = Utc::now() - self.fetched_at;
        if duration.num_seconds() < 60 {
            "just now".to_string()
        } else if duration.num_minutes() < 60 {
            format!("{} min ago", duration.num_minutes())
        } else if duration.num_hours() < 24 {
            format!("{} hours ago", duration.num_hours())
        } else {
            format!("{} days ago", duration.num_days())
        }
    }
}

/// Cost calculation result for a material
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialCost {
    pub material: Material,
    pub weight_grams: f64,
    pub price_per_gram: f64,
    pub total_cost_usd: f64,
}

/// Wax pricing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaxPricing {
    /// Cost per gram of wax in USD
    pub cost_per_gram: f64,
}

impl Default for WaxPricing {
    fn default() -> Self {
        Self {
            cost_per_gram: 0.10, // Default $0.10 per gram
        }
    }
}

/// Calculate the cost for a material weight
pub fn calculate_cost(
    weight: &MaterialWeight,
    prices: &MetalPrices,
    wax_pricing: &WaxPricing,
) -> MaterialCost {
    let price_per_gram = if weight.material == Material::Wax {
        wax_pricing.cost_per_gram
    } else {
        prices.price_per_gram(weight.material)
    };

    MaterialCost {
        material: weight.material,
        weight_grams: weight.weight_grams,
        price_per_gram,
        total_cost_usd: weight.weight_grams * price_per_gram,
    }
}

/// Calculate costs for all material weights
pub fn calculate_all_costs(
    weights: &[MaterialWeight],
    prices: &MetalPrices,
    wax_pricing: &WaxPricing,
) -> Vec<MaterialCost> {
    weights
        .iter()
        .map(|w| calculate_cost(w, prices, wax_pricing))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_prices() {
        let prices = MetalPrices::default();
        assert!(prices.gold_per_troy_oz > 0.0);
        assert!(prices.silver_per_troy_oz > 0.0);
        assert!(!prices.is_live);
    }

    #[test]
    fn test_price_per_gram() {
        let prices = MetalPrices {
            gold_per_troy_oz: 3110.35, // $100 per gram exactly
            silver_per_troy_oz: 311.035, // $10 per gram exactly
            bronze_per_kg: 10.0,
            fetched_at: Utc::now(),
            is_live: false,
        };

        assert!((prices.price_per_gram(Material::Gold24K) - 100.0).abs() < 0.01);
        assert!((prices.price_per_gram(Material::Silver) - 10.0).abs() < 0.01);
        assert!((prices.price_per_gram(Material::Bronze) - 0.01).abs() < 0.001);
    }

    #[test]
    fn test_gold_karats_pricing() {
        let prices = MetalPrices {
            gold_per_troy_oz: 3110.35, // $100 per gram for 24K
            silver_per_troy_oz: 311.035,
            bronze_per_kg: 10.0,
            fetched_at: Utc::now(),
            is_live: false,
        };

        // 18K is 75% gold
        let price_18k = prices.price_per_gram(Material::Gold18K);
        assert!((price_18k - 75.0).abs() < 0.01);

        // 14K is ~58.3% gold
        let price_14k = prices.price_per_gram(Material::Gold14K);
        assert!((price_14k - 58.33).abs() < 0.5);
    }
}
