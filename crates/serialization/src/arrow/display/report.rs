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

//! Display-mode Arrow encoder for [`OrderStatusReport`].

use std::sync::Arc;

use arrow::{
    array::{BooleanBuilder, Float64Builder, StringBuilder, TimestampNanosecondBuilder},
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::reports::OrderStatusReport;
use rust_decimal::prelude::ToPrimitive;

use super::{
    bool_field, float64_field, quantity_to_f64, timestamp_field, unix_nanos_to_i64, utf8_field,
};

/// Returns the display-mode Arrow schema for [`OrderStatusReport`].
#[must_use]
pub fn order_status_report_schema() -> Schema {
    Schema::new(vec![
        utf8_field("account_id", false),
        utf8_field("instrument_id", false),
        utf8_field("client_order_id", true),
        utf8_field("venue_order_id", false),
        utf8_field("order_side", false),
        utf8_field("order_type", false),
        utf8_field("time_in_force", false),
        utf8_field("order_status", false),
        float64_field("quantity", false),
        float64_field("filled_qty", false),
        utf8_field("report_id", false),
        timestamp_field("ts_accepted", false),
        timestamp_field("ts_last", false),
        timestamp_field("ts_init", false),
        utf8_field("order_list_id", true),
        utf8_field("venue_position_id", true),
        utf8_field("linked_order_ids", true),
        utf8_field("parent_order_id", true),
        utf8_field("contingency_type", false),
        timestamp_field("expire_time", true),
        float64_field("price", true),
        float64_field("trigger_price", true),
        utf8_field("trigger_type", true),
        float64_field("limit_offset", true),
        float64_field("trailing_offset", true),
        utf8_field("trailing_offset_type", false),
        float64_field("avg_px", true),
        float64_field("display_qty", true),
        bool_field("post_only", false),
        bool_field("reduce_only", false),
        utf8_field("cancel_reason", true),
        timestamp_field("ts_triggered", true),
    ])
}

/// Encodes order status reports as a display-friendly Arrow [`RecordBatch`].
///
/// Emits `Float64` columns for quantities, prices, and offsets,
/// `Timestamp(Nanosecond)` columns for all time fields, and `Utf8` columns for
/// identifiers and enums. Mixed-instrument batches are supported.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_order_status_reports(data: &[OrderStatusReport]) -> Result<RecordBatch, ArrowError> {
    let mut account_id = StringBuilder::new();
    let mut instrument_id = StringBuilder::new();
    let mut client_order_id = StringBuilder::new();
    let mut venue_order_id = StringBuilder::new();
    let mut order_side = StringBuilder::new();
    let mut order_type = StringBuilder::new();
    let mut time_in_force = StringBuilder::new();
    let mut order_status = StringBuilder::new();
    let mut quantity = Float64Builder::with_capacity(data.len());
    let mut filled_qty = Float64Builder::with_capacity(data.len());
    let mut report_id = StringBuilder::new();
    let mut ts_accepted = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_last = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_init = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut order_list_id = StringBuilder::new();
    let mut venue_position_id = StringBuilder::new();
    let mut linked_order_ids = StringBuilder::new();
    let mut parent_order_id = StringBuilder::new();
    let mut contingency_type = StringBuilder::new();
    let mut expire_time = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut price = Float64Builder::with_capacity(data.len());
    let mut trigger_price = Float64Builder::with_capacity(data.len());
    let mut trigger_type = StringBuilder::new();
    let mut limit_offset = Float64Builder::with_capacity(data.len());
    let mut trailing_offset = Float64Builder::with_capacity(data.len());
    let mut trailing_offset_type = StringBuilder::new();
    let mut avg_px = Float64Builder::with_capacity(data.len());
    let mut display_qty = Float64Builder::with_capacity(data.len());
    let mut post_only = BooleanBuilder::with_capacity(data.len());
    let mut reduce_only = BooleanBuilder::with_capacity(data.len());
    let mut cancel_reason = StringBuilder::new();
    let mut ts_triggered = TimestampNanosecondBuilder::with_capacity(data.len());

    for report in data {
        account_id.append_value(report.account_id);
        instrument_id.append_value(report.instrument_id.to_string());
        client_order_id.append_option(report.client_order_id.map(|v| v.to_string()));
        venue_order_id.append_value(report.venue_order_id);
        order_side.append_value(format!("{}", report.order_side));
        order_type.append_value(format!("{}", report.order_type));
        time_in_force.append_value(format!("{}", report.time_in_force));
        order_status.append_value(format!("{}", report.order_status));
        quantity.append_value(quantity_to_f64(&report.quantity));
        filled_qty.append_value(quantity_to_f64(&report.filled_qty));
        report_id.append_value(report.report_id.to_string());
        ts_accepted.append_value(unix_nanos_to_i64(report.ts_accepted.as_u64()));
        ts_last.append_value(unix_nanos_to_i64(report.ts_last.as_u64()));
        ts_init.append_value(unix_nanos_to_i64(report.ts_init.as_u64()));
        order_list_id.append_option(report.order_list_id.map(|v| v.to_string()));
        venue_position_id.append_option(report.venue_position_id.map(|v| v.to_string()));
        linked_order_ids.append_option(report.linked_order_ids.as_ref().map(|ids| {
            let values: Vec<String> = ids.iter().map(ToString::to_string).collect();
            serde_json::to_string(&values).unwrap_or_default()
        }));
        parent_order_id.append_option(report.parent_order_id.map(|v| v.to_string()));
        contingency_type.append_value(format!("{}", report.contingency_type));
        expire_time.append_option(report.expire_time.map(|v| unix_nanos_to_i64(v.as_u64())));
        price.append_option(report.price.map(|v| v.as_f64()));
        trigger_price.append_option(report.trigger_price.map(|v| v.as_f64()));
        trigger_type.append_option(report.trigger_type.map(|v| format!("{v}")));
        limit_offset.append_option(report.limit_offset.and_then(|v| v.to_f64()));
        trailing_offset.append_option(report.trailing_offset.and_then(|v| v.to_f64()));
        trailing_offset_type.append_value(format!("{}", report.trailing_offset_type));
        avg_px.append_option(report.avg_px.and_then(|v| v.to_f64()));
        display_qty.append_option(report.display_qty.map(|v| quantity_to_f64(&v)));
        post_only.append_value(report.post_only);
        reduce_only.append_value(report.reduce_only);
        cancel_reason.append_option(report.cancel_reason.clone());
        ts_triggered.append_option(report.ts_triggered.map(|v| unix_nanos_to_i64(v.as_u64())));
    }

    RecordBatch::try_new(
        Arc::new(order_status_report_schema()),
        vec![
            Arc::new(account_id.finish()),
            Arc::new(instrument_id.finish()),
            Arc::new(client_order_id.finish()),
            Arc::new(venue_order_id.finish()),
            Arc::new(order_side.finish()),
            Arc::new(order_type.finish()),
            Arc::new(time_in_force.finish()),
            Arc::new(order_status.finish()),
            Arc::new(quantity.finish()),
            Arc::new(filled_qty.finish()),
            Arc::new(report_id.finish()),
            Arc::new(ts_accepted.finish()),
            Arc::new(ts_last.finish()),
            Arc::new(ts_init.finish()),
            Arc::new(order_list_id.finish()),
            Arc::new(venue_position_id.finish()),
            Arc::new(linked_order_ids.finish()),
            Arc::new(parent_order_id.finish()),
            Arc::new(contingency_type.finish()),
            Arc::new(expire_time.finish()),
            Arc::new(price.finish()),
            Arc::new(trigger_price.finish()),
            Arc::new(trigger_type.finish()),
            Arc::new(limit_offset.finish()),
            Arc::new(trailing_offset.finish()),
            Arc::new(trailing_offset_type.finish()),
            Arc::new(avg_px.finish()),
            Arc::new(display_qty.finish()),
            Arc::new(post_only.finish()),
            Arc::new(reduce_only.finish()),
            Arc::new(cancel_reason.finish()),
            Arc::new(ts_triggered.finish()),
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
        enums::{
            ContingencyType, OrderSide, OrderStatus, OrderType, TimeInForce, TrailingOffsetType,
        },
        identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn make_report(instrument_id: &str, ts: u64) -> OrderStatusReport {
        OrderStatusReport {
            account_id: AccountId::from("SIM-001"),
            instrument_id: InstrumentId::from(instrument_id),
            client_order_id: Some(ClientOrderId::from("O-001")),
            venue_order_id: VenueOrderId::from("V-001"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            order_status: OrderStatus::Accepted,
            quantity: Quantity::from(100),
            filled_qty: Quantity::from(50),
            report_id: UUID4::default(),
            ts_accepted: ts.into(),
            ts_last: (ts + 1_000).into(),
            ts_init: (ts + 1).into(),
            order_list_id: None,
            venue_position_id: None,
            linked_order_ids: None,
            parent_order_id: None,
            contingency_type: ContingencyType::NoContingency,
            expire_time: None,
            price: Some(Price::from("100.50")),
            trigger_price: None,
            trigger_type: None,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: TrailingOffsetType::NoTrailingOffset,
            avg_px: None,
            display_qty: None,
            post_only: true,
            reduce_only: false,
            cancel_reason: None,
            ts_triggered: None,
        }
    }

    #[rstest]
    fn test_encode_order_status_reports_schema() {
        let batch = encode_order_status_reports(&[]).unwrap();
        let schema = batch.schema();
        let fields = schema.fields();
        assert_eq!(fields.len(), 32);
        assert_eq!(fields[0].name(), "account_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert_eq!(fields[8].name(), "quantity");
        assert_eq!(fields[8].data_type(), &DataType::Float64);
        assert_eq!(fields[11].name(), "ts_accepted");
        assert_eq!(
            fields[11].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
        assert_eq!(fields[28].name(), "post_only");
        assert_eq!(fields[28].data_type(), &DataType::Boolean);
    }

    #[rstest]
    fn test_encode_order_status_reports_values() {
        let reports = vec![make_report("AAPL.XNAS", 1_000_000)];
        let batch = encode_order_status_reports(&reports).unwrap();

        assert_eq!(batch.num_rows(), 1);

        let quantity_col = batch
            .column(8)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let filled_qty_col = batch
            .column(9)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let price_col = batch
            .column(20)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let post_only_col = batch
            .column(28)
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap();
        let ts_accepted_col = batch
            .column(11)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert!((quantity_col.value(0) - 100.0).abs() < 1e-9);
        assert!((filled_qty_col.value(0) - 50.0).abs() < 1e-9);
        assert!((price_col.value(0) - 100.50).abs() < 1e-9);
        assert!(post_only_col.value(0));
        assert_eq!(ts_accepted_col.value(0), 1_000_000);
    }

    #[rstest]
    fn test_encode_order_status_reports_linked_order_ids_round_trip() {
        let mut report = make_report("AAPL.XNAS", 1_000);
        report.linked_order_ids = Some(vec![
            ClientOrderId::from("O-Z"),
            ClientOrderId::from("O-A"),
            ClientOrderId::from("O-M"),
        ]);
        let batch = encode_order_status_reports(&[report]).unwrap();

        let linked_col = batch
            .column(16)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(!linked_col.is_null(0));

        let parsed: Vec<String> = serde_json::from_str(linked_col.value(0)).unwrap();
        assert_eq!(parsed, vec!["O-Z", "O-A", "O-M"]);
    }

    #[rstest]
    fn test_encode_order_status_reports_linked_order_ids_null_when_absent() {
        let batch = encode_order_status_reports(&[make_report("AAPL.XNAS", 1_000)]).unwrap();
        let linked_col = batch
            .column(16)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(linked_col.is_null(0));
    }

    #[rstest]
    fn test_encode_order_status_reports_nullable_fields() {
        let reports = vec![make_report("AAPL.XNAS", 1_000)];
        let batch = encode_order_status_reports(&reports).unwrap();

        let trigger_price_col = batch
            .column(21)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let expire_time_col = batch
            .column(19)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert!(trigger_price_col.is_null(0));
        assert!(expire_time_col.is_null(0));
    }

    #[rstest]
    fn test_encode_order_status_reports_empty() {
        let batch = encode_order_status_reports(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.schema().fields().len(), 32);
    }

    #[rstest]
    fn test_encode_order_status_reports_mixed_instruments() {
        let reports = vec![make_report("AAPL.XNAS", 1), make_report("MSFT.XNAS", 2)];
        let batch = encode_order_status_reports(&reports).unwrap();

        let instrument_id_col = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(instrument_id_col.value(0), "AAPL.XNAS");
        assert_eq!(instrument_id_col.value(1), "MSFT.XNAS");
    }
}
