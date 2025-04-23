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

/// Custom deserializer function for hex numbers.
pub fn deserialize_hex_number<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let hex_string = String::deserialize(deserializer)?;
    let without_prefix = hex_string.trim_start_matches("0x");

    u64::from_str_radix(without_prefix, 16).map_err(serde::de::Error::custom)
}

/// Custom deserializer function for hex timestamps to convert hex seconds to `UnixNanos`.
pub fn deserialize_hex_timestamp<'de, D>(deserializer: D) -> Result<UnixNanos, D::Error>
where
    D: Deserializer<'de>,
{
    let hex_string = String::deserialize(deserializer)?;
    let without_prefix = hex_string.trim_start_matches("0x");

    u64::from_str_radix(without_prefix, 16)
        .map(|num| UnixNanos::new(num * NANOSECONDS_IN_SECOND))
        .map_err(serde::de::Error::custom)
}
