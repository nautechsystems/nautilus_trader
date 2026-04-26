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

//! Display-mode Arrow encoder for [`Position`].

use std::sync::Arc;

use arrow::{
    array::{
        BooleanBuilder, Float64Builder, StringBuilder, TimestampNanosecondBuilder, UInt8Builder,
        UInt32Builder, UInt64Builder,
    },
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::position::Position;

use super::{
    bool_field, float64_field, money_to_f64, quantity_to_f64, timestamp_field, uint8_field,
    uint32_field, uint64_field, unix_nanos_to_i64, utf8_field,
};

/// Returns the display-mode Arrow schema for [`Position`].
#[must_use]
pub fn position_schema() -> Schema {
    Schema::new(vec![
        utf8_field("trader_id", false),
        utf8_field("strategy_id", false),
        utf8_field("instrument_id", false),
        utf8_field("position_id", false),
        utf8_field("account_id", false),
        utf8_field("opening_order_id", false),
        utf8_field("closing_order_id", true),
        utf8_field("entry", false),
        utf8_field("side", false),
        float64_field("signed_qty", false),
        float64_field("quantity", false),
        float64_field("peak_qty", false),
        uint8_field("price_precision", false),
        uint8_field("size_precision", false),
        float64_field("multiplier", false),
        bool_field("is_inverse", false),
        bool_field("is_currency_pair", false),
        utf8_field("instrument_class", false),
        utf8_field("base_currency", true),
        utf8_field("quote_currency", false),
        utf8_field("settlement_currency", false),
        timestamp_field("ts_init", false),
        timestamp_field("ts_opened", false),
        timestamp_field("ts_last", false),
        timestamp_field("ts_closed", true),
        uint64_field("duration_ns", false),
        float64_field("avg_px_open", false),
        float64_field("avg_px_close", true),
        float64_field("realized_return", false),
        float64_field("realized_pnl_amount", true),
        utf8_field("realized_pnl_currency", true),
        utf8_field("trade_ids", false),
        float64_field("buy_qty", false),
        float64_field("sell_qty", false),
        utf8_field("commissions", false),
        uint32_field("event_count", false),
        uint32_field("adjustment_count", false),
    ])
}

fn trade_ids_to_json(position: &Position) -> String {
    let mut trade_ids: Vec<String> = position.trade_ids.iter().map(ToString::to_string).collect();
    trade_ids.sort();
    serde_json::to_string(&trade_ids).unwrap_or_default()
}

fn commissions_to_json(position: &Position) -> String {
    let mut commissions: Vec<(String, f64)> = position
        .commissions
        .iter()
        .map(|(currency, money)| (currency.to_string(), money_to_f64(money)))
        .collect();
    commissions.sort_by(|lhs, rhs| lhs.0.cmp(&rhs.0));
    serde_json::to_string(&commissions).unwrap_or_default()
}

/// Encodes positions as a display-friendly Arrow [`RecordBatch`].
///
/// Emits `Utf8` columns for identifiers and enums, `Float64` columns for
/// quantities and PnL amounts, `Timestamp(Nanosecond)` columns for all time
/// fields, and `Boolean` columns for `is_inverse` and `is_currency_pair`.
/// The `trade_ids` and `commissions` columns carry deterministic JSON payloads
/// (sorted by trade id and currency respectively) so that repeated encodings
/// of the same position produce identical bytes.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_positions(data: &[Position]) -> Result<RecordBatch, ArrowError> {
    let mut trader_id = StringBuilder::new();
    let mut strategy_id = StringBuilder::new();
    let mut instrument_id = StringBuilder::new();
    let mut position_id = StringBuilder::new();
    let mut account_id = StringBuilder::new();
    let mut opening_order_id = StringBuilder::new();
    let mut closing_order_id = StringBuilder::new();
    let mut entry = StringBuilder::new();
    let mut side = StringBuilder::new();
    let mut signed_qty = Float64Builder::with_capacity(data.len());
    let mut quantity = Float64Builder::with_capacity(data.len());
    let mut peak_qty = Float64Builder::with_capacity(data.len());
    let mut price_precision = UInt8Builder::with_capacity(data.len());
    let mut size_precision = UInt8Builder::with_capacity(data.len());
    let mut multiplier = Float64Builder::with_capacity(data.len());
    let mut is_inverse = BooleanBuilder::with_capacity(data.len());
    let mut is_currency_pair = BooleanBuilder::with_capacity(data.len());
    let mut instrument_class = StringBuilder::new();
    let mut base_currency = StringBuilder::new();
    let mut quote_currency = StringBuilder::new();
    let mut settlement_currency = StringBuilder::new();
    let mut ts_init = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_opened = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_last = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_closed = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut duration_ns = UInt64Builder::with_capacity(data.len());
    let mut avg_px_open = Float64Builder::with_capacity(data.len());
    let mut avg_px_close = Float64Builder::with_capacity(data.len());
    let mut realized_return = Float64Builder::with_capacity(data.len());
    let mut realized_pnl_amount = Float64Builder::with_capacity(data.len());
    let mut realized_pnl_currency = StringBuilder::new();
    let mut trade_ids = StringBuilder::new();
    let mut buy_qty = Float64Builder::with_capacity(data.len());
    let mut sell_qty = Float64Builder::with_capacity(data.len());
    let mut commissions = StringBuilder::new();
    let mut event_count = UInt32Builder::with_capacity(data.len());
    let mut adjustment_count = UInt32Builder::with_capacity(data.len());

    for position in data {
        trader_id.append_value(position.trader_id);
        strategy_id.append_value(position.strategy_id);
        instrument_id.append_value(position.instrument_id.to_string());
        position_id.append_value(position.id);
        account_id.append_value(position.account_id);
        opening_order_id.append_value(position.opening_order_id);
        closing_order_id.append_option(position.closing_order_id.map(|v| v.to_string()));
        entry.append_value(format!("{}", position.entry));
        side.append_value(format!("{}", position.side));
        signed_qty.append_value(position.signed_qty);
        quantity.append_value(quantity_to_f64(&position.quantity));
        peak_qty.append_value(quantity_to_f64(&position.peak_qty));
        price_precision.append_value(position.price_precision);
        size_precision.append_value(position.size_precision);
        multiplier.append_value(quantity_to_f64(&position.multiplier));
        is_inverse.append_value(position.is_inverse);
        is_currency_pair.append_value(position.is_currency_pair);
        instrument_class.append_value(format!("{}", position.instrument_class));
        base_currency.append_option(position.base_currency.map(|v| v.to_string()));
        quote_currency.append_value(position.quote_currency.to_string());
        settlement_currency.append_value(position.settlement_currency.to_string());
        ts_init.append_value(unix_nanos_to_i64(position.ts_init.as_u64()));
        ts_opened.append_value(unix_nanos_to_i64(position.ts_opened.as_u64()));
        ts_last.append_value(unix_nanos_to_i64(position.ts_last.as_u64()));
        ts_closed.append_option(position.ts_closed.map(|v| unix_nanos_to_i64(v.as_u64())));
        duration_ns.append_value(position.duration_ns);
        avg_px_open.append_value(position.avg_px_open);
        avg_px_close.append_option(position.avg_px_close);
        realized_return.append_value(position.realized_return);
        realized_pnl_amount.append_option(position.realized_pnl.map(|v| money_to_f64(&v)));
        realized_pnl_currency.append_option(position.realized_pnl.map(|v| v.currency.to_string()));
        trade_ids.append_value(trade_ids_to_json(position));
        buy_qty.append_value(quantity_to_f64(&position.buy_qty));
        sell_qty.append_value(quantity_to_f64(&position.sell_qty));
        commissions.append_value(commissions_to_json(position));
        event_count.append_value(position.events.len() as u32);
        adjustment_count.append_value(position.adjustments.len() as u32);
    }

    RecordBatch::try_new(
        Arc::new(position_schema()),
        vec![
            Arc::new(trader_id.finish()),
            Arc::new(strategy_id.finish()),
            Arc::new(instrument_id.finish()),
            Arc::new(position_id.finish()),
            Arc::new(account_id.finish()),
            Arc::new(opening_order_id.finish()),
            Arc::new(closing_order_id.finish()),
            Arc::new(entry.finish()),
            Arc::new(side.finish()),
            Arc::new(signed_qty.finish()),
            Arc::new(quantity.finish()),
            Arc::new(peak_qty.finish()),
            Arc::new(price_precision.finish()),
            Arc::new(size_precision.finish()),
            Arc::new(multiplier.finish()),
            Arc::new(is_inverse.finish()),
            Arc::new(is_currency_pair.finish()),
            Arc::new(instrument_class.finish()),
            Arc::new(base_currency.finish()),
            Arc::new(quote_currency.finish()),
            Arc::new(settlement_currency.finish()),
            Arc::new(ts_init.finish()),
            Arc::new(ts_opened.finish()),
            Arc::new(ts_last.finish()),
            Arc::new(ts_closed.finish()),
            Arc::new(duration_ns.finish()),
            Arc::new(avg_px_open.finish()),
            Arc::new(avg_px_close.finish()),
            Arc::new(realized_return.finish()),
            Arc::new(realized_pnl_amount.finish()),
            Arc::new(realized_pnl_currency.finish()),
            Arc::new(trade_ids.finish()),
            Arc::new(buy_qty.finish()),
            Arc::new(sell_qty.finish()),
            Arc::new(commissions.finish()),
            Arc::new(event_count.finish()),
            Arc::new(adjustment_count.finish()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{
            Array, BooleanArray, Float64Array, StringArray, TimestampNanosecondArray, UInt8Array,
            UInt32Array, UInt64Array,
        },
        datatypes::{DataType, TimeUnit},
    };
    use nautilus_core::UUID4;
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderType},
        events::OrderFilled,
        identifiers::{
            AccountId, ClientOrderId, PositionId, StrategyId, TradeId, TraderId, VenueOrderId,
        },
        instruments::{CurrencyPair, InstrumentAny, stubs::currency_pair_btcusdt},
        types::{Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[expect(clippy::too_many_arguments)]
    fn make_fill(
        instrument: &CurrencyPair,
        side: OrderSide,
        qty: &str,
        price: &str,
        trade_id: &str,
        order_id: &str,
        ts: u64,
        commission: Option<Money>,
    ) -> OrderFilled {
        OrderFilled::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            instrument.id,
            ClientOrderId::from(order_id),
            VenueOrderId::from(order_id),
            AccountId::from("SIM-001"),
            TradeId::from(trade_id),
            side,
            OrderType::Market,
            Quantity::from(qty),
            Price::from(price),
            instrument.quote_currency,
            LiquiditySide::Taker,
            UUID4::default(),
            ts.into(),
            (ts + 1).into(),
            false,
            Some(PositionId::from("P-001")),
            commission,
        )
    }

    fn make_position(ts: u64) -> Position {
        let instrument = currency_pair_btcusdt();
        let fill = make_fill(
            &instrument,
            OrderSide::Buy,
            "1.0",
            "50000.0",
            "T-1",
            "O-1",
            ts,
            None,
        );
        let any = InstrumentAny::CurrencyPair(instrument);
        Position::new(&any, fill)
    }

    #[rstest]
    fn test_encode_positions_schema() {
        let batch = encode_positions(&[]).unwrap();
        let schema = batch.schema();
        let fields = schema.fields();
        assert_eq!(fields.len(), 37);
        assert_eq!(fields[0].name(), "trader_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert_eq!(fields[9].name(), "signed_qty");
        assert_eq!(fields[9].data_type(), &DataType::Float64);
        assert_eq!(fields[12].name(), "price_precision");
        assert_eq!(fields[12].data_type(), &DataType::UInt8);
        assert_eq!(fields[15].name(), "is_inverse");
        assert_eq!(fields[15].data_type(), &DataType::Boolean);
        assert_eq!(fields[21].name(), "ts_init");
        assert_eq!(
            fields[21].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
        assert_eq!(fields[25].name(), "duration_ns");
        assert_eq!(fields[25].data_type(), &DataType::UInt64);
        assert_eq!(fields[35].name(), "event_count");
        assert_eq!(fields[35].data_type(), &DataType::UInt32);
    }

    #[rstest]
    fn test_encode_positions_empty() {
        let batch = encode_positions(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.schema().fields().len(), 37);
    }

    #[rstest]
    fn test_encode_positions_values() {
        let positions = vec![make_position(1_000_000)];
        let batch = encode_positions(&positions).unwrap();

        assert_eq!(batch.num_rows(), 1);

        let trader_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let quantity_col = batch
            .column(10)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let price_precision_col = batch
            .column(12)
            .as_any()
            .downcast_ref::<UInt8Array>()
            .unwrap();
        let is_currency_pair_col = batch
            .column(16)
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap();
        let ts_opened_col = batch
            .column(22)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let duration_col = batch
            .column(25)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let event_count_col = batch
            .column(35)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap();

        assert_eq!(trader_id_col.value(0), "TRADER-001");
        assert!((quantity_col.value(0) - 1.0).abs() < 1e-9);
        assert_eq!(price_precision_col.value(0), 2);
        assert!(is_currency_pair_col.value(0));
        assert_eq!(ts_opened_col.value(0), 1_000_000);
        assert_eq!(duration_col.value(0), 0);
        assert_eq!(event_count_col.value(0), 1);
    }

    #[rstest]
    fn test_encode_positions_nullable_fields() {
        let positions = vec![make_position(1_000)];
        let batch = encode_positions(&positions).unwrap();

        let closing_order_id_col = batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let ts_closed_col = batch
            .column(24)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let avg_px_close_col = batch
            .column(27)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();

        assert!(closing_order_id_col.is_null(0));
        assert!(ts_closed_col.is_null(0));
        assert!(avg_px_close_col.is_null(0));
    }

    #[rstest]
    fn test_encode_positions_trade_ids_sorted() {
        let instrument = currency_pair_btcusdt();
        let any = InstrumentAny::CurrencyPair(instrument.clone());
        let open = make_fill(
            &instrument,
            OrderSide::Buy,
            "1.0",
            "50000.0",
            "T-Z",
            "O-1",
            1_000,
            None,
        );
        let add = make_fill(
            &instrument,
            OrderSide::Buy,
            "1.0",
            "50000.0",
            "T-A",
            "O-2",
            2_000,
            None,
        );
        let mut position = Position::new(&any, open);
        position.apply(&add);

        let batch = encode_positions(&[position]).unwrap();
        let trade_ids_col = batch
            .column(31)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        let parsed: Vec<String> = serde_json::from_str(trade_ids_col.value(0)).unwrap();
        assert_eq!(parsed, vec!["T-A".to_string(), "T-Z".to_string()]);
    }

    #[rstest]
    fn test_encode_positions_closed() {
        let instrument = currency_pair_btcusdt();
        let any = InstrumentAny::CurrencyPair(instrument.clone());
        let open = make_fill(
            &instrument,
            OrderSide::Buy,
            "1.0",
            "50000.0",
            "T-1",
            "O-1",
            1_000,
            None,
        );
        let close = make_fill(
            &instrument,
            OrderSide::Sell,
            "1.0",
            "50500.0",
            "T-2",
            "O-2",
            5_000,
            None,
        );
        let mut position = Position::new(&any, open);
        position.apply(&close);

        let batch = encode_positions(&[position]).unwrap();
        let closing_order_id_col = batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let ts_closed_col = batch
            .column(24)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let duration_col = batch
            .column(25)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let avg_px_close_col = batch
            .column(27)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let realized_pnl_amount_col = batch
            .column(29)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let realized_pnl_currency_col = batch
            .column(30)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let event_count_col = batch
            .column(35)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap();

        assert_eq!(closing_order_id_col.value(0), "O-2");
        assert!(!ts_closed_col.is_null(0));
        assert_eq!(ts_closed_col.value(0), 5_000);
        assert_eq!(duration_col.value(0), 4_000);
        assert!((avg_px_close_col.value(0) - 50_500.0).abs() < 1e-9);
        assert!(!realized_pnl_amount_col.is_null(0));
        assert_eq!(realized_pnl_currency_col.value(0), "USDT");
        assert_eq!(event_count_col.value(0), 2);
    }

    #[rstest]
    fn test_encode_positions_commissions_sorted() {
        let instrument = currency_pair_btcusdt();
        let any = InstrumentAny::CurrencyPair(instrument.clone());
        let usdt_fill = make_fill(
            &instrument,
            OrderSide::Buy,
            "1.0",
            "50000.0",
            "T-1",
            "O-1",
            1_000,
            Some(Money::from("0.50 USDT")),
        );
        let btc_fill = make_fill(
            &instrument,
            OrderSide::Buy,
            "1.0",
            "50000.0",
            "T-2",
            "O-2",
            2_000,
            Some(Money::from("0.00001 BTC")),
        );
        let mut position = Position::new(&any, usdt_fill);
        position.apply(&btc_fill);

        let batch = encode_positions(&[position]).unwrap();
        let commissions_col = batch
            .column(34)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        let parsed: Vec<(String, f64)> = serde_json::from_str(commissions_col.value(0)).unwrap();
        let currencies: Vec<&str> = parsed.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(currencies, vec!["BTC", "USDT"]);
    }
}
