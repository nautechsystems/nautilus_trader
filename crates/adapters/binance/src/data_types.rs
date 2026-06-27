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
    data::{HasTsInit, custom::CustomDataTrait},
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use nautilus_persistence_macros::custom_data;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Binance Futures current open interest snapshot.
#[cfg_attr(
    feature = "arrow",
    custom_data(pyo3, stub_module = "nautilus_trader.adapters.binance")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.binance")
)]
pub struct BinanceFuturesOpenInterest {
    /// The instrument for this snapshot.
    pub instrument_id: InstrumentId,
    /// The total open interest value.
    #[custom_data_field(serde)]
    pub open_interest: Decimal,
    /// UNIX timestamp (nanoseconds) when the snapshot event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Binance Futures historical open interest point.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", from_py_object)
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
/// For COIN-M requests, the current Binance adapter support is limited to
/// perpetual instruments. Although Binance also exposes quarter-delivery
/// contract types on the historical OI endpoint, the futures instrument
/// parsing/symbology path in this adapter is still perpetual-only.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", from_py_object)
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
    custom_data(pyo3, stub_module = "nautilus_trader.adapters.binance")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.binance")
)]
pub struct BinanceFuturesLiquidation {
    /// The instrument for this liquidation event.
    pub instrument_id: InstrumentId,
    /// The liquidation order side.
    #[custom_data_field(serde)]
    pub side: OrderSide,
    /// The order price.
    #[custom_data_field(serde)]
    pub price: Price,
    /// The average fill price.
    #[custom_data_field(serde)]
    pub average_price: Price,
    /// The last filled quantity.
    #[custom_data_field(serde)]
    pub last_filled_qty: Quantity,
    /// The cumulative filled quantity.
    #[custom_data_field(serde)]
    pub accumulated_qty: Quantity,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Binance Futures 24-hour ticker statistics from the `ticker` stream.
#[cfg_attr(
    feature = "arrow",
    custom_data(pyo3, stub_module = "nautilus_trader.adapters.binance")
)]
#[cfg_attr(
    not(feature = "arrow"),
    custom_data(pyo3, no_arrow, stub_module = "nautilus_trader.adapters.binance")
)]
pub struct BinanceFuturesTicker {
    /// The instrument for these 24-hour statistics.
    pub instrument_id: InstrumentId,
    /// Price change over the rolling 24-hour window.
    #[custom_data_field(serde)]
    pub price_change: Decimal,
    /// Price change percentage over the rolling 24-hour window.
    #[custom_data_field(serde)]
    pub price_change_percent: Decimal,
    /// Weighted average price over the rolling 24-hour window.
    #[custom_data_field(serde)]
    pub weighted_avg_price: Decimal,
    /// Last traded price.
    #[custom_data_field(serde)]
    pub last_price: Decimal,
    /// Last traded quantity.
    #[custom_data_field(serde)]
    pub last_qty: Decimal,
    /// Open price for the rolling 24-hour window.
    #[custom_data_field(serde)]
    pub open_price: Decimal,
    /// High price for the rolling 24-hour window.
    #[custom_data_field(serde)]
    pub high_price: Decimal,
    /// Low price for the rolling 24-hour window.
    #[custom_data_field(serde)]
    pub low_price: Decimal,
    /// Total traded base asset volume.
    #[custom_data_field(serde)]
    pub volume: Decimal,
    /// Total traded quote asset volume.
    #[custom_data_field(serde)]
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
        nautilus_serialization::ensure_custom_data_registered::<BinanceFuturesOpenInterest>();
        nautilus_serialization::ensure_custom_data_registered::<BinanceFuturesLiquidation>();
        nautilus_serialization::ensure_custom_data_registered::<BinanceFuturesTicker>();
    }

    #[cfg(not(feature = "arrow"))]
    {
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
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "python")]
    use std::sync::Arc;

    #[cfg(feature = "python")]
    use nautilus_core::Params;
    #[cfg(feature = "arrow")]
    use nautilus_model::data::Data;
    #[cfg(feature = "arrow")]
    use nautilus_model::data::{CustomData, DataType};
    #[cfg(feature = "python")]
    use nautilus_model::data::{CustomData as PyCustomData, DataType as PyDataType};
    #[cfg(feature = "arrow")]
    use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    #[cfg(feature = "arrow")]
    use nautilus_serialization::arrow::{
        ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch,
    };
    #[cfg(feature = "python")]
    use pyo3::{prelude::*, types::PyList};
    use rstest::rstest;
    #[cfg(feature = "python")]
    use rust_decimal::Decimal;
    #[cfg(feature = "arrow")]
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    fn test_register_binance_custom_data_is_idempotent() {
        register_binance_custom_data();
        register_binance_custom_data();
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_binance_futures_open_interest_arrow_round_trip() {
        let original = BinanceFuturesOpenInterest::new(
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
            Decimal::from_str_exact("123456.789012345678").unwrap(),
            UnixNanos::from(1_u64),
            UnixNanos::from(2_u64),
        );
        let metadata = BinanceFuturesOpenInterest::metadata(&original);
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
    fn test_binance_futures_liquidation_arrow_schema_uses_serde_backed_fields() {
        use arrow::datatypes::DataType;

        let schema = BinanceFuturesLiquidation::get_schema(None);

        assert_eq!(schema.fields().len(), 8);
        assert_eq!(schema.field(0).name(), "instrument_id");
        assert!(matches!(
            schema.field(0).data_type(),
            DataType::Utf8 | DataType::Utf8View
        ));

        for field_name in [
            "side",
            "price",
            "average_price",
            "last_filled_qty",
            "accumulated_qty",
        ] {
            let field = schema.field_with_name(field_name).unwrap();
            assert!(matches!(
                field.data_type(),
                DataType::Utf8 | DataType::Utf8View
            ));
        }
        assert_eq!(
            schema.field_with_name("ts_event").unwrap().data_type(),
            &DataType::UInt64
        );
        assert_eq!(
            schema.field_with_name("ts_init").unwrap().data_type(),
            &DataType::UInt64
        );
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
            UnixNanos::from(3_u64),
            UnixNanos::from(4_u64),
        );
        let metadata = BinanceFuturesLiquidation::metadata(&original);
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
            Decimal::from_str_exact("12.34").unwrap(),
            Decimal::from_str_exact("5.67").unwrap(),
            Decimal::from_str_exact("62345.123456").unwrap(),
            Decimal::from_str_exact("62350.000001").unwrap(),
            Decimal::from_str_exact("0.010000").unwrap(),
            Decimal::from_str_exact("62000.000000").unwrap(),
            Decimal::from_str_exact("63000.000000").unwrap(),
            Decimal::from_str_exact("61000.000000").unwrap(),
            Decimal::from_str_exact("1234.567890").unwrap(),
            Decimal::from_str_exact("76543210.123456").unwrap(),
            UnixNanos::from(10_u64),
            UnixNanos::from(11_u64),
            100,
            200,
            300,
            UnixNanos::from(12_u64),
            UnixNanos::from(13_u64),
        );
        let metadata = BinanceFuturesTicker::metadata(&original);
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

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_binance_futures_custom_data_catalog_round_trip() {
        use std::sync::Arc;

        register_binance_custom_data();
        let temp_dir = TempDir::new().unwrap();
        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let ids = vec![instrument_id.to_string()];

        let liquidation_type = DataType::new(
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
            UnixNanos::from(100_u64),
            UnixNanos::from(101_u64),
        );
        let liquidation_path = catalog
            .write_custom_data_batch(
                vec![CustomData::new(
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

        let ticker_type = DataType::new(
            "BinanceFuturesTicker",
            None,
            Some(instrument_id.to_string()),
        );
        let ticker = BinanceFuturesTicker::new(
            instrument_id,
            Decimal::from_str_exact("12.34").unwrap(),
            Decimal::from_str_exact("5.67").unwrap(),
            Decimal::from_str_exact("62345.123456").unwrap(),
            Decimal::from_str_exact("62350.000001").unwrap(),
            Decimal::from_str_exact("0.010000").unwrap(),
            Decimal::from_str_exact("62000.000000").unwrap(),
            Decimal::from_str_exact("63000.000000").unwrap(),
            Decimal::from_str_exact("61000.000000").unwrap(),
            Decimal::from_str_exact("1234.567890").unwrap(),
            Decimal::from_str_exact("76543210.123456").unwrap(),
            UnixNanos::from(110_u64),
            UnixNanos::from(111_u64),
            100,
            200,
            300,
            UnixNanos::from(112_u64),
            UnixNanos::from(113_u64),
        );
        let ticker_path = catalog
            .write_custom_data_batch(
                vec![CustomData::new(Arc::new(ticker.clone()), ticker_type)],
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

        let open_interest_type = DataType::new(
            "BinanceFuturesOpenInterest",
            None,
            Some(instrument_id.to_string()),
        );
        let open_interest = BinanceFuturesOpenInterest::new(
            instrument_id,
            Decimal::from_str_exact("123456.789012345678").unwrap(),
            UnixNanos::from(120_u64),
            UnixNanos::from(121_u64),
        );
        let open_interest_path = catalog
            .write_custom_data_batch(
                vec![CustomData::new(
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

            let custom = PyCustomData::new(
                Arc::new(payload),
                PyDataType::new(
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
