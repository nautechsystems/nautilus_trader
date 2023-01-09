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

use std::collections::BTreeMap;

use arrow2::{
    array::{Array, Int64Array, UInt64Array},
    chunk::Chunk,
    datatypes::{DataType, Field, Schema},
    io::parquet::write::{transverse, Encoding},
};
use nautilus_model::data::tick::QuoteTick;
use nautilus_model::{
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

use crate::parquet::{DecodeFromChunk, EncodeToChunk};

impl EncodeToChunk for QuoteTick {
    fn assert_metadata(metadata: &BTreeMap<String, String>) {
        let keys = ["instrument_id", "price_precision", "size_precision"];
        for key in keys {
            (!metadata.contains_key(key)).then(|| panic!("metadata missing key \"{key}\""));
        }
    }

    fn encodings(metadata: BTreeMap<String, String>) -> Vec<Vec<Encoding>> {
        QuoteTick::encode_schema(metadata)
            .fields
            .iter()
            .map(|f| transverse(&f.data_type, |_| Encoding::Plain))
            .collect()
    }

    fn encode_schema(metadata: BTreeMap<String, String>) -> Schema {
        Self::assert_metadata(&metadata);
        let fields = vec![
            Field::new("bid", DataType::Int64, false),
            Field::new("ask", DataType::Int64, false),
            Field::new("bid_size", DataType::UInt64, false),
            Field::new("ask_size", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        Schema::from(fields).with_metadata(metadata)
    }

    #[allow(clippy::type_complexity)]
    fn encode<'a, I>(data: I) -> Chunk<Box<dyn Array>>
    where
        I: Iterator<Item = &'a Self>,
        Self: 'a,
    {
        let (
            mut bid_column,
            mut ask_column,
            mut bid_size_column,
            mut ask_size_column,
            mut ts_event_column,
            mut ts_init_column,
        ): (Vec<i64>, Vec<i64>, Vec<u64>, Vec<u64>, Vec<u64>, Vec<u64>) =
            (vec![], vec![], vec![], vec![], vec![], vec![]);

        data.fold((), |(), quote| {
            bid_column.push(quote.bid.raw);
            ask_column.push(quote.ask.raw);
            ask_size_column.push(quote.ask_size.raw);
            bid_size_column.push(quote.bid_size.raw);
            ts_event_column.push(quote.ts_event);
            ts_init_column.push(quote.ts_init);
        });

        let bid_array = Int64Array::from_vec(bid_column);
        let ask_array = Int64Array::from_vec(ask_column);
        let bid_size_array = UInt64Array::from_vec(bid_size_column);
        let ask_size_array = UInt64Array::from_vec(ask_size_column);
        let ts_event_array = UInt64Array::from_vec(ts_event_column);
        let ts_init_array = UInt64Array::from_vec(ts_init_column);
        Chunk::new(vec![
            bid_array.to_boxed(),
            ask_array.to_boxed(),
            bid_size_array.to_boxed(),
            ask_size_array.to_boxed(),
            ts_event_array.to_boxed(),
            ts_init_array.to_boxed(),
        ])
    }
}

impl DecodeFromChunk for QuoteTick {
    fn decode(schema: &Schema, cols: Chunk<Box<dyn Array>>) -> Vec<Self> {
        let instrument_id =
            InstrumentId::from(schema.metadata.get("instrument_id").unwrap().as_str());
        let price_precision = schema
            .metadata
            .get("price_precision")
            .unwrap()
            .parse::<u8>()
            .unwrap();
        let size_precision = schema
            .metadata
            .get("size_precision")
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
        let ts_event_values = cols.arrays()[4]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let ts_init_values = cols.arrays()[5]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();

        // construct iterator of values from field value arrays
        let values = bid_values
            .into_iter()
            .zip(ask_values.into_iter())
            .zip(ask_size_values.into_iter())
            .zip(bid_size_values.into_iter())
            .zip(ts_event_values.into_iter())
            .zip(ts_init_values.into_iter())
            .map(
                |(((((bid, ask), ask_size), bid_size), ts_event), ts_init)| QuoteTick {
                    instrument_id: instrument_id.clone(),
                    bid: Price::from_raw(*bid.unwrap(), price_precision),
                    ask: Price::from_raw(*ask.unwrap(), price_precision),
                    bid_size: Quantity::from_raw(*bid_size.unwrap(), size_precision),
                    ask_size: Quantity::from_raw(*ask_size.unwrap(), size_precision),
                    ts_event: *ts_event.unwrap(),
                    ts_init: *ts_init.unwrap(),
                },
            );

        values.collect()
    }
}
