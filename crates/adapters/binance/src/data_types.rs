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

//! Binance-specific custom data types.
//!
//! These types carry Binance domain data through the Nautilus data engine as
//! [`CustomData`](nautilus_model::data::CustomData).

use std::sync::Arc;

use nautilus_core::UnixNanos;
use nautilus_model::{
    custom_data,
    data::{HasTsInit, custom::CustomDataTrait},
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
#[cfg(feature = "arrow")]
use nautilus_serialization::arrow_custom_data;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::common::bar::BinanceBar;

/// Binance Futures current open interest snapshot.
#[cfg_attr(
    feature = "arrow",
    arrow_custom_data(pyo3, stub_module = "nautilus_trader.adapters.binance")
)]
#[custom_data(pyo3, stub_module = "nautilus_trader.adapters.binance")]
pub struct BinanceFuturesOpenInterest {
    /// The instrument for this snapshot.
    pub instrument_id: InstrumentId,
    /// The total open interest value.
    pub open_interest: Decimal,
    /// UNIX timestamp (nanoseconds) when the snapshot event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Binance Futures historical open interest point.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.binance")
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinanceFuturesOpenInterestHistPoint {
    /// The total open interest value.
    pub sum_open_interest: Decimal,
    /// The total open interest notional value.
    pub sum_open_interest_value: Decimal,
    /// UNIX timestamp (nanoseconds) for the bucket represented by this point.
    pub ts_event: UnixNanos,
}

impl BinanceFuturesOpenInterestHistPoint {
    /// Creates a new [`BinanceFuturesOpenInterestHistPoint`] instance.
    #[must_use]
    pub fn new(
        sum_open_interest: Decimal,
        sum_open_interest_value: Decimal,
        ts_event: UnixNanos,
    ) -> Self {
        Self {
            sum_open_interest,
            sum_open_interest_value,
            ts_event,
        }
    }
}

/// Binance Futures historical open interest batch.
///
/// COIN-M requests are keyed by pair and contract type rather than by symbol.
/// Perpetuals derive both from the `_PERP` symbol suffix, while delivery
/// contracts resolve them from the cached instrument definition.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.binance")
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinanceFuturesOpenInterestHist {
    /// The instrument for this batch.
    pub instrument_id: InstrumentId,
    /// The Binance period string used for the request (e.g. "5m").
    pub period: String,
    /// Ordered open interest history points returned by Binance.
    pub points: Vec<BinanceFuturesOpenInterestHistPoint>,
    /// UNIX timestamp (nanoseconds) for the batch, represented by the final point.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl BinanceFuturesOpenInterestHist {
    /// Creates a new [`BinanceFuturesOpenInterestHist`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        period: String,
        points: Vec<BinanceFuturesOpenInterestHistPoint>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            period,
            points,
            ts_event,
            ts_init,
        }
    }
}

impl HasTsInit for BinanceFuturesOpenInterestHist {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for BinanceFuturesOpenInterestHist {
    fn type_name(&self) -> &'static str {
        "BinanceFuturesOpenInterestHist"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        if let Some(o) = other.as_any().downcast_ref::<Self>() {
            self == o
        } else {
            false
        }
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        nautilus_model::data::custom::clone_pyclass_to_pyobject(self, py)
    }

    fn type_name_static() -> &'static str {
        "BinanceFuturesOpenInterestHist"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let json_str = serde_json::to_string(&value)?;
        let parsed: Self = serde_json::from_str(&json_str)?;
        Ok(Arc::new(parsed))
    }
}

/// Binance Futures liquidation update from the `forceOrder` stream.
#[cfg_attr(
    feature = "arrow",
    arrow_custom_data(pyo3, stub_module = "nautilus_trader.adapters.binance")
)]
#[custom_data(pyo3, stub_module = "nautilus_trader.adapters.binance")]
pub struct BinanceFuturesLiquidation {
    /// The instrument for this liquidation event.
    pub instrument_id: InstrumentId,
    /// The liquidation order side.
    #[custom_data_field(native_enum)]
    pub side: OrderSide,
    /// The order price.
    pub price: Price,
    /// The average fill price.
    pub average_price: Price,
    /// The last filled quantity.
    pub last_filled_qty: Quantity,
    /// The cumulative filled quantity.
    pub accumulated_qty: Quantity,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Binance Spot 24-hour ticker statistics from the `ticker` stream.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.binance")
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinanceSpotTicker {
    /// The instrument for these 24-hour statistics.
    pub instrument_id: InstrumentId,
    /// Price change over the rolling 24-hour window.
    pub price_change: Decimal,
    /// Price change percentage over the rolling 24-hour window.
    pub price_change_percent: Decimal,
    /// Weighted average price over the rolling 24-hour window.
    pub weighted_avg_price: Decimal,
    /// Previous close price before the rolling window.
    pub prev_close_price: Decimal,
    /// Last traded price.
    pub last_price: Decimal,
    /// Last traded quantity.
    pub last_qty: Decimal,
    /// Best bid price.
    pub bid_price: Decimal,
    /// Best bid quantity.
    pub bid_qty: Decimal,
    /// Best ask price.
    pub ask_price: Decimal,
    /// Best ask quantity.
    pub ask_qty: Decimal,
    /// Open price for the rolling 24-hour window.
    pub open_price: Decimal,
    /// High price for the rolling 24-hour window.
    pub high_price: Decimal,
    /// Low price for the rolling 24-hour window.
    pub low_price: Decimal,
    /// Total traded base asset volume.
    pub volume: Decimal,
    /// Total traded quote asset volume.
    pub quote_volume: Decimal,
    /// Statistics open time.
    pub open_time: UnixNanos,
    /// Statistics close time.
    pub close_time: UnixNanos,
    /// First trade ID included in the statistics window.
    pub first_trade_id: i64,
    /// Last trade ID included in the statistics window.
    pub last_trade_id: i64,
    /// Total number of trades in the statistics window.
    pub num_trades: i64,
    /// UNIX timestamp (nanoseconds) when the ticker event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl HasTsInit for BinanceSpotTicker {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for BinanceSpotTicker {
    fn type_name(&self) -> &'static str {
        "BinanceSpotTicker"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        nautilus_model::data::custom::clone_pyclass_to_pyobject(self, py)
    }

    fn type_name_static() -> &'static str {
        "BinanceSpotTicker"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let json_str = serde_json::to_string(&value)?;
        Ok(Arc::new(serde_json::from_str::<Self>(&json_str)?))
    }
}

/// Binance Futures mark-price stream update with venue-specific fields.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.binance")
)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BinanceFuturesMarkPriceUpdate {
    /// The instrument for this update.
    pub instrument_id: InstrumentId,
    /// Mark price.
    pub mark_price: Price,
    /// Index price.
    pub index_price: Price,
    /// Estimated settlement price.
    pub estimated_settle_price: Price,
    /// Funding rate.
    pub funding_rate: Decimal,
    /// Next funding time.
    pub next_funding_time: Option<UnixNanos>,
    /// UNIX timestamp (nanoseconds) when the update occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl HasTsInit for BinanceFuturesMarkPriceUpdate {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for BinanceFuturesMarkPriceUpdate {
    fn type_name(&self) -> &'static str {
        "BinanceFuturesMarkPriceUpdate"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        nautilus_model::data::custom::clone_pyclass_to_pyobject(self, py)
    }

    fn type_name_static() -> &'static str {
        "BinanceFuturesMarkPriceUpdate"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let json_str = serde_json::to_string(&value)?;
        Ok(Arc::new(serde_json::from_str::<Self>(&json_str)?))
    }
}

/// Binance Futures 24-hour ticker statistics from the `ticker` stream.
#[cfg_attr(
    feature = "arrow",
    arrow_custom_data(pyo3, stub_module = "nautilus_trader.adapters.binance")
)]
#[custom_data(pyo3, stub_module = "nautilus_trader.adapters.binance")]
pub struct BinanceFuturesTicker {
    /// The instrument for these 24-hour statistics.
    pub instrument_id: InstrumentId,
    /// Price change over the rolling 24-hour window.
    pub price_change: Decimal,
    /// Price change percentage over the rolling 24-hour window.
    pub price_change_percent: Decimal,
    /// Weighted average price over the rolling 24-hour window.
    pub weighted_avg_price: Decimal,
    /// Last traded price.
    pub last_price: Decimal,
    /// Last traded quantity.
    pub last_qty: Decimal,
    /// Open price for the rolling 24-hour window.
    pub open_price: Decimal,
    /// High price for the rolling 24-hour window.
    pub high_price: Decimal,
    /// Low price for the rolling 24-hour window.
    pub low_price: Decimal,
    /// Total traded base asset volume.
    pub volume: Decimal,
    /// Total traded quote asset volume.
    pub quote_volume: Decimal,
    /// Statistics open time.
    pub open_time: UnixNanos,
    /// Statistics close time.
    pub close_time: UnixNanos,
    /// First trade ID included in the statistics window.
    pub first_trade_id: i64,
    /// Last trade ID included in the statistics window.
    pub last_trade_id: i64,
    /// Total number of trades in the statistics window.
    pub num_trades: i64,
    /// UNIX timestamp (nanoseconds) when the ticker event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Registers Binance custom data types.
///
/// Safe to call multiple times (idempotent via internal `Once` guards).
pub fn register_binance_custom_data() {
    #[cfg(feature = "arrow")]
    {
        nautilus_serialization::ensure_custom_data_registered::<BinanceBar>();
        nautilus_serialization::ensure_custom_data_registered::<BinanceFuturesOpenInterest>();
        nautilus_serialization::ensure_custom_data_registered::<BinanceFuturesLiquidation>();
        nautilus_serialization::ensure_custom_data_registered::<BinanceFuturesTicker>();
    }

    #[cfg(not(feature = "arrow"))]
    {
        let _ = nautilus_model::data::ensure_custom_data_json_registered::<BinanceBar>();
        let _ = nautilus_model::data::ensure_custom_data_json_registered::<
            BinanceFuturesOpenInterest,
        >();
        let _ =
            nautilus_model::data::ensure_custom_data_json_registered::<BinanceFuturesLiquidation>();
        let _ = nautilus_model::data::ensure_custom_data_json_registered::<BinanceFuturesTicker>();
    }

    let _ = nautilus_model::data::ensure_custom_data_json_registered::<
        BinanceFuturesOpenInterestHist,
    >();
    let _ = nautilus_model::data::ensure_custom_data_json_registered::<BinanceSpotTicker>();
    let _ =
        nautilus_model::data::ensure_custom_data_json_registered::<BinanceFuturesMarkPriceUpdate>();
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    #[cfg(any(feature = "arrow", feature = "python"))]
    use std::sync::Arc;

    #[cfg(feature = "python")]
    use nautilus_core::Params;
    #[cfg(feature = "arrow")]
    use nautilus_model::data::Data;
    use nautilus_model::data::register_custom_data_json;
    #[cfg(feature = "python")]
    use nautilus_model::data::{CustomData, DataType};
    #[cfg(feature = "arrow")]
    use nautilus_serialization::arrow::{
        ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch,
    };
    #[cfg(feature = "python")]
    use pyo3::{prelude::*, types::PyList};
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;

    #[rstest]
    fn test_register_binance_custom_data_is_idempotent() {
        register_binance_custom_data();
        register_binance_custom_data();
    }

    #[rstest]
    fn test_register_binance_custom_data_registers_json_deserializers() {
        register_binance_custom_data();

        assert_json_registered::<BinanceBar>();
        assert_json_registered::<BinanceFuturesOpenInterest>();
        assert_json_registered::<BinanceFuturesLiquidation>();
        assert_json_registered::<BinanceFuturesTicker>();
    }

    fn assert_json_registered<T: CustomDataTrait>() {
        let type_name = T::type_name_static();
        let err = register_custom_data_json::<T>().unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("Custom data type \"{type_name}\" is already registered for JSON"),
        );
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_binance_futures_open_interest_arrow_round_trip() {
        let original = BinanceFuturesOpenInterest::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Decimal::from_str("123456.789012345678").unwrap(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let metadata = EncodeToRecordBatch::metadata(&original);
        let batch =
            BinanceFuturesOpenInterest::encode_batch(&metadata, std::slice::from_ref(&original))
                .unwrap();
        let decoded = BinanceFuturesOpenInterest::decode_data_batch(&metadata, batch).unwrap();

        assert_eq!(decoded.len(), 1);

        match &decoded[0] {
            Data::Custom(custom) => {
                let round_trip = custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesOpenInterest>()
                    .expect("expected BinanceFuturesOpenInterest");
                assert_eq!(round_trip, &original);
            }
            other => panic!("Expected Data::Custom, was {other:?}"),
        }
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_binance_futures_liquidation_arrow_schema_uses_native_types() {
        use arrow::datatypes::DataType;

        let schema = BinanceFuturesLiquidation::get_schema(None);

        assert_eq!(schema.fields().len(), 8);
        assert_eq!(schema.field(0).name(), "instrument_id");
        assert!(matches!(
            schema.field(0).data_type(),
            DataType::Utf8 | DataType::Utf8View
        ));
        assert_eq!(
            schema.field_with_name("side").unwrap().data_type(),
            &nautilus_serialization::arrow::enum_dictionary_data_type(),
        );

        for field_name in [
            "price",
            "average_price",
            "last_filled_qty",
            "accumulated_qty",
        ] {
            assert_eq!(
                schema.field_with_name(field_name).unwrap().data_type(),
                &DataType::Decimal128(38, 16),
            );
        }
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_binance_futures_liquidation_arrow_round_trip() {
        let original = BinanceFuturesLiquidation::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            OrderSide::Sell,
            Price::from("65432.10"),
            Price::from("65431.50"),
            Quantity::from("0.250"),
            Quantity::from("1.500"),
            UnixNanos::from(3),
            UnixNanos::from(4),
        );
        let metadata = EncodeToRecordBatch::metadata(&original);
        let batch =
            BinanceFuturesLiquidation::encode_batch(&metadata, std::slice::from_ref(&original))
                .unwrap();
        let decoded = BinanceFuturesLiquidation::decode_data_batch(&metadata, batch).unwrap();

        assert_eq!(decoded.len(), 1);

        match &decoded[0] {
            Data::Custom(custom) => {
                let round_trip = custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesLiquidation>()
                    .expect("expected BinanceFuturesLiquidation");
                assert_eq!(round_trip, &original);
            }
            other => panic!("Expected Data::Custom, was {other:?}"),
        }
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_binance_futures_ticker_arrow_round_trip() {
        let original = BinanceFuturesTicker::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Decimal::from_str("12.34").unwrap(),
            Decimal::from_str("5.67").unwrap(),
            Decimal::from_str("62345.123456").unwrap(),
            Decimal::from_str("62350.000001").unwrap(),
            Decimal::from_str("0.010000").unwrap(),
            Decimal::from_str("62000.000000").unwrap(),
            Decimal::from_str("63000.000000").unwrap(),
            Decimal::from_str("61000.000000").unwrap(),
            Decimal::from_str("1234.567890").unwrap(),
            Decimal::from_str("76543210.123456").unwrap(),
            UnixNanos::from(10),
            UnixNanos::from(11),
            100,
            200,
            300,
            UnixNanos::from(12),
            UnixNanos::from(13),
        );
        let metadata = EncodeToRecordBatch::metadata(&original);
        let batch =
            BinanceFuturesTicker::encode_batch(&metadata, std::slice::from_ref(&original)).unwrap();
        let decoded = BinanceFuturesTicker::decode_data_batch(&metadata, batch).unwrap();

        assert_eq!(decoded.len(), 1);

        match &decoded[0] {
            Data::Custom(custom) => {
                let round_trip = custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesTicker>()
                    .expect("expected BinanceFuturesTicker");
                assert_eq!(round_trip, &original);
            }
            other => panic!("Expected Data::Custom, was {other:?}"),
        }
    }

    #[rstest]
    fn test_binance_futures_custom_data_json_round_trip() {
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");

        let open_interest = BinanceFuturesOpenInterest::new(
            instrument_id,
            Decimal::from_str("123456.789012345678").unwrap(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let value: serde_json::Value =
            serde_json::from_str(&open_interest.to_json().unwrap()).unwrap();
        let restored = <BinanceFuturesOpenInterest as CustomDataTrait>::from_json(value).unwrap();
        assert_eq!(
            restored
                .as_any()
                .downcast_ref::<BinanceFuturesOpenInterest>()
                .unwrap(),
            &open_interest,
        );

        let liquidation = BinanceFuturesLiquidation::new(
            instrument_id,
            OrderSide::Sell,
            Price::from("65432.10"),
            Price::from("65431.50"),
            Quantity::from("0.250"),
            Quantity::from("1.500"),
            UnixNanos::from(3),
            UnixNanos::from(4),
        );
        let value: serde_json::Value =
            serde_json::from_str(&liquidation.to_json().unwrap()).unwrap();
        let restored = <BinanceFuturesLiquidation as CustomDataTrait>::from_json(value).unwrap();
        assert_eq!(
            restored
                .as_any()
                .downcast_ref::<BinanceFuturesLiquidation>()
                .unwrap(),
            &liquidation,
        );

        let ticker = BinanceFuturesTicker::new(
            instrument_id,
            Decimal::from_str("12.34").unwrap(),
            Decimal::from_str("5.67").unwrap(),
            Decimal::from_str("62345.123456").unwrap(),
            Decimal::from_str("62350.000001").unwrap(),
            Decimal::from_str("0.010000").unwrap(),
            Decimal::from_str("62000.000000").unwrap(),
            Decimal::from_str("63000.000000").unwrap(),
            Decimal::from_str("61000.000000").unwrap(),
            Decimal::from_str("1234.567890").unwrap(),
            Decimal::from_str("76543210.123456").unwrap(),
            UnixNanos::from(10),
            UnixNanos::from(11),
            100,
            200,
            300,
            UnixNanos::from(12),
            UnixNanos::from(13),
        );
        let value: serde_json::Value = serde_json::from_str(&ticker.to_json().unwrap()).unwrap();
        let restored = <BinanceFuturesTicker as CustomDataTrait>::from_json(value).unwrap();
        assert_eq!(
            restored
                .as_any()
                .downcast_ref::<BinanceFuturesTicker>()
                .unwrap(),
            &ticker,
        );
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_binance_futures_custom_data_catalog_round_trip() {
        use nautilus_model::data::{CustomData as CatalogCustomData, DataType as CatalogDataType};
        use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
        use tempfile::TempDir;

        register_binance_custom_data();
        let temp_dir = TempDir::new().unwrap();
        let catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        let mut catalog = catalog;
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ids = vec![instrument_id.to_string()];

        let liquidation_type = CatalogDataType::new(
            "BinanceFuturesLiquidation",
            None,
            Some(instrument_id.to_string()),
        );

        let liquidation = BinanceFuturesLiquidation::new(
            instrument_id,
            OrderSide::Sell,
            Price::from("65432.10"),
            Price::from("65431.50"),
            Quantity::from("0.250"),
            Quantity::from("1.500"),
            UnixNanos::from(100),
            UnixNanos::from(101),
        );
        let liquidation_path = catalog
            .write_custom_data_batch(
                vec![CatalogCustomData::new(
                    Arc::new(liquidation.clone()),
                    liquidation_type,
                )],
                None,
                None,
                Some(false),
            )
            .unwrap();
        assert!(
            liquidation_path
                .to_string_lossy()
                .contains("data/custom/BinanceFuturesLiquidation/BTCUSDT-PERP.BINANCE")
        );

        let liquidation_rows = catalog
            .query_custom_data_dynamic(
                "BinanceFuturesLiquidation",
                Some(&ids),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();
        assert_eq!(liquidation_rows.len(), 1);

        match &liquidation_rows[0] {
            Data::Custom(custom) => {
                let row = custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesLiquidation>()
                    .expect("expected BinanceFuturesLiquidation");
                assert_eq!(row, &liquidation);
            }
            other => panic!("Expected Data::Custom, was {other:?}"),
        }

        let ticker_type = CatalogDataType::new(
            "BinanceFuturesTicker",
            None,
            Some(instrument_id.to_string()),
        );

        let ticker = BinanceFuturesTicker::new(
            instrument_id,
            Decimal::from_str("12.34").unwrap(),
            Decimal::from_str("5.67").unwrap(),
            Decimal::from_str("62345.123456").unwrap(),
            Decimal::from_str("62350.000001").unwrap(),
            Decimal::from_str("0.010000").unwrap(),
            Decimal::from_str("62000.000000").unwrap(),
            Decimal::from_str("63000.000000").unwrap(),
            Decimal::from_str("61000.000000").unwrap(),
            Decimal::from_str("1234.567890").unwrap(),
            Decimal::from_str("76543210.123456").unwrap(),
            UnixNanos::from(110),
            UnixNanos::from(111),
            100,
            200,
            300,
            UnixNanos::from(112),
            UnixNanos::from(113),
        );
        let ticker_path = catalog
            .write_custom_data_batch(
                vec![CatalogCustomData::new(
                    Arc::new(ticker.clone()),
                    ticker_type,
                )],
                None,
                None,
                Some(false),
            )
            .unwrap();
        assert!(
            ticker_path
                .to_string_lossy()
                .contains("data/custom/BinanceFuturesTicker/BTCUSDT-PERP.BINANCE")
        );

        let ticker_rows = catalog
            .query_custom_data_dynamic(
                "BinanceFuturesTicker",
                Some(&ids),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();
        assert_eq!(ticker_rows.len(), 1);

        match &ticker_rows[0] {
            Data::Custom(custom) => {
                let row = custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesTicker>()
                    .expect("expected BinanceFuturesTicker");
                assert_eq!(row, &ticker);
            }
            other => panic!("Expected Data::Custom, was {other:?}"),
        }

        let open_interest_type = CatalogDataType::new(
            "BinanceFuturesOpenInterest",
            None,
            Some(instrument_id.to_string()),
        );

        let open_interest = BinanceFuturesOpenInterest::new(
            instrument_id,
            Decimal::from_str("123456.789012345678").unwrap(),
            UnixNanos::from(120),
            UnixNanos::from(121),
        );
        let open_interest_path = catalog
            .write_custom_data_batch(
                vec![CatalogCustomData::new(
                    Arc::new(open_interest.clone()),
                    open_interest_type,
                )],
                None,
                None,
                Some(false),
            )
            .unwrap();
        assert!(
            open_interest_path
                .to_string_lossy()
                .contains("data/custom/BinanceFuturesOpenInterest/BTCUSDT-PERP.BINANCE")
        );

        let open_interest_rows = catalog
            .query_custom_data_dynamic(
                "BinanceFuturesOpenInterest",
                Some(&ids),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();
        assert_eq!(open_interest_rows.len(), 1);

        match &open_interest_rows[0] {
            Data::Custom(custom) => {
                let row = custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesOpenInterest>()
                    .expect("expected BinanceFuturesOpenInterest");
                assert_eq!(row, &open_interest);
            }
            other => panic!("Expected Data::Custom, was {other:?}"),
        }
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_open_interest_hist_points_roundtrip_as_typed_python_list() {
        pyo3::Python::initialize();
        register_binance_custom_data();

        Python::attach(|py| {
            let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
            let points = vec![
                BinanceFuturesOpenInterestHistPoint::new(
                    Decimal::from_str_exact("100.0").unwrap(),
                    Decimal::from_str_exact("1000.0").unwrap(),
                    UnixNanos::from_millis(1_700_000_000_000),
                ),
                BinanceFuturesOpenInterestHistPoint::new(
                    Decimal::from_str_exact("101.0").unwrap(),
                    Decimal::from_str_exact("1005.0").unwrap(),
                    UnixNanos::from_millis(1_700_000_300_000),
                ),
            ];
            let payload = BinanceFuturesOpenInterestHist::new(
                instrument_id,
                "5m".to_string(),
                points,
                UnixNanos::from_millis(1_700_000_300_000),
                UnixNanos::from(42_u64),
            );

            let mut metadata = Params::new();
            metadata.insert(
                "instrument_id".to_string(),
                serde_json::Value::String("BTCUSDT-PERP.BINANCE".to_string()),
            );
            metadata.insert(
                "period".to_string(),
                serde_json::Value::String("5m".to_string()),
            );

            let custom = CustomData::new(
                Arc::new(payload),
                DataType::new(
                    "BinanceFuturesOpenInterestHist",
                    Some(metadata),
                    Some("BTCUSDT-PERP.BINANCE".to_string()),
                ),
            );

            let py_custom = Py::new(py, custom).unwrap();
            let py_payload = py_custom.bind(py).getattr("data").unwrap();
            let py_points = py_payload
                .getattr("points")
                .unwrap()
                .cast_into::<PyList>()
                .unwrap();

            assert_eq!(py_points.len(), 2);
            assert!(
                py_points
                    .get_item(0)
                    .unwrap()
                    .is_instance_of::<BinanceFuturesOpenInterestHistPoint>()
            );

            let point0 = py_points
                .get_item(0)
                .unwrap()
                .extract::<BinanceFuturesOpenInterestHistPoint>()
                .unwrap();
            let point1 = py_points
                .get_item(1)
                .unwrap()
                .extract::<BinanceFuturesOpenInterestHistPoint>()
                .unwrap();

            assert_eq!(
                point0.sum_open_interest,
                Decimal::from_str_exact("100.0").unwrap()
            );
            assert_eq!(
                point1.sum_open_interest_value,
                Decimal::from_str_exact("1005.0").unwrap()
            );
        });
    }
}
