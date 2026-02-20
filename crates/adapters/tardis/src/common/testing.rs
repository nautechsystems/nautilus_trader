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

//! Test utilities for the Tardis adapter.

use std::{fs, path::PathBuf};

/// Returns the path to a test data file.
///
/// # Panics
///
/// Panics if the file does not exist.
#[cfg(test)]
#[must_use]
pub fn get_test_data_path(file_name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(file_name);

    assert!(path.exists(), "Test data file not found: {path:?}");
    path
}

/// Loads a JSON fixture from the adapter test data directory.
///
/// # Panics
///
/// Panics if the test file cannot be read.
#[cfg(test)]
#[must_use]
pub fn load_test_json(file_name: &str) -> String {
    let path = get_test_data_path(file_name);
    fs::read_to_string(path).expect("Failed to read test JSON file")
}
