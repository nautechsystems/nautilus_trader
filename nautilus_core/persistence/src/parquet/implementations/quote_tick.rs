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

use datafusion::arrow::array::*;
use datafusion::arrow::datatypes::*;
use datafusion::arrow::record_batch::RecordBatch;
use nautilus_model::data::tick::QuoteTick;
use nautilus_model::{
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

use crate::parquet::{Data, DecodeDataFromRecordBatch};

impl DecodeDataFromRecordBatch for QuoteTick {
    fn decode_batch(metadata: &HashMap<String, String>, record_batch: RecordBatch) -> Vec<Data> {
        let instrument_id = InstrumentId::from(metadata.get("instrument_id").unwrap().as_str());
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

        // Extract field value arrays from record batch
        let cols = record_batch.columns();
        let bid_values = cols[0].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_values = cols[1].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_size_values = cols[2].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_values = cols[3].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = cols[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = cols[5].as_any().downcast_ref::<UInt64Array>().unwrap();

        // Construct iterator of values from field value arrays
        let values = bid_values
            .into_iter()
            .zip(ask_values.iter())
            .zip(ask_size_values.iter())
            .zip(bid_size_values.iter())
            .zip(ts_event_values.iter())
            .zip(ts_init_values.iter())
            .map(
                |(((((bid, ask), ask_size), bid_size), ts_event), ts_init)| {
                    QuoteTick {
                        instrument_id: instrument_id.clone(),
                        bid: Price::from_raw(bid.unwrap(), price_precision),
                        ask: Price::from_raw(ask.unwrap(), price_precision),
                        bid_size: Quantity::from_raw(bid_size.unwrap(), size_precision),
                        ask_size: Quantity::from_raw(ask_size.unwrap(), size_precision),
                        ts_event: ts_event.unwrap(),
                        ts_init: ts_init.unwrap(),
                    }
                    .into()
                },
            );

        values.collect()
    }

    fn get_schema(metadata: std::collections::HashMap<String, String>) -> SchemaRef {
        let fields = vec![
            Field::new("bid", DataType::Int64, false),
            Field::new("ask", DataType::Int64, false),
            Field::new("bid_size", DataType::UInt64, false),
            Field::new("ask_size", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        Schema::new_with_metadata(fields, metadata).into()
    }
}
