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

use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    enums::Exchange,
    parse::{deserialize_trade_id, deserialize_uppercase},
};

/// Represents a Tardis format order book update record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TardisBookUpdateRecord {
    /// The exchange ID.
    pub exchange: Exchange,
    /// The instrument symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
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

/// Represents a Tardis format order book 5 level snapshot record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TardisOrderBookSnapshot5Record {
    /// The exchange ID.
    pub exchange: Exchange,
    /// The instrument symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    // UNIX microseconds timestamp provided by the exchange.
    pub timestamp: u64,
    // UNIX microseconds timestamp of message received.
    pub local_timestamp: u64,
    /// The price of the first ask.
    pub asks_0_price: Option<f64>,
    /// The amount of the first ask.
    pub asks_0_amount: Option<f64>,
    /// The price of the first bid.
    pub bids_0_price: Option<f64>,
    /// The amount of the first bid.
    pub bids_0_amount: Option<f64>,
    /// The price of the second ask.
    pub asks_1_price: Option<f64>,
    /// The amount of the second ask.
    pub asks_1_amount: Option<f64>,
    /// The price of the second bid.
    pub bids_1_price: Option<f64>,
    /// The amount of the second bid.
    pub bids_1_amount: Option<f64>,
    /// The price of the third ask.
    pub asks_2_price: Option<f64>,
    /// The amount of the third ask.
    pub asks_2_amount: Option<f64>,
    /// The price of the third bid.
    pub bids_2_price: Option<f64>,
    /// The amount of the third bid.
    pub bids_2_amount: Option<f64>,
    /// The price of the fourth ask.
    pub asks_3_price: Option<f64>,
    /// The amount of the fourth ask.
    pub asks_3_amount: Option<f64>,
    /// The price of the fourth bid.
    pub bids_3_price: Option<f64>,
    /// The amount of the fourth bid.
    pub bids_3_amount: Option<f64>,
    /// The price of the fifth ask.
    pub asks_4_price: Option<f64>,
    /// The amount of the fifth ask.
    pub asks_4_amount: Option<f64>,
    /// The price of the fifth bid.
    pub bids_4_price: Option<f64>,
    /// The amount of the fifth bid.
    pub bids_4_amount: Option<f64>,
}

/// Represents a Tardis format order book 25 level snapshot record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TardisOrderBookSnapshot25Record {
    /// The exchange ID.
    pub exchange: Exchange,
    /// The instrument symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    // UNIX microseconds timestamp provided by the exchange.
    pub timestamp: u64,
    // UNIX microseconds timestamp of message received.
    pub local_timestamp: u64,

    pub asks_0_price: Option<f64>,
    pub asks_0_amount: Option<f64>,
    pub bids_0_price: Option<f64>,
    pub bids_0_amount: Option<f64>,

    pub asks_1_price: Option<f64>,
    pub asks_1_amount: Option<f64>,
    pub bids_1_price: Option<f64>,
    pub bids_1_amount: Option<f64>,

    pub asks_2_price: Option<f64>,
    pub asks_2_amount: Option<f64>,
    pub bids_2_price: Option<f64>,
    pub bids_2_amount: Option<f64>,

    pub asks_3_price: Option<f64>,
    pub asks_3_amount: Option<f64>,
    pub bids_3_price: Option<f64>,
    pub bids_3_amount: Option<f64>,

    pub asks_4_price: Option<f64>,
    pub asks_4_amount: Option<f64>,
    pub bids_4_price: Option<f64>,
    pub bids_4_amount: Option<f64>,

    pub asks_5_price: Option<f64>,
    pub asks_5_amount: Option<f64>,
    pub bids_5_price: Option<f64>,
    pub bids_5_amount: Option<f64>,

    pub asks_6_price: Option<f64>,
    pub asks_6_amount: Option<f64>,
    pub bids_6_price: Option<f64>,
    pub bids_6_amount: Option<f64>,

    pub asks_7_price: Option<f64>,
    pub asks_7_amount: Option<f64>,
    pub bids_7_price: Option<f64>,
    pub bids_7_amount: Option<f64>,

    pub asks_8_price: Option<f64>,
    pub asks_8_amount: Option<f64>,
    pub bids_8_price: Option<f64>,
    pub bids_8_amount: Option<f64>,

    pub asks_9_price: Option<f64>,
    pub asks_9_amount: Option<f64>,
    pub bids_9_price: Option<f64>,
    pub bids_9_amount: Option<f64>,

    pub asks_10_price: Option<f64>,
    pub asks_10_amount: Option<f64>,
    pub bids_10_price: Option<f64>,
    pub bids_10_amount: Option<f64>,

    pub asks_11_price: Option<f64>,
    pub asks_11_amount: Option<f64>,
    pub bids_11_price: Option<f64>,
    pub bids_11_amount: Option<f64>,

    pub asks_12_price: Option<f64>,
    pub asks_12_amount: Option<f64>,
    pub bids_12_price: Option<f64>,
    pub bids_12_amount: Option<f64>,

    pub asks_13_price: Option<f64>,
    pub asks_13_amount: Option<f64>,
    pub bids_13_price: Option<f64>,
    pub bids_13_amount: Option<f64>,

    pub asks_14_price: Option<f64>,
    pub asks_14_amount: Option<f64>,
    pub bids_14_price: Option<f64>,
    pub bids_14_amount: Option<f64>,

    pub asks_15_price: Option<f64>,
    pub asks_15_amount: Option<f64>,
    pub bids_15_price: Option<f64>,
    pub bids_15_amount: Option<f64>,

    pub asks_16_price: Option<f64>,
    pub asks_16_amount: Option<f64>,
    pub bids_16_price: Option<f64>,
    pub bids_16_amount: Option<f64>,

    pub asks_17_price: Option<f64>,
    pub asks_17_amount: Option<f64>,
    pub bids_17_price: Option<f64>,
    pub bids_17_amount: Option<f64>,

    pub asks_18_price: Option<f64>,
    pub asks_18_amount: Option<f64>,
    pub bids_18_price: Option<f64>,
    pub bids_18_amount: Option<f64>,

    pub asks_19_price: Option<f64>,
    pub asks_19_amount: Option<f64>,
    pub bids_19_price: Option<f64>,
    pub bids_19_amount: Option<f64>,

    pub asks_20_price: Option<f64>,
    pub asks_20_amount: Option<f64>,
    pub bids_20_price: Option<f64>,
    pub bids_20_amount: Option<f64>,

    pub asks_21_price: Option<f64>,
    pub asks_21_amount: Option<f64>,
    pub bids_21_price: Option<f64>,
    pub bids_21_amount: Option<f64>,

    pub asks_22_price: Option<f64>,
    pub asks_22_amount: Option<f64>,
    pub bids_22_price: Option<f64>,
    pub bids_22_amount: Option<f64>,

    pub asks_23_price: Option<f64>,
    pub asks_23_amount: Option<f64>,
    pub bids_23_price: Option<f64>,
    pub bids_23_amount: Option<f64>,

    pub asks_24_price: Option<f64>,
    pub asks_24_amount: Option<f64>,
    pub bids_24_price: Option<f64>,
    pub bids_24_amount: Option<f64>,
}

/// Represents a Tardis format quote record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TardisQuoteRecord {
    /// The exchande ID.
    pub exchange: Exchange,
    /// The instrument symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TardisTradeRecord {
    /// The exchande ID.
    pub exchange: Exchange,
    /// The instrument symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    // UNIX microseconds timestamp provided by the exchange.
    pub timestamp: u64,
    // UNIX microseconds timestamp of message received.
    pub local_timestamp: u64,
    /// The trade ID provided by the exchange. If empty, a new `UUIDv4` string is generated.
    #[serde(deserialize_with = "deserialize_trade_id")]
    pub id: String,
    /// The liquidity taker (aggressor) side provided by the exchange.
    pub side: String,
    /// The trade price as provided by the exchange.
    pub price: f64,
    /// The trade amount as provided by the exchange.
    pub amount: f64,
}
