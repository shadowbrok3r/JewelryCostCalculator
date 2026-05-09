//! JSON report generation for jewelry cost calculations

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::pricing::{MaterialCost, MetalPrices, WaxPricing};
use crate::ring_sizing::RingSize;

/// Type of jewelry item
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JewelryType {
    Pendant,
    Ring,
}

impl Default for JewelryType {
    fn default() -> Self {
        JewelryType::Pendant
    }
}

impl JewelryType {
    pub fn display_name(&self) -> &'static str {
        match self {
            JewelryType::Pendant => "Pendant",
            JewelryType::Ring => "Ring",
        }
    }
}

/// Material cost entry in the report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialCostEntry {
    pub weight_g: f64,
    pub price_usd: f64,
}

/// Size entry in the report (for rings)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeEntry {
    pub ring_size: String,
    pub inner_diameter_mm: f64,
    pub scale_factor: f64,
    pub volume_cm3: f64,
    pub materials: HashMap<String, MaterialCostEntry>,
}

/// Complete cost report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostReport {
    pub file_name: String,
    pub jewelry_type: JewelryType,
    pub base_volume_cm3: f64,
    pub sizes: Vec<SizeEntry>,
    pub prices_fetched_at: DateTime<Utc>,
    pub prices_are_live: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl CostReport {
    /// Create a new report for a pendant (single size, no scaling)
    pub fn new_pendant(
        file_name: String,
        volume_cm3: f64,
        costs: &[MaterialCost],
        prices: &MetalPrices,
    ) -> Self {
        let materials = costs_to_map(costs);

        Self {
            file_name,
            jewelry_type: JewelryType::Pendant,
            base_volume_cm3: volume_cm3,
            sizes: vec![SizeEntry {
                ring_size: "N/A".to_string(),
                inner_diameter_mm: 0.0,
                scale_factor: 1.0,
                volume_cm3,
                materials,
            }],
            prices_fetched_at: prices.fetched_at,
            prices_are_live: prices.is_live,
            notes: None,
        }
    }

    /// Create a new report for a ring with multiple sizes
    pub fn new_ring(
        file_name: String,
        base_volume_cm3: f64,
        current_diameter_mm: f64,
        target_sizes: &[RingSize],
        prices: &MetalPrices,
        wax_pricing: &WaxPricing,
    ) -> Self {
        use crate::materials::calculate_all_weights;
        use crate::pricing::calculate_all_costs;
        use crate::ring_sizing::{calculate_scale_factor, calculate_scaled_volume};

        let sizes: Vec<SizeEntry> = target_sizes
            .iter()
            .map(|size| {
                let scale_factor = calculate_scale_factor(current_diameter_mm, *size);
                let scaled_volume = calculate_scaled_volume(base_volume_cm3, scale_factor);
                let weights = calculate_all_weights(scaled_volume, Some(wax_pricing.cost_per_gram));
                let costs = calculate_all_costs(&weights, prices, wax_pricing);

                SizeEntry {
                    ring_size: size.display(),
                    inner_diameter_mm: size.inner_diameter_mm(),
                    scale_factor,
                    volume_cm3: scaled_volume,
                    materials: costs_to_map(&costs),
                }
            })
            .collect();

        Self {
            file_name,
            jewelry_type: JewelryType::Ring,
            base_volume_cm3,
            sizes,
            prices_fetched_at: prices.fetched_at,
            prices_are_live: prices.is_live,
            notes: None,
        }
    }

    /// Create a report for a ring with a single target size
    pub fn new_ring_single(
        file_name: String,
        base_volume_cm3: f64,
        current_diameter_mm: f64,
        target_size: RingSize,
        prices: &MetalPrices,
        wax_pricing: &WaxPricing,
    ) -> Self {
        Self::new_ring(
            file_name,
            base_volume_cm3,
            current_diameter_mm,
            &[target_size],
            prices,
            wax_pricing,
        )
    }

    /// Convert to pretty-printed JSON string
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
    }

    /// Convert to compact JSON string
    pub fn to_json_compact(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
    }

    /// Add a note to the report
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes = Some(note.into());
        self
    }

    /// Get total cost summary for a specific size index
    pub fn total_cost_for_size(&self, size_idx: usize) -> Option<f64> {
        self.sizes.get(size_idx).map(|entry| {
            entry
                .materials
                .values()
                .map(|m| m.price_usd)
                .sum()
        })
    }
}

/// Convert material costs to a HashMap for JSON serialization
fn costs_to_map(costs: &[MaterialCost]) -> HashMap<String, MaterialCostEntry> {
    costs
        .iter()
        .map(|c| {
            (
                c.material.short_name().to_string(),
                MaterialCostEntry {
                    weight_g: round_to(c.weight_grams, 2),
                    price_usd: round_to(c.total_cost_usd, 2),
                },
            )
        })
        .collect()
}

/// Round a float to a specified number of decimal places
fn round_to(value: f64, decimals: u32) -> f64 {
    let multiplier = 10_f64.powi(decimals as i32);
    (value * multiplier).round() / multiplier
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::calculate_all_weights;
    use crate::pricing::calculate_all_costs;

    #[test]
    fn test_pendant_report() {
        let prices = MetalPrices::default();
        let wax_pricing = WaxPricing::default();
        let volume = 1.0; // 1 cm³

        let weights = calculate_all_weights(volume, Some(wax_pricing.cost_per_gram));
        let costs = calculate_all_costs(&weights, &prices, &wax_pricing);

        let report = CostReport::new_pendant(
            "test.stl".to_string(),
            volume,
            &costs,
            &prices,
        );

        assert_eq!(report.jewelry_type, JewelryType::Pendant);
        assert_eq!(report.sizes.len(), 1);

        let json = report.to_json();
        assert!(json.contains("pendant"));
        assert!(json.contains("silver"));
    }

    #[test]
    fn test_ring_report() {
        let prices = MetalPrices::default();
        let wax_pricing = WaxPricing::default();
        let volume = 0.5; // 0.5 cm³
        let current_diameter = 17.3; // Size 7

        let sizes = RingSize::range(6.0, 8.0);
        let report = CostReport::new_ring(
            "ring.stl".to_string(),
            volume,
            current_diameter,
            &sizes,
            &prices,
            &wax_pricing,
        );

        assert_eq!(report.jewelry_type, JewelryType::Ring);
        assert_eq!(report.sizes.len(), 5); // 6, 6.5, 7, 7.5, 8

        // Size 7 should have scale factor of ~1.0
        let size_7 = report.sizes.iter().find(|s| s.ring_size == "US 7").unwrap();
        assert!((size_7.scale_factor - 1.0).abs() < 0.05);
    }

    #[test]
    fn test_json_serialization() {
        let prices = MetalPrices::default();
        let wax_pricing = WaxPricing::default();
        let weights = calculate_all_weights(1.0, None);
        let costs = calculate_all_costs(&weights, &prices, &wax_pricing);

        let report = CostReport::new_pendant("test.stl".to_string(), 1.0, &costs, &prices)
            .with_note("Test note");

        let json = report.to_json();
        assert!(json.contains("Test note"));

        // Verify it can be deserialized
        let parsed: CostReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.file_name, "test.stl");
    }
}
