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
use nautilus_model::reports::{
    ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport,
};

use super::{
    ArrowSchemaProvider, DecodeTypedFromRecordBatch, EncodeToRecordBatch, EncodingError,
    KEY_INSTRUMENT_ID,
    json::{JsonFieldSpec, decode_batch, encode_batch, metadata_for_type, schema_for_type},
};

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
];

fn instrument_metadata(type_name: &'static str, instrument_id: &str) -> HashMap<String, String> {
    let mut metadata = metadata_for_type(type_name);
    metadata.insert(KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string());
    metadata
}

macro_rules! impl_report_arrow {
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

impl_report_arrow!(
    OrderStatusReport,
    "OrderStatusReport",
    ORDER_STATUS_REPORT_FIELDS
);
impl_report_arrow!(FillReport, "FillReport", FILL_REPORT_FIELDS);
impl_report_arrow!(
    PositionStatusReport,
    "PositionStatusReport",
    POSITION_STATUS_REPORT_FIELDS
);

impl ArrowSchemaProvider for ExecutionMassStatus {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        schema_for_type(
            "ExecutionMassStatus",
            metadata,
            EXECUTION_MASS_STATUS_FIELDS,
        )
    }
}

impl EncodeToRecordBatch for ExecutionMassStatus {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        encode_batch(
            "ExecutionMassStatus",
            metadata,
            data,
            EXECUTION_MASS_STATUS_FIELDS,
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        metadata_for_type("ExecutionMassStatus")
    }
}

impl DecodeTypedFromRecordBatch for ExecutionMassStatus {
    fn decode_typed_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        decode_batch(
            metadata,
            &record_batch,
            EXECUTION_MASS_STATUS_FIELDS,
            Some("ExecutionMassStatus"),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, OrderStatus, OrderType, PositionSideSpecified, TimeInForce},
        identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, VenueOrderId},
        reports::{OrderStatusReport, PositionStatusReport},
        types::Quantity,
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;

    #[rstest]
    fn test_order_status_report_round_trip() {
        let report = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUDUSD.SIM"),
            Some(ClientOrderId::from("O-19700101-000000-001-001-1")),
            VenueOrderId::from("1"),
            OrderSide::Buy,
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
            position_side: PositionSideSpecified::Long,
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
}
