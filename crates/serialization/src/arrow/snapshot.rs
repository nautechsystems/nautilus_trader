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
use nautilus_model::events::{OrderSnapshot, PositionSnapshot};

use super::{
    ArrowSchemaProvider, DecodeTypedFromRecordBatch, EncodeToRecordBatch, EncodingError,
    KEY_INSTRUMENT_ID,
    json::{JsonFieldSpec, decode_batch, encode_batch, metadata_for_type, schema_for_type},
};

const ORDER_SNAPSHOT_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("trader_id", false),
    JsonFieldSpec::utf8("strategy_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", false),
    JsonFieldSpec::utf8("venue_order_id", true),
    JsonFieldSpec::utf8("position_id", true),
    JsonFieldSpec::utf8("account_id", true),
    JsonFieldSpec::utf8("last_trade_id", true),
    JsonFieldSpec::utf8("order_type", false),
    JsonFieldSpec::utf8("order_side", false),
    JsonFieldSpec::utf8("quantity", false),
    JsonFieldSpec::utf8("price", true),
    JsonFieldSpec::utf8("trigger_price", true),
    JsonFieldSpec::utf8("trigger_type", true),
    JsonFieldSpec::utf8("limit_offset", true),
    JsonFieldSpec::utf8("trailing_offset", true),
    JsonFieldSpec::utf8("trailing_offset_type", true),
    JsonFieldSpec::utf8("time_in_force", false),
    JsonFieldSpec::u64("expire_time", true),
    JsonFieldSpec::utf8("filled_qty", false),
    JsonFieldSpec::utf8("liquidity_side", true),
    JsonFieldSpec::f64("avg_px", true),
    JsonFieldSpec::f64("slippage", true),
    JsonFieldSpec::utf8_json("commissions", false),
    JsonFieldSpec::utf8("status", false),
    JsonFieldSpec::boolean("is_post_only", false),
    JsonFieldSpec::boolean("is_reduce_only", false),
    JsonFieldSpec::boolean("is_quote_quantity", false),
    JsonFieldSpec::utf8("display_qty", true),
    JsonFieldSpec::utf8("emulation_trigger", true),
    JsonFieldSpec::utf8("trigger_instrument_id", true),
    JsonFieldSpec::utf8("contingency_type", true),
    JsonFieldSpec::utf8("order_list_id", true),
    JsonFieldSpec::utf8_json("linked_order_ids", true),
    JsonFieldSpec::utf8("parent_order_id", true),
    JsonFieldSpec::utf8("exec_algorithm_id", true),
    JsonFieldSpec::utf8_json("exec_algorithm_params", true),
    JsonFieldSpec::utf8("exec_spawn_id", true),
    JsonFieldSpec::utf8_json("tags", true),
    JsonFieldSpec::utf8("init_id", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("ts_last", false),
];

const POSITION_SNAPSHOT_FIELDS: &[JsonFieldSpec] = &[
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
    JsonFieldSpec::utf8("peak_qty", false),
    JsonFieldSpec::utf8("quote_currency", false),
    JsonFieldSpec::utf8("base_currency", true),
    JsonFieldSpec::utf8("settlement_currency", false),
    JsonFieldSpec::f64("avg_px_open", false),
    JsonFieldSpec::f64("avg_px_close", true),
    JsonFieldSpec::f64("realized_return", true),
    JsonFieldSpec::utf8("realized_pnl", true),
    JsonFieldSpec::utf8("unrealized_pnl", true),
    JsonFieldSpec::utf8_json("commissions", false),
    JsonFieldSpec::u64("duration_ns", true),
    JsonFieldSpec::u64("ts_opened", false),
    JsonFieldSpec::u64("ts_closed", true),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::u64("ts_last", false),
];

fn instrument_metadata(type_name: &'static str, instrument_id: &str) -> HashMap<String, String> {
    let mut metadata = metadata_for_type(type_name);
    metadata.insert(KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string());
    metadata
}

macro_rules! impl_snapshot_arrow {
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

impl_snapshot_arrow!(OrderSnapshot, "OrderSnapshot", ORDER_SNAPSHOT_FIELDS);
impl_snapshot_arrow!(
    PositionSnapshot,
    "PositionSnapshot",
    POSITION_SNAPSHOT_FIELDS
);

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::{OrderSide, OrderType, PositionSide},
        identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
        orders::OrderTestBuilder,
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;

    #[rstest]
    fn test_order_snapshot_round_trip_preserves_decimal_precision() {
        let order = OrderTestBuilder::new(OrderType::TrailingStopLimit)
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .side(OrderSide::Buy)
            .price(Price::from("50000"))
            .trigger_price(Price::from("50500"))
            .limit_offset(Decimal::from_str("0.123456789123456789").unwrap())
            .trailing_offset(Decimal::from_str("0.987654321987654321").unwrap())
            .quantity(Quantity::from("0.5"))
            .build();
        let snapshot = OrderSnapshot::from(order);
        let metadata = snapshot.metadata();
        let batch =
            OrderSnapshot::encode_batch(&metadata, std::slice::from_ref(&snapshot)).unwrap();
        let decoded = OrderSnapshot::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![snapshot]);
    }

    fn make_position_snapshot() -> PositionSnapshot {
        PositionSnapshot {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("EMA-CROSS"),
            instrument_id: InstrumentId::from("EURUSD.SIM"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-1"),
            closing_order_id: Some(ClientOrderId::from("O-2")),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 100.0,
            quantity: Quantity::from("100"),
            peak_qty: Quantity::from("100"),
            quote_currency: Currency::USD(),
            base_currency: Some(Currency::EUR()),
            settlement_currency: Currency::USD(),
            avg_px_open: 1.0500,
            avg_px_close: Some(1.0600),
            realized_return: Some(0.0095),
            realized_pnl: Some(Money::new(100.0, Currency::USD())),
            unrealized_pnl: Some(Money::new(50.0, Currency::USD())),
            commissions: vec![Money::new(2.0, Currency::USD())],
            duration_ns: Some(3_600_000_000_000),
            ts_opened: UnixNanos::from(1_000_000_000),
            ts_closed: Some(UnixNanos::from(4_600_000_000)),
            ts_init: UnixNanos::from(2_000_000_000),
            ts_last: UnixNanos::from(4_600_000_000),
        }
    }

    #[rstest]
    fn test_position_snapshot_round_trip() {
        let snapshot = make_position_snapshot();
        let metadata = snapshot.metadata();
        let batch =
            PositionSnapshot::encode_batch(&metadata, std::slice::from_ref(&snapshot)).unwrap();
        let decoded =
            PositionSnapshot::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![snapshot]);
    }

    #[rstest]
    fn test_position_snapshot_round_trip_null_optionals() {
        let mut snapshot = make_position_snapshot();
        snapshot.closing_order_id = None;
        snapshot.base_currency = None;
        snapshot.avg_px_close = None;
        snapshot.realized_return = None;
        snapshot.realized_pnl = None;
        snapshot.unrealized_pnl = None;
        snapshot.duration_ns = None;
        snapshot.ts_closed = None;

        let metadata = snapshot.metadata();
        let batch =
            PositionSnapshot::encode_batch(&metadata, std::slice::from_ref(&snapshot)).unwrap();
        let decoded =
            PositionSnapshot::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![snapshot]);
    }
}
