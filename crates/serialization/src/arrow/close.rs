// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::HashMap, str::FromStr, sync::Arc};

use arrow::{
    array::{FixedSizeBinaryArray, FixedSizeBinaryBuilder, UInt8Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::close::InstrumentClose,
    enums::{FromU8, InstrumentCloseType},
    identifiers::InstrumentId,
    types::{Price, fixed::PRECISION_BYTES},
};

use super::{
    DecodeDataFromRecordBatch, EncodingError, KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION,
    extract_column,
};
use crate::arrow::{
    ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch, get_raw_price,
};

impl ArrowSchemaProvider for InstrumentClose {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new(
                "close_price",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new("close_type", DataType::UInt8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

fn parse_metadata(metadata: &HashMap<String, String>) -> Result<(InstrumentId, u8), EncodingError> {
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

    Ok((instrument_id, price_precision))
}

impl EncodeToRecordBatch for InstrumentClose {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut close_price_builder =
            FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut close_type_builder = UInt8Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for item in data {
            close_price_builder
                .append_value(item.close_price.raw.to_le_bytes())
                .unwrap();
            close_type_builder.append_value(item.close_type as u8);
            ts_event_builder.append_value(item.ts_event.as_u64());
            ts_init_builder.append_value(item.ts_init.as_u64());
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(close_price_builder.finish()),
                Arc::new(close_type_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        InstrumentClose::get_metadata(&self.instrument_id, self.close_price.precision)
    }
}

impl DecodeFromRecordBatch for InstrumentClose {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision) = parse_metadata(metadata)?;
        let cols = record_batch.columns();

        let close_price_values = extract_column::<FixedSizeBinaryArray>(
            cols,
            "close_price",
            0,
            DataType::FixedSizeBinary(PRECISION_BYTES),
        )?;
        let close_type_values =
            extract_column::<UInt8Array>(cols, "close_type", 1, DataType::UInt8)?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 2, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 3, DataType::UInt64)?;

        assert_eq!(
            close_price_values.value_length(),
            PRECISION_BYTES,
            "Price precision uses {PRECISION_BYTES} byte value"
        );

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|row| {
                let close_type_value = close_type_values.value(row);
                let close_type =
                    InstrumentCloseType::from_u8(close_type_value).ok_or_else(|| {
                        EncodingError::ParseError(
                            stringify!(InstrumentCloseType),
                            format!("Invalid enum value, was {close_type_value}"),
                        )
                    })?;

                Ok(Self {
                    instrument_id,
                    close_price: Price::from_raw(
                        get_raw_price(close_price_values.value(row)),
                        price_precision,
                    ),
                    close_type,
                    ts_event: ts_event_values.value(row).into(),
                    ts_init: ts_init_values.value(row).into(),
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for InstrumentClose {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let items: Vec<Self> = Self::decode_batch(metadata, record_batch)?;
        Ok(items.into_iter().map(Data::from).collect())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{array::Array, record_batch::RecordBatch};
    use nautilus_model::{
        enums::InstrumentCloseType,
        types::{fixed::FIXED_SCALAR, price::PriceRaw},
    };
    use rstest::rstest;

    use super::*;
    use crate::arrow::get_raw_price;

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);
        let schema = InstrumentClose::get_schema(Some(metadata.clone()));

        let expected_fields = vec![
            Field::new(
                "close_price",
                DataType::FixedSizeBinary(PRECISION_BYTES),
                false,
            ),
            Field::new("close_type", DataType::UInt8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = InstrumentClose::get_schema_map();
        let mut expected_map = HashMap::new();

        let fixed_size_binary = format!("FixedSizeBinary({PRECISION_BYTES})");
        expected_map.insert("close_price".to_string(), fixed_size_binary);
        expected_map.insert("close_type".to_string(), "UInt8".to_string());
        expected_map.insert("ts_event".to_string(), "UInt64".to_string());
        expected_map.insert("ts_init".to_string(), "UInt64".to_string());
        assert_eq!(schema_map, expected_map);
    }

    #[rstest]
    fn test_encode_batch() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let close1 = InstrumentClose {
            instrument_id,
            close_price: Price::from("150.50"),
            close_type: InstrumentCloseType::EndOfSession,
            ts_event: 1.into(),
            ts_init: 3.into(),
        };

        let close2 = InstrumentClose {
            instrument_id,
            close_price: Price::from("151.25"),
            close_type: InstrumentCloseType::ContractExpired,
            ts_event: 2.into(),
            ts_init: 4.into(),
        };

        let data = vec![close1, close2];
        let record_batch = InstrumentClose::encode_batch(&metadata, &data).unwrap();

        let columns = record_batch.columns();
        let close_price_values = columns[0]
            .as_any()
            .downcast_ref::<FixedSizeBinaryArray>()
            .unwrap();
        let close_type_values = columns[1].as_any().downcast_ref::<UInt8Array>().unwrap();
        let ts_event_values = columns[2].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = columns[3].as_any().downcast_ref::<UInt64Array>().unwrap();

        assert_eq!(columns.len(), 4);
        assert_eq!(close_price_values.len(), 2);
        assert_eq!(
            get_raw_price(close_price_values.value(0)),
            (150.50 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(close_price_values.value(1)),
            (151.25 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(close_type_values.len(), 2);
        assert_eq!(
            close_type_values.value(0),
            InstrumentCloseType::EndOfSession as u8
        );
        assert_eq!(
            close_type_values.value(1),
            InstrumentCloseType::ContractExpired as u8
        );
        assert_eq!(ts_event_values.len(), 2);
        assert_eq!(ts_event_values.value(0), 1);
        assert_eq!(ts_event_values.value(1), 2);
        assert_eq!(ts_init_values.len(), 2);
        assert_eq!(ts_init_values.value(0), 3);
        assert_eq!(ts_init_values.value(1), 4);
    }

    #[rstest]
    fn test_decode_batch() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let close_price = FixedSizeBinaryArray::from(vec![
            &(15050 as PriceRaw).to_le_bytes(),
            &(15125 as PriceRaw).to_le_bytes(),
        ]);
        let close_type = UInt8Array::from(vec![
            InstrumentCloseType::EndOfSession as u8,
            InstrumentCloseType::ContractExpired as u8,
        ]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = RecordBatch::try_new(
            InstrumentClose::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(close_price),
                Arc::new(close_type),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let decoded_data = InstrumentClose::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded_data.len(), 2);
        assert_eq!(decoded_data[0].instrument_id, instrument_id);
        assert_eq!(decoded_data[0].close_price, Price::from_raw(15050, 2));
        assert_eq!(
            decoded_data[0].close_type,
            InstrumentCloseType::EndOfSession
        );
        assert_eq!(decoded_data[0].ts_event.as_u64(), 1);
        assert_eq!(decoded_data[0].ts_init.as_u64(), 3);

        assert_eq!(decoded_data[1].instrument_id, instrument_id);
        assert_eq!(decoded_data[1].close_price, Price::from_raw(15125, 2));
        assert_eq!(
            decoded_data[1].close_type,
            InstrumentCloseType::ContractExpired
        );
        assert_eq!(decoded_data[1].ts_event.as_u64(), 2);
        assert_eq!(decoded_data[1].ts_init.as_u64(), 4);
    }
}
