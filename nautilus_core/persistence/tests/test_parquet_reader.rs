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

use std::{collections::BTreeMap, fs::File, io::Cursor};

use nautilus_model::{
    data::tick::{QuoteTick, TradeTick},
    identifiers::trade_id::TradeId,
    types::{price::Price, quantity::Quantity},
};
use nautilus_persistence::parquet::{EncodeToChunk, GroupFilterArg, ParquetReader, ParquetWriter};

mod test_util;

#[test]
fn test_parquet_reader_native_quote_ticks() {
    let file_path = "../../tests/test_data/quote_tick_data.parquet";
    let file = File::open(file_path).expect("Unable to open given file");

    let reader: ParquetReader<QuoteTick, File> =
        ParquetReader::new(file, 100, GroupFilterArg::None);
    let data: Vec<QuoteTick> = reader.flatten().collect();

    assert_eq!("EUR/USD.SIM", data[0].instrument_id.to_string());
    assert_eq!(data.len(), 9500);
}

#[test]
fn test_parquet_trade_ticks_round_trip() {
    let file_path = "../../tests/test_data/trade_tick_data.parquet";
    let file = File::open(file_path).expect("Unable to open given file");

    let reader: ParquetReader<TradeTick, File> =
        ParquetReader::new(file, 1000, GroupFilterArg::None);
    let data: Vec<TradeTick> = reader.flatten().collect();

    let metadata: BTreeMap<String, String> = BTreeMap::from([
        ("instrument_id".to_string(), "EUR/USD.SIM".to_string()),
        ("price_precision".to_string(), "5".to_string()),
        ("size_precision".to_string(), "0".to_string()),
    ]);
    let schema = TradeTick::encode_schema(metadata);
    let mut writer: ParquetWriter<TradeTick, Vec<u8>> = ParquetWriter::new(Vec::new(), schema);
    writer.write(&data).unwrap();
    let buffer = writer.flush();

    let buf_reader: ParquetReader<TradeTick, Cursor<&[u8]>> =
        ParquetReader::new(Cursor::new(&buffer), 1000, GroupFilterArg::None);
    let buf_data: Vec<TradeTick> = buf_reader.flatten().collect();

    assert_eq!(buf_data, data);
}

#[test]
fn test_parquet_reader_native_trade_ticks() {
    let file_path = "../../tests/test_data/trade_tick_data.parquet";
    let file = File::open(file_path).expect("Unable to open given file");

    let reader: ParquetReader<TradeTick, File> =
        ParquetReader::new(file, 100, GroupFilterArg::None);
    let data: Vec<TradeTick> = reader.flatten().collect();

    assert_eq!("EUR/USD.SIM", data[0].instrument_id.to_string());
    assert_eq!(data.len(), 100);
}

#[test]
#[ignore] // Temporarily flaky
fn test_parquet_filter() {
    use rand::Rng;

    let mut rng = rand::thread_rng();
    let len = 10234;
    let ts_init_cutoff = 11;
    let data1 = vec![
        TradeTick {
            instrument_id: "EUR/USD.DUKA".into(),
            price: Price::new(rng.gen_range(1.2..1.5), 4),
            size: Quantity::new(40.0, 0),
            ts_event: 0,
            ts_init: rng.gen_range(1..10),
            aggressor_side: nautilus_model::enums::AggressorSide::Buyer,
            trade_id: TradeId::new("hey")
        };
        len
    ];
    let data2 = vec![
        TradeTick {
            instrument_id: "EUR/USD.DUKA".into(),
            price: Price::new(rng.gen_range(1.2..1.5), 4),
            size: Quantity::new(40.0, 0),
            ts_event: 0,
            ts_init: rng.gen_range(11..24),
            aggressor_side: nautilus_model::enums::AggressorSide::Buyer,
            trade_id: TradeId::new("hey")
        };
        len
    ];

    let mut metadata: BTreeMap<String, String> = BTreeMap::new();
    metadata.insert("instrument_id".to_string(), "EUR/USD.DUKA".to_string());
    metadata.insert("price_precision".to_string(), "4".to_string());
    metadata.insert("size_precision".to_string(), "4".to_string());
    let mut writer: ParquetWriter<TradeTick, Vec<u8>> =
        ParquetWriter::new(Vec::new(), TradeTick::encode_schema(metadata));

    writer.write(&data1).unwrap();
    writer.write(&data2).unwrap();

    let buffer = writer.flush();
    let filtered_reader: ParquetReader<TradeTick, Cursor<&[u8]>> = ParquetReader::new(
        Cursor::new(&buffer),
        1000,
        GroupFilterArg::TsInitGt(ts_init_cutoff),
    );
    let data_filtered: Vec<TradeTick> = filtered_reader
        .flat_map(|ticks| ticks.into_iter())
        .collect();
    let unfiltered_reader: ParquetReader<TradeTick, Cursor<&[u8]>> =
        ParquetReader::new(Cursor::new(&buffer), 1000, GroupFilterArg::None);
    let data_unfiltered: Vec<TradeTick> = unfiltered_reader
        .flat_map(|ticks| ticks.into_iter())
        .collect();

    assert_eq!(data_filtered.len(), len);
    assert_eq!(data_unfiltered.len(), len + len);
    assert!(
        data_filtered.len() < data_unfiltered.len(),
        "Filtered data must be less than unfiltered data"
    );
    assert!(data_filtered
        .iter()
        .all(|tick| tick.ts_init > ts_init_cutoff),);
}
