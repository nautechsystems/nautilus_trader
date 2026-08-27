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

//! Polymarket-specific custom data types.
//!
//! These types carry Polymarket RTDS domain data through the Nautilus data engine as
//! [`CustomData`](nautilus_model::data::CustomData).

use nautilus_core::UnixNanos;
use nautilus_model::types::Price;
use nautilus_persistence_macros::custom_data;
use rust_decimal::Decimal;

/// Polymarket RTDS crypto price sample from the `crypto_prices` topic.
///
/// The adapter normalizes both live `update` frames and `subscribe` backfill
/// snapshots into this per-tick custom data type.
#[custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.polymarket")]
pub struct PolymarketRtdsCryptoPrice {
    /// Lowercase venue symbol, e.g. `btcusdt`.
    pub symbol: String,
    /// Current spot price.
    #[custom_data_field(serde)]
    pub value: Price,
    /// Price measurement timestamp in Unix milliseconds.
    pub price_timestamp_ms: u64,
    /// RTDS envelope timestamp in Unix milliseconds.
    pub message_timestamp_ms: u64,
    /// UNIX timestamp (nanoseconds) when the price event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Chainlink-computed crypto TWAP sample relayed by Polymarket RTDS.
///
/// The adapter derives `value` only from the exact signed E18 provider field.
/// The numeric display field in the RTDS payload is never authoritative.
#[custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.polymarket")]
pub struct PolymarketRtdsCryptoTwap {
    /// Lowercase slash-delimited venue symbol, e.g. `btc/usd`.
    pub symbol: String,
    /// Provider TWAP lookback window in seconds (`30` or `60`).
    pub window_seconds: u32,
    /// Exact Chainlink-computed TWAP decoded from its signed E18 integer.
    #[custom_data_field(serde)]
    pub value: Decimal,
    /// Chainlink observation timestamp in Unix milliseconds.
    pub observation_timestamp_ms: u64,
    /// RTDS publisher timestamp in Unix milliseconds.
    pub message_timestamp_ms: u64,
    /// UNIX timestamp (nanoseconds) when the Chainlink observation occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Polymarket RTDS equity price sample from the `equity_prices` topic.
///
/// The adapter normalizes both live `update` frames and `subscribe` backfill
/// snapshots into this per-tick custom data type.
#[custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.polymarket")]
pub struct PolymarketRtdsEquityPrice {
    /// Lowercase venue symbol, e.g. `aapl`, `eurusd`, or `xauusd`.
    pub symbol: String,
    /// Spot price rounded to the venue's float payload precision.
    #[custom_data_field(serde)]
    pub value: Price,
    /// Full-precision spot price when supplied, otherwise the venue's `value`.
    #[custom_data_field(serde)]
    pub full_accuracy_value: Price,
    /// Price measurement timestamp in Unix milliseconds.
    pub price_timestamp_ms: u64,
    /// RTDS envelope timestamp in Unix milliseconds.
    pub message_timestamp_ms: u64,
    /// System receipt timestamp in Unix milliseconds when present.
    #[custom_data_field(serde)]
    pub received_at_ms: Option<u64>,
    /// `true` when the venue is carrying forward the last known value.
    pub is_carried_forward: bool,
    /// UNIX timestamp (nanoseconds) when the price event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Registers Polymarket custom data types.
///
/// Safe to call multiple times (idempotent via internal `Once` guards).
pub fn register_polymarket_custom_data() {
    let _ = nautilus_model::data::ensure_custom_data_json_registered::<PolymarketRtdsCryptoPrice>();
    let _ = nautilus_model::data::ensure_custom_data_json_registered::<PolymarketRtdsCryptoTwap>();
    let _ = nautilus_model::data::ensure_custom_data_json_registered::<PolymarketRtdsEquityPrice>();
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::{PolymarketRtdsCryptoTwap, register_polymarket_custom_data};

    #[rstest]
    fn test_register_polymarket_custom_data_is_idempotent() {
        register_polymarket_custom_data();
        register_polymarket_custom_data();
    }

    #[rstest]
    fn test_crypto_twap_json_round_trip_preserves_exact_decimal() {
        let value = PolymarketRtdsCryptoTwap::new(
            "btc/usd".to_string(),
            60,
            dec!(64997.810000000000000001),
            1786179814000,
            1786179814147,
            UnixNanos::from_millis(1786179814000),
            UnixNanos::from_millis(1786179814148),
        );

        let encoded = serde_json::to_string(&value).expect("serialize exact TWAP value");
        let decoded: PolymarketRtdsCryptoTwap =
            serde_json::from_str(&encoded).expect("deserialize exact TWAP value");

        let adjacent = PolymarketRtdsCryptoTwap::new(
            "btc/usd".to_string(),
            60,
            dec!(64997.810000000000000002),
            1786179814000,
            1786179814147,
            UnixNanos::from_millis(1786179814000),
            UnixNanos::from_millis(1786179814148),
        );
        let adjacent_encoded =
            serde_json::to_string(&adjacent).expect("serialize adjacent exact TWAP value");

        assert_eq!(decoded, value);
        assert_eq!(decoded.value, dec!(64997.810000000000000001));
        assert!(
            encoded.contains("\"value\":\"64997.810000000000000001\""),
            "encoded TWAP value was {encoded}"
        );
        assert!(
            adjacent_encoded.contains("\"value\":\"64997.810000000000000002\""),
            "encoded adjacent TWAP value was {adjacent_encoded}"
        );
        assert_ne!(encoded, adjacent_encoded);
    }
}
