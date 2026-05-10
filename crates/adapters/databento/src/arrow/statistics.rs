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

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::{
        FixedSizeBinaryArray, FixedSizeBinaryBuilder, Int32Array, UInt8Array, UInt16Array,
        UInt32Array, UInt64Array,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{Data, custom::CustomData},
    enums::FromU8,
    types::{
        PRICE_UNDEF, QUANTITY_UNDEF,
        fixed::{FIXED_PRECISION, PRECISION_BYTES},
    },
};
use nautilus_serialization::arrow::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch, EncodingError,
    decode_price_with_sentinel, decode_quantity_with_sentinel, extract_column,
    validate_precision_bytes,
};

use super::parse_metadata;
use crate::{
    enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction},
    types::DatabentoStatistics,
};

impl ArrowSchemaProvider for DatabentoStatistics {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("stat_type", DataType::UInt8, false),
            Field::new("update_action", DataType::UInt8, false),
            Field::new("price", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new(
                "quantity",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
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

impl EncodeToRecordBatch for DatabentoStatistics {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut stat_type_builder = UInt8Array::builder(data.len());
        let mut update_action_builder = UInt8Array::builder(data.len());
        let mut price_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut quantity_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
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
            let price_raw = item.price.map_or(PRICE_UNDEF, |p| p.raw);
            price_builder.append_value(price_raw.to_le_bytes()).unwrap();
            let quantity_raw = item.quantity.map_or(QUANTITY_UNDEF, |q| q.raw);
            quantity_builder
                .append_value(quantity_raw.to_le_bytes())
                .unwrap();
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
                Arc::new(quantity_builder.finish()),
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
        Self::get_metadata(
            &self.instrument_id,
            self.price.map_or(FIXED_PRECISION, |p| p.precision),
            self.quantity.map_or(FIXED_PRECISION, |q| q.precision),
        )
    }

    fn chunk_metadata(chunk: &[Self]) -> HashMap<String, String> {
        let first = chunk
            .first()
            .expect("Chunk should have at least one element to encode");

        let price_precision = chunk
            .iter()
            .find_map(|s| s.price.map(|p| p.precision))
            .unwrap_or(FIXED_PRECISION);
        let size_precision = chunk
            .iter()
            .find_map(|s| s.quantity.map(|q| q.precision))
            .unwrap_or(FIXED_PRECISION);

        Self::get_metadata(&first.instrument_id, price_precision, size_precision)
    }
}

impl DecodeDataFromRecordBatch for DatabentoStatistics {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let items = decode_statistics_batch(metadata, &record_batch)?;
        Ok(items
            .into_iter()
            .map(|item| Data::Custom(CustomData::from_arc(Arc::new(item))))
            .collect())
    }
}

/// Decodes a `RecordBatch` into a vector of [`DatabentoStatistics`].
///
/// # Errors
///
/// Returns an `EncodingError` if decoding fails.
pub fn decode_statistics_batch(
    metadata: &HashMap<String, String>,
    record_batch: &RecordBatch,
) -> Result<Vec<DatabentoStatistics>, EncodingError> {
    let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
    let cols = record_batch.columns();

    let stat_type_values = extract_column::<UInt8Array>(cols, "stat_type", 0, DataType::UInt8)?;
    let update_action_values =
        extract_column::<UInt8Array>(cols, "update_action", 1, DataType::UInt8)?;
    let price_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "price",
        2,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let quantity_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "quantity",
        3,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let channel_id_values = extract_column::<UInt16Array>(cols, "channel_id", 4, DataType::UInt16)?;
    let stat_flags_values = extract_column::<UInt8Array>(cols, "stat_flags", 5, DataType::UInt8)?;
    let sequence_values = extract_column::<UInt32Array>(cols, "sequence", 6, DataType::UInt32)?;
    let ts_ref_values = extract_column::<UInt64Array>(cols, "ts_ref", 7, DataType::UInt64)?;
    let ts_in_delta_values = extract_column::<Int32Array>(cols, "ts_in_delta", 8, DataType::Int32)?;
    let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 9, DataType::UInt64)?;
    let ts_recv_values = extract_column::<UInt64Array>(cols, "ts_recv", 10, DataType::UInt64)?;
    let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 11, DataType::UInt64)?;

    validate_precision_bytes(price_values, "price")?;
    validate_precision_bytes(quantity_values, "quantity")?;

    (0..record_batch.num_rows())
        .map(|row| {
            let stat_type_value = stat_type_values.value(row);
            let stat_type = DatabentoStatisticType::from_u8(stat_type_value).ok_or_else(|| {
                EncodingError::ParseError(
                    stringify!(DatabentoStatisticType),
                    format!("Invalid enum value, was {stat_type_value}"),
                )
            })?;
            let update_action_value = update_action_values.value(row);
            let update_action = DatabentoStatisticUpdateAction::from_u8(update_action_value)
                .ok_or_else(|| {
                    EncodingError::ParseError(
                        stringify!(DatabentoStatisticUpdateAction),
                        format!("Invalid enum value, was {update_action_value}"),
                    )
                })?;

            let price_decoded =
                decode_price_with_sentinel(price_values.value(row), price_precision, "price", row)?;
            let price = if price_decoded.raw == PRICE_UNDEF {
                None
            } else {
                Some(price_decoded)
            };

            let quantity_decoded = decode_quantity_with_sentinel(
                quantity_values.value(row),
                size_precision,
                "quantity",
                row,
            )?;
            let quantity = if quantity_decoded.raw == QUANTITY_UNDEF {
                None
            } else {
                Some(quantity_decoded)
            };

            Ok(DatabentoStatistics {
                instrument_id,
                stat_type,
                update_action,
                price,
                quantity,
                channel_id: channel_id_values.value(row),
                stat_flags: stat_flags_values.value(row),
                sequence: sequence_values.value(row),
                ts_ref: ts_ref_values.value(row).into(),
                ts_in_delta: ts_in_delta_values.value(row),
                ts_event: ts_event_values.value(row).into(),
                ts_recv: ts_recv_values.value(row).into(),
                ts_init: ts_init_values.value(row).into(),
            })
        })
        .collect()
}

/// Encodes a vector of [`DatabentoStatistics`] into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if `data` is empty or encoding fails.
// Guarded by empty check
pub fn statistics_to_arrow_record_batch(
    data: &[DatabentoStatistics],
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    let metadata = DatabentoStatistics::chunk_metadata(data);
    DatabentoStatistics::encode_batch(&metadata, data).map_err(EncodingError::ArrowError)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use nautilus_model::{
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use nautilus_serialization::arrow::{
        ArrowSchemaProvider, EncodeToRecordBatch, KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION,
        KEY_SIZE_PRECISION,
    };
    use rstest::rstest;

    use super::*;

    fn test_metadata() -> HashMap<String, String> {
        HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), "ESM4.GLBX".to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ])
    }

    fn test_statistics(instrument_id: InstrumentId) -> DatabentoStatistics {
        DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::OpeningPrice,
            DatabentoStatisticUpdateAction::Added,
            Some(Price::from("5000.50")),
            Some(Quantity::from("100")),
            1,
            0,
            42,
            1_000_000_000.into(),
            500,
            2_000_000_000.into(),
            3_000_000_000.into(),
            4_000_000_000.into(),
        )
    }

    #[rstest]
    fn test_get_schema() {
        let schema = DatabentoStatistics::get_schema(None);
        assert_eq!(schema.fields().len(), 12);
        assert_eq!(schema.field(0).name(), "stat_type");
        assert_eq!(schema.field(11).name(), "ts_init");
    }

    #[rstest]
    fn test_encode_batch() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let metadata = test_metadata();
        let data = vec![test_statistics(instrument_id)];
        let batch = DatabentoStatistics::encode_batch(&metadata, &data).unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 12);
    }

    #[rstest]
    fn test_encode_decode_round_trip() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let metadata = test_metadata();
        let original = vec![test_statistics(instrument_id)];
        let batch = DatabentoStatistics::encode_batch(&metadata, &original).unwrap();
        let decoded = decode_statistics_batch(&metadata, &batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].instrument_id, instrument_id);
        assert_eq!(decoded[0].stat_type, original[0].stat_type);
        assert_eq!(decoded[0].update_action, original[0].update_action);
        assert_eq!(decoded[0].price, original[0].price);
        assert_eq!(decoded[0].quantity, original[0].quantity);
        assert_eq!(decoded[0].channel_id, original[0].channel_id);
        assert_eq!(decoded[0].stat_flags, original[0].stat_flags);
        assert_eq!(decoded[0].sequence, original[0].sequence);
        assert_eq!(decoded[0].ts_ref, original[0].ts_ref);
        assert_eq!(decoded[0].ts_in_delta, original[0].ts_in_delta);
        assert_eq!(decoded[0].ts_event, original[0].ts_event);
        assert_eq!(decoded[0].ts_recv, original[0].ts_recv);
        assert_eq!(decoded[0].ts_init, original[0].ts_init);
    }

    #[rstest]
    fn test_encode_decode_round_trip_with_none_values() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let metadata = test_metadata();
        let stats = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::ClearedVolume,
            DatabentoStatisticUpdateAction::Added,
            None,
            None,
            1,
            0,
            42,
            1_000_000_000.into(),
            500,
            2_000_000_000.into(),
            3_000_000_000.into(),
            4_000_000_000.into(),
        );
        let original = vec![stats];
        let batch = DatabentoStatistics::encode_batch(&metadata, &original).unwrap();
        let decoded = decode_statistics_batch(&metadata, &batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].price, None);
        assert_eq!(decoded[0].quantity, None);
    }

    #[rstest]
    fn test_chunk_metadata_uses_first_non_none_precision() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let none_stats = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::ClearedVolume,
            DatabentoStatisticUpdateAction::Added,
            None,
            None,
            1,
            0,
            42,
            1_000_000_000.into(),
            500,
            2_000_000_000.into(),
            3_000_000_000.into(),
            4_000_000_000.into(),
        );
        let some_stats = test_statistics(instrument_id);
        let data = vec![none_stats, some_stats];

        let batch = statistics_to_arrow_record_batch(&data).unwrap();
        let metadata = batch.schema().metadata().clone();
        let decoded = decode_statistics_batch(&metadata, &batch).unwrap();

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].price, None);
        assert_eq!(decoded[0].quantity, None);
        assert_eq!(decoded[1].price, data[1].price);
        assert_eq!(decoded[1].quantity, data[1].quantity);
    }

    #[rstest]
    fn test_encode_decode_multiple_rows() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let metadata = test_metadata();
        let stats1 = test_statistics(instrument_id);
        let stats2 = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::ClearedVolume,
            DatabentoStatisticUpdateAction::Added,
            Some(Price::from("5100.25")),
            None,
            2,
            1,
            43,
            2_000_000_000.into(),
            600,
            3_000_000_000.into(),
            4_000_000_000.into(),
            5_000_000_000.into(),
        );
        let stats3 = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::OpeningPrice,
            DatabentoStatisticUpdateAction::Added,
            None,
            Some(Quantity::from("200")),
            3,
            0,
            44,
            3_000_000_000.into(),
            700,
            4_000_000_000.into(),
            5_000_000_000.into(),
            6_000_000_000.into(),
        );
        let original = vec![stats1, stats2, stats3];

        let batch = DatabentoStatistics::encode_batch(&metadata, &original).unwrap();
        assert_eq!(batch.num_rows(), 3);

        let decoded = decode_statistics_batch(&metadata, &batch).unwrap();
        assert_eq!(decoded.len(), 3);
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec.instrument_id, orig.instrument_id);
            assert_eq!(dec.stat_type, orig.stat_type);
            assert_eq!(dec.price, orig.price);
            assert_eq!(dec.quantity, orig.quantity);
            assert_eq!(dec.channel_id, orig.channel_id);
            assert_eq!(dec.sequence, orig.sequence);
        }
    }

    #[rstest]
    fn test_statistics_to_arrow_record_batch_round_trip() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let original = vec![test_statistics(instrument_id)];
        let batch = statistics_to_arrow_record_batch(&original).unwrap();
        let metadata = batch.schema().metadata().clone();
        let decoded = decode_statistics_batch(&metadata, &batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].price, original[0].price);
        assert_eq!(decoded[0].quantity, original[0].quantity);
    }

    #[rstest]
    fn test_chunk_metadata_all_none_uses_fixed_precision() {
        use nautilus_model::types::fixed::FIXED_PRECISION;

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let stats = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::ClearedVolume,
            DatabentoStatisticUpdateAction::Added,
            None,
            None,
            1,
            0,
            42,
            1_000_000_000.into(),
            500,
            2_000_000_000.into(),
            3_000_000_000.into(),
            4_000_000_000.into(),
        );
        let data = vec![stats];
        let metadata = DatabentoStatistics::chunk_metadata(&data);

        assert_eq!(
            metadata.get(KEY_PRICE_PRECISION).unwrap(),
            &FIXED_PRECISION.to_string(),
        );
        assert_eq!(
            metadata.get(KEY_SIZE_PRECISION).unwrap(),
            &FIXED_PRECISION.to_string(),
        );
    }

    #[rstest]
    fn test_all_none_metadata_decodes_real_prices_correctly() {
        use nautilus_model::types::fixed::FIXED_PRECISION;

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let price = Price::from("5000.50");
        let quantity = Quantity::from("100");
        let stats = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::OpeningPrice,
            DatabentoStatisticUpdateAction::Added,
            Some(price),
            Some(quantity),
            1,
            0,
            42,
            1_000_000_000.into(),
            500,
            2_000_000_000.into(),
            3_000_000_000.into(),
            4_000_000_000.into(),
        );

        // Encode with FIXED_PRECISION metadata (as if from an all-None chunk)
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), "ESM4.GLBX".to_string()),
            (KEY_PRICE_PRECISION.to_string(), FIXED_PRECISION.to_string()),
            (KEY_SIZE_PRECISION.to_string(), FIXED_PRECISION.to_string()),
        ]);

        let batch = DatabentoStatistics::encode_batch(&metadata, &[stats]).unwrap();
        let decoded = decode_statistics_batch(&metadata, &batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].price.unwrap().as_f64(), price.as_f64());
        assert_eq!(decoded[0].quantity.unwrap().as_f64(), quantity.as_f64());
    }

    #[rstest]
    fn test_get_schema_with_metadata() {
        let metadata = test_metadata();
        let schema = DatabentoStatistics::get_schema(Some(metadata.clone()));
        assert_eq!(schema.metadata(), &metadata);
        assert_eq!(schema.fields().len(), 12);
    }

    #[rstest]
    fn test_decode_missing_metadata_returns_error() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let metadata = test_metadata();
        let data = vec![test_statistics(instrument_id)];
        let batch = DatabentoStatistics::encode_batch(&metadata, &data).unwrap();

        let empty_metadata = HashMap::new();
        let result = decode_statistics_batch(&empty_metadata, &batch);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_statistics_to_arrow_record_batch_empty() {
        let result = statistics_to_arrow_record_batch(&[]);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_decode_data_batch_produces_custom_data() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let metadata = test_metadata();
        let original = vec![test_statistics(instrument_id)];
        let batch = DatabentoStatistics::encode_batch(&metadata, &original).unwrap();
        let data_vec = DatabentoStatistics::decode_data_batch(&metadata, batch).unwrap();

        assert_eq!(data_vec.len(), 1);
        match &data_vec[0] {
            Data::Custom(custom) => {
                assert_eq!(custom.data.type_name(), "DatabentoStatistics");
                let stats = custom
                    .data
                    .as_any()
                    .downcast_ref::<DatabentoStatistics>()
                    .unwrap();
                assert_eq!(stats.instrument_id, instrument_id);
                assert_eq!(stats.stat_type, original[0].stat_type);
                assert_eq!(stats.price, original[0].price);
                assert_eq!(stats.quantity, original[0].quantity);
                assert_eq!(stats.ts_event, original[0].ts_event);
                assert_eq!(stats.ts_init, original[0].ts_init);
            }
            other => panic!("Expected Data::Custom, was {other:?}"),
        }
    }

    #[rstest]
    fn test_decode_data_batch_multiple_rows() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let metadata = test_metadata();
        let stats2 = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::ClearedVolume,
            DatabentoStatisticUpdateAction::Added,
            None,
            Some(Quantity::from("200")),
            2,
            1,
            43,
            2_000_000_000.into(),
            600,
            3_000_000_000.into(),
            4_000_000_000.into(),
            5_000_000_000.into(),
        );
        let original = vec![test_statistics(instrument_id), stats2];
        let batch = DatabentoStatistics::encode_batch(&metadata, &original).unwrap();
        let data_vec = DatabentoStatistics::decode_data_batch(&metadata, batch).unwrap();

        assert_eq!(data_vec.len(), 2);
        for (i, data) in data_vec.iter().enumerate() {
            match data {
                Data::Custom(custom) => {
                    let stats = custom
                        .data
                        .as_any()
                        .downcast_ref::<DatabentoStatistics>()
                        .unwrap();
                    assert_eq!(stats.instrument_id, original[i].instrument_id);
                    assert_eq!(stats.stat_type, original[i].stat_type);
                    assert_eq!(stats.price, original[i].price);
                    assert_eq!(stats.quantity, original[i].quantity);
                }
                other => panic!("Expected Data::Custom, was {other:?}"),
            }
        }
    }

    #[rstest]
    fn test_ipc_stream_round_trip() {
        use std::io::Cursor;

        use arrow::ipc::{reader::StreamReader, writer::StreamWriter};

        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let original = vec![
            test_statistics(instrument_id),
            DatabentoStatistics::new(
                instrument_id,
                DatabentoStatisticType::ClearedVolume,
                DatabentoStatisticUpdateAction::Added,
                None,
                Some(Quantity::from("200")),
                2,
                1,
                43,
                2_000_000_000.into(),
                600,
                3_000_000_000.into(),
                4_000_000_000.into(),
                5_000_000_000.into(),
            ),
        ];
        let batch = statistics_to_arrow_record_batch(&original).unwrap();

        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = StreamWriter::try_new(&mut cursor, &batch.schema()).unwrap();
            writer.write(&batch).unwrap();
            writer.finish().unwrap();
        }

        let buffer = cursor.into_inner();
        let reader = StreamReader::try_new(Cursor::new(buffer), None).unwrap();
        let mut decoded = Vec::new();

        for batch_result in reader {
            let batch = batch_result.unwrap();
            let metadata = batch.schema().metadata().clone();
            decoded.extend(decode_statistics_batch(&metadata, &batch).unwrap());
        }

        assert_eq!(decoded.len(), 2);
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec, orig);
        }
    }
}
