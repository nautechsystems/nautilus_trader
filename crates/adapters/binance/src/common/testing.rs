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

//! Test fixtures and utilities for Binance adapter tests.

use std::{fs, path::PathBuf};

use serde_json::Value;

/// Returns the path to the adapter's `test_data` directory.
pub fn test_data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

/// Loads a text fixture from the adapter's `test_data` directory.
///
/// # Panics
///
/// Panics if the fixture file cannot be read.
pub fn load_fixture_string(relpath: &str) -> String {
    let path = test_data_path().join(relpath);
    fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("Failed to read Binance test fixture: {}", path.display()))
}

/// Loads a JSON fixture from the adapter's `test_data` directory.
///
/// # Panics
///
/// Panics if the fixture file cannot be read or contains invalid JSON.
pub fn load_json_fixture(relpath: &str) -> Value {
    let content = load_fixture_string(relpath);
    serde_json::from_str(&content).expect("Invalid JSON in Binance test fixture")
}

/// Loads a binary fixture from the adapter's `test_data` directory.
///
/// # Panics
///
/// Panics if the fixture file cannot be read.
pub fn load_fixture_bytes(relpath: &str) -> Vec<u8> {
    let path = test_data_path().join(relpath);
    fs::read(&path)
        .unwrap_or_else(|_| panic!("Failed to read Binance test fixture: {}", path.display()))
}

/// Loads a JSON fixture and returns the nested `event` object when present.
///
/// Binance docs examples for user-data streams wrap the payload in an
/// outer object containing `subscriptionId` and `event`.
///
/// # Panics
///
/// Panics if the fixture file cannot be read or contains invalid JSON.
pub fn load_event_fixture(relpath: &str) -> Value {
    let json = load_json_fixture(relpath);
    json.get("event").cloned().unwrap_or(json)
}
