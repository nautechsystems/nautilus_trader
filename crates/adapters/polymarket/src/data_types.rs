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
//! These types carry Polymarket PolyBolt reference-price data through the Nautilus data engine
//! as [`CustomData`](nautilus_model::data::CustomData). Values use the venue's exact
//! `full_accuracy_value` decimal (up to 18 fractional digits), which exceeds the `Price`
//! precision limit, so values are carried as `Decimal`. Spot prices fall back to the display
//! `value` when the exact field is absent; TWAP requires the exact field.

use nautilus_core::UnixNanos;
use nautilus_model::custom_data;
use rust_decimal::Decimal;

/// Polymarket PolyBolt crypto spot price sample from the `price.crypto` channel.
///
/// The adapter normalizes both live frames and the two-minute subscription snapshot into this
/// per-tick custom data type.
#[custom_data(pyo3, stub_module = "nautilus_trader.adapters.polymarket")]
pub struct PolymarketCryptoPrice {
    /// Lowercase USD-quoted venue symbol, e.g. `btcusd`.
    pub symbol: String,
    /// Price provider reported by the venue, e.g. `chainlink` or `pyth`.
    pub source: String,
    /// Exact spot price from the venue's `full_accuracy_value`, otherwise the display `value`.
    #[custom_data_field(serde)]
    pub value: Decimal,
    /// Price measurement timestamp in Unix milliseconds.
    pub price_timestamp_ms: u64,
    /// PolyBolt envelope timestamp in Unix milliseconds.
    pub message_timestamp_ms: u64,
    /// UNIX timestamp (nanoseconds) when the price event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Chainlink-computed 60-second crypto TWAP sample from the `price.crypto.twap` channel.
///
/// The adapter derives `value` only from the exact `full_accuracy_value` field. The numeric
/// display field in the payload is validated but never authoritative.
///
/// Subscriptions begin with a two-minute snapshot and continue with live updates. The adapter
/// suppresses older observations and exact same-timestamp redeliveries, including snapshot
/// replay after a reconnect. A changed value at the same observation timestamp is reported as a
/// protocol error and is not emitted; the previous observation remains authoritative, so
/// emission resumes only at a newer timestamp in a subsequent frame. Snapshot processing stops
/// at the first conflicting or invalid point and discards the remaining points in that frame.
#[custom_data(pyo3, stub_module = "nautilus_trader.adapters.polymarket")]
pub struct PolymarketCryptoTwap {
    /// Lowercase USD-quoted venue symbol, e.g. `btcusd`.
    pub symbol: String,
    /// Price provider reported by the venue, always `chainlink`.
    pub source: String,
    /// Provider TWAP lookback window in seconds, always `60`.
    pub window_seconds: u32,
    /// Exact Chainlink-computed TWAP from the venue's `full_accuracy_value`.
    #[custom_data_field(serde)]
    pub value: Decimal,
    /// Chainlink observation timestamp in Unix milliseconds.
    pub observation_timestamp_ms: u64,
    /// PolyBolt envelope timestamp in Unix milliseconds.
    pub message_timestamp_ms: u64,
    /// UNIX timestamp (nanoseconds) when the Chainlink observation occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Polymarket PolyBolt equity price sample from the `price.equity` channel.
///
/// Covers equities, ETFs, forex pairs, metals, and commodities. The adapter normalizes both
/// live frames and the two-minute subscription snapshot into this per-tick custom data type.
#[custom_data(pyo3, stub_module = "nautilus_trader.adapters.polymarket")]
pub struct PolymarketEquityPrice {
    /// Lowercase venue symbol, e.g. `aapl`, `eurusd`, or `xauusd`.
    pub symbol: String,
    /// Price provider reported by the venue, e.g. `pyth`, `massive`, or `chainlink`.
    pub source: String,
    /// Exact spot price from the venue's `full_accuracy_value`, otherwise the display `value`.
    #[custom_data_field(serde)]
    pub value: Decimal,
    /// Price measurement timestamp in Unix milliseconds.
    pub price_timestamp_ms: u64,
    /// PolyBolt envelope timestamp in Unix milliseconds.
    pub message_timestamp_ms: u64,
    /// Venue ingest receipt timestamp in Unix milliseconds when present; `None` for snapshots.
    #[custom_data_field(serde)]
    pub received_at_ms: Option<u64>,
    /// `true` when the venue is carrying forward the last session's value; `false` for snapshots.
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
    let _ = nautilus_model::data::ensure_custom_data_json_registered::<PolymarketCryptoPrice>();
    let _ = nautilus_model::data::ensure_custom_data_json_registered::<PolymarketCryptoTwap>();
    let _ = nautilus_model::data::ensure_custom_data_json_registered::<PolymarketEquityPrice>();
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::{PolymarketCryptoTwap, register_polymarket_custom_data};

    #[rstest]
    fn test_register_polymarket_custom_data_is_idempotent() {
        register_polymarket_custom_data();
        register_polymarket_custom_data();
    }

    #[rstest]
    fn test_crypto_twap_json_round_trip_preserves_exact_decimal() {
        let value = PolymarketCryptoTwap::new(
            "btcusd".to_string(),
            "chainlink".to_string(),
            60,
            dec!(84291.577380840025358336),
            1791355667000,
            1791355667000,
            UnixNanos::from_millis(1791355667000),
            UnixNanos::from_millis(1791355667148),
        );

        let encoded = serde_json::to_string(&value).expect("serialize exact TWAP value");
        let decoded: PolymarketCryptoTwap =
            serde_json::from_str(&encoded).expect("deserialize exact TWAP value");

        let adjacent = PolymarketCryptoTwap::new(
            "btcusd".to_string(),
            "chainlink".to_string(),
            60,
            dec!(84291.577380840025358337),
            1791355667000,
            1791355667000,
            UnixNanos::from_millis(1791355667000),
            UnixNanos::from_millis(1791355667148),
        );
        let adjacent_encoded =
            serde_json::to_string(&adjacent).expect("serialize adjacent exact TWAP value");

        assert_eq!(decoded, value);
        assert_eq!(decoded.value, dec!(84291.577380840025358336));
        assert!(
            encoded.contains("\"value\":\"84291.577380840025358336\""),
            "encoded TWAP value was {encoded}"
        );
        assert!(
            adjacent_encoded.contains("\"value\":\"84291.577380840025358337\""),
            "encoded adjacent TWAP value was {adjacent_encoded}"
        );
        assert_ne!(encoded, adjacent_encoded);
    }
}
