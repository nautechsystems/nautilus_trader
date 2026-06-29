// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

#![allow(
    dead_code,
    reason = "bench targets share this module but use different setup functions"
)]

use std::path::{Path, PathBuf};

use databento::dbn::{
    self,
    decode::{DecodeStream, dbn::Decoder},
};
use fallible_streaming_iterator::FallibleStreamingIterator;
use nautilus_databento::loader::DatabentoDataLoader;
use nautilus_model::identifiers::{InstrumentId, Symbol};

pub(crate) const PRICE_PRECISION: u8 = 2;

pub(crate) fn instrument_id() -> InstrumentId {
    InstrumentId::from("ESM4.GLBX")
}

pub(crate) fn large_mbo_instrument_id() -> InstrumentId {
    InstrumentId::from("ESH4.GLBX")
}

pub(crate) fn data_path(filename: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(filename)
}

pub(crate) fn repository_path(relative_path: impl AsRef<Path>) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative_path)
}

pub(crate) fn publishers_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("publishers.json")
}

pub(crate) fn loader() -> DatabentoDataLoader {
    let mut loader = DatabentoDataLoader::new(Some(publishers_path())).unwrap();
    loader.set_price_precision(Symbol::from("ESM4"), PRICE_PRECISION);
    loader
}

pub(crate) fn first_record<T>(filename: &str) -> T
where
    T: dbn::Record + dbn::HasRType + Clone + 'static,
{
    let path = data_path(filename);
    let decoder = Decoder::from_zstd_file(&path).unwrap();
    let mut stream = decoder.decode_stream::<T>();
    stream.advance().unwrap();
    stream
        .get()
        .unwrap_or_else(|| panic!("fixture {filename} contains no records"))
        .clone()
}

pub(crate) fn record_count<T>(path: &Path) -> u64
where
    T: dbn::Record + dbn::HasRType + 'static,
{
    let decoder = Decoder::from_zstd_file(path).unwrap();
    let mut stream = decoder.decode_stream::<T>();
    let mut count = 0;

    loop {
        stream.advance().unwrap();
        if stream.get().is_none() {
            break;
        }
        count += 1;
    }

    count
}
