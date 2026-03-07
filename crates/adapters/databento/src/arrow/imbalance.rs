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

//! Apache Arrow schema definition and encoding/decoding for [`DatabentoImbalance`].

use std::{collections::HashMap, ffi::c_char, str::FromStr, sync::Arc};

use arrow::{
    array::{FixedSizeBinaryArray, FixedSizeBinaryBuilder, Int8Array, UInt64Array, UInt8Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    enums::{FromU8, OrderSide},
    identifiers::InstrumentId,
    types::{Price, Quantity, fixed::PRECISION_BYTES},
};
use nautilus_serialization::arrow::{ArrowSchemaProvider, EncodeToRecordBatch, EncodingError};

use crate::types::DatabentoImbalance;

// Metadata keys
const KEY_INSTRUMENT_ID: &str = "instrument_id";
const KEY_PRICE_PRECISION: &str = "price_precision";
const KEY_SIZE_PRECISION: &str = "size_precision";

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

impl DatabentoImbalance {
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

impl EncodeToRecordBatch for DatabentoImbalance {
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

/// Decodes a `RecordBatch` into a vector of `DatabentoImbalance`.
///
/// # Errors
///
/// Returns an `EncodingError` if the decoding fails.
pub fn decode_imbalance_batch(
    metadata: &HashMap<String, String>,
    record_batch: RecordBatch,
) -> Result<Vec<DatabentoImbalance>, EncodingError> {
    let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
    let cols = record_batch.columns();

    let ref_price_values = cols[0]
        .as_any()
        .downcast_ref::<FixedSizeBinaryArray>()
        .ok_or(EncodingError::InvalidColumnType(
            "ref_price",
            0,
            DataType::FixedSizeBinary(PRECISION_BYTES),
            cols[0].data_type().clone(),
        ))?;
    let cont_book_clr_price_values = cols[1]
        .as_any()
        .downcast_ref::<FixedSizeBinaryArray>()
        .ok_or(EncodingError::InvalidColumnType(
            "cont_book_clr_price",
            1,
            DataType::FixedSizeBinary(PRECISION_BYTES),
            cols[1].data_type().clone(),
        ))?;
    let auct_interest_clr_price_values = cols[2]
        .as_any()
        .downcast_ref::<FixedSizeBinaryArray>()
        .ok_or(EncodingError::InvalidColumnType(
            "auct_interest_clr_price",
            2,
            DataType::FixedSizeBinary(PRECISION_BYTES),
            cols[2].data_type().clone(),
        ))?;
    let paired_qty_values = cols[3]
        .as_any()
        .downcast_ref::<FixedSizeBinaryArray>()
        .ok_or(EncodingError::InvalidColumnType(
            "paired_qty",
            3,
            DataType::FixedSizeBinary(PRECISION_BYTES),
            cols[3].data_type().clone(),
        ))?;
    let total_imbalance_qty_values = cols[4]
        .as_any()
        .downcast_ref::<FixedSizeBinaryArray>()
        .ok_or(EncodingError::InvalidColumnType(
            "total_imbalance_qty",
            4,
            DataType::FixedSizeBinary(PRECISION_BYTES),
            cols[4].data_type().clone(),
        ))?;
    let side_values = cols[5]
        .as_any()
        .downcast_ref::<UInt8Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "side",
            5,
            DataType::UInt8,
            cols[5].data_type().clone(),
        ))?;
    let significant_imbalance_values = cols[6]
        .as_any()
        .downcast_ref::<Int8Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "significant_imbalance",
            6,
            DataType::Int8,
            cols[6].data_type().clone(),
        ))?;
    let ts_event_values = cols[7]
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "ts_event",
            7,
            DataType::UInt64,
            cols[7].data_type().clone(),
        ))?;
    let ts_recv_values = cols[8]
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "ts_recv",
            8,
            DataType::UInt64,
            cols[8].data_type().clone(),
        ))?;
    let ts_init_values = cols[9]
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or(EncodingError::InvalidColumnType(
            "ts_init",
            9,
            DataType::UInt64,
            cols[9].data_type().clone(),
        ))?;

    let result: Result<Vec<DatabentoImbalance>, EncodingError> = (0..record_batch.num_rows())
        .map(|row| {
            let ref_price_bytes = ref_price_values.value(row);
            let ref_price_raw = i128::from_le_bytes(
                ref_price_bytes
                    .try_into()
                    .map_err(|_| EncodingError::ParseError("ref_price", "Invalid bytes".to_string()))?,
            );
            let ref_price = Price::from_raw(ref_price_raw, price_precision);

            let cont_book_clr_price_bytes = cont_book_clr_price_values.value(row);
            let cont_book_clr_price_raw =
                i128::from_le_bytes(cont_book_clr_price_bytes.try_into().map_err(|_| {
                    EncodingError::ParseError("cont_book_clr_price", "Invalid bytes".to_string())
                })?);
            let cont_book_clr_price = Price::from_raw(cont_book_clr_price_raw, price_precision);

            let auct_interest_clr_price_bytes = auct_interest_clr_price_values.value(row);
            let auct_interest_clr_price_raw = i128::from_le_bytes(
                auct_interest_clr_price_bytes.try_into().map_err(|_| {
                    EncodingError::ParseError(
                        "auct_interest_clr_price",
                        "Invalid bytes".to_string(),
                    )
                })?,
            );
            let auct_interest_clr_price =
                Price::from_raw(auct_interest_clr_price_raw, price_precision);

            let paired_qty_bytes = paired_qty_values.value(row);
            let paired_qty_raw = u128::from_le_bytes(
                paired_qty_bytes
                    .try_into()
                    .map_err(|_| EncodingError::ParseError("paired_qty", "Invalid bytes".to_string()))?,
            );
            let paired_qty = Quantity::from_raw(paired_qty_raw, size_precision);

            let total_imbalance_qty_bytes = total_imbalance_qty_values.value(row);
            let total_imbalance_qty_raw =
                u128::from_le_bytes(total_imbalance_qty_bytes.try_into().map_err(|_| {
                    EncodingError::ParseError("total_imbalance_qty", "Invalid bytes".to_string())
                })?);
            let total_imbalance_qty = Quantity::from_raw(total_imbalance_qty_raw, size_precision);

            let side_value = side_values.value(row);
            let side = OrderSide::from_u8(side_value).ok_or_else(|| {
                EncodingError::ParseError("OrderSide", format!("Invalid enum value: {side_value}"))
            })?;

            let significant_imbalance = significant_imbalance_values.value(row) as c_char;

            Ok(DatabentoImbalance::new(
                instrument_id,
                ref_price,
                cont_book_clr_price,
                auct_interest_clr_price,
                paired_qty,
                total_imbalance_qty,
                side,
                significant_imbalance,
                ts_event_values.value(row).into(),
                ts_recv_values.value(row).into(),
                ts_init_values.value(row).into(),
            ))
        })
        .collect();

    result
}

/// Converts a vector of `DatabentoImbalance` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[allow(clippy::missing_panics_doc)] // Guarded by empty check
pub fn imbalance_to_arrow_record_batch_bytes(
    data: Vec<DatabentoImbalance>,
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    // Extract metadata from first element
    let first = data.first().unwrap();
    let metadata = first.metadata();
    DatabentoImbalance::encode_batch(&metadata, &data).map_err(EncodingError::ArrowError)
}

#[cfg(test)]
mod tests {
    use arrow::array::Array;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ]);
        let schema = DatabentoImbalance::get_schema(Some(metadata.clone()));

        assert_eq!(schema.fields().len(), 10);
        assert_eq!(schema.field(0).name(), "ref_price");
        assert_eq!(schema.field(1).name(), "cont_book_clr_price");
        assert_eq!(schema.field(2).name(), "auct_interest_clr_price");
        assert_eq!(schema.field(3).name(), "paired_qty");
        assert_eq!(schema.field(4).name(), "total_imbalance_qty");
        assert_eq!(schema.field(5).name(), "side");
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = DatabentoImbalance::get_schema_map();
        assert!(schema_map.contains_key("ref_price"));
        assert!(schema_map.contains_key("cont_book_clr_price"));
        assert!(schema_map.contains_key("paired_qty"));
        assert!(schema_map.contains_key("side"));
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

        let imbalance1 = DatabentoImbalance::new(
            instrument_id,
            Price::from("150.50"),
            Price::from("150.51"),
            Price::from("150.52"),
            Quantity::from(1000),
            Quantity::from(500),
            OrderSide::Buy,
            b' ' as c_char,
            1.into(),
            2.into(),
            3.into(),
        );

        let imbalance2 = DatabentoImbalance::new(
            instrument_id,
            Price::from("151.00"),
            Price::from("151.01"),
            Price::from("151.02"),
            Quantity::from(2000),
            Quantity::from(1000),
            OrderSide::Sell,
            b'Y' as c_char,
            2.into(),
            3.into(),
            4.into(),
        );

        let data = vec![imbalance1, imbalance2];
        let record_batch = DatabentoImbalance::encode_batch(&metadata, &data).unwrap();

        assert_eq!(record_batch.num_columns(), 10);
        assert_eq!(record_batch.num_rows(), 2);

        let side_values = record_batch.columns()[5]
            .as_any()
            .downcast_ref::<UInt8Array>()
            .unwrap();
        assert_eq!(side_values.value(0), OrderSide::Buy as u8);
        assert_eq!(side_values.value(1), OrderSide::Sell as u8);
    }

    #[rstest]
    fn test_decode_batch() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ]);

        let imbalance = DatabentoImbalance::new(
            instrument_id,
            Price::from("150.50"),
            Price::from("150.51"),
            Price::from("150.52"),
            Quantity::from(1000),
            Quantity::from(500),
            OrderSide::Buy,
            b' ' as c_char,
            1.into(),
            2.into(),
            3.into(),
        );

        let data = vec![imbalance];
        let record_batch = DatabentoImbalance::encode_batch(&metadata, &data).unwrap();
        let decoded = decode_imbalance_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].instrument_id, instrument_id);
        assert_eq!(decoded[0].side, OrderSide::Buy);
    }

    #[rstest]
    fn test_encode_decode_round_trip() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ]);

        let imbalance1 = DatabentoImbalance::new(
            instrument_id,
            Price::from("150.50"),
            Price::from("150.51"),
            Price::from("150.52"),
            Quantity::from(1000),
            Quantity::from(500),
            OrderSide::Buy,
            b' ' as c_char,
            1_000_000_000.into(),
            1_000_000_001.into(),
            1_000_000_002.into(),
        );

        let imbalance2 = DatabentoImbalance::new(
            instrument_id,
            Price::from("151.00"),
            Price::from("151.01"),
            Price::from("151.02"),
            Quantity::from(2000),
            Quantity::from(1000),
            OrderSide::Sell,
            b'Y' as c_char,
            2_000_000_000.into(),
            2_000_000_001.into(),
            2_000_000_002.into(),
        );

        let original = vec![imbalance1, imbalance2];
        let record_batch = DatabentoImbalance::encode_batch(&metadata, &original).unwrap();
        let decoded = decode_imbalance_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), original.len());
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec.instrument_id, orig.instrument_id);
            assert_eq!(dec.ref_price, orig.ref_price);
            assert_eq!(dec.cont_book_clr_price, orig.cont_book_clr_price);
            assert_eq!(dec.auct_interest_clr_price, orig.auct_interest_clr_price);
            assert_eq!(dec.paired_qty, orig.paired_qty);
            assert_eq!(dec.total_imbalance_qty, orig.total_imbalance_qty);
            assert_eq!(dec.side, orig.side);
            assert_eq!(dec.significant_imbalance, orig.significant_imbalance);
            assert_eq!(dec.ts_event, orig.ts_event);
            assert_eq!(dec.ts_recv, orig.ts_recv);
            assert_eq!(dec.ts_init, orig.ts_init);
        }
    }
}
