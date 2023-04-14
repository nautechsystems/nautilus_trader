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

use datafusion::arrow::datatypes::*;
use datafusion::arrow::{datatypes::SchemaRef, record_batch::RecordBatch};
use nautilus_model::data::tick::TradeTick;
use nautilus_model::enums::AggressorSide;
use nautilus_model::identifiers::trade_id::TradeId;
use nautilus_model::{
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

use crate::parquet::{Data, DecodeDataFromRecordBatch};

impl DecodeDataFromRecordBatch for TradeTick {
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

        use datafusion::arrow::array::*;

        // Extract field value arrays from record batch
        let cols = record_batch.columns();
        let price_values = cols[0].as_any().downcast_ref::<Int64Array>().unwrap();
        let size_values = cols[1].as_any().downcast_ref::<UInt64Array>().unwrap();
        let aggressor_side_values = cols[2].as_any().downcast_ref::<UInt8Array>().unwrap();
        let trade_id_values_values = cols[3].as_any().downcast_ref::<StringArray>().unwrap();
        let ts_event_values = cols[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = cols[5].as_any().downcast_ref::<UInt64Array>().unwrap();

        // Construct iterator of values from field value arrays
        let values = price_values
            .into_iter()
            .zip(size_values.into_iter())
            .zip(aggressor_side_values.into_iter())
            .zip(trade_id_values_values.into_iter())
            .zip(ts_event_values.into_iter())
            .zip(ts_init_values.into_iter())
            .map(
                |(((((price, size), aggressor_side), trade_id), ts_event), ts_init)| {
                    TradeTick {
                        instrument_id: instrument_id.clone(),
                        price: Price::from_raw(price.unwrap(), price_precision),
                        size: Quantity::from_raw(size.unwrap(), size_precision),
                        aggressor_side: AggressorSide::from_repr(aggressor_side.unwrap() as usize)
                            .expect("cannot parse enum value"),
                        trade_id: TradeId::new(trade_id.unwrap()),
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
            Field::new("price", DataType::Int64, false),
            Field::new("size", DataType::UInt64, false),
            Field::new("aggressor_side", DataType::UInt8, false),
            Field::new("trade_id", DataType::Utf8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        Schema::new_with_metadata(fields, metadata).into()
    }
}
