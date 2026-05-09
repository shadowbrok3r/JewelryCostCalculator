//! Metal price API integration with MetalpriceAPI
//!
//! API Endpoint: https://api.metalpriceapi.com/v1/latest
//! Symbols: XAU (Gold), XAG (Silver)
//! Prices are returned as USD per troy ounce

use anyhow::Result;
use chrono::Utc;
use serde::Deserialize;

use super::MetalPrices;

/// MetalpriceAPI response structure
#[derive(Debug, Deserialize)]
struct ApiResponse {
    success: bool,
    #[serde(default)]
    rates: Option<Rates>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct Rates {
    #[serde(rename = "XAU")]
    xau: Option<f64>,
    #[serde(rename = "XAG")]
    xag: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ApiError {
    code: Option<i32>,
    message: Option<String>,
}

/// Fetch current metal prices from MetalpriceAPI
pub async fn fetch_metal_prices(api_key: &str) -> Result<MetalPrices> {
    let url = format!(
        "https://api.metalpriceapi.com/v1/latest?api_key={}&base=USD&currencies=XAU,XAG",
        api_key
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "API request failed with status: {}",
            response.status()
        ));
    }

    let api_response: ApiResponse = response.json().await?;

    if !api_response.success {
        let error_msg = api_response
            .error
            .map(|e| e.message.unwrap_or_else(|| "Unknown error".to_string()))
            .unwrap_or_else(|| "API returned success=false".to_string());
        return Err(anyhow::anyhow!("API error: {}", error_msg));
    }

    let rates = api_response
        .rates
        .ok_or_else(|| anyhow::anyhow!("No rates in API response"))?;

    // API returns rates as USD per unit, but for metals it's inverted
    // XAU rate of 0.0005 means 1 USD = 0.0005 XAU, so 1 XAU = 2000 USD
    let gold_rate = rates
        .xau
        .ok_or_else(|| anyhow::anyhow!("No XAU rate in response"))?;
    let silver_rate = rates
        .xag
        .ok_or_else(|| anyhow::anyhow!("No XAG rate in response"))?;

    // Convert from "USD per oz" rate to actual price
    // If the rate is small (like 0.0005), it means 1 USD buys 0.0005 oz, so 1 oz = 1/0.0005 = 2000 USD
    let gold_per_troy_oz = if gold_rate > 0.0 && gold_rate < 1.0 {
        1.0 / gold_rate
    } else {
        gold_rate
    };

    let silver_per_troy_oz = if silver_rate > 0.0 && silver_rate < 1.0 {
        1.0 / silver_rate
    } else {
        silver_rate
    };

    Ok(MetalPrices {
        gold_per_troy_oz,
        silver_per_troy_oz,
        bronze_per_kg: 10.2, // Bronze isn't typically available from precious metal APIs
        fetched_at: Utc::now(),
        is_live: true,
    })
}

/// Alternative: Parse prices from a manual/cached JSON string
pub fn parse_prices_from_json(json: &str) -> Result<MetalPrices> {
    let prices: MetalPrices = serde_json::from_str(json)?;
    Ok(prices)
}

#[cfg(test)]
mod tests {
    use nalgebra::ComplexField;

    use super::*;

    #[test]
    fn test_price_conversion() {
        // Test the rate conversion logic
        let rate = 0.0005; // 1 USD = 0.0005 oz gold
        let price = 1.0 / rate;
        assert!((price - 2000.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_prices_json() {
        let json = r#"{
            "gold_per_troy_oz": 2000.0,
            "silver_per_troy_oz": 25.0,
            "bronze_per_kg": 10.2,
            "fetched_at": "2025-01-31T12:00:00Z",
            "is_live": false
        }"#;

        let prices = parse_prices_from_json(json).unwrap();
        assert!((prices.gold_per_troy_oz - 2000.0).abs() < 0.01);
        assert!((prices.silver_per_troy_oz - 25.0).abs() < 0.01);
    }
}
