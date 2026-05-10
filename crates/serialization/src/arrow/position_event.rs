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

use std::collections::HashMap;

use arrow::{datatypes::Schema, error::ArrowError, record_batch::RecordBatch};
use nautilus_model::events::{PositionAdjusted, PositionChanged, PositionClosed, PositionOpened};

use super::{
    ArrowSchemaProvider, DecodeTypedFromRecordBatch, EncodeToRecordBatch, EncodingError,
    KEY_INSTRUMENT_ID,
    json::{JsonFieldSpec, decode_batch, encode_batch, metadata_for_type, schema_for_type},
};

const POSITION_OPENED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("position_id", false),
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("opening_order_id", false),
    JsonFieldSpec::utf8("entry", false),
    JsonFieldSpec::utf8("side", false),
    JsonFieldSpec::f64("signed_qty", false),
    JsonFieldSpec::utf8("quantity", false),
    JsonFieldSpec::utf8("last_qty", false),
    JsonFieldSpec::utf8("last_px", false),
    JsonFieldSpec::utf8("currency", false),
    JsonFieldSpec::f64("avg_px_open", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
];

const POSITION_CHANGED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("position_id", false),
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("opening_order_id", false),
    JsonFieldSpec::utf8("entry", false),
    JsonFieldSpec::utf8("side", false),
    JsonFieldSpec::f64("signed_qty", false),
    JsonFieldSpec::utf8("quantity", false),
    JsonFieldSpec::utf8("peak_quantity", false),
    JsonFieldSpec::utf8("last_qty", false),
    JsonFieldSpec::utf8("last_px", false),
    JsonFieldSpec::utf8("currency", false),
    JsonFieldSpec::f64("avg_px_open", false),
    JsonFieldSpec::f64("avg_px_close", true),
    JsonFieldSpec::f64("realized_return", false),
    JsonFieldSpec::utf8("realized_pnl", true),
    JsonFieldSpec::utf8("unrealized_pnl", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_opened", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
];

const POSITION_CLOSED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("position_id", false),
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("opening_order_id", false),
    JsonFieldSpec::utf8("closing_order_id", true),
    JsonFieldSpec::utf8("entry", false),
    JsonFieldSpec::utf8("side", false),
    JsonFieldSpec::f64("signed_qty", false),
    JsonFieldSpec::utf8("quantity", false),
    JsonFieldSpec::utf8("peak_quantity", false),
    JsonFieldSpec::utf8("last_qty", false),
    JsonFieldSpec::utf8("last_px", false),
    JsonFieldSpec::utf8("currency", false),
    JsonFieldSpec::f64("avg_px_open", false),
    JsonFieldSpec::f64("avg_px_close", true),
    JsonFieldSpec::f64("realized_return", false),
    JsonFieldSpec::utf8("realized_pnl", true),
    JsonFieldSpec::utf8("unrealized_pnl", false),
    JsonFieldSpec::u64("duration", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_opened", false),
    JsonFieldSpec::u64("ts_closed", true),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
];

const POSITION_ADJUSTED_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("position_id", false),
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("adjustment_type", false),
    JsonFieldSpec::utf8("quantity_change", true),
    JsonFieldSpec::utf8("pnl_change", true),
    JsonFieldSpec::utf8("reason", true),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
];

fn instrument_metadata(type_name: &'static str, instrument_id: &str) -> HashMap<String, String> {
    let mut metadata = metadata_for_type(type_name);
    metadata.insert(KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string());
    metadata
}

macro_rules! impl_position_event_arrow {
    ($type:ty, $type_name:expr, $fields:expr) => {
        impl ArrowSchemaProvider for $type {
            fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
                schema_for_type($type_name, metadata, $fields)
            }
        }

        impl EncodeToRecordBatch for $type {
            fn encode_batch(
                metadata: &HashMap<String, String>,
                data: &[Self],
            ) -> Result<RecordBatch, ArrowError> {
                encode_batch($type_name, metadata, data, $fields)
            }

            fn metadata(&self) -> HashMap<String, String> {
                instrument_metadata($type_name, &self.instrument_id.to_string())
            }
        }

        impl DecodeTypedFromRecordBatch for $type {
            fn decode_typed_batch(
                metadata: &HashMap<String, String>,
                record_batch: RecordBatch,
            ) -> Result<Vec<Self>, EncodingError> {
                decode_batch(metadata, &record_batch, $fields, Some($type_name))
            }
        }
    };
}

impl_position_event_arrow!(PositionOpened, "PositionOpened", POSITION_OPENED_FIELDS);
impl_position_event_arrow!(PositionChanged, "PositionChanged", POSITION_CHANGED_FIELDS);
impl_position_event_arrow!(PositionClosed, "PositionClosed", POSITION_CLOSED_FIELDS);
impl_position_event_arrow!(
    PositionAdjusted,
    "PositionAdjusted",
    POSITION_ADJUSTED_FIELDS
);

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, PositionAdjustmentType, PositionSide},
        identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_position_adjusted_round_trip() {
        let event = PositionAdjusted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("EMA-CROSS"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            PositionId::from("P-001"),
            AccountId::from("BINANCE-001"),
            PositionAdjustmentType::Funding,
            Some(Decimal::from_str("-0.123456789123456789").unwrap()),
            Some(Money::new(-5.50, Currency::USD())),
            Some(Ustr::from("funding_2024_01_15_08:00")),
            UUID4::default(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );
        let metadata = event.metadata();
        let batch = PositionAdjusted::encode_batch(&metadata, &[event]).unwrap();
        let decoded =
            PositionAdjusted::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![event]);
    }

    #[rstest]
    fn test_position_opened_round_trip() {
        let event = PositionOpened {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("EMA-CROSS"),
            instrument_id: InstrumentId::from("EURUSD.SIM"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-19700101-000000-001-001-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 150.0,
            quantity: Quantity::from("150"),
            last_qty: Quantity::from("150"),
            last_px: Price::from("1.0525"),
            currency: Currency::USD(),
            avg_px_open: 1.0525,
            event_id: UUID4::default(),
            ts_event: UnixNanos::from(1_000_000_000),
            ts_init: UnixNanos::from(1_000_000_001),
        };
        let metadata = event.metadata();
        let batch = PositionOpened::encode_batch(&metadata, std::slice::from_ref(&event)).unwrap();
        let decoded = PositionOpened::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![event]);
    }

    #[rstest]
    fn test_position_changed_round_trip() {
        let event = PositionChanged {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("EMA-CROSS"),
            instrument_id: InstrumentId::from("EURUSD.SIM"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-19700101-000000-001-001-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 300.0,
            quantity: Quantity::from("300"),
            peak_quantity: Quantity::from("300"),
            last_qty: Quantity::from("150"),
            last_px: Price::from("1.0600"),
            currency: Currency::USD(),
            avg_px_open: 1.0562,
            avg_px_close: None,
            realized_return: 0.0,
            realized_pnl: None,
            unrealized_pnl: Money::new(56.25, Currency::USD()),
            event_id: UUID4::default(),
            ts_opened: UnixNanos::from(1_000_000_000),
            ts_event: UnixNanos::from(2_000_000_000),
            ts_init: UnixNanos::from(2_000_000_001),
        };
        let metadata = event.metadata();
        let batch = PositionChanged::encode_batch(&metadata, std::slice::from_ref(&event)).unwrap();
        let decoded =
            PositionChanged::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![event]);
    }

    #[rstest]
    fn test_position_closed_round_trip() {
        let event = PositionClosed {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("EMA-CROSS"),
            instrument_id: InstrumentId::from("EURUSD.SIM"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-19700101-000000-001-001-1"),
            closing_order_id: Some(ClientOrderId::from("O-19700101-000000-001-001-2")),
            entry: OrderSide::Buy,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: Quantity::from("0"),
            peak_quantity: Quantity::from("150"),
            last_qty: Quantity::from("150"),
            last_px: Price::from("1.0600"),
            currency: Currency::USD(),
            avg_px_open: 1.0525,
            avg_px_close: Some(1.0600),
            realized_return: 0.0071,
            realized_pnl: Some(Money::new(112.50, Currency::USD())),
            unrealized_pnl: Money::new(0.0, Currency::USD()),
            duration: 3_600_000_000_000,
            event_id: UUID4::default(),
            ts_opened: UnixNanos::from(1_000_000_000),
            ts_closed: Some(UnixNanos::from(4_600_000_000)),
            ts_event: UnixNanos::from(4_600_000_000),
            ts_init: UnixNanos::from(5_000_000_000),
        };
        let metadata = event.metadata();
        let batch = PositionClosed::encode_batch(&metadata, std::slice::from_ref(&event)).unwrap();
        let decoded = PositionClosed::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![event]);
    }
}
