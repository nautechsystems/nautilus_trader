// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND};
use serde::{Deserialize, Deserializer};

/// Converts a hexadecimal string to a u64 integer.
///
/// # Errors
///
/// Returns a `std::num::ParseIntError` if:
/// - The input string contains non-hexadecimal characters
/// - The hexadecimal value is too large to fit in a u64
pub fn from_str_hex_to_u64(hex_string: &str) -> Result<u64, std::num::ParseIntError> {
    let without_prefix = hex_string.trim_start_matches("0x");
    u64::from_str_radix(without_prefix, 16)
}

/// Custom deserializer function for hex numbers.
///
/// # Errors
///
/// Returns an error if parsing the hex string to a number fails.
pub fn deserialize_hex_number<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let hex_string = String::deserialize(deserializer)?;
    from_str_hex_to_u64(hex_string.as_str()).map_err(serde::de::Error::custom)
}

/// Custom deserializer function for hex timestamps to convert hex seconds to `UnixNanos`.
///
/// # Errors
///
/// Returns an error if parsing the hex string to a timestamp fails.
pub fn deserialize_hex_timestamp<'de, D>(deserializer: D) -> Result<UnixNanos, D::Error>
where
    D: Deserializer<'de>,
{
    let hex_string = String::deserialize(deserializer)?;
    from_str_hex_to_u64(hex_string.as_str())
        .map(|num| UnixNanos::new(num * NANOSECONDS_IN_SECOND))
        .map_err(serde::de::Error::custom)
}
