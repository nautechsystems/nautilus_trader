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

//! Hyperliquid-specific enums and types.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Hyperliquid order types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum HyperliquidOrderType {
    #[serde(rename = "market")]
    #[strum(serialize = "market")]
    Market,
    #[serde(rename = "limit")]
    #[strum(serialize = "limit")]
    Limit,
    #[serde(rename = "stop_market")]
    #[strum(serialize = "stop_market")]
    StopMarket,
    #[serde(rename = "stop_limit")]
    #[strum(serialize = "stop_limit")]
    StopLimit,
    #[serde(rename = "scale")]
    #[strum(serialize = "scale")]
    Scale,
}

/// Hyperliquid order side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum HyperliquidOrderSide {
    #[serde(rename = "A")]
    #[strum(serialize = "A")]
    Ask, // Sell
    #[serde(rename = "B")]
    #[strum(serialize = "B")]
    Bid, // Buy
}

/// Hyperliquid time in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum HyperliquidTimeInForce {
    #[serde(rename = "Gtc")]
    #[strum(serialize = "Gtc")]
    GoodTillCancel,
    #[serde(rename = "Ioc")]
    #[strum(serialize = "Ioc")]
    ImmediateOrCancel,
    #[serde(rename = "Alo")]
    #[strum(serialize = "Alo")]
    AddLiquidityOnly,
}

/// Hyperliquid order status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum HyperliquidOrderStatus {
    #[serde(rename = "open")]
    #[strum(serialize = "open")]
    Open,
    #[serde(rename = "filled")]
    #[strum(serialize = "filled")]
    Filled,
    #[serde(rename = "canceled")]
    #[strum(serialize = "canceled")]
    Canceled,
    #[serde(rename = "triggered")]
    #[strum(serialize = "triggered")]
    Triggered,
    #[serde(rename = "rejected")]
    #[strum(serialize = "rejected")]
    Rejected,
}

/// Hyperliquid WebSocket channel types.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum HyperliquidWsChannel {
    #[serde(rename = "allMids")]
    #[strum(serialize = "allMids")]
    AllMids,
    #[serde(rename = "notification")]
    #[strum(serialize = "notification")]
    Notification,
    #[serde(rename = "webData2")]
    #[strum(serialize = "webData2")]
    WebData2,
    #[serde(rename = "candle")]
    #[strum(serialize = "candle")]
    Candle,
    #[serde(rename = "l2Book")]
    #[strum(serialize = "l2Book")]
    L2Book,
    #[serde(rename = "trades")]
    #[strum(serialize = "trades")]
    Trades,
    #[serde(rename = "orderUpdates")]
    #[strum(serialize = "orderUpdates")]
    OrderUpdates,
    #[serde(rename = "userEvents")]
    #[strum(serialize = "userEvents")]
    UserEvents,
}

/// Hyperliquid WebSocket operation types.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum HyperliquidWsOperation {
    #[serde(rename = "subscribe")]
    #[strum(serialize = "subscribe")]
    Subscribe,
    #[serde(rename = "unsubscribe")]
    #[strum(serialize = "unsubscribe")]
    Unsubscribe,
}

/// Hyperliquid asset class types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum HyperliquidAssetClass {
    #[serde(rename = "spot")]
    #[strum(serialize = "spot")]
    Spot,
    #[serde(rename = "perp")]
    #[strum(serialize = "perp")]
    Perpetual,
}

/// Hyperliquid margin mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum HyperliquidMarginMode {
    #[serde(rename = "cross")]
    #[strum(serialize = "cross")]
    Cross,
    #[serde(rename = "isolated")]
    #[strum(serialize = "isolated")]
    Isolated,
}

/// Hyperliquid position side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum HyperliquidPositionSide {
    #[serde(rename = "long")]
    #[strum(serialize = "long")]
    Long,
    #[serde(rename = "short")]
    #[strum(serialize = "short")]
    Short,
}

/// Hyperliquid execution type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum HyperliquidExecType {
    #[serde(rename = "trade")]
    #[strum(serialize = "trade")]
    Trade,
    #[serde(rename = "liquidation")]
    #[strum(serialize = "liquidation")]
    Liquidation,
}
