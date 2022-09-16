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

use chrono::NaiveDateTime;
use std::collections::BTreeMap;
use std::io::Read;

use arrow2::{
    array::Utf8Array,
    datatypes::DataType,
    io::csv::read::{deserialize_column, read_rows, ByteRecord, Reader, ReaderBuilder},
};

use nautilus_core::time::Timestamp;
use nautilus_model::{
    enums::OrderSide,
    data::tick::QuoteTick,
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};
use nautilus_model::data::tick::TradeTick;
use nautilus_model::identifiers::trade_id::TradeId;
use nautilus_persistence::parquet::{EncodeToChunk, ParquetWriter};

struct CsvReader<R: Read> {
    reader: Reader<R>,
    skip: usize,
}

impl<R> Iterator for CsvReader<R>
where
    R: Read,
{
    type Item = Vec<ByteRecord>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut records = vec![ByteRecord::default(); 1000];
        match read_rows(&mut self.reader, 0, &mut records) {
            Ok(0) | Err(_) => None,
            Ok(rows_read) => {
                self.skip += rows_read;
                Some(records[..rows_read].to_vec())
            }
        }
    }
}

/// Load data from a CSV file and write it to a parquet file
/// Use struct specific schema for writing
fn convert_quote_data_csv_to_parquet(src_file_path: &str, dst_file_path: &str) {
    // Create parquet writer
    let mut metadata: BTreeMap<String, String> = BTreeMap::new();
    metadata.insert("instrument_id".to_string(), "EUR/USD.SIM".to_string());
    metadata.insert("price_precision".to_string(), "5".to_string());
    metadata.insert("size_precision".to_string(), "0".to_string());

    let mut quote_tick_parquet_writer =
        ParquetWriter::<QuoteTick>::new(dst_file_path, QuoteTick::encode_schema(metadata));

    // Create CSV reader
    let csv_reader = CsvReader {
        reader: ReaderBuilder::new()
            .has_headers(false)
            .from_path(src_file_path)
            .unwrap(),
        skip: 0,
    };

    // Use predefined constant values for certain field values
    let instrument = InstrumentId::from("EUR/USD.SIM");
    let bid_size = Quantity::from_raw(100_000, 0);
    let ask_size = Quantity::from_raw(100_000, 0);
    // Closure to decode a slice of byte records into a vector of quote ticks
    let decode_records_fn = move |byte_records: &[ByteRecord]| -> Vec<QuoteTick> {
        let ts: Vec<Timestamp> = deserialize_column(byte_records, 0, DataType::Utf8, 0)
            .unwrap()
            .as_any()
            .downcast_ref::<Utf8Array<i32>>()
            .unwrap()
            .into_iter()
            .map(|ts_val| {
                ts_val.map(|ts_val| {
                    NaiveDateTime::parse_from_str(ts_val, "%Y%m%d %H%M%S%f")
                        .unwrap()
                        .timestamp_nanos() as Timestamp
                })
            })
            .collect::<Option<Vec<_>>>()
            .unwrap();
        let bid: Vec<Price> = deserialize_column(byte_records, 1, DataType::Utf8, 0)
            .unwrap()
            .as_any()
            .downcast_ref::<Utf8Array<i32>>()
            .unwrap()
            .into_iter()
            .map(|bid_val| bid_val.map(Price::from))
            .collect::<Option<Vec<_>>>()
            .unwrap();
        let ask: Vec<Price> = deserialize_column(byte_records, 2, DataType::Utf8, 0)
            .unwrap()
            .as_any()
            .downcast_ref::<Utf8Array<i32>>()
            .unwrap()
            .into_iter()
            .map(|ask_val| ask_val.map(Price::from))
            .collect::<Option<Vec<_>>>()
            .unwrap();

        // Construct iterator of values from field value arrays
        let values = ts
            .into_iter()
            .zip(bid.into_iter())
            .zip(ask.into_iter())
            .map(|((ts, bid), ask)| QuoteTick {
                instrument_id: instrument.clone(),
                bid,
                ask,
                bid_size: bid_size.clone(),
                ask_size: ask_size.clone(),
                ts_event: ts,
                ts_init: ts,
            });

        values.collect()
    };

    let csv_quote_tick = csv_reader.map(|byte_records| decode_records_fn(&byte_records));
    quote_tick_parquet_writer
        .write_bulk(csv_quote_tick)
        .unwrap();
}

fn convert_trade_data_csv_to_parquet(src_file_path: &str, dst_file_path: &str) {
    // Create parquet writer
    let mut metadata: BTreeMap<String, String> = BTreeMap::new();
    metadata.insert("instrument_id".to_string(), "ETHUSDT.BINANCE".to_string());
    metadata.insert("price_precision".to_string(), "2".to_string());
    metadata.insert("size_precision".to_string(), "5".to_string());

    let mut trade_tick_parquet_writer =
        ParquetWriter::<TradeTick>::new(dst_file_path, TradeTick::encode_schema(metadata));

    // Create CSV reader
    let csv_reader = CsvReader {
        reader: ReaderBuilder::new()
            .has_headers(false)
            .from_path(src_file_path)
            .unwrap(),
        skip: 0,
    };

    // Use predefined constant values for certain field values
    let instrument = InstrumentId::from("ETHUSDT.BINANCE");
    // let bid_size = Quantity::from_raw(100_000, 0);
    // let ask_size = Quantity::from_raw(100_000, 0);
    // Closure to decode a slice of byte records into a vector of quote ticks
    let decode_records_fn = move |byte_records: &[ByteRecord]| -> Vec<TradeTick> {
        let ts: Vec<Timestamp> = deserialize_column(byte_records, 0, DataType::Utf8, 0)
            .unwrap()
            .as_any()
            .downcast_ref::<Utf8Array<i32>>()
            .unwrap()
            .into_iter()
            .map(|ts_val| {
                ts_val.map(|ts_val| {
                    NaiveDateTime::parse_from_str(ts_val, "%Y-%m-%d %H:%M:%S.%f+00:00")
                        .unwrap()
                        .timestamp_nanos() as Timestamp
                })
            })
            .collect::<Option<Vec<_>>>()
            .unwrap();
        let price: Vec<Price> = deserialize_column(byte_records, 2, DataType::Utf8, 0)
            .unwrap()
            .as_any()
            .downcast_ref::<Utf8Array<i32>>()
            .unwrap()
            .into_iter()
            .map(|bid_val| bid_val.map(Price::from))
            .collect::<Option<Vec<_>>>()
            .unwrap();
        let size: Vec<Quantity> = deserialize_column(byte_records, 3, DataType::Utf8, 0)
            .unwrap()
            .as_any()
            .downcast_ref::<Utf8Array<i32>>()
            .unwrap()
            .into_iter()
            .map(|ask_val| ask_val.map(Quantity::from))
            .collect::<Option<Vec<_>>>()
            .unwrap();

        // Construct iterator of values from field value arrays
        let values = ts
            .into_iter()
            .zip(price.into_iter())
            .zip(size.into_iter())
            .map(|((ts, price), size)| TradeTick {
                instrument_id: instrument.clone(),
                price,
                size,
                aggressor_side: OrderSide::Buy,
                trade_id: TradeId::new("123456"),
                ts_event: ts,
                ts_init: ts,
            });

        values.collect()
    };

    let csv_trade_tick = csv_reader.map(|byte_records| decode_records_fn(&byte_records));
    trade_tick_parquet_writer
        .write_bulk(csv_trade_tick)
        .unwrap();
}

fn main() {
    let quote_csv_data_path = "../tests/test_kit/data/quote_tick_data.csv";
    let quote_parquet_data_path = "../tests/test_kit/data/quote_tick_data.parquet";
    convert_quote_data_csv_to_parquet(quote_csv_data_path, quote_parquet_data_path);

    let trade_csv_data_path = "../tests/test_kit/data/binance-ethusdt-trades.csv";
    let trade_parquet_data_path = "../tests/test_kit/data/binance-ethusdt-trades.parquet";
    convert_trade_data_csv_to_parquet(trade_csv_data_path, trade_parquet_data_path);
}
