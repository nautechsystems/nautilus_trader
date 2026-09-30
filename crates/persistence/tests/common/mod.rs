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

use std::path::{Path, PathBuf};

use parquet::{
    basic::Compression,
    file::reader::{FileReader, SerializedFileReader},
};

/// Returns the distinct codec run of the column chunks in each Parquet file under `root`, keyed by path.
#[allow(
    dead_code,
    reason = "each test binary uses its own subset of these functions"
)]
pub(crate) fn parquet_codecs(root: &Path) -> Vec<(PathBuf, Vec<Compression>)> {
    let mut files = Vec::new();
    collect_parquet_files(root, &mut files);
    files.sort();

    files
        .into_iter()
        .map(|path| {
            let reader = SerializedFileReader::new(std::fs::File::open(&path).unwrap()).unwrap();
            let mut codecs: Vec<Compression> = reader
                .metadata()
                .row_groups()
                .iter()
                .flat_map(|group| group.columns().iter().map(|column| column.compression()))
                .collect();
            codecs.dedup();
            (path, codecs)
        })
        .collect()
}

fn collect_parquet_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();

        if path.is_dir() {
            collect_parquet_files(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "parquet")
        {
            files.push(path);
        }
    }
}

#[allow(
    dead_code,
    reason = "each test binary uses its own subset of these functions"
)]
pub(crate) fn timescale_test_uri() -> Option<String> {
    let uri = std::env::var("TIMESCALE_TEST_DATABASE_URL")
        .ok()
        .filter(|uri| !uri.trim().is_empty());
    if uri.is_some() {
        return uri;
    }

    assert!(
        std::env::var("NAUTILUS_REQUIRE_TIMESCALE").as_deref() != Ok("1"),
        "SKIPPED: TIMESCALE_TEST_DATABASE_URL is unset while NAUTILUS_REQUIRE_TIMESCALE=1"
    );

    eprintln!("SKIPPED: TIMESCALE_TEST_DATABASE_URL is unset");
    None
}
