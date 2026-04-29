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
    array::{FixedSizeBinaryArray, FixedSizeBinaryBuilder, Int8Array, UInt8Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{Data, custom::CustomData},
    enums::{FromU8, OrderSide},
    types::fixed::PRECISION_BYTES,
};
use nautilus_serialization::arrow::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch, EncodingError,
    decode_price, decode_quantity, extract_column, validate_precision_bytes,
};

use super::parse_metadata;
use crate::types::DatabentoImbalance;

impl ArrowSchemaProvider for DatabentoImbalance {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new(
                "ref_price",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new(
                "cont_book_clr_price",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new(
                "auct_interest_clr_price",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new(
                "paired_qty",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new(
                "total_imbalance_qty",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new("side", DataType::UInt8, false),
            Field::new("significant_imbalance", DataType::Int8, false),
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

impl EncodeToRecordBatch for DatabentoImbalance {
    #[expect(clippy::unnecessary_cast)] // c_char is u8 on some targets
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut ref_price_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut cont_book_clr_price_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut auct_interest_clr_price_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut paired_qty_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut total_imbalance_qty_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut side_builder = UInt8Array::builder(data.len());
        let mut significant_imbalance_builder = Int8Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_recv_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for item in data {
            ref_price_builder
                .append_value(item.ref_price.raw.to_le_bytes())
                .unwrap();
            cont_book_clr_price_builder
                .append_value(item.cont_book_clr_price.raw.to_le_bytes())
                .unwrap();
            auct_interest_clr_price_builder
                .append_value(item.auct_interest_clr_price.raw.to_le_bytes())
                .unwrap();
            paired_qty_builder
                .append_value(item.paired_qty.raw.to_le_bytes())
                .unwrap();
            total_imbalance_qty_builder
                .append_value(item.total_imbalance_qty.raw.to_le_bytes())
                .unwrap();
            side_builder.append_value(item.side as u8);
            significant_imbalance_builder.append_value(item.significant_imbalance as i8);
            ts_event_builder.append_value(item.ts_event.as_u64());
            ts_recv_builder.append_value(item.ts_recv.as_u64());
            ts_init_builder.append_value(item.ts_init.as_u64());
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(ref_price_builder.finish()),
                Arc::new(cont_book_clr_price_builder.finish()),
                Arc::new(auct_interest_clr_price_builder.finish()),
                Arc::new(paired_qty_builder.finish()),
                Arc::new(total_imbalance_qty_builder.finish()),
                Arc::new(side_builder.finish()),
                Arc::new(significant_imbalance_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_recv_builder.finish()),
                Arc::new(ts_init_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        Self::get_metadata(
            &self.instrument_id,
            self.ref_price.precision,
            self.paired_qty.precision,
        )
    }
}

impl DecodeDataFromRecordBatch for DatabentoImbalance {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let items = decode_imbalance_batch(metadata, &record_batch)?;
        Ok(items
            .into_iter()
            .map(|item| Data::Custom(CustomData::from_arc(Arc::new(item))))
            .collect())
    }
}

/// Decodes a `RecordBatch` into a vector of [`DatabentoImbalance`].
///
/// # Errors
///
/// Returns an `EncodingError` if decoding fails.
pub fn decode_imbalance_batch(
    metadata: &HashMap<String, String>,
    record_batch: &RecordBatch,
) -> Result<Vec<DatabentoImbalance>, EncodingError> {
    let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
    let cols = record_batch.columns();

    let ref_price_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "ref_price",
        0,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let cont_book_clr_price_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "cont_book_clr_price",
        1,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let auct_interest_clr_price_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "auct_interest_clr_price",
        2,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let paired_qty_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "paired_qty",
        3,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let total_imbalance_qty_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "total_imbalance_qty",
        4,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let side_values = extract_column::<UInt8Array>(cols, "side", 5, DataType::UInt8)?;
    let significant_imbalance_values =
        extract_column::<Int8Array>(cols, "significant_imbalance", 6, DataType::Int8)?;
    let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 7, DataType::UInt64)?;
    let ts_recv_values = extract_column::<UInt64Array>(cols, "ts_recv", 8, DataType::UInt64)?;
    let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 9, DataType::UInt64)?;

    validate_precision_bytes(ref_price_values, "ref_price")?;
    validate_precision_bytes(cont_book_clr_price_values, "cont_book_clr_price")?;
    validate_precision_bytes(auct_interest_clr_price_values, "auct_interest_clr_price")?;
    validate_precision_bytes(paired_qty_values, "paired_qty")?;
    validate_precision_bytes(total_imbalance_qty_values, "total_imbalance_qty")?;

    (0..record_batch.num_rows())
        .map(|row| {
            let ref_price = decode_price(
                ref_price_values.value(row),
                price_precision,
                "ref_price",
                row,
            )?;
            let cont_book_clr_price = decode_price(
                cont_book_clr_price_values.value(row),
                price_precision,
                "cont_book_clr_price",
                row,
            )?;
            let auct_interest_clr_price = decode_price(
                auct_interest_clr_price_values.value(row),
                price_precision,
                "auct_interest_clr_price",
                row,
            )?;
            let paired_qty = decode_quantity(
                paired_qty_values.value(row),
                size_precision,
                "paired_qty",
                row,
            )?;
            let total_imbalance_qty = decode_quantity(
                total_imbalance_qty_values.value(row),
                size_precision,
                "total_imbalance_qty",
                row,
            )?;
            let side_value = side_values.value(row);
            let side = OrderSide::from_u8(side_value).ok_or_else(|| {
                EncodingError::ParseError(
                    stringify!(OrderSide),
                    format!("Invalid enum value, was {side_value}"),
                )
            })?;
            let significant_imbalance = significant_imbalance_values.value(row) as std::ffi::c_char;

            Ok(DatabentoImbalance {
                instrument_id,
                ref_price,
                cont_book_clr_price,
                auct_interest_clr_price,
                paired_qty,
                total_imbalance_qty,
                side,
                significant_imbalance,
                ts_event: ts_event_values.value(row).into(),
                ts_recv: ts_recv_values.value(row).into(),
                ts_init: ts_init_values.value(row).into(),
            })
        })
        .collect()
}

/// Encodes a vector of [`DatabentoImbalance`] into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if `data` is empty or encoding fails.
// Guarded by empty check
pub fn imbalance_to_arrow_record_batch(
    data: &[DatabentoImbalance],
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    let metadata = DatabentoImbalance::chunk_metadata(data);
    DatabentoImbalance::encode_batch(&metadata, data).map_err(EncodingError::ArrowError)
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::OrderSide,
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
            (KEY_INSTRUMENT_ID.to_string(), "AAPL.XNAS".to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ])
    }

    fn test_imbalance(instrument_id: InstrumentId) -> DatabentoImbalance {
        DatabentoImbalance::new(
            instrument_id,
            Price::from("100.50"),
            Price::from("100.45"),
            Price::from("100.55"),
            Quantity::from("1000"),
            Quantity::from("500"),
            OrderSide::Buy,
            b'Y' as std::ffi::c_char,
            1.into(),
            2.into(),
            3.into(),
        )
    }

    #[rstest]
    fn test_get_schema() {
        let schema = DatabentoImbalance::get_schema(None);
        assert_eq!(schema.fields().len(), 10);
        assert_eq!(schema.field(0).name(), "ref_price");
        assert_eq!(schema.field(5).name(), "side");
        assert_eq!(schema.field(9).name(), "ts_init");
    }

    #[rstest]
    fn test_encode_batch() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = test_metadata();
        let data = vec![test_imbalance(instrument_id)];
        let batch = DatabentoImbalance::encode_batch(&metadata, &data).unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 10);
    }

    #[rstest]
    fn test_encode_decode_round_trip() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = test_metadata();
        let original = vec![test_imbalance(instrument_id)];
        let batch = DatabentoImbalance::encode_batch(&metadata, &original).unwrap();
        let decoded = decode_imbalance_batch(&metadata, &batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].instrument_id, instrument_id);
        assert_eq!(decoded[0].ref_price, original[0].ref_price);
        assert_eq!(
            decoded[0].cont_book_clr_price,
            original[0].cont_book_clr_price
        );
        assert_eq!(
            decoded[0].auct_interest_clr_price,
            original[0].auct_interest_clr_price
        );
        assert_eq!(decoded[0].paired_qty, original[0].paired_qty);
        assert_eq!(
            decoded[0].total_imbalance_qty,
            original[0].total_imbalance_qty
        );
        assert_eq!(decoded[0].side, original[0].side);
        assert_eq!(
            decoded[0].significant_imbalance,
            original[0].significant_imbalance
        );
        assert_eq!(decoded[0].ts_event, original[0].ts_event);
        assert_eq!(decoded[0].ts_recv, original[0].ts_recv);
        assert_eq!(decoded[0].ts_init, original[0].ts_init);
    }

    #[rstest]
    fn test_encode_decode_multiple_rows() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = test_metadata();
        let imb1 = test_imbalance(instrument_id);
        let mut imb2 = test_imbalance(instrument_id);
        imb2.side = OrderSide::Sell;
        imb2.ref_price = Price::from("101.00");
        imb2.ts_event = 100.into();
        let mut imb3 = test_imbalance(instrument_id);
        imb3.side = OrderSide::NoOrderSide;
        imb3.significant_imbalance = b'N' as std::ffi::c_char;
        let original = vec![imb1, imb2, imb3];

        let batch = DatabentoImbalance::encode_batch(&metadata, &original).unwrap();
        assert_eq!(batch.num_rows(), 3);

        let decoded = decode_imbalance_batch(&metadata, &batch).unwrap();
        assert_eq!(decoded.len(), 3);
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec.instrument_id, orig.instrument_id);
            assert_eq!(dec.ref_price, orig.ref_price);
            assert_eq!(dec.side, orig.side);
            assert_eq!(dec.significant_imbalance, orig.significant_imbalance);
            assert_eq!(dec.ts_event, orig.ts_event);
        }
    }

    #[rstest]
    fn test_imbalance_to_arrow_record_batch_round_trip() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let original = vec![test_imbalance(instrument_id)];
        let batch = imbalance_to_arrow_record_batch(&original).unwrap();
        let metadata = batch.schema().metadata().clone();
        let decoded = decode_imbalance_batch(&metadata, &batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].ref_price, original[0].ref_price);
        assert_eq!(decoded[0].paired_qty, original[0].paired_qty);
    }

    #[rstest]
    fn test_get_schema_with_metadata() {
        let metadata = test_metadata();
        let schema = DatabentoImbalance::get_schema(Some(metadata.clone()));
        assert_eq!(schema.metadata(), &metadata);
        assert_eq!(schema.fields().len(), 10);
    }

    #[rstest]
    fn test_imbalance_to_arrow_record_batch_empty() {
        let result = imbalance_to_arrow_record_batch(&[]);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_decode_missing_metadata_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = test_metadata();
        let data = vec![test_imbalance(instrument_id)];
        let batch = DatabentoImbalance::encode_batch(&metadata, &data).unwrap();

        let empty_metadata = HashMap::new();
        let result = decode_imbalance_batch(&empty_metadata, &batch);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_decode_data_batch_produces_custom_data() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = test_metadata();
        let original = vec![test_imbalance(instrument_id)];
        let batch = DatabentoImbalance::encode_batch(&metadata, &original).unwrap();
        let data_vec = DatabentoImbalance::decode_data_batch(&metadata, batch).unwrap();

        assert_eq!(data_vec.len(), 1);
        match &data_vec[0] {
            Data::Custom(custom) => {
                assert_eq!(custom.data.type_name(), "DatabentoImbalance");
                let imbalance = custom
                    .data
                    .as_any()
                    .downcast_ref::<DatabentoImbalance>()
                    .unwrap();
                assert_eq!(imbalance.instrument_id, instrument_id);
                assert_eq!(imbalance.ref_price, original[0].ref_price);
                assert_eq!(imbalance.paired_qty, original[0].paired_qty);
                assert_eq!(imbalance.side, original[0].side);
                assert_eq!(imbalance.ts_event, original[0].ts_event);
                assert_eq!(imbalance.ts_init, original[0].ts_init);
            }
            other => panic!("Expected Data::Custom, was {other:?}"),
        }
    }

    #[rstest]
    fn test_decode_data_batch_multiple_rows() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = test_metadata();
        let mut imb2 = test_imbalance(instrument_id);
        imb2.side = OrderSide::Sell;
        imb2.ts_event = 100.into();
        let original = vec![test_imbalance(instrument_id), imb2];
        let batch = DatabentoImbalance::encode_batch(&metadata, &original).unwrap();
        let data_vec = DatabentoImbalance::decode_data_batch(&metadata, batch).unwrap();

        assert_eq!(data_vec.len(), 2);
        for (i, data) in data_vec.iter().enumerate() {
            match data {
                Data::Custom(custom) => {
                    let imbalance = custom
                        .data
                        .as_any()
                        .downcast_ref::<DatabentoImbalance>()
                        .unwrap();
                    assert_eq!(imbalance.instrument_id, original[i].instrument_id);
                    assert_eq!(imbalance.side, original[i].side);
                    assert_eq!(imbalance.ts_event, original[i].ts_event);
                }
                other => panic!("Expected Data::Custom, was {other:?}"),
            }
        }
    }

    #[rstest]
    fn test_ipc_stream_round_trip() {
        use std::io::Cursor;

        use arrow::ipc::{reader::StreamReader, writer::StreamWriter};

        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let original = vec![test_imbalance(instrument_id), {
            let mut imb = test_imbalance(instrument_id);
            imb.side = OrderSide::Sell;
            imb.ref_price = Price::from("101.25");
            imb.ts_event = 100.into();
            imb
        }];
        let batch = imbalance_to_arrow_record_batch(&original).unwrap();

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
            decoded.extend(decode_imbalance_batch(&metadata, &batch).unwrap());
        }

        assert_eq!(decoded.len(), 2);
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec, orig);
        }
    }
}
