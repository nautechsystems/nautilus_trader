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
use nautilus_model::{
    enums::{AggressorSide, OrderSide},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use nautilus_persistence_macros::custom_data;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::common::enums::HyperliquidTwapStatus;

/// Hyperliquid all mid prices snapshot from the `allMids` WebSocket channel.
#[cfg_attr(
    feature = "arrow",
    custom_data(pyo3, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
pub struct HyperliquidAllMids {
    /// Mapping of instrument ID to mid price for all tradable coins.
    #[custom_data_field(serde)]
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
    custom_data(pyo3, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
pub struct HyperliquidOpenInterest {
    /// The instrument ID for this open interest update.
    pub instrument_id: InstrumentId,
    /// The current open interest for the perpetual instrument.
    #[custom_data_field(serde)]
    pub open_interest: Decimal,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// A complete public Hyperliquid trade, including the venue-provided counterparties.
///
/// This is opt-in adapter-specific data. It deliberately does not extend the
/// generic [`TradeTick`](nautilus_model::data::TradeTick), and is self-contained
/// so one catalog stream can be recorded and replayed without joining sidecar data.
#[cfg_attr(
    feature = "arrow",
    custom_data(pyo3, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
pub struct HyperliquidPublicTrade {
    /// The instrument ID for this trade.
    pub instrument_id: InstrumentId,
    /// The trade price normalized to the instrument's precision.
    #[custom_data_field(serde)]
    pub price: Price,
    /// The trade size normalized to the instrument's precision.
    #[custom_data_field(serde)]
    pub size: Quantity,
    /// The aggressor side reported by Hyperliquid.
    #[custom_data_field(serde)]
    pub aggressor_side: AggressorSide,
    /// Hyperliquid venue trade identifier.
    pub trade_id: String,
    /// Buyer wallet address reported by Hyperliquid.
    pub buyer: String,
    /// Seller wallet address reported by Hyperliquid.
    pub seller: String,
    /// Hyperliquid trade hash.
    pub hash: String,
    /// UNIX timestamp (nanoseconds) when the trade occurred.
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
#[custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.hyperliquid")]
pub struct HyperliquidAllDexsAssetCtxs {
    /// Normalized per-instrument entries across all perp dexes.
    #[custom_data_field(serde)]
    pub entries: Vec<HyperliquidDexAssetCtx>,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// One history row from the Hyperliquid `userTwapHistory` WebSocket channel.
///
/// Opt-in custom data (not part of default user execution subscriptions).
#[cfg_attr(
    feature = "arrow",
    custom_data(pyo3, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
pub struct HyperliquidTwapHistory {
    /// User address from the subscription envelope.
    pub user: String,
    /// Venue TWAP id (`twapId`) when present on the row.
    #[custom_data_field(serde)]
    pub twap_id: Option<u64>,
    /// Raw Hyperliquid coin symbol from `state.coin`.
    pub coin: String,
    /// Resolved Nautilus instrument ID when the coin is known in cache.
    #[custom_data_field(serde)]
    pub instrument_id: Option<InstrumentId>,
    /// TWAP order side.
    #[custom_data_field(serde)]
    pub side: OrderSide,
    /// Total TWAP size.
    #[custom_data_field(serde)]
    pub size: Decimal,
    /// Executed size so far.
    #[custom_data_field(serde)]
    pub executed_size: Decimal,
    /// Executed notional so far.
    #[custom_data_field(serde)]
    pub executed_notional: Decimal,
    /// TWAP duration in minutes.
    pub minutes: u32,
    /// Whether the TWAP is reduce-only.
    pub reduce_only: bool,
    /// Whether slice timing is randomized.
    pub randomize: bool,
    /// Venue TWAP status.
    #[custom_data_field(serde)]
    pub status: HyperliquidTwapStatus,
    /// Venue status description.
    pub status_description: String,
    /// `state.timestamp` converted to UNIX nanoseconds.
    pub state_timestamp: UnixNanos,
    /// Whether this event belongs to a venue snapshot batch.
    pub is_snapshot: bool,
    /// UNIX timestamp (nanoseconds) when the history row was produced (`history.time`).
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// One slice fill from the Hyperliquid `userTwapSliceFills` WebSocket channel.
///
/// Opt-in custom data (not part of default user execution subscriptions).
#[cfg_attr(
    feature = "arrow",
    custom_data(pyo3, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.hyperliquid")
)]
pub struct HyperliquidTwapSliceFill {
    /// User address from the subscription envelope.
    pub user: String,
    /// Venue TWAP order identifier.
    pub twap_id: u64,
    /// Raw Hyperliquid coin symbol from the fill.
    pub coin: String,
    /// Resolved Nautilus instrument ID when the coin is known in cache.
    #[custom_data_field(serde)]
    pub instrument_id: Option<InstrumentId>,
    /// Fill price.
    #[custom_data_field(serde)]
    pub price: Decimal,
    /// Fill size.
    #[custom_data_field(serde)]
    pub size: Decimal,
    /// Fill side.
    #[custom_data_field(serde)]
    pub side: OrderSide,
    /// L1 transaction hash.
    pub hash: String,
    /// Venue order id for the slice.
    pub oid: u64,
    /// Venue trade id.
    pub tid: u64,
    /// Whether the fill crossed the spread (taker).
    pub crossed: bool,
    /// Fee amount (negative means rebate).
    #[custom_data_field(serde)]
    pub fee: Decimal,
    /// Token the fee was paid in.
    pub fee_token: String,
    /// Frontend display direction string from the venue.
    pub dir: String,
    /// Closed PnL for the fill.
    #[custom_data_field(serde)]
    pub closed_pnl: Decimal,
    /// Whether this event belongs to a venue snapshot batch.
    pub is_snapshot: bool,
    /// UNIX timestamp (nanoseconds) when the fill occurred.
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
        nautilus_serialization::ensure_custom_data_registered::<HyperliquidPublicTrade>();
        nautilus_serialization::ensure_custom_data_registered::<HyperliquidTwapHistory>();
        nautilus_serialization::ensure_custom_data_registered::<HyperliquidTwapSliceFill>();
    }

    #[cfg(not(feature = "arrow"))]
    {
        let _ = nautilus_model::data::ensure_custom_data_json_registered::<HyperliquidAllMids>();
        let _ =
            nautilus_model::data::ensure_custom_data_json_registered::<HyperliquidOpenInterest>();
        let _ =
            nautilus_model::data::ensure_custom_data_json_registered::<HyperliquidPublicTrade>();
        let _ =
            nautilus_model::data::ensure_custom_data_json_registered::<HyperliquidTwapHistory>();
        let _ =
            nautilus_model::data::ensure_custom_data_json_registered::<HyperliquidTwapSliceFill>();
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

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_hyperliquid_public_trade_arrow_round_trip_preserves_counterparties() {
        use nautilus_model::{
            data::Data,
            enums::AggressorSide,
            types::{Price, Quantity},
        };
        use nautilus_serialization::arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch};

        let original = HyperliquidPublicTrade::new(
            InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"),
            Price::from("100000.50"),
            Quantity::from("0.123"),
            AggressorSide::Buyer,
            "123456".to_string(),
            "0xbuyer".to_string(),
            "0xseller".to_string(),
            "0xhash".to_string(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let metadata = EncodeToRecordBatch::metadata(&original);
        let batch =
            HyperliquidPublicTrade::encode_batch(&metadata, std::slice::from_ref(&original))
                .unwrap();
        let decoded = HyperliquidPublicTrade::decode_data_batch(&metadata, batch).unwrap();

        let Data::Custom(custom) = &decoded[0] else {
            panic!("Expected Data::Custom");
        };
        let trade = custom
            .data
            .as_any()
            .downcast_ref::<HyperliquidPublicTrade>()
            .expect("expected HyperliquidPublicTrade");
        assert_eq!(trade.buyer, original.buyer);
        assert_eq!(trade.seller, original.seller);
        assert_eq!(trade.hash, original.hash);
        assert_eq!(trade.price, original.price);
        assert_eq!(trade.size, original.size);
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_hyperliquid_twap_history_arrow_schema() {
        use arrow::datatypes::DataType;
        use nautilus_serialization::arrow::ArrowSchemaProvider;

        let schema = HyperliquidTwapHistory::get_schema(None);
        let names: Vec<_> = schema.fields().iter().map(|f| f.name().as_str()).collect();

        // Serde-backed fields encode as non-null Utf8 JSON.
        assert!(names.contains(&"user"));
        assert!(names.contains(&"twap_id"));
        assert!(names.contains(&"instrument_id"));
        assert!(names.contains(&"size"));
        assert!(names.contains(&"executed_notional"));
        assert!(names.contains(&"status"));
        assert!(names.contains(&"is_snapshot"));
        assert!(names.contains(&"ts_event"));
        assert!(names.contains(&"ts_init"));

        for name in ["twap_id", "size", "status"] {
            assert!(matches!(
                schema.field_with_name(name).unwrap().data_type(),
                DataType::Utf8 | DataType::Utf8View
            ));
        }
        assert_eq!(
            schema.field_with_name("ts_init").unwrap().data_type(),
            &DataType::UInt64
        );
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_hyperliquid_twap_history_arrow_round_trip_preserves_fields() {
        use std::str::FromStr;

        use nautilus_model::{data::Data, enums::OrderSide};
        use nautilus_serialization::arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch};

        let with_options = HyperliquidTwapHistory::new(
            "0xuser".to_string(),
            Some(7),
            "BTC".to_string(),
            Some(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID")),
            OrderSide::Buy,
            Decimal::from_str("1.25").unwrap(),
            Decimal::from_str("0.5").unwrap(),
            Decimal::from_str("50000.123456789").unwrap(),
            15,
            false,
            true,
            HyperliquidTwapStatus::Finished,
            "finished".to_string(),
            UnixNanos::from(10),
            true,
            UnixNanos::from(20),
            UnixNanos::from(30),
        );
        let without_options = HyperliquidTwapHistory::new(
            "0xuser2".to_string(),
            None,
            "UNK".to_string(),
            None,
            OrderSide::Sell,
            Decimal::from_str("2").unwrap(),
            Decimal::ZERO,
            Decimal::ZERO,
            60,
            true,
            false,
            HyperliquidTwapStatus::Activated,
            "activated".to_string(),
            UnixNanos::from(40),
            false,
            UnixNanos::from(50),
            UnixNanos::from(60),
        );

        for original in [&with_options, &without_options] {
            let metadata = EncodeToRecordBatch::metadata(original);
            let batch =
                HyperliquidTwapHistory::encode_batch(&metadata, std::slice::from_ref(original))
                    .unwrap();
            let decoded = HyperliquidTwapHistory::decode_data_batch(&metadata, batch).unwrap();

            let Data::Custom(custom) = &decoded[0] else {
                panic!("Expected Data::Custom");
            };
            let history = custom
                .data
                .as_any()
                .downcast_ref::<HyperliquidTwapHistory>()
                .expect("expected HyperliquidTwapHistory");
            assert_eq!(history, original);
        }
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_hyperliquid_twap_slice_fill_arrow_round_trip_preserves_decimals() {
        use std::str::FromStr;

        use nautilus_model::{data::Data, enums::OrderSide};
        use nautilus_serialization::arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch};

        let original = HyperliquidTwapSliceFill::new(
            "0xslice".to_string(),
            99,
            "ETH".to_string(),
            Some(InstrumentId::from("ETH-USD-PERP.HYPERLIQUID")),
            Decimal::from_str("3456.789012345678").unwrap(),
            Decimal::from_str("0.001").unwrap(),
            OrderSide::Buy,
            "0xhash".to_string(),
            111,
            222,
            true,
            Decimal::from_str("-0.0001").unwrap(),
            "USDC".to_string(),
            "Open Long".to_string(),
            Decimal::from_str("1.23").unwrap(),
            false,
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let metadata = EncodeToRecordBatch::metadata(&original);
        let batch =
            HyperliquidTwapSliceFill::encode_batch(&metadata, std::slice::from_ref(&original))
                .unwrap();
        let decoded = HyperliquidTwapSliceFill::decode_data_batch(&metadata, batch).unwrap();

        let Data::Custom(custom) = &decoded[0] else {
            panic!("Expected Data::Custom");
        };
        let fill = custom
            .data
            .as_any()
            .downcast_ref::<HyperliquidTwapSliceFill>()
            .expect("expected HyperliquidTwapSliceFill");
        assert_eq!(fill, &original);
        assert_eq!(fill.price, original.price);
        assert_eq!(fill.fee, original.fee);
        assert_eq!(fill.closed_pnl, original.closed_pnl);
    }
}
