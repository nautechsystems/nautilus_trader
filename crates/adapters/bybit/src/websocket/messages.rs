// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! WebSocket message types for Bybit public and private channels.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ustr::Ustr;

use crate::{
    common::enums::{
        BybitCancelType, BybitCreateType, BybitExecType, BybitOrderSide, BybitOrderStatus,
        BybitOrderType, BybitProductType, BybitStopOrderType, BybitTimeInForce, BybitTpSlMode,
        BybitTriggerDirection, BybitTriggerType, BybitWsOrderRequestOp,
    },
    websocket::enums::BybitWsOperation,
};

/// Bybit WebSocket subscription message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BybitSubscription {
    pub op: BybitWsOperation,
    pub args: Vec<String>,
}

/// Bybit WebSocket authentication message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BybitAuthRequest {
    pub op: BybitWsOperation,
    pub args: Vec<serde_json::Value>,
}

/// High level message emitted by the Bybit WebSocket client.
#[derive(Debug, Clone)]
pub enum BybitWebSocketMessage {
    /// Generic response (subscribe/auth acknowledgement).
    Response(BybitWsResponse),
    /// Authentication acknowledgement.
    Auth(BybitWsAuthResponse),
    /// Subscription acknowledgement.
    Subscription(BybitWsSubscriptionMsg),
    /// Orderbook snapshot or delta.
    Orderbook(BybitWsOrderbookDepthMsg),
    /// Trade updates.
    Trade(BybitWsTradeMsg),
    /// Kline updates.
    Kline(BybitWsKlineMsg),
    /// Linear/inverse ticker update.
    TickerLinear(BybitWsTickerLinearMsg),
    /// Option ticker update.
    TickerOption(BybitWsTickerOptionMsg),
    /// Order updates from private channel.
    AccountOrder(BybitWsAccountOrderMsg),
    /// Execution/fill updates from private channel.
    AccountExecution(BybitWsAccountExecutionMsg),
    /// Wallet/balance updates from private channel.
    AccountWallet(BybitWsAccountWalletMsg),
    /// Position updates from private channel.
    AccountPosition(BybitWsAccountPositionMsg),
    /// Error received from the venue or client lifecycle.
    Error(BybitWebSocketError),
    /// Raw message payload that does not yet have a typed representation.
    Raw(Value),
    /// Notification that the underlying connection reconnected.
    Reconnected,
    /// Explicit pong event (text-based heartbeat acknowledgement).
    Pong,
}

/// Represents an error event surfaced by the WebSocket client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "python", pyo3::pyclass)]
pub struct BybitWebSocketError {
    /// Error/return code reported by Bybit.
    pub code: i64,
    /// Human readable message.
    pub message: String,
    /// Optional connection identifier.
    #[serde(default)]
    pub conn_id: Option<String>,
    /// Optional topic associated with the error (when applicable).
    #[serde(default)]
    pub topic: Option<String>,
    /// Optional request identifier related to the failure.
    #[serde(default)]
    pub req_id: Option<String>,
}

impl BybitWebSocketError {
    /// Creates a new error with the provided code/message.
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            conn_id: None,
            topic: None,
            req_id: None,
        }
    }

    /// Builds an error payload from a generic response frame.
    #[must_use]
    pub fn from_response(response: &BybitWsResponse) -> Self {
        // Build a more informative error message when ret_msg is missing
        let message = response.ret_msg.clone().unwrap_or_else(|| {
            let mut parts = vec![];

            if let Some(op) = &response.op {
                parts.push(format!("op={}", op));
            }
            if let Some(topic) = &response.topic {
                parts.push(format!("topic={}", topic));
            }
            if let Some(success) = response.success {
                parts.push(format!("success={}", success));
            }

            if parts.is_empty() {
                "Bybit websocket error (no error message provided)".to_string()
            } else {
                format!("Bybit websocket error: {}", parts.join(", "))
            }
        });

        Self {
            code: response.ret_code.unwrap_or_default(),
            message,
            conn_id: response.conn_id.clone(),
            topic: response.topic.map(|t| t.to_string()),
            req_id: response.req_id.clone(),
        }
    }

    /// Convenience constructor for client-side errors (e.g. parsing failures).
    #[must_use]
    pub fn from_message(message: impl Into<String>) -> Self {
        Self::new(-1, message)
    }
}

/// Generic WebSocket request for Bybit trading commands.
#[derive(Debug, Clone, Serialize)]
pub struct BybitWsRequest<T> {
    /// Operation type (order.create, order.amend, order.cancel, etc.).
    pub op: BybitWsOrderRequestOp,
    /// Request header containing timestamp and other metadata.
    pub header: BybitWsHeader,
    /// Arguments payload for the operation.
    pub args: Vec<T>,
}

/// Header for WebSocket trade requests.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub struct BybitWsHeader {
    /// Timestamp in milliseconds.
    pub x_bapi_timestamp: String,
}

impl BybitWsHeader {
    /// Creates a new header with the current timestamp.
    #[must_use]
    pub fn now() -> Self {
        use nautilus_core::time::get_atomic_clock_realtime;
        Self {
            x_bapi_timestamp: get_atomic_clock_realtime().get_time_ms().to_string(),
        }
    }
}

/// Parameters for placing an order via WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsPlaceOrderParams {
    pub category: BybitProductType,
    pub symbol: Ustr,
    pub side: BybitOrderSide,
    pub order_type: BybitOrderType,
    pub qty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<BybitTimeInForce>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_link_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_on_trigger: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_direction: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tpsl_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_trigger_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_trigger_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_order_type: Option<BybitOrderType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_order_type: Option<BybitOrderType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_limit_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_limit_price: Option<String>,
}

/// Parameters for amending an order via WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsAmendOrderParams {
    pub category: BybitProductType,
    pub symbol: Ustr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_link_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_trigger_by: Option<BybitTriggerType>,
}

/// Parameters for canceling an order via WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsCancelOrderParams {
    pub category: BybitProductType,
    pub symbol: Ustr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_link_id: Option<String>,
}

/// Subscription acknowledgement returned by Bybit.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BybitWsSubscriptionMsg {
    pub success: bool,
    pub op: BybitWsOperation,
    #[serde(default)]
    pub conn_id: Option<String>,
    #[serde(default)]
    pub req_id: Option<String>,
    #[serde(default)]
    pub ret_msg: Option<String>,
}

/// Generic response returned by the endpoint when subscribing or authenticating.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BybitWsResponse {
    #[serde(default)]
    pub op: Option<BybitWsOperation>,
    #[serde(default)]
    pub topic: Option<Ustr>,
    #[serde(default)]
    pub success: Option<bool>,
    #[serde(default)]
    pub conn_id: Option<String>,
    #[serde(default)]
    pub req_id: Option<String>,
    #[serde(default)]
    pub ret_code: Option<i64>,
    #[serde(default)]
    pub ret_msg: Option<String>,
}

/// Authentication acknowledgement for private channels.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsAuthResponse {
    pub op: BybitWsOperation,
    #[serde(default)]
    pub conn_id: Option<String>,
    #[serde(default)]
    pub ret_code: Option<i64>,
    #[serde(default)]
    pub ret_msg: Option<String>,
    #[serde(default)]
    pub success: Option<bool>,
}

/// Representation of a kline/candlestick event on the public stream.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsKline {
    pub start: i64,
    pub end: i64,
    pub interval: Ustr,
    pub open: String,
    pub close: String,
    pub high: String,
    pub low: String,
    pub volume: String,
    pub turnover: String,
    pub confirm: bool,
    pub timestamp: i64,
}

/// Envelope for kline updates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsKlineMsg {
    pub topic: Ustr,
    pub ts: i64,
    #[serde(rename = "type")]
    pub msg_type: Ustr,
    pub data: Vec<BybitWsKline>,
}

/// Orderbook depth payload consisting of raw ladder deltas.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BybitWsOrderbookDepth {
    /// Symbol.
    pub s: Ustr,
    /// Bid levels represented as `[price, size]` string pairs.
    pub b: Vec<Vec<String>>,
    /// Ask levels represented as `[price, size]` string pairs.
    pub a: Vec<Vec<String>>,
    /// Update identifier.
    pub u: i64,
    /// Cross sequence number.
    pub seq: i64,
}

/// Envelope for orderbook depth snapshots and updates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsOrderbookDepthMsg {
    pub topic: Ustr,
    #[serde(rename = "type")]
    pub msg_type: Ustr,
    pub ts: i64,
    pub data: BybitWsOrderbookDepth,
    #[serde(default)]
    pub cts: Option<i64>,
}

/// Linear/Inverse ticker event payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsTickerLinear {
    pub symbol: Ustr,
    #[serde(default)]
    pub tick_direction: Option<String>,
    #[serde(default)]
    pub price24h_pcnt: Option<String>,
    #[serde(default)]
    pub last_price: Option<String>,
    #[serde(default)]
    pub prev_price24h: Option<String>,
    #[serde(default)]
    pub high_price24h: Option<String>,
    #[serde(default)]
    pub low_price24h: Option<String>,
    #[serde(default)]
    pub prev_price1h: Option<String>,
    #[serde(default)]
    pub mark_price: Option<String>,
    #[serde(default)]
    pub index_price: Option<String>,
    #[serde(default)]
    pub open_interest: Option<String>,
    #[serde(default)]
    pub open_interest_value: Option<String>,
    #[serde(default)]
    pub turnover24h: Option<String>,
    #[serde(default)]
    pub volume24h: Option<String>,
    #[serde(default)]
    pub next_funding_time: Option<String>,
    #[serde(default)]
    pub funding_rate: Option<String>,
    #[serde(default)]
    pub bid1_price: Option<String>,
    #[serde(default)]
    pub bid1_size: Option<String>,
    #[serde(default)]
    pub ask1_price: Option<String>,
    #[serde(default)]
    pub ask1_size: Option<String>,
}

/// Envelope for linear ticker updates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsTickerLinearMsg {
    pub topic: Ustr,
    #[serde(rename = "type")]
    pub msg_type: Ustr,
    pub ts: i64,
    #[serde(default)]
    pub cs: Option<i64>,
    pub data: BybitWsTickerLinear,
}

/// Option ticker event payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsTickerOption {
    pub symbol: Ustr,
    pub bid_price: String,
    pub bid_size: String,
    pub bid_iv: String,
    pub ask_price: String,
    pub ask_size: String,
    pub ask_iv: String,
    pub last_price: String,
    pub high_price24h: String,
    pub low_price24h: String,
    pub mark_price: String,
    pub index_price: String,
    pub mark_price_iv: String,
    pub underlying_price: String,
    pub open_interest: String,
    pub turnover24h: String,
    pub volume24h: String,
    pub total_volume: String,
    pub total_turnover: String,
    pub delta: String,
    pub gamma: String,
    pub vega: String,
    pub theta: String,
    pub predicted_delivery_price: String,
    pub change24h: String,
}

/// Envelope for option ticker updates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsTickerOptionMsg {
    #[serde(default)]
    pub id: Option<String>,
    pub topic: Ustr,
    #[serde(rename = "type")]
    pub msg_type: Ustr,
    pub ts: i64,
    pub data: BybitWsTickerOption,
}

/// Trade event payload containing trade executions on public feeds.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BybitWsTrade {
    #[serde(rename = "T")]
    pub t: i64,
    #[serde(rename = "s")]
    pub s: Ustr,
    #[serde(rename = "S")]
    pub taker_side: BybitOrderSide,
    #[serde(rename = "v")]
    pub v: String,
    #[serde(rename = "p")]
    pub p: String,
    #[serde(rename = "i")]
    pub i: String,
    #[serde(rename = "BT")]
    pub bt: bool,
    #[serde(rename = "L")]
    #[serde(default)]
    pub l: Option<String>,
    #[serde(rename = "id")]
    #[serde(default)]
    pub id: Option<Ustr>,
    #[serde(rename = "mP")]
    #[serde(default)]
    pub m_p: Option<String>,
    #[serde(rename = "iP")]
    #[serde(default)]
    pub i_p: Option<String>,
    #[serde(rename = "mIv")]
    #[serde(default)]
    pub m_iv: Option<String>,
    #[serde(rename = "iv")]
    #[serde(default)]
    pub iv: Option<String>,
}

/// Envelope for public trade updates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsTradeMsg {
    pub topic: Ustr,
    #[serde(rename = "type")]
    pub msg_type: Ustr,
    pub ts: i64,
    pub data: Vec<BybitWsTrade>,
}

/// Private order stream payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsAccountOrder {
    pub category: BybitProductType,
    pub symbol: Ustr,
    pub order_id: Ustr,
    pub side: BybitOrderSide,
    pub order_type: BybitOrderType,
    pub cancel_type: BybitCancelType,
    pub price: String,
    pub qty: String,
    pub order_iv: String,
    pub time_in_force: BybitTimeInForce,
    pub order_status: BybitOrderStatus,
    pub order_link_id: Ustr,
    pub last_price_on_created: Ustr,
    pub reduce_only: bool,
    pub leaves_qty: String,
    pub leaves_value: String,
    pub cum_exec_qty: String,
    pub cum_exec_value: String,
    pub avg_price: String,
    pub block_trade_id: Ustr,
    pub position_idx: i32,
    pub cum_exec_fee: String,
    pub created_time: String,
    pub updated_time: String,
    pub reject_reason: Ustr,
    pub trigger_price: String,
    pub take_profit: String,
    pub stop_loss: String,
    pub tp_trigger_by: BybitTriggerType,
    pub sl_trigger_by: BybitTriggerType,
    pub tp_limit_price: String,
    pub sl_limit_price: String,
    pub close_on_trigger: bool,
    pub place_type: Ustr,
    pub smp_type: Ustr,
    pub smp_group: i32,
    pub smp_order_id: Ustr,
    pub fee_currency: Ustr,
    pub trigger_by: BybitTriggerType,
    pub stop_order_type: BybitStopOrderType,
    pub trigger_direction: BybitTriggerDirection,
    #[serde(default)]
    pub tpsl_mode: Option<BybitTpSlMode>,
    #[serde(default)]
    pub create_type: Option<BybitCreateType>,
}

/// Envelope for account order updates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsAccountOrderMsg {
    pub topic: String,
    pub id: String,
    pub creation_time: i64,
    pub data: Vec<BybitWsAccountOrder>,
}

/// Private execution (fill) stream payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsAccountExecution {
    pub category: BybitProductType,
    pub symbol: Ustr,
    pub exec_fee: String,
    pub exec_id: String,
    pub exec_price: String,
    pub exec_qty: String,
    pub exec_type: BybitExecType,
    pub exec_value: String,
    pub is_maker: bool,
    pub fee_rate: String,
    pub trade_iv: String,
    pub mark_iv: String,
    pub block_trade_id: Ustr,
    pub mark_price: String,
    pub index_price: String,
    pub underlying_price: String,
    pub leaves_qty: String,
    pub order_id: Ustr,
    pub order_link_id: Ustr,
    pub order_price: String,
    pub order_qty: String,
    pub order_type: BybitOrderType,
    pub side: BybitOrderSide,
    pub exec_time: String,
    pub is_leverage: String,
    pub closed_size: String,
    pub seq: i64,
    pub stop_order_type: BybitStopOrderType,
}

/// Envelope for account execution updates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsAccountExecutionMsg {
    pub topic: String,
    pub id: String,
    pub creation_time: i64,
    pub data: Vec<BybitWsAccountExecution>,
}

/// Coin level wallet update payload on private streams.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsAccountWalletCoin {
    pub coin: Ustr,
    pub wallet_balance: String,
    pub available_to_withdraw: String,
    pub available_to_borrow: String,
    pub accrued_interest: String,
    #[serde(default, rename = "totalOrderIM")]
    pub total_order_im: Option<String>,
    #[serde(default, rename = "totalPositionIM")]
    pub total_position_im: Option<String>,
    #[serde(default, rename = "totalPositionMM")]
    pub total_position_mm: Option<String>,
    pub equity: String,
    #[serde(default)]
    pub spot_borrow: Option<String>,
}

/// Wallet summary payload covering all coins.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsAccountWallet {
    pub total_wallet_balance: String,
    pub total_equity: String,
    pub total_available_balance: String,
    pub total_margin_balance: String,
    pub total_initial_margin: String,
    pub total_maintenance_margin: String,
    #[serde(rename = "accountIMRate")]
    pub account_im_rate: String,
    #[serde(rename = "accountMMRate")]
    pub account_mm_rate: String,
    #[serde(rename = "accountLTV")]
    pub account_ltv: String,
    pub coin: Vec<BybitWsAccountWalletCoin>,
}

/// Envelope for wallet updates on private streams.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsAccountWalletMsg {
    pub topic: String,
    pub id: String,
    pub creation_time: i64,
    pub data: Vec<BybitWsAccountWallet>,
}

/// Position data from private position stream.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsAccountPosition {
    pub position_idx: i32,
    pub risk_id: i64,
    pub risk_limit_value: String,
    pub symbol: Ustr,
    pub side: String,
    pub size: String,
    #[serde(default)]
    pub avg_price: Option<String>,
    pub position_value: String,
    pub trade_mode: i32,
    pub position_status: String,
    pub auto_add_margin: i32,
    pub adl_rank_indicator: i32,
    pub leverage: String,
    pub position_balance: String,
    pub mark_price: String,
    pub liq_price: String,
    pub bust_price: String,
    #[serde(rename = "positionMM")]
    pub position_mm: String,
    #[serde(rename = "positionIM")]
    pub position_im: String,
    pub tpsl_mode: String,
    pub take_profit: String,
    pub stop_loss: String,
    pub trailing_stop: String,
    pub unrealised_pnl: String,
    pub cur_realised_pnl: String,
    pub cum_realised_pnl: String,
    pub seq: i64,
    #[serde(default)]
    pub is_reduce_only: bool,
    pub created_time: String,
    pub updated_time: String,
}

/// Envelope for position updates on private streams.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsAccountPositionMsg {
    pub topic: String,
    pub id: String,
    pub creation_time: i64,
    pub data: Vec<BybitWsAccountPosition>,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    #[rstest]
    fn deserialize_account_order_frame_uses_enums() {
        let json = load_test_json("ws_account_order.json");
        let frame: BybitWsAccountOrderMsg = serde_json::from_str(&json).unwrap();
        let order = &frame.data[0];

        assert_eq!(order.cancel_type, BybitCancelType::CancelByUser);
        assert_eq!(order.tp_trigger_by, BybitTriggerType::MarkPrice);
        assert_eq!(order.sl_trigger_by, BybitTriggerType::LastPrice);
        assert_eq!(order.tpsl_mode, Some(BybitTpSlMode::Full));
        assert_eq!(order.create_type, Some(BybitCreateType::CreateByUser));
        assert_eq!(order.side, BybitOrderSide::Buy);
    }
}
