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

//! Apache Arrow schema definition and encoding/decoding for [`DatabentoStatistics`].

use std::{collections::HashMap, str::FromStr, sync::Arc};

use arrow::{
    array::{
        BooleanArray, BooleanBuilder, FixedSizeBinaryArray, FixedSizeBinaryBuilder, Int32Array,
        UInt16Array, UInt32Array, UInt64Array, UInt8Array,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    enums::FromU8,
    identifiers::InstrumentId,
    types::{
        Price, Quantity,
        fixed::{PRECISION_BYTES, PRICE_UNDEF, QUANTITY_UNDEF},
    },
};
use nautilus_serialization::arrow::{ArrowSchemaProvider, EncodeToRecordBatch, EncodingError};

use crate::{
    enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction},
    types::DatabentoStatistics,
};

// Metadata keys
const KEY_INSTRUMENT_ID: &str = "instrument_id";
const KEY_PRICE_PRECISION: &str = "price_precision";
const KEY_SIZE_PRECISION: &str = "size_precision";

impl ArrowSchemaProvider for DatabentoStatistics {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("stat_type", DataType::UInt8, false),
            Field::new("update_action", DataType::UInt8, false),
            Field::new(
                "price",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false, // We use PRICE_UNDEF sentinel for None
            ),
            Field::new("price_is_null", DataType::Boolean, false),
            Field::new(
                "quantity",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false, // We use QUANTITY_UNDEF sentinel for None
            ),
            Field::new("quantity_is_null", DataType::Boolean, false),
            Field::new("channel_id", DataType::UInt16, false),
            Field::new("stat_flags", DataType::UInt8, false),
            Field::new("sequence", DataType::UInt32, false),
            Field::new("ts_ref", DataType::UInt64, false),
            Field::new("ts_in_delta", DataType::Int32, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_recv", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl DatabentoStatistics {
    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert(KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string());
        metadata.insert(KEY_PRICE_PRECISION.to_string(), price_precision.to_string());
        metadata.insert(KEY_SIZE_PRECISION.to_string(), size_precision.to_string());
        metadata
    }
}

fn parse_metadata(
    metadata: &HashMap<String, String>,
) -> Result<(InstrumentId, u8, u8), EncodingError> {
    let instrument_id_str = metadata
        .get(KEY_INSTRUMENT_ID)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_INSTRUMENT_ID))?;
    let instrument_id = InstrumentId::from_str(instrument_id_str)
        .map_err(|e| EncodingError::ParseError(KEY_INSTRUMENT_ID, e.to_string()))?;

    let price_precision = metadata
        .get(KEY_PRICE_PRECISION)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_PRICE_PRECISION))?
        .parse::<u8>()
        .map_err(|e| EncodingError::ParseError(KEY_PRICE_PRECISION, e.to_string()))?;

    let size_precision = metadata
        .get(KEY_SIZE_PRECISION)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_SIZE_PRECISION))?
        .parse::<u8>()
        .map_err(|e| EncodingError::ParseError(KEY_SIZE_PRECISION, e.to_string()))?;

    Ok((instrument_id, price_precision, size_precision))
}

impl EncodeToRecordBatch for DatabentoStatistics {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut stat_type_builder = UInt8Array::builder(data.len());
        let mut update_action_builder = UInt8Array::builder(data.len());
        let mut price_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut price_is_null_builder = BooleanBuilder::with_capacity(data.len());
        let mut quantity_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut quantity_is_null_builder = BooleanBuilder::with_capacity(data.len());
        let mut channel_id_builder = UInt16Array::builder(data.len());
        let mut stat_flags_builder = UInt8Array::builder(data.len());
        let mut sequence_builder = UInt32Array::builder(data.len());
        let mut ts_ref_builder = UInt64Array::builder(data.len());
        let mut ts_in_delta_builder = Int32Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_recv_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for item in data {
            stat_type_builder.append_value(item.stat_type as u8);
            update_action_builder.append_value(item.update_action as u8);

            // Handle optional price
            match item.price {
                Some(p) => {
                    price_builder.append_value(p.raw.to_le_bytes()).unwrap();
                    price_is_null_builder.append_value(false);
                }
                None => {
                    price_builder
                        .append_value(PRICE_UNDEF.to_le_bytes())
                        .unwrap();
                    price_is_null_builder.append_value(true);
                }
            }

            // Handle optional quantity
            match item.quantity {
                Some(q) => {
                    quantity_builder.append_value(q.raw.to_le_bytes()).unwrap();
                    quantity_is_null_builder.append_value(false);
                }
                None => {
                    quantity_builder
                        .append_value(QUANTITY_UNDEF.to_le_bytes())
                        .unwrap();
                    quantity_is_null_builder.append_value(true);
                }
            }

            channel_id_builder.append_value(item.channel_id);
            stat_flags_builder.append_value(item.stat_flags);
            sequence_builder.append_value(item.sequence);
            ts_ref_builder.append_value(item.ts_ref.as_u64());
            ts_in_delta_builder.append_value(item.ts_in_delta);
            ts_event_builder.append_value(item.ts_event.as_u64());
            ts_recv_builder.append_value(item.ts_recv.as_u64());
            ts_init_builder.append_value(item.ts_init.as_u64());
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(stat_type_builder.finish()),
                Arc::new(update_action_builder.finish()),
                Arc::new(price_builder.finish()),
                Arc::new(price_is_null_builder.finish()),
                Arc::new(quantity_builder.finish()),
                Arc::new(quantity_is_null_builder.finish()),
                Arc::new(channel_id_builder.finish()),
                Arc::new(stat_flags_builder.finish()),
                Arc::new(sequence_builder.finish()),
                Arc::new(ts_ref_builder.finish()),
                Arc::new(ts_in_delta_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_recv_builder.finish()),
                Arc::new(ts_init_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        let price_precision = self.price.map_or(0, |p| p.precision);
        let size_precision = self.quantity.map_or(0, |q| q.precision);
        Self::get_metadata(&self.instrument_id, price_precision, size_precision)
    }
}

/// Decodes a `RecordBatch` into a vector of `DatabentoStatistics`.
///
/// # Errors
///
/// Returns an `EncodingError` if the decoding fails.
pub fn decode_statistics_batch(
    metadata: &HashMap<String, String>,
    record_batch: RecordBatch,
) -> Result<Vec<DatabentoStatistics>, EncodingError> {
    let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
    let cols = record_batch.columns();

    let stat_type_values = cols[0]
        .as_any()
        .downcast_ref::<UInt8Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "stat_type",
            0,
            DataType::UInt8,
            cols[0].data_type().clone(),
        ))?;
    let update_action_values = cols[1]
        .as_any()
        .downcast_ref::<UInt8Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "update_action",
            1,
            DataType::UInt8,
            cols[1].data_type().clone(),
        ))?;
    let price_values = cols[2]
        .as_any()
        .downcast_ref::<FixedSizeBinaryArray>()
        .ok_or(EncodingError::InvalidColumnType(
            "price",
            2,
            DataType::FixedSizeBinary(PRECISION_BYTES),
            cols[2].data_type().clone(),
        ))?;
    let price_is_null_values = cols[3]
        .as_any()
        .downcast_ref::<BooleanArray>()
        .ok_or(EncodingError::InvalidColumnType(
            "price_is_null",
            3,
            DataType::Boolean,
            cols[3].data_type().clone(),
        ))?;
    let quantity_values = cols[4]
        .as_any()
        .downcast_ref::<FixedSizeBinaryArray>()
        .ok_or(EncodingError::InvalidColumnType(
            "quantity",
            4,
            DataType::FixedSizeBinary(PRECISION_BYTES),
            cols[4].data_type().clone(),
        ))?;
    let quantity_is_null_values = cols[5]
        .as_any()
        .downcast_ref::<BooleanArray>()
        .ok_or(EncodingError::InvalidColumnType(
            "quantity_is_null",
            5,
            DataType::Boolean,
            cols[5].data_type().clone(),
        ))?;
    let channel_id_values = cols[6]
        .as_any()
        .downcast_ref::<UInt16Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "channel_id",
            6,
            DataType::UInt16,
            cols[6].data_type().clone(),
        ))?;
    let stat_flags_values = cols[7]
        .as_any()
        .downcast_ref::<UInt8Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "stat_flags",
            7,
            DataType::UInt8,
            cols[7].data_type().clone(),
        ))?;
    let sequence_values = cols[8]
        .as_any()
        .downcast_ref::<UInt32Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "sequence",
            8,
            DataType::UInt32,
            cols[8].data_type().clone(),
        ))?;
    let ts_ref_values = cols[9]
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "ts_ref",
            9,
            DataType::UInt64,
            cols[9].data_type().clone(),
        ))?;
    let ts_in_delta_values = cols[10]
        .as_any()
        .downcast_ref::<Int32Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "ts_in_delta",
            10,
            DataType::Int32,
            cols[10].data_type().clone(),
        ))?;
    let ts_event_values = cols[11]
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "ts_event",
            11,
            DataType::UInt64,
            cols[11].data_type().clone(),
        ))?;
    let ts_recv_values = cols[12]
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "ts_recv",
            12,
            DataType::UInt64,
            cols[12].data_type().clone(),
        ))?;
    let ts_init_values = cols[13]
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "ts_init",
            13,
            DataType::UInt64,
            cols[13].data_type().clone(),
        ))?;

    let result: Result<Vec<DatabentoStatistics>, EncodingError> = (0..record_batch.num_rows())
        .map(|row| {
            let stat_type_value = stat_type_values.value(row);
            let stat_type = DatabentoStatisticType::from_u8(stat_type_value).ok_or_else(|| {
                EncodingError::ParseError(
                    "DatabentoStatisticType",
                    format!("Invalid enum value: {stat_type_value}"),
                )
            })?;

            let update_action_value = update_action_values.value(row);
            let update_action =
                DatabentoStatisticUpdateAction::from_u8(update_action_value).ok_or_else(|| {
                    EncodingError::ParseError(
                        "DatabentoStatisticUpdateAction",
                        format!("Invalid enum value: {update_action_value}"),
                    )
                })?;

            // Decode optional price
            let price = if price_is_null_values.value(row) {
                None
            } else {
                let bytes = price_values.value(row);
                let raw = i128::from_le_bytes(
                    bytes
                        .try_into()
                        .map_err(|_| EncodingError::ParseError("price", "Invalid bytes".to_string()))?,
                );
                Some(Price::from_raw(raw, price_precision))
            };

            // Decode optional quantity
            let quantity = if quantity_is_null_values.value(row) {
                None
            } else {
                let bytes = quantity_values.value(row);
                let raw = u128::from_le_bytes(bytes.try_into().map_err(|_| {
                    EncodingError::ParseError("quantity", "Invalid bytes".to_string())
                })?);
                Some(Quantity::from_raw(raw, size_precision))
            };

            Ok(DatabentoStatistics::new(
                instrument_id,
                stat_type,
                update_action,
                price,
                quantity,
                channel_id_values.value(row),
                stat_flags_values.value(row),
                sequence_values.value(row),
                ts_ref_values.value(row).into(),
                ts_in_delta_values.value(row),
                ts_event_values.value(row).into(),
                ts_recv_values.value(row).into(),
                ts_init_values.value(row).into(),
            ))
        })
        .collect();

    result
}

/// Converts a vector of `DatabentoStatistics` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[allow(clippy::missing_panics_doc)] // Guarded by empty check
pub fn statistics_to_arrow_record_batch_bytes(
    data: Vec<DatabentoStatistics>,
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    // Extract metadata from first element
    let first = data.first().unwrap();
    let metadata = first.metadata();
    DatabentoStatistics::encode_batch(&metadata, &data).map_err(EncodingError::ArrowError)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{array::Array, record_batch::RecordBatch};
    use nautilus_model::types::{Price, Quantity};
    use rstest::rstest;

    use super::*;
    use crate::enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction};

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ]);
        let schema = DatabentoStatistics::get_schema(Some(metadata.clone()));

        assert_eq!(schema.fields().len(), 14);
        assert_eq!(schema.field(0).name(), "stat_type");
        assert_eq!(schema.field(1).name(), "update_action");
        assert_eq!(schema.field(2).name(), "price");
        assert_eq!(schema.field(3).name(), "price_is_null");
        assert_eq!(schema.field(4).name(), "quantity");
        assert_eq!(schema.field(5).name(), "quantity_is_null");
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = DatabentoStatistics::get_schema_map();
        assert!(schema_map.contains_key("stat_type"));
        assert!(schema_map.contains_key("update_action"));
        assert!(schema_map.contains_key("price"));
        assert!(schema_map.contains_key("quantity"));
        assert!(schema_map.contains_key("ts_event"));
        assert!(schema_map.contains_key("ts_init"));
    }

    #[rstest]
    fn test_encode_batch() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ]);

        let stat1 = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::OpeningPrice,
            DatabentoStatisticUpdateAction::Added,
            Some(Price::from("150.50")),
            None,
            1,
            0,
            100,
            1.into(),
            0,
            1.into(),
            2.into(),
            3.into(),
        );

        let stat2 = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::ClearedVolume,
            DatabentoStatisticUpdateAction::Added,
            None,
            Some(Quantity::from(1000)),
            2,
            0,
            101,
            2.into(),
            0,
            2.into(),
            3.into(),
            4.into(),
        );

        let data = vec![stat1, stat2];
        let record_batch = DatabentoStatistics::encode_batch(&metadata, &data).unwrap();

        assert_eq!(record_batch.num_columns(), 14);
        assert_eq!(record_batch.num_rows(), 2);

        let stat_type_values = record_batch.columns()[0]
            .as_any()
            .downcast_ref::<UInt8Array>()
            .unwrap();
        assert_eq!(
            stat_type_values.value(0),
            DatabentoStatisticType::OpeningPrice as u8
        );
        assert_eq!(
            stat_type_values.value(1),
            DatabentoStatisticType::ClearedVolume as u8
        );
    }

    #[rstest]
    fn test_decode_batch() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ]);

        let stat = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::OpeningPrice,
            DatabentoStatisticUpdateAction::Added,
            Some(Price::from("150.50")),
            None,
            1,
            0,
            100,
            1.into(),
            0,
            1.into(),
            2.into(),
            3.into(),
        );

        let data = vec![stat];
        let record_batch = DatabentoStatistics::encode_batch(&metadata, &data).unwrap();
        let decoded = decode_statistics_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].instrument_id, instrument_id);
        assert_eq!(decoded[0].stat_type, DatabentoStatisticType::OpeningPrice);
        assert_eq!(
            decoded[0].update_action,
            DatabentoStatisticUpdateAction::Added
        );
        assert!(decoded[0].price.is_some());
        assert!(decoded[0].quantity.is_none());
    }

    #[rstest]
    fn test_encode_decode_round_trip() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ]);

        let stat1 = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::SettlementPrice,
            DatabentoStatisticUpdateAction::Added,
            Some(Price::from("150.50")),
            Some(Quantity::from(1000)),
            1,
            0,
            100,
            1_000_000_000.into(),
            500,
            1_000_000_000.into(),
            1_000_000_001.into(),
            1_000_000_002.into(),
        );

        let stat2 = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::OpenInterest,
            DatabentoStatisticUpdateAction::Deleted,
            None,
            None,
            2,
            1,
            101,
            2_000_000_000.into(),
            -100,
            2_000_000_000.into(),
            2_000_000_001.into(),
            2_000_000_002.into(),
        );

        let original = vec![stat1, stat2];
        let record_batch = DatabentoStatistics::encode_batch(&metadata, &original).unwrap();
        let decoded = decode_statistics_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), original.len());
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec.instrument_id, orig.instrument_id);
            assert_eq!(dec.stat_type, orig.stat_type);
            assert_eq!(dec.update_action, orig.update_action);
            assert_eq!(dec.channel_id, orig.channel_id);
            assert_eq!(dec.stat_flags, orig.stat_flags);
            assert_eq!(dec.sequence, orig.sequence);
            assert_eq!(dec.ts_ref, orig.ts_ref);
            assert_eq!(dec.ts_in_delta, orig.ts_in_delta);
            assert_eq!(dec.ts_event, orig.ts_event);
            assert_eq!(dec.ts_recv, orig.ts_recv);
            assert_eq!(dec.ts_init, orig.ts_init);
        }
    }
}
