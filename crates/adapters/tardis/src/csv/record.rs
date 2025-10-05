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
    enums::TardisExchange,
    parse::{deserialize_trade_id, deserialize_uppercase},
};

/// Represents a Tardis format order book update record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TardisBookUpdateRecord {
    /// The exchange ID.
    pub exchange: TardisExchange,
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
    pub exchange: TardisExchange,
    /// The instrument symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    // UNIX microseconds timestamp provided by the exchange.
    pub timestamp: u64,
    // UNIX microseconds timestamp of message received.
    pub local_timestamp: u64,
    /// The price of the first ask.
    #[serde(rename = "asks[0].price")]
    pub asks_0_price: Option<f64>,
    /// The amount of the first ask.
    #[serde(rename = "asks[0].amount")]
    pub asks_0_amount: Option<f64>,
    /// The price of the first bid.
    #[serde(rename = "bids[0].price")]
    pub bids_0_price: Option<f64>,
    /// The amount of the first bid.
    #[serde(rename = "bids[0].amount")]
    pub bids_0_amount: Option<f64>,
    /// The price of the second ask.
    #[serde(rename = "asks[1].price")]
    pub asks_1_price: Option<f64>,
    /// The amount of the second ask.
    #[serde(rename = "asks[1].amount")]
    pub asks_1_amount: Option<f64>,
    /// The price of the second bid.
    #[serde(rename = "bids[1].price")]
    pub bids_1_price: Option<f64>,
    /// The amount of the second bid.
    #[serde(rename = "bids[1].amount")]
    pub bids_1_amount: Option<f64>,
    /// The price of the third ask.
    #[serde(rename = "asks[2].price")]
    pub asks_2_price: Option<f64>,
    /// The amount of the third ask.
    #[serde(rename = "asks[2].amount")]
    pub asks_2_amount: Option<f64>,
    /// The price of the third bid.
    #[serde(rename = "bids[2].price")]
    pub bids_2_price: Option<f64>,
    /// The amount of the third bid.
    #[serde(rename = "bids[2].amount")]
    pub bids_2_amount: Option<f64>,
    /// The price of the fourth ask.
    #[serde(rename = "asks[3].price")]
    pub asks_3_price: Option<f64>,
    /// The amount of the fourth ask.
    #[serde(rename = "asks[3].amount")]
    pub asks_3_amount: Option<f64>,
    /// The price of the fourth bid.
    #[serde(rename = "bids[3].price")]
    pub bids_3_price: Option<f64>,
    /// The amount of the fourth bid.
    #[serde(rename = "bids[3].amount")]
    pub bids_3_amount: Option<f64>,
    /// The price of the fifth ask.
    #[serde(rename = "asks[4].price")]
    pub asks_4_price: Option<f64>,
    /// The amount of the fifth ask.
    #[serde(rename = "asks[4].amount")]
    pub asks_4_amount: Option<f64>,
    /// The price of the fifth bid.
    #[serde(rename = "bids[4].price")]
    pub bids_4_price: Option<f64>,
    /// The amount of the fifth bid.
    #[serde(rename = "bids[4].amount")]
    pub bids_4_amount: Option<f64>,
}

/// Represents a Tardis format order book 25 level snapshot record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TardisOrderBookSnapshot25Record {
    /// The exchange ID.
    pub exchange: TardisExchange,
    /// The instrument symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    // UNIX microseconds timestamp provided by the exchange.
    pub timestamp: u64,
    // UNIX microseconds timestamp of message received.
    pub local_timestamp: u64,

    #[serde(rename = "asks[0].price")]
    pub asks_0_price: Option<f64>,
    #[serde(rename = "asks[0].amount")]
    pub asks_0_amount: Option<f64>,
    #[serde(rename = "bids[0].price")]
    pub bids_0_price: Option<f64>,
    #[serde(rename = "bids[0].amount")]
    pub bids_0_amount: Option<f64>,
    #[serde(rename = "asks[1].price")]
    pub asks_1_price: Option<f64>,
    #[serde(rename = "asks[1].amount")]
    pub asks_1_amount: Option<f64>,
    #[serde(rename = "bids[1].price")]
    pub bids_1_price: Option<f64>,
    #[serde(rename = "bids[1].amount")]
    pub bids_1_amount: Option<f64>,
    #[serde(rename = "asks[2].price")]
    pub asks_2_price: Option<f64>,
    #[serde(rename = "asks[2].amount")]
    pub asks_2_amount: Option<f64>,
    #[serde(rename = "bids[2].price")]
    pub bids_2_price: Option<f64>,
    #[serde(rename = "bids[2].amount")]
    pub bids_2_amount: Option<f64>,
    #[serde(rename = "asks[3].price")]
    pub asks_3_price: Option<f64>,
    #[serde(rename = "asks[3].amount")]
    pub asks_3_amount: Option<f64>,
    #[serde(rename = "bids[3].price")]
    pub bids_3_price: Option<f64>,
    #[serde(rename = "bids[3].amount")]
    pub bids_3_amount: Option<f64>,
    #[serde(rename = "asks[4].price")]
    pub asks_4_price: Option<f64>,
    #[serde(rename = "asks[4].amount")]
    pub asks_4_amount: Option<f64>,
    #[serde(rename = "bids[4].price")]
    pub bids_4_price: Option<f64>,
    #[serde(rename = "bids[4].amount")]
    pub bids_4_amount: Option<f64>,
    #[serde(rename = "asks[5].price")]
    pub asks_5_price: Option<f64>,
    #[serde(rename = "asks[5].amount")]
    pub asks_5_amount: Option<f64>,
    #[serde(rename = "bids[5].price")]
    pub bids_5_price: Option<f64>,
    #[serde(rename = "bids[5].amount")]
    pub bids_5_amount: Option<f64>,
    #[serde(rename = "asks[6].price")]
    pub asks_6_price: Option<f64>,
    #[serde(rename = "asks[6].amount")]
    pub asks_6_amount: Option<f64>,
    #[serde(rename = "bids[6].price")]
    pub bids_6_price: Option<f64>,
    #[serde(rename = "bids[6].amount")]
    pub bids_6_amount: Option<f64>,
    #[serde(rename = "asks[7].price")]
    pub asks_7_price: Option<f64>,
    #[serde(rename = "asks[7].amount")]
    pub asks_7_amount: Option<f64>,
    #[serde(rename = "bids[7].price")]
    pub bids_7_price: Option<f64>,
    #[serde(rename = "bids[7].amount")]
    pub bids_7_amount: Option<f64>,
    #[serde(rename = "asks[8].price")]
    pub asks_8_price: Option<f64>,
    #[serde(rename = "asks[8].amount")]
    pub asks_8_amount: Option<f64>,
    #[serde(rename = "bids[8].price")]
    pub bids_8_price: Option<f64>,
    #[serde(rename = "bids[8].amount")]
    pub bids_8_amount: Option<f64>,
    #[serde(rename = "asks[9].price")]
    pub asks_9_price: Option<f64>,
    #[serde(rename = "asks[9].amount")]
    pub asks_9_amount: Option<f64>,
    #[serde(rename = "bids[9].price")]
    pub bids_9_price: Option<f64>,
    #[serde(rename = "bids[9].amount")]
    pub bids_9_amount: Option<f64>,
    #[serde(rename = "asks[10].price")]
    pub asks_10_price: Option<f64>,
    #[serde(rename = "asks[10].amount")]
    pub asks_10_amount: Option<f64>,
    #[serde(rename = "bids[10].price")]
    pub bids_10_price: Option<f64>,
    #[serde(rename = "bids[10].amount")]
    pub bids_10_amount: Option<f64>,
    #[serde(rename = "asks[11].price")]
    pub asks_11_price: Option<f64>,
    #[serde(rename = "asks[11].amount")]
    pub asks_11_amount: Option<f64>,
    #[serde(rename = "bids[11].price")]
    pub bids_11_price: Option<f64>,
    #[serde(rename = "bids[11].amount")]
    pub bids_11_amount: Option<f64>,
    #[serde(rename = "asks[12].price")]
    pub asks_12_price: Option<f64>,
    #[serde(rename = "asks[12].amount")]
    pub asks_12_amount: Option<f64>,
    #[serde(rename = "bids[12].price")]
    pub bids_12_price: Option<f64>,
    #[serde(rename = "bids[12].amount")]
    pub bids_12_amount: Option<f64>,
    #[serde(rename = "asks[13].price")]
    pub asks_13_price: Option<f64>,
    #[serde(rename = "asks[13].amount")]
    pub asks_13_amount: Option<f64>,
    #[serde(rename = "bids[13].price")]
    pub bids_13_price: Option<f64>,
    #[serde(rename = "bids[13].amount")]
    pub bids_13_amount: Option<f64>,
    #[serde(rename = "asks[14].price")]
    pub asks_14_price: Option<f64>,
    #[serde(rename = "asks[14].amount")]
    pub asks_14_amount: Option<f64>,
    #[serde(rename = "bids[14].price")]
    pub bids_14_price: Option<f64>,
    #[serde(rename = "bids[14].amount")]
    pub bids_14_amount: Option<f64>,
    #[serde(rename = "asks[15].price")]
    pub asks_15_price: Option<f64>,
    #[serde(rename = "asks[15].amount")]
    pub asks_15_amount: Option<f64>,
    #[serde(rename = "bids[15].price")]
    pub bids_15_price: Option<f64>,
    #[serde(rename = "bids[15].amount")]
    pub bids_15_amount: Option<f64>,
    #[serde(rename = "asks[16].price")]
    pub asks_16_price: Option<f64>,
    #[serde(rename = "asks[16].amount")]
    pub asks_16_amount: Option<f64>,
    #[serde(rename = "bids[16].price")]
    pub bids_16_price: Option<f64>,
    #[serde(rename = "bids[16].amount")]
    pub bids_16_amount: Option<f64>,
    #[serde(rename = "asks[17].price")]
    pub asks_17_price: Option<f64>,
    #[serde(rename = "asks[17].amount")]
    pub asks_17_amount: Option<f64>,
    #[serde(rename = "bids[17].price")]
    pub bids_17_price: Option<f64>,
    #[serde(rename = "bids[17].amount")]
    pub bids_17_amount: Option<f64>,
    #[serde(rename = "asks[18].price")]
    pub asks_18_price: Option<f64>,
    #[serde(rename = "asks[18].amount")]
    pub asks_18_amount: Option<f64>,
    #[serde(rename = "bids[18].price")]
    pub bids_18_price: Option<f64>,
    #[serde(rename = "bids[18].amount")]
    pub bids_18_amount: Option<f64>,
    #[serde(rename = "asks[19].price")]
    pub asks_19_price: Option<f64>,
    #[serde(rename = "asks[19].amount")]
    pub asks_19_amount: Option<f64>,
    #[serde(rename = "bids[19].price")]
    pub bids_19_price: Option<f64>,
    #[serde(rename = "bids[19].amount")]
    pub bids_19_amount: Option<f64>,
    #[serde(rename = "asks[20].price")]
    pub asks_20_price: Option<f64>,
    #[serde(rename = "asks[20].amount")]
    pub asks_20_amount: Option<f64>,
    #[serde(rename = "bids[20].price")]
    pub bids_20_price: Option<f64>,
    #[serde(rename = "bids[20].amount")]
    pub bids_20_amount: Option<f64>,
    #[serde(rename = "asks[21].price")]
    pub asks_21_price: Option<f64>,
    #[serde(rename = "asks[21].amount")]
    pub asks_21_amount: Option<f64>,
    #[serde(rename = "bids[21].price")]
    pub bids_21_price: Option<f64>,
    #[serde(rename = "bids[21].amount")]
    pub bids_21_amount: Option<f64>,
    #[serde(rename = "asks[22].price")]
    pub asks_22_price: Option<f64>,
    #[serde(rename = "asks[22].amount")]
    pub asks_22_amount: Option<f64>,
    #[serde(rename = "bids[22].price")]
    pub bids_22_price: Option<f64>,
    #[serde(rename = "bids[22].amount")]
    pub bids_22_amount: Option<f64>,
    #[serde(rename = "asks[23].price")]
    pub asks_23_price: Option<f64>,
    #[serde(rename = "asks[23].amount")]
    pub asks_23_amount: Option<f64>,
    #[serde(rename = "bids[23].price")]
    pub bids_23_price: Option<f64>,
    #[serde(rename = "bids[23].amount")]
    pub bids_23_amount: Option<f64>,
    #[serde(rename = "asks[24].price")]
    pub asks_24_price: Option<f64>,
    #[serde(rename = "asks[24].amount")]
    pub asks_24_amount: Option<f64>,
    #[serde(rename = "bids[24].price")]
    pub bids_24_price: Option<f64>,
    #[serde(rename = "bids[24].amount")]
    pub bids_24_amount: Option<f64>,
}

/// Represents a Tardis format quote record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TardisQuoteRecord {
    /// The exchande ID.
    pub exchange: TardisExchange,
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
    pub exchange: TardisExchange,
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

/// Represents a Tardis format derivative ticker record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TardisDerivativeTickerRecord {
    /// The exchange ID.
    pub exchange: TardisExchange,
    /// The instrument symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    /// UNIX microseconds timestamp provided by the exchange.
    pub timestamp: u64,
    /// UNIX microseconds timestamp of message received.
    pub local_timestamp: u64,
    /// UNIX microseconds timestamp of the next funding event.
    pub funding_timestamp: Option<u64>,
    /// The current funding rate.
    pub funding_rate: Option<f64>,
    /// The predicted funding rate for the next period.
    pub predicted_funding_rate: Option<f64>,
    /// The open interest for the derivative.
    pub open_interest: Option<f64>,
    /// The last traded price.
    pub last_price: Option<f64>,
    /// The index price.
    pub index_price: Option<f64>,
    /// The mark price.
    pub mark_price: Option<f64>,
}
