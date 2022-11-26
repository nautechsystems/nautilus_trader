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
//
use pyo3::{prelude::*, types::*, AsPyPointer};

use nautilus_core::cvec::CVec;
use nautilus_persistence::parquet::{
    parquet_reader_drop, parquet_reader_drop_chunk, parquet_reader_new, parquet_reader_next_chunk,
    ParquetType,
};

mod test_util;

#[test]
#[allow(unused_assignments)]
fn test_parquet_reader() {
    pyo3::prepare_freethreaded_python();

    let file_path = "../../tests/test_data/quote_tick_data.parquet";

    // return an opaque reader pointer
    let reader = Python::with_gil(|py| {
        let file_path = PyString::new(py, file_path);
        unsafe { parquet_reader_new(file_path.as_ptr(), ParquetType::QuoteTick, 100) }
    });

    let mut total = 0;
    let mut chunk = CVec::default();
    unsafe {
        loop {
            chunk = parquet_reader_next_chunk(reader, ParquetType::QuoteTick);
            if chunk.len == 0 {
                parquet_reader_drop_chunk(chunk, ParquetType::QuoteTick);
                break;
            } else {
                total += chunk.len;
                parquet_reader_drop_chunk(chunk, ParquetType::QuoteTick);
            }
        }
    }

    unsafe {
        parquet_reader_drop(reader, ParquetType::QuoteTick);
    }

    assert_eq!(total, 9500);
}
