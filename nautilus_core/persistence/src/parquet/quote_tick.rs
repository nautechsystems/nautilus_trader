// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use std::collections::BTreeMap;

use arrow2::{
    array::{Array, Int64Array, UInt64Array},
    chunk::Chunk,
    datatypes::{DataType, Field, Schema},
    io::parquet::write::{transverse, Encoding},
};

use super::{DecodeFromChunk, EncodeToChunk};
use nautilus_model::data::tick::QuoteTick;
use nautilus_model::{
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

impl EncodeToChunk for QuoteTick {
    fn encodings() -> Vec<Vec<Encoding>> {
        QuoteTick::encode_schema()
            .fields
            .iter()
            .map(|f| transverse(&f.data_type, |_| Encoding::Plain))
            .collect()
    }

    fn encode_schema() -> Schema {
        let instrument_id = InstrumentId::from("EUR/USD.SIM");
        let fields = vec![
            Field::new("bid", DataType::Int64, false),
            Field::new("ask", DataType::Int64, false),
            Field::new("bid_size", DataType::UInt64, false),
            Field::new("ask_size", DataType::UInt64, false),
            Field::new("ts", DataType::UInt64, false),
        ];

        let mut metadata = BTreeMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata.insert("price_precision".to_string(), "8".to_string());
        metadata.insert("qty_precision".to_string(), "0".to_string());
        Schema::from(fields).with_metadata(metadata)
    }

    #[allow(clippy::type_complexity)]
    fn encode(data: Vec<Self>) -> Chunk<Box<dyn Array>> {
        let (mut bid_field, mut ask_field, mut bid_size, mut ask_size, mut ts): (
            Vec<i64>,
            Vec<i64>,
            Vec<u64>,
            Vec<u64>,
            Vec<u64>,
        ) = (vec![], vec![], vec![], vec![], vec![]);

        data.iter().fold((), |(), quote| {
            bid_field.push(quote.bid.raw);
            ask_field.push(quote.ask.raw);
            ask_size.push(quote.ask_size.raw);
            bid_size.push(quote.bid_size.raw);
            ts.push(quote.ts_init);
        });

        let ask_array = Int64Array::from_vec(ask_field);
        let bid_array = Int64Array::from_vec(bid_field);
        let ask_size_array = UInt64Array::from_vec(ask_size);
        let bid_size_array = UInt64Array::from_vec(bid_size);
        let ts_array = UInt64Array::from_vec(ts);
        Chunk::new(vec![
            bid_array.to_boxed(),
            ask_array.to_boxed(),
            ask_size_array.to_boxed(),
            bid_size_array.to_boxed(),
            ts_array.to_boxed(),
        ])
    }
}

impl DecodeFromChunk for QuoteTick {
    fn decode(schema: &Schema, cols: Chunk<Box<dyn Array>>) -> Vec<Self> {
        let instrument_id = InstrumentId::from(schema.metadata.get("instrument_id").unwrap());
        let price_precision = schema
            .metadata
            .get("price_precision")
            .unwrap()
            .parse::<u8>()
            .unwrap();
        let qty_precision = schema
            .metadata
            .get("qty_precision")
            .unwrap()
            .parse::<u8>()
            .unwrap();

        // extract field value arrays from chunk separately
        let bid_values = cols.arrays()[0]
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        let ask_values = cols.arrays()[1]
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        let ask_size_values = cols.arrays()[2]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let bid_size_values = cols.arrays()[3]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let ts_values = cols.arrays()[4]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();

        // construct iterator of values from field value arrays
        let values = bid_values
            .into_iter()
            .zip(ask_values.into_iter())
            .zip(ask_size_values.into_iter())
            .zip(bid_size_values.into_iter())
            .zip(ts_values.into_iter())
            .map(|((((bid, ask), ask_size), bid_size), ts)| QuoteTick {
                instrument_id: instrument_id.clone(),
                bid: Price::from_raw(*bid.unwrap(), price_precision),
                ask: Price::from_raw(*ask.unwrap(), price_precision),
                bid_size: Quantity::from_raw(*bid_size.unwrap(), qty_precision),
                ask_size: Quantity::from_raw(*ask_size.unwrap(), qty_precision),
                ts_event: *ts.unwrap(),
                ts_init: *ts.unwrap(),
            });

        values.collect()
    }
}
