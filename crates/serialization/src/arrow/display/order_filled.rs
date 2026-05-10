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

//! Display-mode Arrow encoder for [`OrderFilled`].

use std::sync::Arc;

use arrow::{
    array::{BooleanBuilder, Float64Builder, StringBuilder, TimestampNanosecondBuilder},
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::events::OrderFilled;

use super::{
    bool_field, float64_field, price_to_f64, quantity_to_f64, timestamp_field, unix_nanos_to_i64,
    utf8_field,
};

/// Returns the display-mode Arrow schema for [`OrderFilled`].
#[must_use]
pub fn order_filled_schema() -> Schema {
    Schema::new(vec![
        utf8_field("trader_id", false),
        utf8_field("strategy_id", false),
        utf8_field("instrument_id", false),
        utf8_field("client_order_id", false),
        utf8_field("venue_order_id", false),
        utf8_field("account_id", false),
        utf8_field("trade_id", false),
        utf8_field("order_side", false),
        utf8_field("order_type", false),
        float64_field("last_qty", false),
        float64_field("last_px", false),
        utf8_field("currency", false),
        utf8_field("liquidity_side", false),
        utf8_field("event_id", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
        bool_field("reconciliation", false),
        utf8_field("position_id", true),
        utf8_field("commission", true),
    ])
}

/// Encodes order fills as a display-friendly Arrow [`RecordBatch`].
///
/// Emits `Float64` columns for `last_qty` and `last_px`, `Timestamp(Nanosecond)`
/// columns for event and init times, and `Utf8` columns for identifiers and enums.
/// Commission renders as its `Display` representation (e.g. `"100.50 USD"`).
/// Mixed-instrument batches are supported.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_order_fills(data: &[OrderFilled]) -> Result<RecordBatch, ArrowError> {
    let mut trader_id = StringBuilder::new();
    let mut strategy_id = StringBuilder::new();
    let mut instrument_id = StringBuilder::new();
    let mut client_order_id = StringBuilder::new();
    let mut venue_order_id = StringBuilder::new();
    let mut account_id = StringBuilder::new();
    let mut trade_id = StringBuilder::new();
    let mut order_side = StringBuilder::new();
    let mut order_type = StringBuilder::new();
    let mut last_qty = Float64Builder::with_capacity(data.len());
    let mut last_px = Float64Builder::with_capacity(data.len());
    let mut currency = StringBuilder::new();
    let mut liquidity_side = StringBuilder::new();
    let mut event_id = StringBuilder::new();
    let mut ts_event = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_init = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut reconciliation = BooleanBuilder::with_capacity(data.len());
    let mut position_id = StringBuilder::new();
    let mut commission = StringBuilder::new();

    for fill in data {
        trader_id.append_value(fill.trader_id);
        strategy_id.append_value(fill.strategy_id);
        instrument_id.append_value(fill.instrument_id.to_string());
        client_order_id.append_value(fill.client_order_id);
        venue_order_id.append_value(fill.venue_order_id);
        account_id.append_value(fill.account_id);
        trade_id.append_value(fill.trade_id.to_string());
        order_side.append_value(format!("{}", fill.order_side));
        order_type.append_value(format!("{}", fill.order_type));
        last_qty.append_value(quantity_to_f64(&fill.last_qty));
        last_px.append_value(price_to_f64(&fill.last_px));
        currency.append_value(fill.currency.to_string());
        liquidity_side.append_value(format!("{}", fill.liquidity_side));
        event_id.append_value(fill.event_id.to_string());
        ts_event.append_value(unix_nanos_to_i64(fill.ts_event.as_u64()));
        ts_init.append_value(unix_nanos_to_i64(fill.ts_init.as_u64()));
        reconciliation.append_value(fill.reconciliation);
        position_id.append_option(fill.position_id.map(|v| v.to_string()));
        commission.append_option(fill.commission.map(|v| format!("{v}")));
    }

    RecordBatch::try_new(
        Arc::new(order_filled_schema()),
        vec![
            Arc::new(trader_id.finish()),
            Arc::new(strategy_id.finish()),
            Arc::new(instrument_id.finish()),
            Arc::new(client_order_id.finish()),
            Arc::new(venue_order_id.finish()),
            Arc::new(account_id.finish()),
            Arc::new(trade_id.finish()),
            Arc::new(order_side.finish()),
            Arc::new(order_type.finish()),
            Arc::new(last_qty.finish()),
            Arc::new(last_px.finish()),
            Arc::new(currency.finish()),
            Arc::new(liquidity_side.finish()),
            Arc::new(event_id.finish()),
            Arc::new(ts_event.finish()),
            Arc::new(ts_init.finish()),
            Arc::new(reconciliation.finish()),
            Arc::new(position_id.finish()),
            Arc::new(commission.finish()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{Array, BooleanArray, Float64Array, StringArray, TimestampNanosecondArray},
        datatypes::{DataType, TimeUnit},
    };
    use nautilus_core::UUID4;
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderType},
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
            VenueOrderId,
        },
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn make_fill(instrument_id: &str, commission: Option<Money>, ts: u64) -> OrderFilled {
        OrderFilled {
            trader_id: TraderId::from("TESTER-001"),
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from(instrument_id),
            client_order_id: ClientOrderId::from("O-001"),
            venue_order_id: VenueOrderId::from("V-001"),
            account_id: AccountId::from("SIM-001"),
            trade_id: TradeId::new("T-001"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
            last_qty: Quantity::from(100),
            last_px: Price::from("50.25"),
            currency: Currency::USD(),
            liquidity_side: LiquiditySide::Maker,
            event_id: UUID4::default(),
            ts_event: ts.into(),
            ts_init: (ts + 1).into(),
            reconciliation: false,
            position_id: Some(PositionId::from("P-001")),
            commission,
        }
    }

    #[rstest]
    fn test_encode_order_fills_schema() {
        let batch = encode_order_fills(&[]).unwrap();
        let schema = batch.schema();
        let fields = schema.fields();
        assert_eq!(fields.len(), 19);
        assert_eq!(fields[0].name(), "trader_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert_eq!(fields[9].name(), "last_qty");
        assert_eq!(fields[9].data_type(), &DataType::Float64);
        assert_eq!(fields[10].name(), "last_px");
        assert_eq!(fields[10].data_type(), &DataType::Float64);
        assert_eq!(fields[14].name(), "ts_event");
        assert_eq!(
            fields[14].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
        assert_eq!(fields[16].name(), "reconciliation");
        assert_eq!(fields[16].data_type(), &DataType::Boolean);
        assert_eq!(fields[18].name(), "commission");
        assert!(fields[18].is_nullable());
    }

    #[rstest]
    fn test_encode_order_fills_values() {
        let commission = Money::new(10.50, Currency::USD());
        let fills = vec![make_fill("AAPL.XNAS", Some(commission), 1_000)];
        let batch = encode_order_fills(&fills).unwrap();

        assert_eq!(batch.num_rows(), 1);

        let last_qty_col = batch
            .column(9)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let last_px_col = batch
            .column(10)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let ts_event_col = batch
            .column(14)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let reconciliation_col = batch
            .column(16)
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap();
        let commission_col = batch
            .column(18)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        assert!((last_qty_col.value(0) - 100.0).abs() < 1e-9);
        assert!((last_px_col.value(0) - 50.25).abs() < 1e-9);
        assert_eq!(ts_event_col.value(0), 1_000);
        assert!(!reconciliation_col.value(0));
        assert_eq!(commission_col.value(0), "10.50 USD");
    }

    #[rstest]
    fn test_encode_order_fills_null_commission() {
        let fills = vec![make_fill("AAPL.XNAS", None, 1_000)];
        let batch = encode_order_fills(&fills).unwrap();

        let commission_col = batch
            .column(18)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(commission_col.is_null(0));
    }

    #[rstest]
    fn test_encode_order_fills_empty() {
        let batch = encode_order_fills(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.schema().fields().len(), 19);
    }

    #[rstest]
    fn test_encode_order_fills_mixed_instruments() {
        let fills = vec![
            make_fill("AAPL.XNAS", None, 1),
            make_fill("MSFT.XNAS", None, 2),
        ];
        let batch = encode_order_fills(&fills).unwrap();

        let instrument_id_col = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(instrument_id_col.value(0), "AAPL.XNAS");
        assert_eq!(instrument_id_col.value(1), "MSFT.XNAS");
    }
}
