// // -------------------------------------------------------------------------------------------------
// //  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
// //  https://nautechsystems.io
// //
// //  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
// //  You may not use this file except in compliance with the License.
// //  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// //
// //  Unless required by applicable law or agreed to in writing, software
// //  distributed under the License is distributed on an "AS IS" BASIS,
// //  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// //  See the License for the specific language governing permissions and
// //  limitations under the License.
// // -------------------------------------------------------------------------------------------------

use std::{collections::BTreeMap, ffi::CString, fs::File, io::Cursor};

use nautilus_core::cvec::CVec;
use nautilus_model::{
    data::tick::{QuoteTick, TradeTick},
    identifiers::trade_id::TradeId,
    types::{price::Price, quantity::Quantity},
};
use nautilus_persistence::parquet::{
    parquet_reader_drop_chunk, parquet_reader_file_new, parquet_reader_free,
    parquet_reader_next_chunk, EncodeToChunk, GroupFilterArg, ParquetReader, ParquetReaderType,
    ParquetType, ParquetWriter,
};

mod test_util;

#[test]
#[allow(unused_assignments)]
fn test_parquet_reader_ffi() {
    let file_path = CString::new("../../tests/test_data/quote_tick_data.parquet").unwrap();

    // return an opaque reader pointer
    let reader =
        unsafe { parquet_reader_file_new(file_path.as_ptr(), ParquetType::QuoteTick, 100) };

    let mut total = 0;
    let mut chunk = CVec::empty();
    let mut data: Vec<CVec> = Vec::new();
    unsafe {
        loop {
            chunk =
                parquet_reader_next_chunk(reader, ParquetType::QuoteTick, ParquetReaderType::File);
            if chunk.len == 0 {
                parquet_reader_drop_chunk(chunk, ParquetType::QuoteTick);
                break;
            } else {
                total += chunk.len;
                data.push(chunk);
            }
        }
    }

    let test_tick = unsafe { &*(data[0].ptr as *mut QuoteTick) };

    assert_eq!("EUR/USD.SIM", test_tick.instrument_id.to_string());
    assert_eq!(total, 9500);

    unsafe {
        data.into_iter()
            .for_each(|chunk| parquet_reader_drop_chunk(chunk, ParquetType::QuoteTick));
        parquet_reader_free(reader, ParquetType::QuoteTick, ParquetReaderType::File);
    }
}

#[test]
fn test_parquet_reader_native() {
    let file_path = "../../tests/test_data/quote_tick_data.parquet";
    let file = File::open(file_path).expect("Unable to open given file");

    let reader: ParquetReader<QuoteTick, File> =
        ParquetReader::new(file, 100, GroupFilterArg::None);
    let data: Vec<QuoteTick> = reader.flatten().collect();

    assert_eq!("EUR/USD.SIM", data[0].instrument_id.to_string());
    assert_eq!(data.len(), 9500);
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
