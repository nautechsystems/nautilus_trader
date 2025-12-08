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

//! Shared market data models used across REST and WebSocket handling.

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};

/// An order book level containing price and size.
#[derive(Debug, Clone, Deserialize)]
pub struct LighterBookLevel {
    #[serde(deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub size: Decimal,
}

/// Depth snapshot/delta payload returned by REST and WebSocket streams.
#[derive(Debug, Clone, Deserialize)]
pub struct LighterOrderBookDepth {
    #[serde(default)]
    pub code: Option<i32>,
    #[serde(default)]
    pub asks: Vec<LighterBookLevel>,
    #[serde(default)]
    pub bids: Vec<LighterBookLevel>,
    #[serde(default)]
    pub offset: Option<u64>,
    #[serde(default)]
    pub nonce: Option<u64>,
}

fn deserialize_decimal<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(num) => {
            Decimal::from_str_exact(&num.to_string()).map_err(serde::de::Error::custom)
        }
        serde_json::Value::String(s) => {
            Decimal::from_str_exact(&s).map_err(serde::de::Error::custom)
        }
        other => Err(serde::de::Error::custom(format!(
            "expected decimal-compatible value, found {other}"
        ))),
    }
}
