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

use nautilus_model::reports::{
    ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport,
};

use super::json::{JsonFieldSpec, impl_json_arrow};

const ORDER_STATUS_REPORT_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("client_order_id", true),
    JsonFieldSpec::utf8("venue_order_id", false),
    JsonFieldSpec::utf8("order_side", false),
    JsonFieldSpec::utf8("order_type", false),
    JsonFieldSpec::utf8("time_in_force", false),
    JsonFieldSpec::utf8("order_status", false),
    JsonFieldSpec::utf8("quantity", false),
    JsonFieldSpec::utf8("filled_qty", false),
    JsonFieldSpec::utf8("report_id", false),
    JsonFieldSpec::u64("ts_accepted", false),
    JsonFieldSpec::u64("ts_last", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::utf8("order_list_id", true),
    JsonFieldSpec::utf8("venue_position_id", true),
    JsonFieldSpec::utf8_json("linked_order_ids", true),
    JsonFieldSpec::utf8("parent_order_id", true),
    JsonFieldSpec::utf8("contingency_type", false),
    JsonFieldSpec::u64("expire_time", true),
    JsonFieldSpec::utf8("price", true),
    JsonFieldSpec::utf8("activation_price", true),
    JsonFieldSpec::utf8("trigger_price", true),
    JsonFieldSpec::utf8("trigger_type", true),
    JsonFieldSpec::utf8("limit_offset", true),
    JsonFieldSpec::utf8("trailing_offset", true),
    JsonFieldSpec::utf8("trailing_offset_type", false),
    JsonFieldSpec::utf8("avg_px", true),
    JsonFieldSpec::utf8("display_qty", true),
    JsonFieldSpec::boolean("post_only", false),
    JsonFieldSpec::boolean("reduce_only", false),
    JsonFieldSpec::utf8("cancel_reason", true),
    JsonFieldSpec::u64("ts_triggered", true),
];

const FILL_REPORT_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("venue_order_id", false),
    JsonFieldSpec::utf8("trade_id", false),
    JsonFieldSpec::utf8("order_side", false),
    JsonFieldSpec::utf8("last_qty", false),
    JsonFieldSpec::utf8("last_px", false),
    JsonFieldSpec::utf8("commission", false),
    JsonFieldSpec::utf8("liquidity_side", false),
    JsonFieldSpec::utf8("report_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::utf8("client_order_id", true),
    JsonFieldSpec::utf8("venue_position_id", true),
    // Appended (not inserted) so a batch written here still decodes positionally in builds
    // that predate name-based column resolution.
    JsonFieldSpec::decimal_str("avg_px", true),
];

const POSITION_STATUS_REPORT_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("position_side", false),
    JsonFieldSpec::utf8("quantity", false),
    JsonFieldSpec::utf8("signed_decimal_qty", false),
    JsonFieldSpec::utf8("report_id", false),
    JsonFieldSpec::u64("ts_last", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::utf8("venue_position_id", true),
    JsonFieldSpec::utf8("avg_px_open", true),
];

const EXECUTION_MASS_STATUS_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("client_id", false),
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("venue", false),
    JsonFieldSpec::utf8("report_id", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::utf8_json("order_reports", false),
    JsonFieldSpec::utf8_json("fill_reports", false),
    JsonFieldSpec::utf8_json("position_reports", false),
    // Appended (not inserted) so a batch written here still decodes positionally in builds
    // that predate name-based column resolution.
    JsonFieldSpec::u64("lookback_start", true),
    JsonFieldSpec::boolean_default_true("reports_complete"),
];

impl_json_arrow!(instrument OrderStatusReport, "OrderStatusReport", ORDER_STATUS_REPORT_FIELDS);
impl_json_arrow!(instrument FillReport, "FillReport", FILL_REPORT_FIELDS, &["avg_px"]);
impl_json_arrow!(instrument PositionStatusReport, "PositionStatusReport", POSITION_STATUS_REPORT_FIELDS);
impl_json_arrow!(
    typed ExecutionMassStatus,
    "ExecutionMassStatus",
    EXECUTION_MASS_STATUS_FIELDS,
    &["lookback_start", "reports_complete"]
);

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::Arc};

    use arrow::{
        array::{ArrayRef, BooleanArray},
        datatypes::{DataType, Field, Schema},
        record_batch::RecordBatch,
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce},
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, TradeId, Venue,
            VenueOrderId,
        },
        reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use crate::arrow::{
        ArrowSchemaProvider, DecodeTypedFromRecordBatch, EncodeToRecordBatch, EncodingError,
    };

    // Builds that predate name-based column resolution decode these batches by position, so a
    // new column must be appended rather than inserted. Pinning the order fails a mid-schema
    // insertion here instead of silently shifting every later column for those readers.
    #[rstest]
    #[case::fill_report(
        FillReport::get_schema(None),
        &[
            "account_id",
            "instrument_id",
            "venue_order_id",
            "trade_id",
            "order_side",
            "last_qty",
            "last_px",
            "commission",
            "liquidity_side",
            "report_id",
            "ts_event",
            "ts_init",
            "client_order_id",
            "venue_position_id",
            "avg_px",
        ]
    )]
    #[case::execution_mass_status(
        ExecutionMassStatus::get_schema(None),
        &[
            "client_id",
            "account_id",
            "venue",
            "report_id",
            "ts_init",
            "order_reports",
            "fill_reports",
            "position_reports",
            "lookback_start",
            "reports_complete",
        ]
    )]
    fn test_schema_column_order(#[case] schema: Schema, #[case] expected: &[&str]) {
        let names: Vec<&str> = schema
            .fields()
            .iter()
            .map(|field| field.name().as_str())
            .collect();

        assert_eq!(names, expected);
    }

    #[rstest]
    fn test_order_status_report_round_trip() {
        let report = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            Some(ClientOrderId::from("O-19700101-000000-001-001-1")),
            VenueOrderId::from("1"),
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("100"),
            Quantity::from("25"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
            None,
        )
        .with_linked_order_ids([ClientOrderId::from("O-19700101-000000-001-001-2")]);
        let report = OrderStatusReport {
            activation_price: Some(Price::from("1.05000")),
            limit_offset: Some(Decimal::from_str("0.123456789123456789").unwrap()),
            trailing_offset: Some(Decimal::from_str("0.987654321987654321").unwrap()),
            avg_px: Some(Decimal::from_str("1.23456789123456789").unwrap()),
            ..report
        };

        let metadata = report.metadata();
        let batch =
            OrderStatusReport::encode_batch(&metadata, std::slice::from_ref(&report)).unwrap();
        let decoded =
            OrderStatusReport::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![report]);
    }

    #[rstest]
    fn test_position_status_report_round_trip_preserves_decimal_precision() {
        let report = PositionStatusReport {
            account_id: AccountId::from("SIM-001"),
            instrument_id: InstrumentId::from("AUDUSD.SIM"),
            position_side: PositionSide::Long,
            quantity: Quantity::from("100.25"),
            signed_decimal_qty: Decimal::from_str("100.250000000123456789").unwrap(),
            report_id: UUID4::default(),
            ts_last: UnixNanos::from(1_000_000_000),
            ts_init: UnixNanos::from(2_000_000_000),
            venue_position_id: Some(PositionId::from("P-001")),
            avg_px_open: Some(Decimal::from_str("1.23456789123456789").unwrap()),
        };
        let metadata = report.metadata();
        let batch =
            PositionStatusReport::encode_batch(&metadata, std::slice::from_ref(&report)).unwrap();
        let decoded =
            PositionStatusReport::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![report]);
    }

    #[rstest]
    fn test_fill_report_round_trip_preserves_average_price() {
        let report = sample_fill_report(Some(Decimal::from_str("1.23456789123456789").unwrap()));

        let metadata = report.metadata();
        let batch = FillReport::encode_batch(&metadata, std::slice::from_ref(&report)).unwrap();
        let decoded = FillReport::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![report]);
    }

    #[rstest]
    fn test_fill_report_decodes_merged_schema_order() {
        let report = sample_fill_report(Some(Decimal::from_str("1.23456789123456789").unwrap()));
        let metadata = report.metadata();
        let batch = FillReport::encode_batch(&metadata, std::slice::from_ref(&report)).unwrap();
        let merged_batch = batch_with_columns_at_end(&batch, &["account_id", "avg_px"]);

        let decoded =
            FillReport::decode_typed_batch(merged_batch.schema().metadata(), merged_batch).unwrap();

        assert_eq!(decoded, vec![report]);
    }

    #[rstest]
    fn test_fill_report_decodes_legacy_batch_without_average_price() {
        let report = sample_fill_report(None);
        let metadata = report.metadata();
        let batch = FillReport::encode_batch(&metadata, std::slice::from_ref(&report)).unwrap();
        let legacy_batch = batch_without_columns(&batch, &["avg_px"]);

        let decoded =
            FillReport::decode_typed_batch(legacy_batch.schema().metadata(), legacy_batch).unwrap();

        assert_eq!(decoded, vec![report]);
    }

    #[rstest]
    fn test_execution_mass_status_round_trip_preserves_report_window_and_reports() {
        let order_report = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            Some(ClientOrderId::from("O-19700101-000000-001-001-4")),
            VenueOrderId::from("3"),
            OrderSide::Buy.into(),
            OrderType::Market,
            TimeInForce::Ioc,
            OrderStatus::Filled,
            Quantity::from("20"),
            Quantity::from("20"),
            UnixNanos::from(6_000_000_000),
            UnixNanos::from(7_000_000_000),
            UnixNanos::from(8_000_000_000),
            None,
        );
        let fill_report = sample_fill_report(Some(Decimal::from_str("1.25001").unwrap()));
        let position_report = PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            PositionSide::Long,
            Quantity::from("20"),
            UnixNanos::from(7_000_000_000),
            UnixNanos::from(8_000_000_000),
            None,
            Some(PositionId::from("P-003")),
            Some(Decimal::from_str("1.25001").unwrap()),
        );
        let mut report = ExecutionMassStatus::new(
            ClientId::from("SIM"),
            AccountId::from("SIM-001"),
            Venue::from("SIM"),
            UnixNanos::from(9_000_000_000),
            None,
        );
        report.set_report_window(Some(UnixNanos::from(5_000_000_000)), false);
        report.add_order_reports(vec![order_report]);
        report.add_fill_reports(vec![fill_report]);
        report.add_position_reports(vec![position_report]);

        let metadata = report.metadata();
        let batch =
            ExecutionMassStatus::encode_batch(&metadata, std::slice::from_ref(&report)).unwrap();
        let decoded =
            ExecutionMassStatus::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded, vec![report]);
    }

    #[rstest]
    fn test_execution_mass_status_decodes_legacy_batch_with_report_window_defaults() {
        let report = ExecutionMassStatus::new(
            ClientId::from("SIM"),
            AccountId::from("SIM-001"),
            Venue::from("SIM"),
            UnixNanos::from(10_000_000_000),
            None,
        );
        let metadata = report.metadata();
        let batch =
            ExecutionMassStatus::encode_batch(&metadata, std::slice::from_ref(&report)).unwrap();
        let legacy_batch = batch_without_columns(&batch, &["lookback_start", "reports_complete"]);

        let decoded =
            ExecutionMassStatus::decode_typed_batch(legacy_batch.schema().metadata(), legacy_batch)
                .unwrap();

        assert_eq!(decoded, vec![report]);
    }

    #[rstest]
    fn test_execution_mass_status_decodes_merged_legacy_rows_with_report_window_defaults() {
        let report = ExecutionMassStatus::new(
            ClientId::from("SIM"),
            AccountId::from("SIM-001"),
            Venue::from("SIM"),
            UnixNanos::from(11_000_000_000),
            None,
        );
        let metadata = report.metadata();
        let batch =
            ExecutionMassStatus::encode_batch(&metadata, std::slice::from_ref(&report)).unwrap();
        let batch = batch_with_null_boolean_column(&batch, "reports_complete");
        let merged_batch = batch_with_columns_at_end(&batch, &["client_id", "lookback_start"]);

        let decoded =
            ExecutionMassStatus::decode_typed_batch(merged_batch.schema().metadata(), merged_batch)
                .unwrap();

        assert_eq!(decoded, vec![report]);
    }

    #[rstest]
    #[case("lookback_start", 8)]
    #[case("reports_complete", 9)]
    fn test_execution_mass_status_rejects_partial_report_window_schema(
        #[case] missing: &'static str,
        #[case] expected_index: usize,
    ) {
        let report = ExecutionMassStatus::new(
            ClientId::from("SIM"),
            AccountId::from("SIM-001"),
            Venue::from("SIM"),
            UnixNanos::from(12_000_000_000),
            None,
        );
        let metadata = report.metadata();
        let batch =
            ExecutionMassStatus::encode_batch(&metadata, std::slice::from_ref(&report)).unwrap();
        let partial_batch = batch_without_columns(&batch, &[missing]);

        let error = ExecutionMassStatus::decode_typed_batch(
            partial_batch.schema().metadata(),
            partial_batch,
        )
        .expect_err("partial report window schema must be rejected");

        assert!(matches!(
            error,
            EncodingError::MissingColumn(field, index)
                if field == missing && index == expected_index
        ));
    }

    fn sample_fill_report(avg_px: Option<Decimal>) -> FillReport {
        let report = FillReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            VenueOrderId::from("2"),
            TradeId::from("T-002"),
            OrderSide::Sell,
            Quantity::from("17.25"),
            Price::from("1.23456"),
            Money::new(2.75, Currency::USD()),
            LiquiditySide::Maker,
            Some(ClientOrderId::from("O-19700101-000000-001-001-3")),
            Some(PositionId::from("P-002")),
            UnixNanos::from(4_000_000_000),
            UnixNanos::from(5_000_000_000),
            None,
        );

        FillReport { avg_px, ..report }
    }

    fn batch_without_columns(batch: &RecordBatch, names: &[&str]) -> RecordBatch {
        batch_with_column_order(batch, &column_indices(batch, names, false))
    }

    fn batch_with_columns_at_end(batch: &RecordBatch, names: &[&str]) -> RecordBatch {
        let mut indices = column_indices(batch, names, false);
        indices.extend(column_indices(batch, names, true));

        batch_with_column_order(batch, &indices)
    }

    fn column_indices(batch: &RecordBatch, names: &[&str], named: bool) -> Vec<usize> {
        batch
            .schema()
            .fields()
            .iter()
            .enumerate()
            .filter(|(_, field)| names.contains(&field.name().as_str()) == named)
            .map(|(index, _)| index)
            .collect()
    }

    fn batch_with_column_order(batch: &RecordBatch, indices: &[usize]) -> RecordBatch {
        let schema = batch.schema();
        let fields: Vec<_> = indices
            .iter()
            .map(|index| schema.field(*index).clone())
            .collect();
        let columns: Vec<ArrayRef> = indices
            .iter()
            .map(|index| Arc::clone(batch.column(*index)))
            .collect();
        let reordered = Schema::new_with_metadata(fields, schema.metadata().clone());

        RecordBatch::try_new(Arc::new(reordered), columns).unwrap()
    }

    fn batch_with_null_boolean_column(batch: &RecordBatch, name: &str) -> RecordBatch {
        let schema = batch.schema();
        let column_index = schema.index_of(name).unwrap();
        let mut fields = schema.fields().to_vec();
        fields[column_index] = Arc::new(Field::new(name, DataType::Boolean, true));
        let mut columns = batch.columns().to_vec();
        columns[column_index] = Arc::new(BooleanArray::from(vec![None; batch.num_rows()]));
        let schema = Schema::new_with_metadata(fields, schema.metadata().clone());

        RecordBatch::try_new(Arc::new(schema), columns).unwrap()
    }
}
