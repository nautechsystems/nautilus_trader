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
use ustr::Ustr;

use crate::common::enums::{
    BybitCancelType, BybitCreateType, BybitExecType, BybitOrderSide, BybitOrderStatus,
    BybitOrderType, BybitProductType, BybitStopOrderType, BybitTimeInForce, BybitTpSlMode,
    BybitTriggerDirection, BybitTriggerType,
};

/// Generic response returned by the endpoint when subscribing or authenticating.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWsResponse {
    #[serde(default)]
    pub op: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
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
    pub op: String,
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
    pub interval: String,
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
    pub topic: String,
    pub ts: i64,
    #[serde(rename = "type")]
    pub msg_type: String,
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
    pub topic: String,
    #[serde(rename = "type")]
    pub msg_type: String,
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
    pub topic: String,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub ts: i64,
    #[serde(default)]
    pub cs: Option<i64>,
    pub data: BybitWsTickerLinear,
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
    pub topic: String,
    #[serde(rename = "type")]
    pub msg_type: String,
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
    pub total_order_im: String,
    pub total_position_im: String,
    pub total_position_mm: String,
    pub equity: String,
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
    pub account_im_rate: String,
    pub account_mm_rate: String,
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::testing::load_test_json;

    #[test]
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
