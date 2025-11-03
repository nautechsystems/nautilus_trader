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

//! Query parameter builders for dYdX v4 Indexer REST API endpoints.
//!
//! This module provides type-safe builders for constructing query parameters
//! that are sent to various dYdX Indexer API endpoints.

use derive_builder::Builder;
use serde::Serialize;

// TODO: Add query parameter builders for:
// - GetPerpetualMarketsParams
// - GetOrderbookParams
// - GetTradesParams
// - GetCandlesParams
// - GetSubaccountParams
// - GetOrdersParams
// - GetFillsParams
// etc.

/// Query parameters for fetching markets from the dYdX Indexer API.
#[derive(Debug, Clone, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetMarketsParams {
    /// Optional ticker filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    /// Optional limit for number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}
