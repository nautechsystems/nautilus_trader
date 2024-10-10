// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use serde::Deserialize;

/// Represents a Tardis format order book update record.
#[derive(Debug, Deserialize)]
pub struct TardisBookUpdateRecord {
    /// The exchande ID.
    pub exchange: String,
    /// The instrument symbol as provided by the exchange.
    pub symbol: String,
    // UNIX microseconds timestamp provided by the exchange.
    pub timestamp: u64,
    // UNIX microseconds timestamp of message received.
    pub local_timestamp: u64,
    /// If update was a part of initial order book snapshot.
    pub is_snapshot: bool,
    /// The book side the update belongs to.
    pub side: String,
    /// The price identifying book level being updated.
    pub price: f64,
    /// The updated price level amount.
    pub amount: f64,
}

/// Represents a Tardis format quote record.
#[derive(Debug, Deserialize)]
pub struct TardisQuoteRecord {
    /// The exchande ID.
    pub exchange: String,
    /// The instrument symbol as provided by the exchange.
    pub symbol: String,
    // UNIX microseconds timestamp provided by the exchange.
    pub timestamp: u64,
    // UNIX microseconds timestamp of message received.
    pub local_timestamp: u64,
    // The best ask amount as provided by exchange, empty if there aren't any asks.
    pub ask_amount: Option<f64>,
    // The best ask price as provided by exchange, empty if there aren't any asks.
    pub ask_price: Option<f64>,
    // The best bid price as provided by exchange, empty if there aren't any bids.
    pub bid_price: Option<f64>,
    // The best bid amount as provided by exchange, empty if there aren't any bids.
    pub bid_amount: Option<f64>,
}

/// Represents a Tardis format trade record.
#[derive(Debug, Deserialize)]
pub struct TardisTradeRecord {
    /// The exchande ID.
    pub exchange: String,
    /// The instrument symbol as provided by the exchange.
    pub symbol: String,
    // UNIX microseconds timestamp provided by the exchange.
    pub timestamp: u64,
    // UNIX microseconds timestamp of message received.
    pub local_timestamp: u64,
    /// The trade ID provided by the exchange.
    pub id: String,
    /// The liquidity taker (aggressor) side provided by the exchange.
    pub side: String,
    /// The trade price as provided by the exchange.
    pub price: f64,
    /// The trade amount as provided by the exchange.
    pub amount: f64,
}
