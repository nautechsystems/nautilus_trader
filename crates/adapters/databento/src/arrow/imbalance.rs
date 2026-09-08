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
    array::{Decimal128Array, Int8Array, TimestampNanosecondArray},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{Data, custom::CustomData},
    enums::OrderSide,
};
use nautilus_serialization::arrow::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch, EncodingError,
    decode_decimal_price, decode_decimal_quantity, decode_timestamp, enum_dictionary_array,
    enum_dictionary_data_type, extract_column, fixed_decimal_data_type, price_decimal_array,
    quantity_decimal_array, timestamp_array, timestamp_data_type,
};

use super::{EnumColumn, parse_metadata};
use crate::types::DatabentoImbalance;

impl ArrowSchemaProvider for DatabentoImbalance {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("ref_price", fixed_decimal_data_type(), false),
            Field::new("cont_book_clr_price", fixed_decimal_data_type(), false),
            Field::new("auct_interest_clr_price", fixed_decimal_data_type(), false),
            Field::new("paired_qty", fixed_decimal_data_type(), false),
            Field::new("total_imbalance_qty", fixed_decimal_data_type(), false),
            Field::new("side", enum_dictionary_data_type(), false),
            Field::new("significant_imbalance", DataType::Int8, false),
            Field::new("ts_event", timestamp_data_type(), false),
            Field::new("ts_recv", timestamp_data_type(), false),
            Field::new("ts_init", timestamp_data_type(), false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for DatabentoImbalance {
    #[expect(clippy::unnecessary_cast)] // c_char is u8 on some targets
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: std::borrow::Borrow<Self>,
    {
        let mut significant_imbalance_builder = Int8Array::builder(data.len());

        for item in data.iter().map(std::borrow::Borrow::borrow) {
            significant_imbalance_builder.append_value(item.significant_imbalance as i8);
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(price_decimal_array(
                    data.iter().map(|item| item.borrow().ref_price.raw),
                    "ref_price",
                )?),
                Arc::new(price_decimal_array(
                    data.iter()
                        .map(|item| item.borrow().cont_book_clr_price.raw),
                    "cont_book_clr_price",
                )?),
                Arc::new(price_decimal_array(
                    data.iter()
                        .map(|item| item.borrow().auct_interest_clr_price.raw),
                    "auct_interest_clr_price",
                )?),
                Arc::new(quantity_decimal_array(
                    data.iter().map(|item| item.borrow().paired_qty.raw),
                    "paired_qty",
                )?),
                Arc::new(quantity_decimal_array(
                    data.iter()
                        .map(|item| item.borrow().total_imbalance_qty.raw),
                    "total_imbalance_qty",
                )?),
                Arc::new(enum_dictionary_array(data.iter().map(|item| {
                    item.borrow()
                        .side
                        .map_or_else(|| "NO_ORDER_SIDE".to_string(), |side| side.to_string())
                }))?),
                Arc::new(significant_imbalance_builder.finish()),
                Arc::new(timestamp_array(
                    data.iter().map(|item| item.borrow().ts_event.as_u64()),
                )?),
                Arc::new(timestamp_array(
                    data.iter().map(|item| item.borrow().ts_recv.as_u64()),
                )?),
                Arc::new(timestamp_array(
                    data.iter().map(|item| item.borrow().ts_init.as_u64()),
                )?),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = Self::get_metadata(
            &self.instrument_id,
            self.ref_price.precision,
            self.paired_qty.precision,
        );
        metadata.insert("type_name".to_string(), "DatabentoImbalance".to_string());
        metadata
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

    let decimal_type = fixed_decimal_data_type();
    let ref_price_values =
        extract_column::<Decimal128Array>(cols, "ref_price", 0, decimal_type.clone())?;
    let cont_book_clr_price_values =
        extract_column::<Decimal128Array>(cols, "cont_book_clr_price", 1, decimal_type.clone())?;
    let auct_interest_clr_price_values = extract_column::<Decimal128Array>(
        cols,
        "auct_interest_clr_price",
        2,
        decimal_type.clone(),
    )?;
    let paired_qty_values =
        extract_column::<Decimal128Array>(cols, "paired_qty", 3, decimal_type.clone())?;
    let total_imbalance_qty_values =
        extract_column::<Decimal128Array>(cols, "total_imbalance_qty", 4, decimal_type)?;
    let significant_imbalance_values =
        extract_column::<Int8Array>(cols, "significant_imbalance", 6, DataType::Int8)?;
    let side_column = EnumColumn::try_from_column(&cols[5], "side", 5)?;
    let ts_event_values =
        extract_column::<TimestampNanosecondArray>(cols, "ts_event", 7, timestamp_data_type())?;
    let ts_recv_values =
        extract_column::<TimestampNanosecondArray>(cols, "ts_recv", 8, timestamp_data_type())?;
    let ts_init_values =
        extract_column::<TimestampNanosecondArray>(cols, "ts_init", 9, timestamp_data_type())?;

    (0..record_batch.num_rows())
        .map(|row| {
            let ref_price =
                decode_decimal_price(ref_price_values, price_precision, "ref_price", row)?;
            let cont_book_clr_price = decode_decimal_price(
                cont_book_clr_price_values,
                price_precision,
                "cont_book_clr_price",
                row,
            )?;
            let auct_interest_clr_price = decode_decimal_price(
                auct_interest_clr_price_values,
                price_precision,
                "auct_interest_clr_price",
                row,
            )?;
            let paired_qty =
                decode_decimal_quantity(paired_qty_values, size_precision, "paired_qty", row)?;
            let total_imbalance_qty = decode_decimal_quantity(
                total_imbalance_qty_values,
                size_precision,
                "total_imbalance_qty",
                row,
            )?;
            let side = side_column.decode_optional(row, "NO_ORDER_SIDE", |value| match value {
                1 => Some(OrderSide::Buy),
                2 => Some(OrderSide::Sell),
                _ => None,
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
                ts_event: decode_timestamp(ts_event_values, "ts_event", row)?.into(),
                ts_recv: decode_timestamp(ts_recv_values, "ts_recv", row)?.into(),
                ts_init: decode_timestamp(ts_init_values, "ts_init", row)?.into(),
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
    use arrow::array::UInt8Array;
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
            Some(OrderSide::Buy),
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
        assert_eq!(schema.field(0).data_type(), &fixed_decimal_data_type());
        assert_eq!(schema.field(5).data_type(), &enum_dictionary_data_type());
        assert_eq!(schema.field(8).data_type(), &timestamp_data_type());
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
    fn test_decode_legacy_side_column() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = test_metadata();
        let original = test_imbalance(instrument_id);
        let batch =
            DatabentoImbalance::encode_batch(&metadata, std::slice::from_ref(&original)).unwrap();
        let mut fields = batch.schema().fields().to_vec();
        fields[5] = Arc::new(Field::new("side", DataType::UInt8, false));
        let mut columns = batch.columns().to_vec();
        columns[5] = Arc::new(UInt8Array::from(vec![
            original.side.map_or(0, |side| side as u8),
        ]));
        let legacy_batch = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(fields, metadata.clone())),
            columns,
        )
        .unwrap();

        let decoded = decode_imbalance_batch(&metadata, &legacy_batch).unwrap();

        assert_eq!(decoded, vec![original]);
    }

    #[rstest]
    fn test_encode_decode_multiple_rows() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = test_metadata();
        let imb1 = test_imbalance(instrument_id);
        let mut imb2 = test_imbalance(instrument_id);
        imb2.side = Some(OrderSide::Sell);
        imb2.ref_price = Price::from("101.00");
        imb2.ts_event = 100.into();
        let mut imb3 = test_imbalance(instrument_id);
        imb3.side = None;
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
        imb2.side = Some(OrderSide::Sell);
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
            imb.side = Some(OrderSide::Sell);
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
