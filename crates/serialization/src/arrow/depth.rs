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
    array::{Array, Decimal128Array, ListArray, StructArray, UInt8Array, UInt32Array, UInt64Array},
    buffer::{OffsetBuffer, ScalarBuffer},
    datatypes::{DataType, Field, Fields, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
#[cfg(test)]
use nautilus_model::identifiers::InstrumentId;
use nautilus_model::{
    data::{
        depth::{OrderBookDepth, is_depth_level_present},
        order::BookOrder,
    },
    enums::OrderSide,
    types::{price::PriceRaw, quantity::QuantityRaw},
};

use super::{
    DecodeDataFromRecordBatch, EMPTY_DEPTH_PRECISION, EncodingError, KEY_IDENTIFIER,
    decode_decimal_price, decode_decimal_quantity, decode_required_timestamp, decode_required_u8,
    decode_required_u64, extract_column, fixed_decimal_data_type, identifier_array_from_display,
    parse_metadata, price_decimal_array, price_raw_to_decimal, quantity_decimal_array,
};
#[cfg(test)]
use super::{KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION, KEY_SIZE_PRECISION};
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

fn depth_level_fields() -> Fields {
    vec![
        Field::new("price", fixed_decimal_data_type(), false),
        Field::new("size", fixed_decimal_data_type(), false),
        Field::new("count", DataType::UInt32, false),
        Field::new("order_id", DataType::UInt64, false),
    ]
    .into()
}

fn depth_side_data_type() -> DataType {
    let fields = depth_level_fields();
    DataType::List(Arc::new(Field::new(
        "item",
        DataType::Struct(fields),
        false,
    )))
}

impl ArrowSchemaProvider for OrderBookDepth {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let mut fields = vec![
            Field::new("bids", depth_side_data_type(), false),
            Field::new("asks", depth_side_data_type(), false),
        ];
        fields.push(Field::new("flags", DataType::UInt8, false));
        fields.push(Field::new("sequence", DataType::UInt64, false));
        fields.push(Field::new(
            "ts_event",
            crate::arrow::timestamp_data_type(),
            false,
        ));
        fields.push(Field::new(
            "ts_init",
            crate::arrow::timestamp_data_type(),
            false,
        ));
        fields.push(Field::new(KEY_IDENTIFIER, DataType::Utf8, true));

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for OrderBookDepth {
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: std::borrow::Borrow<Self>,
    {
        let bid_capacity = data
            .iter()
            .map(std::borrow::Borrow::borrow)
            .map(|depth| depth.bids.len())
            .sum();
        let ask_capacity = data
            .iter()
            .map(std::borrow::Borrow::borrow)
            .map(|depth| depth.asks.len())
            .sum();
        let mut bid_prices = Vec::with_capacity(bid_capacity);
        let mut ask_prices = Vec::with_capacity(ask_capacity);
        let mut bid_sizes = Vec::with_capacity(bid_capacity);
        let mut ask_sizes = Vec::with_capacity(ask_capacity);
        let mut bid_order_ids = Vec::with_capacity(bid_capacity);
        let mut ask_order_ids = Vec::with_capacity(ask_capacity);
        let mut bid_counts = Vec::with_capacity(bid_capacity);
        let mut ask_counts = Vec::with_capacity(ask_capacity);
        let mut bid_offsets = Vec::with_capacity(data.len() + 1);
        let mut ask_offsets = Vec::with_capacity(data.len() + 1);
        bid_offsets.push(0);
        ask_offsets.push(0);

        let mut flags_builder = UInt8Array::builder(data.len());
        let mut sequence_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for depth in data.iter().map(std::borrow::Borrow::borrow) {
            if depth.bids.len() != depth.bid_counts.len()
                || depth.asks.len() != depth.ask_counts.len()
            {
                return Err(ArrowError::InvalidArgumentError(format!(
                    "OrderBookDepth for '{}' has mismatched level and count lengths: \
                     bids {} vs bid_counts {}, asks {} vs ask_counts {}",
                    depth.instrument_id,
                    depth.bids.len(),
                    depth.bid_counts.len(),
                    depth.asks.len(),
                    depth.ask_counts.len(),
                )));
            }

            for (bid, count) in depth.bids.iter().zip(&depth.bid_counts) {
                price_raw_to_decimal(bid.price.raw(), "bids.price")?;
                if is_depth_level_present(bid) {
                    bid_prices.push(bid.price.raw());
                    bid_sizes.push(bid.size.raw());
                    bid_order_ids.push(bid.order_id);
                    bid_counts.push(*count);
                }
            }

            for (ask, count) in depth.asks.iter().zip(&depth.ask_counts) {
                price_raw_to_decimal(ask.price.raw(), "asks.price")?;
                if is_depth_level_present(ask) {
                    ask_prices.push(ask.price.raw());
                    ask_sizes.push(ask.size.raw());
                    ask_order_ids.push(ask.order_id);
                    ask_counts.push(*count);
                }
            }
            bid_offsets.push(i32::try_from(bid_prices.len()).map_err(|_| {
                ArrowError::InvalidArgumentError(
                    "Depth bid values exceed the Arrow List offset range".to_string(),
                )
            })?);
            ask_offsets.push(i32::try_from(ask_prices.len()).map_err(|_| {
                ArrowError::InvalidArgumentError(
                    "Depth ask values exceed the Arrow List offset range".to_string(),
                )
            })?);

            flags_builder.append_value(depth.flags);
            sequence_builder.append_value(depth.sequence);
            ts_event_builder.append_value(depth.ts_event.as_u64());
            ts_init_builder.append_value(depth.ts_init.as_u64());
        }

        let bids = depth_side_array(
            bid_prices,
            bid_sizes,
            bid_counts,
            bid_order_ids,
            bid_offsets,
            "bids.price",
        )?;
        let asks = depth_side_array(
            ask_prices,
            ask_sizes,
            ask_counts,
            ask_order_ids,
            ask_offsets,
            "asks.price",
        )?;

        let flags_array = Arc::new(flags_builder.finish()) as Arc<dyn Array>;
        let sequence_array = Arc::new(sequence_builder.finish()) as Arc<dyn Array>;
        let ts_event_array = Arc::new(ts_event_builder.finish()) as Arc<dyn Array>;
        let ts_init_array = Arc::new(ts_init_builder.finish()) as Arc<dyn Array>;

        let mut columns = vec![
            Arc::new(bids) as Arc<dyn Array>,
            Arc::new(asks) as Arc<dyn Array>,
        ];
        columns.push(flags_array);
        columns.push(sequence_array);
        columns.push(ts_event_array);
        columns.push(ts_init_array);
        columns.push(Arc::new(identifier_array_from_display(
            data.iter()
                .map(std::borrow::Borrow::borrow)
                .map(|depth| depth.instrument_id),
        )));

        crate::arrow::record_batch_with_timestamps(
            Self::get_schema(Some(metadata.clone())).into(),
            columns,
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        let precision = self
            .bids
            .first()
            .or_else(|| self.asks.first())
            .map_or(EMPTY_DEPTH_PRECISION, |level| {
                (level.price.precision, level.size.precision)
            });
        Self::get_metadata(&self.instrument_id, precision.0, precision.1)
    }

    fn chunk_metadata<T>(chunk: &[T]) -> HashMap<String, String>
    where
        T: std::borrow::Borrow<Self>,
    {
        let first = chunk
            .first()
            .map(std::borrow::Borrow::borrow)
            .expect("Chunk must contain at least one element to encode");
        let precision = chunk
            .iter()
            .map(std::borrow::Borrow::borrow)
            .flat_map(|depth| depth.bids.iter().chain(&depth.asks))
            .find(|order| is_depth_level_present(order))
            .map_or(EMPTY_DEPTH_PRECISION, |order| {
                (order.price.precision, order.size.precision)
            });

        Self::get_metadata(&first.instrument_id, precision.0, precision.1)
    }

    fn matches_chunk_metadata(&self, metadata: &HashMap<String, String>) -> bool {
        let Ok((instrument_id, price_precision, size_precision)) = parse_metadata(metadata) else {
            return false;
        };

        if self.instrument_id != instrument_id {
            return false;
        }

        self.bids
            .iter()
            .chain(&self.asks)
            .filter(|order| is_depth_level_present(order))
            .all(|order| {
                order.price.precision == price_precision && order.size.precision == size_precision
            })
    }
}

fn depth_side_array(
    prices: Vec<PriceRaw>,
    sizes: Vec<QuantityRaw>,
    counts: Vec<u32>,
    order_ids: Vec<u64>,
    offsets: Vec<i32>,
    price_field: &'static str,
) -> Result<ListArray, ArrowError> {
    let fields = depth_level_fields();
    let values = StructArray::try_new(
        fields.clone(),
        vec![
            Arc::new(price_decimal_array(prices, price_field)?),
            Arc::new(quantity_decimal_array(sizes, "size")?),
            Arc::new(UInt32Array::from(counts)),
            Arc::new(UInt64Array::from(order_ids)),
        ],
        None,
    )?;
    ListArray::try_new(
        Arc::new(Field::new("item", DataType::Struct(fields), false)),
        OffsetBuffer::new(ScalarBuffer::from(offsets)),
        Arc::new(values),
        None,
    )
}

impl DecodeFromRecordBatch for OrderBookDepth {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
        let record_batch = crate::arrow::record_batch_with_u64_timestamps(&record_batch)?;

        let (bid_list, bid_values, bid_prices, bid_sizes, bid_counts, bid_order_ids) =
            depth_side_values(&record_batch, "bids")?;
        let (ask_list, ask_values, ask_prices, ask_sizes, ask_counts, ask_order_ids) =
            depth_side_values(&record_batch, "asks")?;

        let flags = named_column::<UInt8Array>(&record_batch, "flags", DataType::UInt8)?;
        let sequence = named_column::<UInt64Array>(&record_batch, "sequence", DataType::UInt64)?;
        let ts_event = named_column::<UInt64Array>(&record_batch, "ts_event", DataType::UInt64)?;
        let ts_init = named_column::<UInt64Array>(&record_batch, "ts_init", DataType::UInt64)?;

        // Map record batch rows to vector of OrderBookDepth
        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|row| {
                let (bid_start, bid_end) = depth_row_range(bid_list, "bids", row)?;
                let bid_end = depth_present_end(bid_values, "bids", row, bid_start, bid_end)?;
                let (ask_start, ask_end) = depth_row_range(ask_list, "asks", row)?;
                let ask_end = depth_present_end(ask_values, "asks", row, ask_start, ask_end)?;
                let mut bids = Vec::with_capacity(bid_end - bid_start);
                let mut asks = Vec::with_capacity(ask_end - ask_start);
                let mut bid_count_arr = Vec::with_capacity(bid_end - bid_start);
                let mut ask_count_arr = Vec::with_capacity(ask_end - ask_start);

                for value_index in bid_start..bid_end {
                    ensure_depth_value(bid_prices, "price", "bids.price", row, value_index)?;
                    ensure_depth_value(bid_sizes, "size", "bids.size", row, value_index)?;
                    ensure_depth_value(bid_counts, "count", "bids.count", row, value_index)?;
                    ensure_depth_value(
                        bid_order_ids,
                        "order_id",
                        "bids.order_id",
                        row,
                        value_index,
                    )?;
                    let bid_price = decode_decimal_price(
                        bid_prices,
                        price_precision,
                        "bids.price",
                        value_index,
                    )?;
                    let bid_size = decode_decimal_quantity(
                        bid_sizes,
                        size_precision,
                        "bids.size",
                        value_index,
                    )?;
                    bids.push(BookOrder::new(
                        OrderSide::Buy,
                        bid_price,
                        bid_size,
                        bid_order_ids.value(value_index),
                    ));
                    bid_count_arr.push(bid_counts.value(value_index));
                }

                for value_index in ask_start..ask_end {
                    ensure_depth_value(ask_prices, "price", "asks.price", row, value_index)?;
                    ensure_depth_value(ask_sizes, "size", "asks.size", row, value_index)?;
                    ensure_depth_value(ask_counts, "count", "asks.count", row, value_index)?;
                    ensure_depth_value(
                        ask_order_ids,
                        "order_id",
                        "asks.order_id",
                        row,
                        value_index,
                    )?;
                    let ask_price = decode_decimal_price(
                        ask_prices,
                        price_precision,
                        "asks.price",
                        value_index,
                    )?;
                    let ask_size = decode_decimal_quantity(
                        ask_sizes,
                        size_precision,
                        "asks.size",
                        value_index,
                    )?;
                    asks.push(BookOrder::new(
                        OrderSide::Sell,
                        ask_price,
                        ask_size,
                        ask_order_ids.value(value_index),
                    ));
                    ask_count_arr.push(ask_counts.value(value_index));
                }

                Self::new_checked(
                    instrument_id,
                    bids,
                    asks,
                    bid_count_arr,
                    ask_count_arr,
                    decode_required_u8(flags, "flags", row)?,
                    decode_required_u64(sequence, "sequence", row)?,
                    decode_required_timestamp(ts_event, "ts_event", row)?,
                    decode_required_timestamp(ts_init, "ts_init", row)?,
                )
                .map_err(|e| EncodingError::ParseError("depth", format!("row {row}: {e}")))
            })
            .collect();

        result
    }
}

fn named_column<'a, T: Array + 'static>(
    record_batch: &'a RecordBatch,
    name: &'static str,
    data_type: DataType,
) -> Result<&'a T, EncodingError> {
    let index = record_batch.schema().index_of(name)?;
    extract_column::<T>(record_batch.columns(), name, index, data_type)
}

#[allow(clippy::type_complexity)]
fn depth_side_values<'a>(
    record_batch: &'a RecordBatch,
    name: &'static str,
) -> Result<
    (
        &'a ListArray,
        &'a StructArray,
        &'a Decimal128Array,
        &'a Decimal128Array,
        &'a UInt32Array,
        &'a UInt64Array,
    ),
    EncodingError,
> {
    let index = record_batch.schema().index_of(name)?;
    let list = dynamic_column::<ListArray>(record_batch, name, index, &depth_side_data_type())?;
    let values = list
        .values()
        .as_any()
        .downcast_ref::<StructArray>()
        .ok_or_else(|| {
            ArrowError::CastError(format!(
                "Invalid list value type `{name}`: expected {}, found {}",
                DataType::Struct(depth_level_fields()),
                list.value_type(),
            ))
        })?;
    Ok((
        list,
        values,
        depth_struct_column(values, name, "price", &fixed_decimal_data_type())?,
        depth_struct_column(values, name, "size", &fixed_decimal_data_type())?,
        depth_struct_column(values, name, "count", &DataType::UInt32)?,
        depth_struct_column(values, name, "order_id", &DataType::UInt64)?,
    ))
}

fn depth_present_end(
    values: &StructArray,
    side: &'static str,
    row: usize,
    start: usize,
    end: usize,
) -> Result<usize, EncodingError> {
    let mut present_end = end;
    while present_end > start && values.is_null(present_end - 1) {
        present_end -= 1;
    }

    if values.null_count() > 0 && (start..present_end).any(|index| values.is_null(index)) {
        return Err(EncodingError::ParseError(
            side,
            format!("row {row}: levels must be contiguous"),
        ));
    }

    Ok(present_end)
}

fn ensure_depth_value(
    values: &dyn Array,
    kind: &'static str,
    column: &'static str,
    row: usize,
    value_index: usize,
) -> Result<(), EncodingError> {
    if values.is_null(value_index) {
        return Err(EncodingError::ParseError(
            column,
            format!("{kind} column '{column}' row {row} is null"),
        ));
    }

    Ok(())
}

fn depth_row_range(
    list: &ListArray,
    name: &'static str,
    row: usize,
) -> Result<(usize, usize), EncodingError> {
    if list.is_null(row) {
        return Err(ArrowError::InvalidArgumentError(format!(
            "Depth side `{name}` is null at row {row}"
        ))
        .into());
    }
    let offsets = list.value_offsets();
    let start = usize::try_from(offsets[row]).map_err(|_| {
        ArrowError::InvalidArgumentError(format!(
            "Depth side `{name}` has a negative offset at row {row}"
        ))
    })?;
    let end = usize::try_from(offsets[row + 1]).map_err(|_| {
        ArrowError::InvalidArgumentError(format!(
            "Depth side `{name}` has a negative offset at row {row}"
        ))
    })?;
    Ok((start, end))
}

fn depth_struct_column<'a, T: Array + 'static>(
    values: &'a StructArray,
    side: &str,
    name: &str,
    expected_type: &DataType,
) -> Result<&'a T, EncodingError> {
    let column = values
        .column_by_name(name)
        .ok_or_else(|| ArrowError::SchemaError(format!("Missing depth field `{side}.{name}`")))?;
    column.as_any().downcast_ref::<T>().ok_or_else(|| {
        ArrowError::CastError(format!(
            "Invalid depth field `{side}.{name}`: expected {expected_type}, found {}",
            column.data_type(),
        ))
        .into()
    })
}

fn dynamic_column<'a, T: Array + 'static>(
    record_batch: &'a RecordBatch,
    name: &str,
    index: usize,
    expected_type: &DataType,
) -> Result<&'a T, EncodingError> {
    let column = record_batch.columns().get(index).ok_or_else(|| {
        ArrowError::SchemaError(format!("Missing data column: `{name}` at index {index}"))
    })?;
    column.as_any().downcast_ref::<T>().ok_or_else(|| {
        ArrowError::CastError(format!(
            "Invalid column type `{name}` at index {index}: expected {expected_type}, found {}",
            column.data_type(),
        ))
        .into()
    })
}

impl DecodeDataFromRecordBatch for OrderBookDepth {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let depths: Vec<Self> = Self::decode_batch(metadata, record_batch)?;
        Ok(depths.into_iter().map(Data::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::TimestampNanosecondArray,
        datatypes::{DataType, Field},
    };
    use nautilus_model::{
        data::{BookOrder, DEPTH10_LEN, stubs::stub_depth10},
        enums::{BookType, OrderSide, RecordFlag},
        orderbook::OrderBook,
        types::{
            PRICE_ERROR, PRICE_UNDEF, Price, QUANTITY_UNDEF, Quantity, fixed::FIXED_SCALAR,
            price::PriceRaw, quantity::QuantityRaw,
        },
    };
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;
    use crate::arrow::{get_raw_price, get_raw_quantity};

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDepth::get_metadata(&instrument_id, 2, 0);
        let schema = OrderBookDepth::get_schema(Some(metadata));

        assert_eq!(
            schema.field(0),
            &Field::new("bids", depth_side_data_type(), false),
        );
        assert_eq!(
            schema.field(1),
            &Field::new("asks", depth_side_data_type(), false),
        );
        let flags_field = schema.field(2).clone();
        assert_eq!(flags_field, Field::new("flags", DataType::UInt8, false));
        let sequence_field = schema.field(3).clone();
        assert_eq!(
            sequence_field,
            Field::new("sequence", DataType::UInt64, false)
        );
        let ts_event_field = schema.field(4).clone();
        assert_eq!(
            ts_event_field,
            Field::new("ts_event", crate::arrow::timestamp_data_type(), false)
        );
        let ts_init_field = schema.field(5).clone();
        assert_eq!(
            ts_init_field,
            Field::new("ts_init", crate::arrow::timestamp_data_type(), false)
        );

        assert_eq!(schema.metadata()["instrument_id"], "AAPL.XNAS");
        assert_eq!(schema.metadata()["price_precision"], "2");
        assert_eq!(schema.metadata()["size_precision"], "0");
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = OrderBookDepth::get_schema_map();

        let depth_type = format!("{:?}", depth_side_data_type());
        assert_eq!(
            schema_map.get("bids").map(String::as_str),
            Some(depth_type.as_str()),
        );
        assert_eq!(
            schema_map.get("asks").map(String::as_str),
            Some(depth_type.as_str()),
        );

        assert_eq!(schema_map.get("flags").map(String::as_str), Some("UInt8"));
        assert_eq!(
            schema_map.get("sequence").map(String::as_str),
            Some("UInt64")
        );
        assert_eq!(
            schema_map.get("ts_event").map(String::as_str),
            Some("Timestamp(Nanosecond, Some(\"UTC\"))")
        );
        assert_eq!(
            schema_map.get("ts_init").map(String::as_str),
            Some("Timestamp(Nanosecond, Some(\"UTC\"))")
        );
        assert_eq!(
            schema_map.get(KEY_IDENTIFIER).map(String::as_str),
            Some("Utf8")
        );
    }

    #[rstest]
    fn test_chunk_metadata_skips_leading_empty_snapshot(stub_depth10: OrderBookDepth) {
        let mut empty = stub_depth10.clone();
        empty.bids.clear();
        empty.asks.clear();
        empty.bid_counts.clear();
        empty.ask_counts.clear();

        let metadata = OrderBookDepth::chunk_metadata(&[empty.clone(), stub_depth10.clone()]);

        assert_eq!(
            metadata[KEY_INSTRUMENT_ID],
            stub_depth10.instrument_id.to_string()
        );
        assert_eq!(metadata[KEY_PRICE_PRECISION], "2");
        assert_eq!(metadata[KEY_SIZE_PRECISION], "0");
        assert!(empty.matches_chunk_metadata(&metadata));
        assert!(stub_depth10.matches_chunk_metadata(&metadata));
    }

    #[rstest]
    fn test_all_empty_depth_chunk_uses_zero_precision(stub_depth10: OrderBookDepth) {
        let mut first = stub_depth10;
        first.bids.clear();
        first.asks.clear();
        first.bid_counts.clear();
        first.ask_counts.clear();
        let mut second = first.clone();
        second.sequence = 2;

        let depths = [first, second];
        let metadata = OrderBookDepth::chunk_metadata(&depths);
        let batch = OrderBookDepth::encode_batch(&metadata, &depths).unwrap();
        let decoded = OrderBookDepth::decode_batch(&metadata, batch).unwrap();

        assert_eq!(
            metadata[KEY_INSTRUMENT_ID],
            depths[0].instrument_id.to_string()
        );
        assert_eq!(metadata[KEY_PRICE_PRECISION], "0");
        assert_eq!(metadata[KEY_SIZE_PRECISION], "0");
        assert_eq!(decoded, depths);
    }

    #[rstest]
    fn test_encode_batch(stub_depth10: OrderBookDepth) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let price_precision = 2;
        let metadata = OrderBookDepth::get_metadata(&instrument_id, price_precision, 0);

        let data = vec![stub_depth10];
        let record_batch = OrderBookDepth::encode_batch(&metadata, &data).unwrap();
        let columns = record_batch.columns();

        assert_eq!(columns.len(), 7);

        let bids = columns[0]
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap()
            .values()
            .as_any()
            .downcast_ref::<StructArray>()
            .unwrap();
        let bid_prices = bids
            .column_by_name("price")
            .unwrap()
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();

        let expected_bid_prices: Vec<f64> =
            vec![99.0, 98.0, 97.0, 96.0, 95.0, 94.0, 93.0, 92.0, 91.0, 90.0];

        for (i, expected) in expected_bid_prices.iter().enumerate() {
            assert_eq!(
                get_raw_price(bid_prices.value(i)),
                (expected * FIXED_SCALAR) as PriceRaw
            );
            assert_eq!(
                Price::from_raw(get_raw_price(bid_prices.value(i)), price_precision).as_f64(),
                *expected
            );
        }

        let asks = columns[1]
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap()
            .values()
            .as_any()
            .downcast_ref::<StructArray>()
            .unwrap();
        let ask_prices = asks
            .column_by_name("price")
            .unwrap()
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();

        let expected_ask_prices: Vec<f64> = vec![
            100.0, 101.0, 102.0, 103.0, 104.0, 105.0, 106.0, 107.0, 108.0, 109.0,
        ];

        for (i, expected) in expected_ask_prices.iter().enumerate() {
            assert_eq!(
                get_raw_price(ask_prices.value(i)),
                (expected * FIXED_SCALAR) as PriceRaw
            );
            assert_eq!(
                Price::from_raw(get_raw_price(ask_prices.value(i)), price_precision).as_f64(),
                *expected
            );
        }

        let bid_sizes = bids
            .column_by_name("size")
            .unwrap()
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();

        for i in 0..DEPTH10_LEN {
            assert_eq!(
                get_raw_quantity(bid_sizes.value(i)),
                ((100.0 * FIXED_SCALAR * (i + 1) as f64) as QuantityRaw)
            );
        }

        let ask_sizes = asks
            .column_by_name("size")
            .unwrap()
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();

        for i in 0..DEPTH10_LEN {
            assert_eq!(
                get_raw_quantity(ask_sizes.value(i)),
                ((100.0 * FIXED_SCALAR * ((i + 1) as f64)) as QuantityRaw)
            );
        }

        let bid_order_ids = bids
            .column_by_name("order_id")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();

        for i in 0..DEPTH10_LEN {
            assert_eq!(bid_order_ids.value(i), (i + 1) as u64);
        }

        let ask_order_ids = asks
            .column_by_name("order_id")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();

        for i in 0..DEPTH10_LEN {
            assert_eq!(ask_order_ids.value(i), (DEPTH10_LEN + i + 1) as u64);
        }

        let flags_values = columns[2].as_any().downcast_ref::<UInt8Array>().unwrap();
        let sequence_values = columns[3].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = columns[4]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let ts_init_values = columns[5]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert_eq!(flags_values.len(), 1);
        assert_eq!(flags_values.value(0), 0);
        assert_eq!(sequence_values.len(), 1);
        assert_eq!(sequence_values.value(0), 0);
        assert_eq!(ts_event_values.len(), 1);
        assert_eq!(ts_event_values.value(0), 1);
        assert_eq!(ts_init_values.len(), 1);
        assert_eq!(ts_init_values.value(0), 2);
    }

    #[rstest]
    fn test_encode_batch_rejects_price_error(mut stub_depth10: OrderBookDepth) {
        stub_depth10.bids[0].price = Price::from_raw(PRICE_ERROR, 0);
        let metadata = stub_depth10.metadata();

        let error = OrderBookDepth::encode_batch(&metadata, &[stub_depth10]).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "Invalid argument error: Price field 'bids.price' contains PRICE_ERROR raw value {PRICE_ERROR}"
            ),
        );
    }

    #[rstest]
    fn test_decode_batch(stub_depth10: OrderBookDepth) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDepth::get_metadata(&instrument_id, 2, 0);

        let data = vec![stub_depth10];
        let record_batch = OrderBookDepth::encode_batch(&metadata, &data).unwrap();
        let decoded_data = OrderBookDepth::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded_data.len(), 1);
    }

    #[rstest]
    fn test_decode_batch_rejects_null_nested_price(stub_depth10: OrderBookDepth) {
        let metadata = stub_depth10.metadata();
        let batch =
            OrderBookDepth::encode_batch(&metadata, std::slice::from_ref(&stub_depth10)).unwrap();
        let bids_index = batch.schema().index_of("bids").unwrap();
        let bids = batch
            .column(bids_index)
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();
        let values = bids.values();
        let values = values.as_any().downcast_ref::<StructArray>().unwrap();
        let prices = values
            .column_by_name("price")
            .unwrap()
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let prices = Decimal128Array::from(
            (0..prices.len())
                .map(|index| (index != 0).then(|| prices.value(index)))
                .collect::<Vec<_>>(),
        )
        .with_precision_and_scale(38, 16)
        .unwrap();
        let mut value_fields = values.fields().to_vec();
        value_fields[0] = Arc::new(Field::new("price", fixed_decimal_data_type(), true));
        let value_fields: Fields = value_fields.into();
        let mut value_columns = values.columns().to_vec();
        value_columns[0] = Arc::new(prices);
        let values =
            StructArray::try_new(value_fields.clone(), value_columns, values.nulls().cloned())
                .unwrap();
        let item = Arc::new(Field::new("item", DataType::Struct(value_fields), false));
        let bids = ListArray::try_new(
            Arc::clone(&item),
            bids.offsets().clone(),
            Arc::new(values),
            bids.nulls().cloned(),
        )
        .unwrap();
        let mut columns = batch.columns().to_vec();
        columns[bids_index] = Arc::new(bids);
        let mut fields = batch.schema().fields().to_vec();
        fields[bids_index] = Arc::new(Field::new("bids", DataType::List(item), false));
        let schema = Arc::new(Schema::new_with_metadata(
            fields,
            batch.schema().metadata().clone(),
        ));
        let malformed = RecordBatch::try_new(schema, columns).unwrap();

        let error = OrderBookDepth::decode_batch(&metadata, malformed).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Error parsing `bids.price`: price column 'bids.price' row 0 is null"
        );
    }

    #[rstest]
    fn test_decode_batch_uses_column_names(stub_depth10: OrderBookDepth) {
        let metadata = OrderBookDepth::get_metadata(&stub_depth10.instrument_id, 2, 0);
        let batch =
            OrderBookDepth::encode_batch(&metadata, std::slice::from_ref(&stub_depth10)).unwrap();
        let schema = batch.schema();
        let mut fields = schema.fields().to_vec();
        fields.reverse();
        let mut columns = batch.columns().to_vec();
        columns.reverse();
        let reordered = crate::arrow::record_batch_with_timestamps(
            Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone())),
            columns,
        )
        .unwrap();

        let decoded = OrderBookDepth::decode_batch(&metadata, reordered).unwrap();

        assert_eq!(decoded, vec![stub_depth10]);
    }

    #[rstest]
    fn test_depth_column_type_error_names_column() {
        let batch = crate::arrow::record_batch_with_timestamps(
            Arc::new(Schema::new(vec![Field::new(
                "bids",
                DataType::UInt64,
                false,
            )])),
            vec![Arc::new(UInt64Array::from(vec![1]))],
        )
        .unwrap();

        let error = depth_side_values(&batch, "bids").unwrap_err();

        match error {
            EncodingError::ArrowError(ArrowError::CastError(message)) => assert_eq!(
                message,
                "Invalid column type `bids` at index 0: expected List(non-null \
                 Struct(\"price\": non-null Decimal128(38, 16), \"size\": non-null \
                 Decimal128(38, 16), \"count\": non-null UInt32, \"order_id\": non-null UInt64)), \
                 found UInt64",
            ),
            other => panic!("expected Arrow cast error, was {other:?}"),
        }
    }

    #[rstest]
    fn test_decode_batch_missing_instrument_id_returns_error(stub_depth10: OrderBookDepth) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut metadata = OrderBookDepth::get_metadata(&instrument_id, 2, 0);
        let record_batch = OrderBookDepth::encode_batch(&metadata, &[stub_depth10]).unwrap();

        metadata.remove(KEY_INSTRUMENT_ID);

        let result = OrderBookDepth::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("instrument_id"),
            "Expected missing instrument_id error, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_missing_price_precision_returns_error(stub_depth10: OrderBookDepth) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut metadata = OrderBookDepth::get_metadata(&instrument_id, 2, 0);
        let record_batch = OrderBookDepth::encode_batch(&metadata, &[stub_depth10]).unwrap();

        metadata.remove(KEY_PRICE_PRECISION);

        let result = OrderBookDepth::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("price_precision"),
            "Expected missing price_precision error, was: {err}"
        );
    }

    #[rstest]
    fn test_encode_decode_round_trip(stub_depth10: OrderBookDepth) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDepth::get_metadata(&instrument_id, 2, 0);

        let original = vec![stub_depth10];
        let record_batch = OrderBookDepth::encode_batch(&metadata, &original).unwrap();
        let decoded = OrderBookDepth::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), original.len());
        let orig = &original[0];
        let dec = &decoded[0];

        assert_eq!(dec.instrument_id, orig.instrument_id);
        assert_eq!(dec.flags, orig.flags);
        assert_eq!(dec.sequence, orig.sequence);
        assert_eq!(dec.ts_event, orig.ts_event);
        assert_eq!(dec.ts_init, orig.ts_init);

        for i in 0..DEPTH10_LEN {
            assert_eq!(
                dec.bids[i].price, orig.bids[i].price,
                "bid price mismatch at level {i}"
            );
            assert_eq!(
                dec.bids[i].size, orig.bids[i].size,
                "bid size mismatch at level {i}"
            );
            assert_eq!(
                dec.bids[i].order_id, orig.bids[i].order_id,
                "bid order ID mismatch at level {i}"
            );
            assert_eq!(
                dec.asks[i].price, orig.asks[i].price,
                "ask price mismatch at level {i}"
            );
            assert_eq!(
                dec.asks[i].size, orig.asks[i].size,
                "ask size mismatch at level {i}"
            );
            assert_eq!(
                dec.asks[i].order_id, orig.asks[i].order_id,
                "ask order ID mismatch at level {i}"
            );
        }
    }

    #[rstest]
    fn zero_size_level_round_trip_preserves_applied_book() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let zero_bid = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from("0"),
            1,
        );
        let bid = BookOrder::new(
            OrderSide::Buy,
            Price::from("99.00"),
            Quantity::from("10"),
            2,
        );
        let ask = BookOrder::new(
            OrderSide::Sell,
            Price::from("101.00"),
            Quantity::from("20"),
            3,
        );
        let original = OrderBookDepth {
            instrument_id,
            bids: [zero_bid, bid].into_iter().collect(),
            asks: [ask].into_iter().collect(),
            bid_counts: [11, 12].into_iter().collect(),
            ask_counts: [21].into_iter().collect(),
            flags: RecordFlag::F_SNAPSHOT as u8,
            sequence: 1,
            ts_event: 2.into(),
            ts_init: 3.into(),
        };
        let metadata = original.metadata();
        let batch =
            OrderBookDepth::encode_batch(&metadata, std::slice::from_ref(&original)).unwrap();
        let [decoded] = OrderBookDepth::decode_batch(&metadata, batch)
            .unwrap()
            .try_into()
            .unwrap();
        let mut expected_book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let mut decoded_book = OrderBook::new(instrument_id, BookType::L2_MBP);

        expected_book.apply_depth(&original).unwrap();
        decoded_book.apply_depth(&decoded).unwrap();

        assert_eq!(decoded.bids.as_slice(), &[bid]);
        assert_eq!(decoded.bid_counts.as_slice(), &[12]);
        assert_eq!(decoded.asks.as_slice(), &[ask]);
        assert_eq!(decoded.ask_counts.as_slice(), &[21]);
        assert_eq!(decoded_book, expected_book);
    }

    #[rstest]
    #[case(5)]
    #[case(10)]
    #[case(25)]
    fn test_encode_decode_runtime_depth_round_trip(
        stub_depth10: OrderBookDepth,
        #[case] depth: usize,
    ) {
        let value = OrderBookDepth::new(
            stub_depth10.instrument_id,
            stub_depth10.bids.iter().copied().cycle().take(depth),
            stub_depth10.asks.iter().copied().cycle().take(depth),
            0..u32::try_from(depth).unwrap(),
            100..100 + u32::try_from(depth).unwrap(),
            stub_depth10.flags,
            stub_depth10.sequence,
            stub_depth10.ts_event,
            stub_depth10.ts_init,
        );
        let metadata = value.metadata();
        let record_batch =
            OrderBookDepth::encode_batch(&metadata, std::slice::from_ref(&value)).unwrap();
        let bids = record_batch
            .column_by_name("bids")
            .unwrap()
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();
        let asks = record_batch
            .column_by_name("asks")
            .unwrap()
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();

        assert_eq!(bids.value_length(0), i32::try_from(depth).unwrap());
        assert_eq!(asks.value_length(0), i32::try_from(depth).unwrap());
        assert_eq!(
            OrderBookDepth::decode_batch(&metadata, record_batch).unwrap(),
            vec![value]
        );
    }

    #[rstest]
    fn test_encode_batch_rejects_mismatched_level_and_count_lengths(stub_depth10: OrderBookDepth) {
        let mut value = stub_depth10;
        value.bid_counts.pop();
        let metadata = value.metadata();
        let error =
            OrderBookDepth::encode_batch(&metadata, std::slice::from_ref(&value)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("mismatched level and count lengths")
        );
        assert!(error.to_string().contains("bids 10 vs bid_counts 9"));
    }

    #[rstest]
    #[case::price_only(true, false)]
    #[case::size_only(false, true)]
    #[case::both(true, true)]
    #[case::neither(false, false)]
    fn test_decode_batch_with_undefined_levels(
        stub_depth10: OrderBookDepth,
        #[case] price_undef: bool,
        #[case] size_undef: bool,
    ) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let price_precision = 2;
        let size_precision = 0;
        let metadata =
            OrderBookDepth::get_metadata(&instrument_id, price_precision, size_precision);

        let original_depth = stub_depth10;
        let mut depth = original_depth.clone();
        let original_bid = depth.bids[5];
        let original_ask = depth.asks[7];
        let sentinel_bid_price = if price_undef {
            Price::from_raw(PRICE_UNDEF, 0)
        } else {
            original_bid.price
        };
        let sentinel_bid_size = if size_undef {
            Quantity::from_raw(QUANTITY_UNDEF, 0)
        } else {
            original_bid.size
        };
        depth.bids[5] = BookOrder {
            side: OrderSide::Buy.into(),
            price: sentinel_bid_price,
            size: sentinel_bid_size,
            order_id: 0,
        };
        let sentinel_ask_price = if price_undef {
            Price::from_raw(PRICE_UNDEF, 0)
        } else {
            original_ask.price
        };
        let sentinel_ask_size = if size_undef {
            Quantity::from_raw(QUANTITY_UNDEF, 0)
        } else {
            original_ask.size
        };
        depth.asks[7] = BookOrder {
            side: OrderSide::Sell.into(),
            price: sentinel_ask_price,
            size: sentinel_ask_size,
            order_id: 0,
        };

        let record_batch = OrderBookDepth::encode_batch(&metadata, &[depth]);

        let record_batch = record_batch.unwrap();
        let bids = record_batch
            .column_by_name("bids")
            .unwrap()
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();
        let asks = record_batch
            .column_by_name("asks")
            .unwrap()
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();
        let expected_len = if price_undef || size_undef { 9 } else { 10 };
        assert_eq!(bids.value_length(0), expected_len);
        assert_eq!(asks.value_length(0), expected_len);
        let decoded = OrderBookDepth::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), 1);
        let decoded = &decoded[0];

        let expect_null = price_undef || size_undef;
        if expect_null {
            assert_eq!(decoded.bids.len(), 9);
            assert_eq!(decoded.asks.len(), 9);
            assert_eq!(decoded.bids[5], original_depth.bids[6]);
            assert_eq!(decoded.bids[8], original_depth.bids[9]);
            assert_eq!(decoded.asks[7], original_depth.asks[8]);
            assert_eq!(decoded.asks[8], original_depth.asks[9]);
        } else {
            assert_eq!(decoded.bids[5].side, Some(OrderSide::Buy));
            assert_eq!(decoded.bids[5].price, original_bid.price);
            assert_eq!(decoded.bids[5].size, original_bid.size);
            assert_eq!(decoded.asks[7].side, Some(OrderSide::Sell));
            assert_eq!(decoded.asks[7].price, original_ask.price);
            assert_eq!(decoded.asks[7].size, original_ask.size);
        }

        // Surrounding defined levels always round-trip with the instrument precision
        assert_eq!(decoded.bids[0].side, Some(OrderSide::Buy));
        assert_eq!(decoded.bids[0].price.precision, price_precision);
        assert_eq!(decoded.bids[0].size.precision, size_precision);
        assert_eq!(decoded.asks[0].side, Some(OrderSide::Sell));
        assert_eq!(decoded.asks[0].price.precision, price_precision);
        assert_eq!(decoded.asks[0].size.precision, size_precision);
    }
}
