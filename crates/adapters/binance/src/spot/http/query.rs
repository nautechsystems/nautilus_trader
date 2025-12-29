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

//! Query parameter builders for Binance Spot HTTP requests.

use serde::Serialize;

/// Query parameters for the depth endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct DepthParams {
    /// Trading pair symbol (e.g., "BTCUSDT").
    pub symbol: String,
    /// Number of price levels to return (default 100, max 5000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl DepthParams {
    /// Create new depth query params.
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            limit: None,
        }
    }

    /// Set the limit.
    #[must_use]
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Query parameters for the trades endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct TradesParams {
    /// Trading pair symbol (e.g., "BTCUSDT").
    pub symbol: String,
    /// Number of trades to return (default 500, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl TradesParams {
    /// Create new trades query params.
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            limit: None,
        }
    }

    /// Set the limit.
    #[must_use]
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}
