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
    array::{Array, Int64Array, UInt64Array, UInt8Array, Utf8Array},
    chunk::Chunk,
    datatypes::{DataType, Field, Schema},
    io::parquet::write::{transverse, Encoding},
};
use nautilus_model::data::tick::TradeTick;
use nautilus_model::enums::AggressorSide;
use nautilus_model::identifiers::trade_id::TradeId;
use nautilus_model::{
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

use crate::parquet::{DecodeFromChunk, EncodeToChunk};

impl EncodeToChunk for TradeTick {
    fn assert_metadata(metadata: &BTreeMap<String, String>) {
        let keys = ["instrument_id", "price_precision", "size_precision"];
        for key in keys {
            (!metadata.contains_key(key)).then(|| panic!("metadata missing key {key}"));
        }
    }

    fn encodings(metadata: BTreeMap<String, String>) -> Vec<Vec<Encoding>> {
        TradeTick::encode_schema(metadata)
            .fields
            .iter()
            .map(|f| transverse(&f.data_type, |_| Encoding::Plain))
            .collect()
    }

    fn encode_schema(metadata: BTreeMap<String, String>) -> Schema {
        Self::assert_metadata(&metadata);
        let fields = vec![
            Field::new("price", DataType::Int64, false),
            Field::new("size", DataType::UInt64, false),
            Field::new("aggressor_side", DataType::UInt8, false),
            Field::new("trade_id", DataType::Utf8, false),
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
            mut price_column,
            mut size_column,
            mut aggressor_side_column,
            mut trade_id_column,
            mut ts_event_column,
            mut ts_init_column,
        ): (Vec<i64>, Vec<u64>, Vec<u8>, Vec<&str>, Vec<u64>, Vec<u64>) =
            (vec![], vec![], vec![], vec![], vec![], vec![]);

        data.fold((), |(), tick| {
            price_column.push(tick.price.raw);
            size_column.push(tick.size.raw);
            aggressor_side_column.push(tick.aggressor_side as u8);
            trade_id_column.push(&tick.trade_id.value);
            ts_event_column.push(tick.ts_event);
            ts_init_column.push(tick.ts_init);
        });

        let price_array = Int64Array::from_vec(price_column);
        let size_array = UInt64Array::from_vec(size_column);
        let aggressor_side_array = UInt8Array::from_vec(aggressor_side_column);
        let trade_id_array = Utf8Array::<i32>::from_iter_values(trade_id_column.into_iter());
        let ts_event_array = UInt64Array::from_vec(ts_event_column);
        let ts_init_array = UInt64Array::from_vec(ts_init_column);
        Chunk::new(vec![
            price_array.to_boxed(),
            size_array.to_boxed(),
            aggressor_side_array.to_boxed(),
            trade_id_array.to_boxed(),
            ts_event_array.to_boxed(),
            ts_init_array.to_boxed(),
        ])
    }
}

impl DecodeFromChunk for TradeTick {
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
        let price_values = cols.arrays()[0]
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        let size_values = cols.arrays()[1]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let aggressor_side_values = cols.arrays()[2]
            .as_any()
            .downcast_ref::<UInt8Array>()
            .unwrap();
        let trade_id_values_values = cols.arrays()[3]
            .as_any()
            .downcast_ref::<Utf8Array<i32>>()
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
        let values = price_values
            .into_iter()
            .zip(size_values.into_iter())
            .zip(aggressor_side_values.into_iter())
            .zip(trade_id_values_values.into_iter())
            .zip(ts_event_values.into_iter())
            .zip(ts_init_values.into_iter())
            .map(
                |(((((price, size), aggressor_side), trade_id), ts_event), ts_init)| TradeTick {
                    instrument_id: instrument_id.clone(),
                    price: Price::from_raw(*price.unwrap(), price_precision),
                    size: Quantity::from_raw(*size.unwrap(), size_precision),
                    aggressor_side: AggressorSide::from_repr(*aggressor_side.unwrap() as usize)
                        .expect("cannot parse enum value"),
                    trade_id: TradeId::new(trade_id.unwrap()),
                    ts_event: *ts_event.unwrap(),
                    ts_init: *ts_init.unwrap(),
                },
            );

        values.collect()
    }
}
