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

//! WebSocket message models for Lighter public feeds.

use nautilus_model::data::{
    FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick,
};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::data::models::LighterOrderBookDepth;

/// Parsed WebSocket messages emitted to consumers.
#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    Deltas(OrderBookDeltas),
    Quote(QuoteTick),
    Trades(Vec<TradeTick>),
    MarkPrice(MarkPriceUpdate),
    IndexPrice(IndexPriceUpdate),
    FundingRate(FundingRateUpdate),
}

/// Raw WebSocket envelope types.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "connected")]
    Connected { session_id: Option<String> },
    #[serde(rename = "subscribed/order_book")]
    OrderBookSnapshot(WsOrderBookMessage),
    #[serde(rename = "update/order_book")]
    OrderBookUpdate(WsOrderBookMessage),
    #[serde(rename = "subscribed/trade")]
    TradesSnapshot(WsTradesMessage),
    #[serde(rename = "update/trade")]
    TradesUpdate(WsTradesMessage),
    #[serde(rename = "subscribed/market_stats")]
    MarketStats(Box<WsMarketStatsMessage>),
}

/// Order book snapshot or delta message.
#[derive(Debug, Deserialize)]
pub struct WsOrderBookMessage {
    pub channel: String,
    #[serde(default)]
    pub offset: Option<u64>,
    pub order_book: LighterOrderBookDepth,
    #[serde(default)]
    pub timestamp: Option<i64>,
}

/// Trades message (snapshot or incremental).
#[derive(Debug, Deserialize)]
pub struct WsTradesMessage {
    pub channel: String,
    #[serde(default)]
    pub trades: Vec<LighterTrade>,
    #[serde(default)]
    pub liquidation_trades: Vec<LighterTrade>,
    #[serde(default)]
    pub nonce: Option<u64>,
}

/// Market stats update.
#[derive(Debug, Deserialize)]
pub struct WsMarketStatsMessage {
    pub channel: String,
    pub market_stats: LighterMarketStats,
}

/// Trade payload for both regular and liquidation trades.
#[derive(Debug, Clone, Deserialize)]
pub struct LighterTrade {
    pub trade_id: u64,
    #[serde(default)]
    pub market_id: Option<u32>,
    pub size: Decimal,
    pub price: Decimal,
    #[serde(default)]
    pub is_maker_ask: bool,
    #[serde(default)]
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub r#type: Option<String>,
}

/// Market stats payload containing mark/index prices and funding info.
#[derive(Debug, Clone, Deserialize)]
pub struct LighterMarketStats {
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub market_id: Option<u32>,
    #[serde(default)]
    pub index_price: Option<Decimal>,
    #[serde(default)]
    pub mark_price: Option<Decimal>,
    #[serde(default)]
    pub last_trade_price: Option<Decimal>,
    #[serde(default)]
    pub current_funding_rate: Option<Decimal>,
    #[serde(default)]
    pub funding_rate: Option<Decimal>,
    #[serde(default)]
    pub funding_timestamp: Option<i64>,
    #[serde(default)]
    pub open_interest: Option<Decimal>,
    #[serde(default)]
    pub open_interest_limit: Option<Decimal>,
    #[serde(default)]
    pub funding_clamp_small: Option<Decimal>,
    #[serde(default)]
    pub funding_clamp_big: Option<Decimal>,
    #[serde(default)]
    pub daily_base_token_volume: Option<Decimal>,
    #[serde(default)]
    pub daily_quote_token_volume: Option<Decimal>,
    #[serde(default)]
    pub daily_price_low: Option<Decimal>,
    #[serde(default)]
    pub daily_price_high: Option<Decimal>,
    #[serde(default)]
    pub daily_price_change: Option<Decimal>,
    #[serde(default)]
    pub extra: Option<serde_json::Value>,
}
