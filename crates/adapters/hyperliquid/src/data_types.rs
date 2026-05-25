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

//! Hyperliquid-specific custom data types.
//!
//! These types carry Hyperliquid domain data through the Nautilus data engine as
//! [`CustomData`](nautilus_model::data::CustomData).

use std::collections::HashMap;

use nautilus_core::UnixNanos;
use nautilus_model::{identifiers::InstrumentId, types::Price};
use nautilus_persistence_macros::custom_data;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Hyperliquid all mid prices snapshot from the `allMids` WebSocket channel.
#[cfg_attr(
    feature = "arrow",
    custom_data(pyo3, stub_module = "nautilus_trader.hyperliquid")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.hyperliquid")
)]
pub struct HyperliquidAllMids {
    /// Mapping of instrument ID to mid price for all tradable coins.
    #[custom_data_field(json)]
    pub mids: HashMap<InstrumentId, Price>,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Hyperliquid open interest update from the `activeAssetCtx` WebSocket channel.
///
/// Hyperliquid does not provide a native event timestamp on this payload, so
/// `ts_event` mirrors `ts_init` like the peer asset-context update types.
#[cfg_attr(
    feature = "arrow",
    custom_data(pyo3, stub_module = "nautilus_trader.hyperliquid")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.hyperliquid")
)]
pub struct HyperliquidOpenInterest {
    /// The instrument ID for this open interest update.
    pub instrument_id: InstrumentId,
    /// The current open interest for the perpetual instrument.
    #[custom_data_field(json)]
    pub open_interest: Decimal,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Impact prices reported by Hyperliquid for venue-side execution estimates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HyperliquidImpactPrices {
    /// Impact bid price.
    pub bid: Price,
    /// Impact ask price.
    pub ask: Price,
}

/// Normalized per-instrument entry within `allDexsAssetCtxs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HyperliquidDexAssetCtx {
    /// Hyperliquid dex identifier. The default perp dex uses the empty string.
    pub dex: String,
    /// Canonical Nautilus instrument ID.
    pub instrument_id: InstrumentId,
    /// Mark price.
    pub mark_price: Price,
    /// Oracle/index price.
    pub oracle_price: Price,
    /// Previous day price.
    pub prev_day_price: Price,
    /// Optional mid price.
    pub mid_price: Option<Price>,
    /// Optional impact prices.
    pub impact_prices: Option<HyperliquidImpactPrices>,
    /// Current funding rate.
    pub funding_rate: Decimal,
    /// Current open interest.
    pub open_interest: Decimal,
    /// Optional premium.
    pub premium: Option<Decimal>,
    /// 24h notional volume.
    pub day_ntl_volume: Decimal,
    /// 24h base volume.
    pub day_base_volume: Decimal,
}

/// Hyperliquid normalized aggregate snapshot from the `allDexsAssetCtxs` WebSocket channel.
///
/// This feed is live-only and intentionally JSON-backed; it is not coupled to Arrow persistence.
#[custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.hyperliquid")]
pub struct HyperliquidAllDexsAssetCtxs {
    /// Normalized per-instrument entries across all perp dexes.
    #[custom_data_field(json)]
    pub entries: Vec<HyperliquidDexAssetCtx>,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Registers Hyperliquid custom data types.
///
/// Safe to call multiple times (idempotent via internal `Once` guards).
pub fn register_hyperliquid_custom_data() {
    #[cfg(feature = "arrow")]
    {
        nautilus_serialization::ensure_custom_data_registered::<HyperliquidAllMids>();
        nautilus_serialization::ensure_custom_data_registered::<HyperliquidOpenInterest>();
    }

    #[cfg(not(feature = "arrow"))]
    {
        let _ = nautilus_model::data::ensure_custom_data_json_registered::<HyperliquidAllMids>();
        let _ =
            nautilus_model::data::ensure_custom_data_json_registered::<HyperliquidOpenInterest>();
    }

    let _ =
        nautilus_model::data::ensure_custom_data_json_registered::<HyperliquidAllDexsAssetCtxs>();
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_register_hyperliquid_custom_data_is_idempotent() {
        register_hyperliquid_custom_data();
        register_hyperliquid_custom_data();
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_hyperliquid_all_mids_arrow_schema() {
        use arrow::datatypes::DataType;
        use nautilus_serialization::arrow::ArrowSchemaProvider;

        let schema = HyperliquidAllMids::get_schema(None);

        assert_eq!(schema.fields().len(), 3);
        assert_eq!(schema.field(0).name(), "mids");
        assert_eq!(schema.field(0).data_type(), &DataType::Utf8);
        assert_eq!(schema.field(1).name(), "ts_event");
        assert_eq!(schema.field(1).data_type(), &DataType::UInt64);
        assert_eq!(schema.field(2).name(), "ts_init");
        assert_eq!(schema.field(2).data_type(), &DataType::UInt64);
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_hyperliquid_open_interest_arrow_schema() {
        use arrow::datatypes::DataType;
        use nautilus_serialization::arrow::ArrowSchemaProvider;

        let schema = HyperliquidOpenInterest::get_schema(None);

        assert_eq!(schema.fields().len(), 4);
        assert_eq!(schema.field(0).name(), "instrument_id");
        assert!(matches!(
            schema.field(0).data_type(),
            DataType::Utf8 | DataType::Utf8View
        ));
        assert_eq!(schema.field(1).name(), "open_interest");
        assert!(matches!(
            schema.field(1).data_type(),
            DataType::Utf8 | DataType::Utf8View
        ));
        assert_eq!(schema.field(2).name(), "ts_event");
        assert_eq!(schema.field(2).data_type(), &DataType::UInt64);
        assert_eq!(schema.field(3).name(), "ts_init");
        assert_eq!(schema.field(3).data_type(), &DataType::UInt64);
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_hyperliquid_open_interest_arrow_round_trip_preserves_decimal() {
        use std::str::FromStr;

        use nautilus_model::data::Data;
        use nautilus_serialization::arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch};

        let original = HyperliquidOpenInterest::new(
            InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"),
            Decimal::from_str("123456.789012345678").unwrap(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let metadata = EncodeToRecordBatch::metadata(&original);
        let batch =
            HyperliquidOpenInterest::encode_batch(&metadata, std::slice::from_ref(&original))
                .unwrap();
        let decoded = HyperliquidOpenInterest::decode_data_batch(&metadata, batch).unwrap();

        assert_eq!(decoded.len(), 1);
        match &decoded[0] {
            Data::Custom(custom) => {
                let open_interest = custom
                    .data
                    .as_any()
                    .downcast_ref::<HyperliquidOpenInterest>()
                    .expect("expected HyperliquidOpenInterest");
                assert_eq!(open_interest.instrument_id, original.instrument_id);
                assert_eq!(open_interest.open_interest, original.open_interest);
                assert_eq!(open_interest.ts_event, original.ts_event);
                assert_eq!(open_interest.ts_init, original.ts_init);
            }
            other => panic!("Expected Data::Custom, was {other:?}"),
        }
    }
}
