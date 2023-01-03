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

use std::{ffi::CString, fs::File};

use nautilus_core::cvec::CVec;
use nautilus_model::data::tick::QuoteTick;
use nautilus_persistence::parquet::{
    parquet_reader_drop_chunk, parquet_reader_file_new, parquet_reader_free,
    parquet_reader_next_chunk, GroupFilterArg, ParquetReader, ParquetReaderType, ParquetType,
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
    let data: Vec<QuoteTick> = reader.flat_map(|v| v).collect();

    assert_eq!("EUR/USD.SIM", data[0].instrument_id.to_string());
    assert_eq!(data.len(), 9500);
}
