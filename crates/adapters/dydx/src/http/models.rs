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

//! Data models for dYdX v4 Indexer REST API responses.
//!
//! This module contains Rust types that mirror the JSON structures returned
//! by the dYdX v4 Indexer API endpoints.

use serde::{Deserialize, Serialize};

// TODO: Add data models for:
// - Markets (PerpetualMarketResponse, OrderBookResponse, TradeResponse, CandleResponse)
// - Accounts (SubaccountResponse, PositionResponse, OrderResponse, FillResponse)
// - Utility (TimeResponse, HeightResponse, ComplianceResponse)

/// Placeholder for market data models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxMarketData {
    /// Market ticker symbol.
    pub ticker: String,
}

/// Placeholder for account data models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxAccountData {
    /// Wallet address.
    pub address: String,
    /// Subaccount number.
    pub subaccount_number: u32,
}
