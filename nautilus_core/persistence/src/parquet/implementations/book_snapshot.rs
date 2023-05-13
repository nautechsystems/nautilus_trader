// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
use std::str::FromStr;
use std::sync::Arc;

use datafusion::arrow::array::{
    Array, ArrayRef, Int64Array, ListArray, StructArray, UInt64Array, UInt8Array,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use nautilus_model::data::book::{BookOrder, OrderBookSnapshot};
use nautilus_model::enums::{FromU8, OrderSide};
use nautilus_model::identifiers::instrument_id::InstrumentId;
use nautilus_model::types::{price::Price, quantity::Quantity};

use crate::parquet::{Data, DecodeDataFromRecordBatch};

impl DecodeDataFromRecordBatch for OrderBookSnapshot {
    fn decode_batch(metadata: &HashMap<String, String>, record_batch: RecordBatch) -> Vec<Data> {
        // Parse and validate metadata
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata);

        // Extract field value arrays from record batch
        let cols = record_batch.columns();
        let bids_values = cols[0].as_any().downcast_ref::<Vec<ArrayRef>>().unwrap();
        let asks_values = cols[1].as_any().downcast_ref::<Vec<ArrayRef>>().unwrap();
        let sequence_values = cols[2].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = cols[3].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = cols[4].as_any().downcast_ref::<UInt64Array>().unwrap();

        // Construct iterator of values from field value arrays
        let values = bids_values
            .iter()
            .zip(asks_values.iter())
            .zip(sequence_values.iter())
            .zip(ts_event_values.iter())
            .zip(ts_init_values.iter())
            .map(|((((bids, asks), sequence), ts_event), ts_init)| {
                let bids = decode_book_orders(bids, price_precision, size_precision);
                let asks = decode_book_orders(asks, price_precision, size_precision);

                Self {
                    instrument_id: instrument_id.clone(),
                    bids: bids.into(),
                    asks: asks.into(),
                    sequence: sequence.unwrap(),
                    ts_event: ts_event.unwrap(),
                    ts_init: ts_init.unwrap(),
                }
                .into()
            });

        values.collect()
    }

    fn get_schema(metadata: HashMap<String, String>) -> SchemaRef {
        let new_fields = vec![
            Field::new("bids", get_order_list_data_type(), false),
            Field::new("asks", get_order_list_data_type(), false),
            Field::new("sequence", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        Arc::new(Schema::new_with_metadata(new_fields, metadata))
    }
}

fn parse_metadata(metadata: &HashMap<String, String>) -> (InstrumentId, u8, u8) {
    let instrument_id =
        InstrumentId::from_str(metadata.get("instrument_id").unwrap().as_str()).unwrap();
    let price_precision = metadata
        .get("price_precision")
        .unwrap()
        .parse::<u8>()
        .unwrap();
    let size_precision = metadata
        .get("size_precision")
        .unwrap()
        .parse::<u8>()
        .unwrap();

    (instrument_id, price_precision, size_precision)
}

fn decode_book_orders(array: &ArrayRef, price_precision: u8, size_precision: u8) -> Vec<BookOrder> {
    let struct_array = array
        .as_any()
        .downcast_ref::<ListArray>()
        .expect("Expected ListArray");

    let values_array = struct_array.values();
    let offsets = struct_array.offsets();

    let mut orders = Vec::with_capacity(struct_array.len());

    for i in 0..struct_array.len() {
        let start = offsets[i] as usize;
        let end = offsets[i + 1] as usize;

        let order_array = values_array.slice(start, end);
        let order_struct_array = order_array
            .as_any()
            .downcast_ref::<StructArray>()
            .expect("Expected StructArray");

        let price_array = order_struct_array.column(0);
        let size_array = order_struct_array.column(1);
        let side_array = order_struct_array.column(2);
        let order_id_array = order_struct_array.column(3);

        let price_values = price_array
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("Expected Int64Array");
        let size_values = size_array
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("Expected UInt64Array");
        let side_values = side_array
            .as_any()
            .downcast_ref::<UInt8Array>()
            .expect("Expected UInt8Array");
        let order_id_values = order_id_array
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("Expected UInt64Array");

        let order = BookOrder {
            price: Price::from_raw(price_values.value(i), price_precision),
            size: Quantity::from_raw(size_values.value(i), size_precision),
            side: OrderSide::from_u8(side_values.value(i)).expect("Invalid OrderSide value"),
            order_id: order_id_values.value(i),
        };

        orders.push(order);
    }

    orders
}

fn get_book_order_schema() -> SchemaRef {
    let fields = vec![
        Field::new("price", DataType::Int64, false),
        Field::new("size", DataType::UInt64, false),
        Field::new("side", DataType::UInt8, false),
        Field::new("order_id", DataType::UInt64, false),
    ];

    Schema::new(fields).into()
}

fn get_book_order_field() -> Field {
    Field::new(
        "order",
        DataType::Struct(get_book_order_schema().fields().clone()),
        false,
    )
}

fn get_order_list_data_type() -> DataType {
    DataType::List(Arc::new(get_book_order_field()))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
