// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Binance Spot User Data Stream message types.
//!
//! Pure venue types with no Nautilus model imports. These structs map directly
//! to the JSON payloads from the Binance Spot user data stream WebSocket.

use serde::Deserialize;
use ustr::Ustr;

use crate::common::enums::{BinanceOrderStatus, BinanceSide, BinanceTimeInForce};

/// Spot-specific execution type for order updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceSpotExecutionType {
    /// New order accepted.
    New,
    /// Order canceled.
    Canceled,
    /// Order replaced (cancel-replace).
    Replaced,
    /// Order rejected.
    Rejected,
    /// Trade (partial or full fill).
    Trade,
    /// Order expired (IOC/FOK not filled, or GTD expiration).
    Expired,
    /// Self-trade prevention triggered.
    TradePrevention,
}

/// Execution report event (`executionReport`) from the Spot user data stream.
///
/// Contains all fields needed to determine order lifecycle state and fill details.
///
/// # References
///
/// - <https://developers.binance.com/docs/binance-spot-api-docs/user-data-stream/event-order-update>
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotExecutionReport {
    /// Event type ("executionReport").
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Client order ID.
    #[serde(rename = "c")]
    pub client_order_id: String,
    /// Side.
    #[serde(rename = "S")]
    pub side: BinanceSide,
    /// Order type (LIMIT, MARKET, STOP_LOSS, etc.).
    #[serde(rename = "o")]
    pub order_type: String,
    /// Time in force.
    #[serde(rename = "f")]
    pub time_in_force: BinanceTimeInForce,
    /// Original quantity.
    #[serde(rename = "q")]
    pub original_qty: String,
    /// Original price.
    #[serde(rename = "p")]
    pub price: String,
    /// Stop price.
    #[serde(rename = "P")]
    pub stop_price: String,
    /// Current execution type.
    #[serde(rename = "x")]
    pub execution_type: BinanceSpotExecutionType,
    /// Current order status.
    #[serde(rename = "X")]
    pub order_status: BinanceOrderStatus,
    /// Order reject reason (only for Rejected).
    #[serde(rename = "r")]
    pub reject_reason: String,
    /// Order ID.
    #[serde(rename = "i")]
    pub order_id: i64,
    /// Last executed quantity.
    #[serde(rename = "l")]
    pub last_filled_qty: String,
    /// Cumulative filled quantity.
    #[serde(rename = "z")]
    pub cumulative_filled_qty: String,
    /// Last executed price.
    #[serde(rename = "L")]
    pub last_filled_price: String,
    /// Commission amount.
    #[serde(rename = "n")]
    pub commission: String,
    /// Commission asset.
    #[serde(rename = "N", default)]
    pub commission_asset: Option<Ustr>,
    /// Transaction time in milliseconds.
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Trade ID (-1 if not a trade).
    #[serde(rename = "t")]
    pub trade_id: i64,
    /// Is the order on the book?
    #[serde(rename = "w")]
    pub is_working: bool,
    /// Is this a maker trade?
    #[serde(rename = "m")]
    pub is_maker: bool,
    /// Order creation time in milliseconds.
    #[serde(rename = "O")]
    pub order_creation_time: i64,
    /// Cumulative quote asset transacted quantity.
    #[serde(rename = "Z")]
    pub cumulative_quote_qty: String,
    /// Original client order ID (for cancel-replace).
    #[serde(rename = "C", default)]
    pub original_client_order_id: Option<String>,
}

/// Account position update event (`outboundAccountPosition`).
///
/// Sent whenever there is a balance change (not associated with an order).
///
/// # References
///
/// - <https://developers.binance.com/docs/binance-spot-api-docs/user-data-stream/event-outbound-account-position>
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotAccountPositionMsg {
    /// Event type ("outboundAccountPosition").
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Last account update time.
    #[serde(rename = "u")]
    pub last_update_time: i64,
    /// Account balances.
    #[serde(rename = "B")]
    pub balances: Vec<BinanceSpotBalanceEntry>,
}

/// Individual balance entry within an account position update.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotBalanceEntry {
    /// Asset name.
    #[serde(rename = "a")]
    pub asset: Ustr,
    /// Free balance.
    #[serde(rename = "f")]
    pub free: String,
    /// Locked balance.
    #[serde(rename = "l")]
    pub locked: String,
}

/// Balance update event (`balanceUpdate`).
///
/// Sent when a deposit or withdrawal is processed, or when balances change
/// outside of trading (e.g., interest, fees).
///
/// # References
///
/// - <https://developers.binance.com/docs/binance-spot-api-docs/user-data-stream/event-balance-update>
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotBalanceUpdateMsg {
    /// Event type ("balanceUpdate").
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Asset.
    #[serde(rename = "a")]
    pub asset: Ustr,
    /// Balance delta.
    #[serde(rename = "d")]
    pub delta: String,
    /// Clear time in milliseconds.
    #[serde(rename = "T")]
    pub clear_time: i64,
}

/// Output message from the Spot user data stream handler.
///
/// These are the processed messages sent from the handler to the execution
/// client for further conversion to Nautilus types.
#[derive(Debug, Clone)]
pub enum BinanceSpotUdsMessage {
    /// Order execution report.
    ExecutionReport(Box<BinanceSpotExecutionReport>),
    /// Account position (balance) update.
    AccountPosition(BinanceSpotAccountPositionMsg),
    /// Balance update (deposit/withdrawal).
    BalanceUpdate(BinanceSpotBalanceUpdateMsg),
    /// Listen key expired (reconnection needed).
    ListenKeyExpired,
    /// WebSocket reconnected.
    Reconnected,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_deserialize_execution_report_new() {
        let json = include_str!("../../../../test_data/ws_spot_execution_report_new.json");
        let msg: BinanceSpotExecutionReport = serde_json::from_str(json).unwrap();

        assert_eq!(msg.event_type, "executionReport");
        assert_eq!(msg.symbol.as_str(), "ETHUSDT");
        assert_eq!(msg.execution_type, BinanceSpotExecutionType::New);
        assert_eq!(msg.order_status, BinanceOrderStatus::New);
        assert_eq!(msg.order_id, 12345678);
        assert_eq!(msg.side, BinanceSide::Buy);
    }

    #[rstest]
    fn test_deserialize_execution_report_trade() {
        let json = include_str!("../../../../test_data/ws_spot_execution_report_trade.json");
        let msg: BinanceSpotExecutionReport = serde_json::from_str(json).unwrap();

        assert_eq!(msg.execution_type, BinanceSpotExecutionType::Trade);
        assert_eq!(msg.order_status, BinanceOrderStatus::Filled);
        assert_eq!(msg.trade_id, 98765432);
        assert_eq!(msg.last_filled_qty, "1.00000000");
        assert_eq!(msg.last_filled_price, "2500.00000000");
        assert!(msg.is_maker);
    }

    #[rstest]
    fn test_deserialize_execution_report_canceled() {
        let json = include_str!("../../../../test_data/ws_spot_execution_report_canceled.json");
        let msg: BinanceSpotExecutionReport = serde_json::from_str(json).unwrap();

        assert_eq!(msg.execution_type, BinanceSpotExecutionType::Canceled);
        assert_eq!(msg.order_status, BinanceOrderStatus::Canceled);
    }

    #[rstest]
    fn test_deserialize_account_position() {
        let json = include_str!("../../../../test_data/ws_spot_account_position.json");
        let msg: BinanceSpotAccountPositionMsg = serde_json::from_str(json).unwrap();

        assert_eq!(msg.event_type, "outboundAccountPosition");
        assert!(!msg.balances.is_empty());
        assert_eq!(msg.balances[0].asset.as_str(), "USDT");
    }

    #[rstest]
    fn test_deserialize_balance_update() {
        let json = include_str!("../../../../test_data/ws_spot_balance_update.json");
        let msg: BinanceSpotBalanceUpdateMsg = serde_json::from_str(json).unwrap();

        assert_eq!(msg.event_type, "balanceUpdate");
        assert_eq!(msg.asset.as_str(), "BTC");
        assert_eq!(msg.delta, "0.00100000");
    }
}
