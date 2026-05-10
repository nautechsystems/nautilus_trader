// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Instrument provider for loading and caching Coinbase instruments.

use std::sync::Arc;

use nautilus_core::AtomicMap;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

use crate::{
    common::enums::{CoinbaseFuturesAssetType, CoinbaseProductType},
    http::{
        client::CoinbaseHttpClient,
        models::{Product, ProductsResponse},
        parse::{is_perpetual_product, parse_instrument},
    },
};

/// Loads and caches Coinbase instruments.
///
/// Wraps a [`CoinbaseHttpClient`] and provides methods for loading instruments
/// from the REST API or from pre-fetched JSON responses. Parsed instruments are
/// cached in the HTTP client's shared `AtomicMap`.
#[derive(Debug, Clone)]
pub struct CoinbaseInstrumentProvider {
    client: CoinbaseHttpClient,
}

impl CoinbaseInstrumentProvider {
    /// Creates a new [`CoinbaseInstrumentProvider`].
    #[must_use]
    pub fn new(client: CoinbaseHttpClient) -> Self {
        Self { client }
    }

    /// Returns a reference to the instrument cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<AtomicMap<InstrumentId, InstrumentAny>> {
        self.client.instruments()
    }

    /// Returns the number of cached instruments.
    #[must_use]
    pub fn count(&self) -> usize {
        self.client.instruments().len()
    }

    /// Returns a cached instrument by ID, if present.
    #[must_use]
    pub fn get(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.client.instruments().get_cloned(instrument_id)
    }

    /// Loads all instruments from the Coinbase REST API and caches them.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn load_all(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let json = self
            .client
            .get_products()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch products: {e}"))?;

        self.load_from_products_response(&json)
    }

    /// Loads all instruments of a specific product type from the REST API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn load_all_filtered(
        &self,
        product_type: CoinbaseProductType,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let json = self
            .client
            .get_products()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch products: {e}"))?;

        self.load_from_products_response_filtered(&json, product_type)
    }

    /// Loads a single instrument by product ID from the REST API.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn load(&self, product_id: &str) -> anyhow::Result<InstrumentAny> {
        let json = self
            .client
            .get_product(product_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch product '{product_id}': {e}"))?;

        self.load_from_product_response(&json)
    }

    /// Parses a products list response and caches the instruments.
    ///
    /// Expects the JSON shape returned by `GET /products`: `{"products": [...]}`.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON cannot be deserialized or any product fails to parse.
    pub fn load_from_products_response(
        &self,
        json: &serde_json::Value,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let response: ProductsResponse =
            serde_json::from_value(json.clone()).map_err(|e| anyhow::anyhow!("{e}"))?;

        let instruments = self.parse_and_cache_products(&response.products)?;
        // Populate the alias map so subscribe paths (which only see the parsed
        // `InstrumentAny`) can resolve a caller-supplied product id back to the
        // canonical id Coinbase uses on the wire.
        self.client.record_product_aliases(&response.products);
        Ok(instruments)
    }

    /// Parses a products list response, filtering by product type, and caches the instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON cannot be deserialized or any product fails to parse.
    pub fn load_from_products_response_filtered(
        &self,
        json: &serde_json::Value,
        product_type: CoinbaseProductType,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let response: ProductsResponse =
            serde_json::from_value(json.clone()).map_err(|e| anyhow::anyhow!("{e}"))?;

        let filtered: Vec<&Product> = response
            .products
            .iter()
            .filter(|p| p.product_type == product_type)
            .collect();

        let instruments = self.parse_and_cache_product_refs(&filtered)?;
        // Filtering throws away non-matching products, but their alias
        // metadata is independent of the type filter and should still be
        // recorded so subsequent subscribes can resolve aliased pairs.
        self.client.record_product_aliases(&response.products);
        Ok(instruments)
    }

    /// Parses a single product response and caches the instrument.
    ///
    /// Expects the JSON shape returned by `GET /products/{product_id}`.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON cannot be deserialized or the product fails to parse.
    pub fn load_from_product_response(
        &self,
        json: &serde_json::Value,
    ) -> anyhow::Result<InstrumentAny> {
        let product: Product =
            serde_json::from_value(json.clone()).map_err(|e| anyhow::anyhow!("{e}"))?;

        anyhow::ensure!(
            is_supported_product(&product),
            "Unsupported product '{}' (type={}, non_crypto={})",
            product.product_id,
            product.product_type,
            product
                .future_product_details
                .as_ref()
                .is_some_and(|d| d.non_crypto),
        );

        let ts_init = self.client.ts_now();
        let instrument = parse_instrument(&product, ts_init)?;

        self.cache_instrument(&instrument);
        self.client
            .record_product_aliases(std::slice::from_ref(&product));

        Ok(instrument)
    }

    fn parse_and_cache_products(&self, products: &[Product]) -> anyhow::Result<Vec<InstrumentAny>> {
        let ts_init = self.client.ts_now();
        let mut instruments = Vec::with_capacity(products.len());

        for product in products {
            if !is_supported_product(product) {
                log::debug!("Skipping unsupported product '{}'", product.product_id);
                continue;
            }
            let instrument = parse_instrument(product, ts_init)?;
            instruments.push(instrument);
        }

        self.cache_instruments(&instruments);

        Ok(instruments)
    }

    fn parse_and_cache_product_refs(
        &self,
        products: &[&Product],
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let ts_init = self.client.ts_now();
        let mut instruments = Vec::with_capacity(products.len());

        for product in products {
            if !is_supported_product(product) {
                log::debug!("Skipping unsupported product '{}'", product.product_id);
                continue;
            }
            let instrument = parse_instrument(product, ts_init)?;
            instruments.push(instrument);
        }

        self.cache_instruments(&instruments);

        Ok(instruments)
    }

    fn cache_instrument(&self, instrument: &InstrumentAny) {
        self.client.instruments().rcu(|m| {
            m.insert(instrument.id(), instrument.clone());
        });
    }

    fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        self.client.instruments().rcu(|m| {
            for instrument in instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });
    }
}

/// Returns whether a product is supported for instrument parsing.
///
/// Rejects unknown product types and non-crypto futures (energy, metals, stocks).
fn is_supported_product(product: &Product) -> bool {
    match product.product_type {
        CoinbaseProductType::Unknown => false,
        CoinbaseProductType::Future => {
            match &product.future_product_details {
                Some(details) => {
                    if details.non_crypto {
                        return false;
                    }

                    match details.futures_asset_type {
                        Some(CoinbaseFuturesAssetType::Crypto) | None => true,
                        Some(_) => false,
                    }
                }
                // Dated futures need contract_expiry from details; perpetuals
                // can still be parsed via the display_name heuristic
                None => is_perpetual_product(product),
            }
        }
        CoinbaseProductType::Spot => true,
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{instruments::Instrument, types::Quantity};
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::common::testing::load_test_fixture;

    fn provider() -> CoinbaseInstrumentProvider {
        CoinbaseInstrumentProvider::new(CoinbaseHttpClient::default())
    }

    #[rstest]
    fn test_provider_starts_empty() {
        let provider = provider();
        assert_eq!(provider.count(), 0);
    }

    #[rstest]
    fn test_load_single_spot_product() {
        let provider = provider();
        let json_str = load_test_fixture("http_product.json");
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        let instrument = provider.load_from_product_response(&json).unwrap();

        assert!(matches!(instrument, InstrumentAny::CurrencyPair(_)));
        assert_eq!(instrument.id().symbol.as_str(), "BTC-USD");
        assert_eq!(instrument.id().venue.as_str(), "COINBASE");
        assert_eq!(provider.count(), 1);
        assert!(provider.get(&instrument.id()).is_some());
    }

    #[rstest]
    fn test_load_spot_products_from_list() {
        let provider = provider();
        let json_str = load_test_fixture("http_products.json");
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        let instruments = provider.load_from_products_response(&json).unwrap();

        assert_eq!(instruments.len(), 2);
        assert_eq!(provider.count(), 2);

        for inst in &instruments {
            assert!(matches!(inst, InstrumentAny::CurrencyPair(_)));
            assert!(provider.get(&inst.id()).is_some());
        }
    }

    #[rstest]
    fn test_load_future_products_distinguishes_perp_and_dated() {
        let provider = provider();
        let json_str = load_test_fixture("http_products_future.json");
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        let instruments = provider.load_from_products_response(&json).unwrap();

        assert_eq!(instruments.len(), 2);
        assert_eq!(provider.count(), 2);

        assert!(
            matches!(&instruments[0], InstrumentAny::CryptoPerpetual(_)),
            "Expected CryptoPerpetual, was {:?}",
            &instruments[0]
        );
        assert!(
            matches!(&instruments[1], InstrumentAny::CryptoFuture(_)),
            "Expected CryptoFuture, was {:?}",
            &instruments[1]
        );
    }

    #[rstest]
    fn test_load_filtered_spot_only() {
        let provider = provider();
        let json_str = load_test_fixture("http_products.json");
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        let instruments = provider
            .load_from_products_response_filtered(&json, CoinbaseProductType::Spot)
            .unwrap();

        assert_eq!(instruments.len(), 2);
        for inst in &instruments {
            assert!(matches!(inst, InstrumentAny::CurrencyPair(_)));
        }
    }

    #[rstest]
    fn test_load_filtered_future_excludes_spot() {
        let provider = provider();
        let json_str = load_test_fixture("http_products.json");
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        let instruments = provider
            .load_from_products_response_filtered(&json, CoinbaseProductType::Future)
            .unwrap();

        assert_eq!(instruments.len(), 0);
        assert_eq!(provider.count(), 0);
    }

    #[rstest]
    fn test_cache_updates_on_reload() {
        let provider = provider();
        let json_str = load_test_fixture("http_product.json");
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        let first = provider.load_from_product_response(&json).unwrap();
        assert_eq!(provider.count(), 1);

        let second = provider.load_from_product_response(&json).unwrap();
        assert_eq!(provider.count(), 1);
        assert_eq!(first.id(), second.id());
    }

    #[rstest]
    fn test_cache_accumulates_across_loads() {
        let provider = provider();

        let spot_json_str = load_test_fixture("http_products.json");
        let spot_json: serde_json::Value = serde_json::from_str(&spot_json_str).unwrap();
        provider.load_from_products_response(&spot_json).unwrap();
        assert_eq!(provider.count(), 2);

        let future_json_str = load_test_fixture("http_products_future.json");
        let future_json: serde_json::Value = serde_json::from_str(&future_json_str).unwrap();
        provider.load_from_products_response(&future_json).unwrap();
        assert_eq!(provider.count(), 4);
    }

    #[rstest]
    fn test_get_returns_none_for_missing_instrument() {
        let provider = provider();
        let missing_id = InstrumentId::from("MISSING-PAIR.COINBASE");
        assert!(provider.get(&missing_id).is_none());
    }

    #[rstest]
    fn test_load_from_invalid_json_returns_error() {
        let provider = provider();
        let invalid = json!({"not_a_product": true});
        assert!(provider.load_from_product_response(&invalid).is_err());
    }

    #[rstest]
    fn test_load_from_invalid_products_response_returns_error() {
        let provider = provider();
        let invalid = json!({"not_products": []});
        assert!(provider.load_from_products_response(&invalid).is_err());
    }

    #[rstest]
    fn test_spot_instrument_precision() {
        let provider = provider();
        let json_str = load_test_fixture("http_product.json");
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        let instrument = provider.load_from_product_response(&json).unwrap();

        assert_eq!(instrument.price_precision(), 2);
        assert_eq!(instrument.size_precision(), 8);
    }

    #[rstest]
    fn test_perpetual_instrument_fields() {
        let provider = provider();
        let json_str = load_test_fixture("http_products_future.json");
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        let instruments = provider.load_from_products_response(&json).unwrap();
        let perp = match &instruments[0] {
            InstrumentAny::CryptoPerpetual(p) => p,
            other => panic!("Expected CryptoPerpetual, was {other:?}"),
        };

        assert_eq!(perp.base_currency().unwrap().code.as_str(), "BTC");
        assert_eq!(perp.quote_currency().code.as_str(), "USD");
        assert_eq!(perp.multiplier, Quantity::from("0.01"));
    }

    #[rstest]
    fn test_future_instrument_has_expiry() {
        let provider = provider();
        let json_str = load_test_fixture("http_products_future.json");
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        let instruments = provider.load_from_products_response(&json).unwrap();
        let future = match &instruments[1] {
            InstrumentAny::CryptoFuture(f) => f,
            other => panic!("Expected CryptoFuture, was {other:?}"),
        };

        // 2026-04-24T15:00:00Z
        assert_eq!(future.expiration_ns.as_u64(), 1_777_042_800_000_000_000);
        assert_eq!(future.base_currency().unwrap().code.as_str(), "BTC");
    }

    /// Loads the spot fixture JSON and patches `product_type` and
    /// `future_product_details` to build a Product with controlled support fields.
    fn make_product(product_type: &str, future_details: Option<serde_json::Value>) -> Product {
        let json_str = load_test_fixture("http_product.json");
        let mut json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        json["product_type"] = serde_json::Value::String(product_type.to_string());

        if let Some(details) = future_details {
            json["future_product_details"] = details;
        } else {
            json["future_product_details"] = serde_json::Value::Null;
        }

        serde_json::from_value(json).unwrap()
    }

    #[rstest]
    #[case::spot("SPOT", None, true)]
    #[case::future_crypto(
        "FUTURE",
        Some(json!({"venue":"cde","contract_code":"BIT","contract_expiry":"2026-04-24T15:00:00Z","contract_size":"0.01","contract_root_unit":"BTC","group_description":"","contract_expiry_timezone":"","group_short_description":"","risk_managed_by":"MANAGED_BY_FCM","contract_expiry_type":"EXPIRING","contract_display_name":"","non_crypto":false,"futures_asset_type":"FUTURES_ASSET_TYPE_CRYPTO"})),
        true
    )]
    #[case::future_no_details("FUTURE", None, false)]
    #[case::future_no_asset_type(
        "FUTURE",
        Some(json!({"venue":"cde","contract_code":"BIT","contract_expiry":"2026-04-24T15:00:00Z","contract_size":"0.01","contract_root_unit":"BTC","group_description":"","contract_expiry_timezone":"","group_short_description":"","risk_managed_by":"MANAGED_BY_FCM","contract_expiry_type":"EXPIRING","contract_display_name":"","non_crypto":false})),
        true
    )]
    #[case::unknown("UNKNOWN_PRODUCT_TYPE", None, false)]
    #[case::future_non_crypto(
        "FUTURE",
        Some(json!({"venue":"cde","contract_code":"BIT","contract_expiry":"2026-04-24T15:00:00Z","contract_size":"0.01","contract_root_unit":"BTC","group_description":"","contract_expiry_timezone":"","group_short_description":"","risk_managed_by":"MANAGED_BY_FCM","contract_expiry_type":"EXPIRING","contract_display_name":"","non_crypto":true,"futures_asset_type":"FUTURES_ASSET_TYPE_CRYPTO"})),
        false
    )]
    #[case::future_energy(
        "FUTURE",
        Some(json!({"venue":"cde","contract_code":"OIL","contract_expiry":"2026-04-24T15:00:00Z","contract_size":"1","contract_root_unit":"OIL","group_description":"","contract_expiry_timezone":"","group_short_description":"","risk_managed_by":"MANAGED_BY_FCM","contract_expiry_type":"EXPIRING","contract_display_name":"","non_crypto":false,"futures_asset_type":"FUTURES_ASSET_TYPE_ENERGY"})),
        false
    )]
    #[case::future_metals(
        "FUTURE",
        Some(json!({"venue":"cde","contract_code":"GLD","contract_expiry":"2026-04-24T15:00:00Z","contract_size":"1","contract_root_unit":"GLD","group_description":"","contract_expiry_timezone":"","group_short_description":"","risk_managed_by":"MANAGED_BY_FCM","contract_expiry_type":"EXPIRING","contract_display_name":"","non_crypto":false,"futures_asset_type":"FUTURES_ASSET_TYPE_METALS"})),
        false
    )]
    #[case::future_stocks(
        "FUTURE",
        Some(json!({"venue":"cde","contract_code":"SPX","contract_expiry":"2026-04-24T15:00:00Z","contract_size":"1","contract_root_unit":"SPX","group_description":"","contract_expiry_timezone":"","group_short_description":"","risk_managed_by":"MANAGED_BY_FCM","contract_expiry_type":"EXPIRING","contract_display_name":"","non_crypto":false,"futures_asset_type":"FUTURES_ASSET_TYPE_STOCKS"})),
        false
    )]
    fn test_is_supported_product(
        #[case] product_type: &str,
        #[case] future_details: Option<serde_json::Value>,
        #[case] expected: bool,
    ) {
        let product = make_product(product_type, future_details);
        assert_eq!(is_supported_product(&product), expected);
    }

    #[rstest]
    fn test_load_from_product_response_rejects_unsupported() {
        let provider = provider();
        let json_str = load_test_fixture("http_product.json");
        let mut json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        json["product_type"] = serde_json::Value::String("OPTIONS".to_string());

        let result = provider.load_from_product_response(&json);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Unsupported product"),
            "Expected 'Unsupported product' in error, was: {err_msg}"
        );
    }

    #[rstest]
    fn test_future_no_details_with_perp_display_name_is_supported() {
        let json_str = load_test_fixture("http_product.json");
        let mut json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        json["product_type"] = serde_json::Value::String("FUTURE".to_string());
        json["display_name"] = serde_json::Value::String("BTC PERP".to_string());
        json["future_product_details"] = serde_json::Value::Null;

        let product: Product = serde_json::from_value(json).unwrap();

        assert!(is_supported_product(&product));
    }

    #[rstest]
    fn test_future_no_details_without_perp_display_name_is_unsupported() {
        let product = make_product("FUTURE", None);
        assert!(!is_supported_product(&product));
    }

    #[rstest]
    fn test_bulk_load_skips_unsupported_products() {
        let provider = provider();
        let spot_json_str = load_test_fixture("http_product.json");
        let spot_json: serde_json::Value = serde_json::from_str(&spot_json_str).unwrap();

        let mut unknown_json = spot_json.clone();
        unknown_json["product_id"] = serde_json::Value::String("UNKNOWN-PAIR".to_string());
        unknown_json["product_type"] = serde_json::Value::String("OPTIONS".to_string());

        let response = json!({
            "products": [spot_json, unknown_json],
            "num_products": 2
        });

        let instruments = provider.load_from_products_response(&response).unwrap();

        assert_eq!(instruments.len(), 1);
        assert_eq!(instruments[0].id().symbol.as_str(), "BTC-USD");
        assert_eq!(provider.count(), 1);
    }
}
