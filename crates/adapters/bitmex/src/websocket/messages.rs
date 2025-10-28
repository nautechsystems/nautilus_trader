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

//! BitMEX WebSocket message structures and helper types.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nautilus_model::{
    data::{Data, funding::FundingRateUpdate},
    events::{AccountState, OrderUpdated},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;
use strum::Display;
use ustr::Ustr;
use uuid::Uuid;

use super::enums::{
    BitmexAction, BitmexSide, BitmexTickDirection, BitmexWsAuthAction, BitmexWsOperation,
};
use crate::common::enums::{
    BitmexContingencyType, BitmexExecInstruction, BitmexExecType, BitmexLiquidityIndicator,
    BitmexOrderStatus, BitmexOrderType, BitmexPegPriceType, BitmexTimeInForce,
};

/// Custom deserializer for comma-separated `ExecInstruction` values.
fn deserialize_exec_instructions<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<BitmexExecInstruction>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(ref s) if s.is_empty() => Ok(None),
        Some(s) => {
            let instructions: Result<Vec<BitmexExecInstruction>, _> = s
                .split(',')
                .map(|inst| {
                    let trimmed = inst.trim();
                    match trimmed {
                        "ParticipateDoNotInitiate" => {
                            Ok(BitmexExecInstruction::ParticipateDoNotInitiate)
                        }
                        "AllOrNone" => Ok(BitmexExecInstruction::AllOrNone),
                        "MarkPrice" => Ok(BitmexExecInstruction::MarkPrice),
                        "IndexPrice" => Ok(BitmexExecInstruction::IndexPrice),
                        "LastPrice" => Ok(BitmexExecInstruction::LastPrice),
                        "Close" => Ok(BitmexExecInstruction::Close),
                        "ReduceOnly" => Ok(BitmexExecInstruction::ReduceOnly),
                        "Fixed" => Ok(BitmexExecInstruction::Fixed),
                        "" => Ok(BitmexExecInstruction::Unknown),
                        _ => Err(format!("Unknown exec instruction: {trimmed}")),
                    }
                })
                .collect();
            instructions.map(Some).map_err(de::Error::custom)
        }
    }
}

/// BitMEX WebSocket authentication message.
///
/// The args array contains [api_key, expires/nonce, signature].
/// The second element must be a number (not a string) for proper authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitmexAuthentication {
    pub op: BitmexWsAuthAction,
    pub args: (String, i64, String),
}

/// BitMEX WebSocket subscription message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitmexSubscription {
    pub op: BitmexWsOperation,
    pub args: Vec<Ustr>,
}

/// Unified WebSocket message type for BitMEX.
#[derive(Clone, Debug)]
pub enum NautilusWsMessage {
    Data(Vec<Data>),
    OrderStatusReports(Vec<OrderStatusReport>),
    OrderUpdated(OrderUpdated),
    FillReports(Vec<FillReport>),
    PositionStatusReport(PositionStatusReport),
    FundingRateUpdates(Vec<FundingRateUpdate>),
    AccountState(AccountState),
    Reconnected,
}

/// Represents all possible message types from the BitMEX WebSocket API.
#[derive(Debug, Display, Deserialize)]
#[serde(untagged)]
pub enum BitmexWsMessage {
    /// Table websocket message.
    Table(BitmexTableMessage),
    /// Initial welcome message received when connecting to the WebSocket.
    Welcome {
        /// Welcome message text.
        info: String,
        /// API version string.
        version: String,
        /// Server timestamp.
        timestamp: DateTime<Utc>,
        /// Link to API documentation.
        docs: String,
        /// Whether heartbeat is enabled for this connection.
        #[serde(rename = "heartbeatEnabled")]
        heartbeat_enabled: bool,
        /// Rate limit information.
        limit: BitmexRateLimit,
        /// Application name (testnet only).
        #[serde(rename = "appName")]
        app_name: Option<String>,
    },
    /// Subscription response messages.
    Subscription {
        /// Whether the subscription request was successful.
        success: bool,
        /// The subscription topic if successful.
        subscribe: Option<String>,
        /// Original request metadata (present for subscribe/auth/unsubscribe).
        request: Option<BitmexHttpRequest>,
        /// Error message if subscription failed.
        error: Option<String>,
    },
    /// WebSocket error message.
    Error {
        status: u16,
        error: String,
        meta: HashMap<String, String>,
        request: BitmexHttpRequest,
    },
    /// Indicates a WebSocket reconnection has occurred.
    #[serde(skip)]
    Reconnected,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct BitmexHttpRequest {
    pub op: String,
    pub args: Vec<Value>,
}

/// Rate limit information from BitMEX API.
#[derive(Debug, Deserialize)]
pub struct BitmexRateLimit {
    /// Number of requests remaining in the current time window.
    pub remaining: Option<i32>,
}

/// Represents table-based messages.
#[derive(Debug, Display, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "table")]
pub enum BitmexTableMessage {
    OrderBookL2 {
        action: BitmexAction,
        data: Vec<BitmexOrderBookMsg>,
    },
    OrderBookL2_25 {
        action: BitmexAction,
        data: Vec<BitmexOrderBookMsg>,
    },
    OrderBook10 {
        action: BitmexAction,
        data: Vec<BitmexOrderBook10Msg>,
    },
    Quote {
        action: BitmexAction,
        data: Vec<BitmexQuoteMsg>,
    },
    Trade {
        action: BitmexAction,
        data: Vec<BitmexTradeMsg>,
    },
    TradeBin1m {
        action: BitmexAction,
        data: Vec<BitmexTradeBinMsg>,
    },
    TradeBin5m {
        action: BitmexAction,
        data: Vec<BitmexTradeBinMsg>,
    },
    TradeBin1h {
        action: BitmexAction,
        data: Vec<BitmexTradeBinMsg>,
    },
    TradeBin1d {
        action: BitmexAction,
        data: Vec<BitmexTradeBinMsg>,
    },
    Instrument {
        action: BitmexAction,
        data: Vec<BitmexInstrumentMsg>,
    },
    Order {
        action: BitmexAction,
        #[serde(deserialize_with = "deserialize_order_data")]
        data: Vec<OrderData>,
    },
    Execution {
        action: BitmexAction,
        data: Vec<BitmexExecutionMsg>,
    },
    Position {
        action: BitmexAction,
        data: Vec<BitmexPositionMsg>,
    },
    Wallet {
        action: BitmexAction,
        data: Vec<BitmexWalletMsg>,
    },
    Margin {
        action: BitmexAction,
        data: Vec<BitmexMarginMsg>,
    },
    Funding {
        action: BitmexAction,
        data: Vec<BitmexFundingMsg>,
    },
    Insurance {
        action: BitmexAction,
        data: Vec<BitmexInsuranceMsg>,
    },
    Liquidation {
        action: BitmexAction,
        data: Vec<BitmexLiquidationMsg>,
    },
}

/// Represents a single order book entry in the BitMEX order book.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexOrderBookMsg {
    /// The instrument symbol (e.g., "XBTUSD").
    pub symbol: Ustr,
    /// Unique order ID.
    pub id: u64,
    /// Side of the order ("Buy" or "Sell").
    pub side: BitmexSide,
    /// Size of the order, can be None for deletes.
    pub size: Option<u64>,
    /// Price level of the order.
    pub price: f64,
    /// Timestamp of the update.
    pub timestamp: DateTime<Utc>,
    /// Timestamp of the transaction.
    pub transact_time: DateTime<Utc>,
}

/// Represents a single order book entry in the BitMEX order book.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexOrderBook10Msg {
    /// The instrument symbol (e.g., "XBTUSD").
    pub symbol: Ustr,
    /// Array of bid levels, each containing [price, size].
    pub bids: Vec<[f64; 2]>,
    /// Array of ask levels, each containing [price, size].
    pub asks: Vec<[f64; 2]>,
    /// Timestamp of the orderbook snapshot.
    pub timestamp: DateTime<Utc>,
}

/// Represents a top-of-book quote.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexQuoteMsg {
    /// The instrument symbol (e.g., "XBTUSD").
    pub symbol: Ustr,
    /// Price of best bid.
    pub bid_price: Option<f64>,
    /// Size of best bid.
    pub bid_size: Option<u64>,
    /// Price of best ask.
    pub ask_price: Option<f64>,
    /// Size of best ask.
    pub ask_size: Option<u64>,
    /// Timestamp of the quote.
    pub timestamp: DateTime<Utc>,
}

/// Represents a single trade execution on BitMEX.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexTradeMsg {
    /// Timestamp of the trade.
    pub timestamp: DateTime<Utc>,
    /// The instrument symbol.
    pub symbol: Ustr,
    /// Side of the trade ("Buy" or "Sell").
    pub side: BitmexSide,
    /// Size of the trade.
    pub size: u64,
    /// Price the trade executed at.
    pub price: f64,
    /// Direction of the tick ("`PlusTick`", "`MinusTick`", "`ZeroPlusTick`", "`ZeroMinusTick`").
    pub tick_direction: BitmexTickDirection,
    /// Unique trade match ID.
    #[serde(rename = "trdMatchID")]
    pub trd_match_id: Option<Uuid>,
    /// Gross value of the trade in satoshis.
    pub gross_value: Option<i64>,
    /// Home currency value of the trade.
    pub home_notional: Option<f64>,
    /// Foreign currency value of the trade.
    pub foreign_notional: Option<f64>,
    /// Trade type.
    #[serde(rename = "trdType")]
    pub trade_type: Ustr, // TODO: Add enum
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexTradeBinMsg {
    /// Start time of the bin
    pub timestamp: DateTime<Utc>,
    /// Trading instrument symbol
    pub symbol: Ustr,
    /// Opening price for the period
    pub open: f64,
    /// Highest price for the period
    pub high: f64,
    /// Lowest price for the period
    pub low: f64,
    /// Closing price for the period
    pub close: f64,
    /// Number of trades in the period
    pub trades: i64,
    /// Volume traded in the period
    pub volume: i64,
    /// Volume weighted average price
    pub vwap: f64,
    /// Size of the last trade in the period
    pub last_size: i64,
    /// Turnover in satoshis
    pub turnover: i64,
    /// Home currency volume
    pub home_notional: f64,
    /// Foreign currency volume
    pub foreign_notional: f64,
}

/// Represents a single order book entry in the BitMEX order book.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexInstrumentMsg {
    /// The instrument symbol (e.g., "XBTUSD").
    pub symbol: Ustr,
    /// Last traded price for the instrument.
    pub last_price: Option<f64>,
    /// Last tick direction for the instrument.
    pub last_tick_direction: Option<BitmexTickDirection>,
    /// Mark price.
    pub mark_price: Option<f64>,
    /// Index price.
    pub index_price: Option<f64>,
    /// Indicative settlement price.
    pub indicative_settle_price: Option<f64>,
    /// Open interest for the instrument.
    pub open_interest: Option<i64>,
    /// Open value for the instrument.
    pub open_value: Option<i64>,
    /// Fair basis.
    pub fair_basis: Option<f64>,
    /// Fair basis rate.
    pub fair_basis_rate: Option<f64>,
    /// Fair price.
    pub fair_price: Option<f64>,
    /// Mark method.
    pub mark_method: Option<Ustr>,
    /// Indicative tax rate.
    pub indicative_tax_rate: Option<f64>,
    /// Timestamp of the update.
    pub timestamp: DateTime<Utc>,
}

/// Represents an order update message with only changed fields.
/// Used for `update` actions where only modified fields are sent.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexOrderUpdateMsg {
    #[serde(rename = "orderID")]
    pub order_id: Uuid,
    #[serde(rename = "clOrdID")]
    pub cl_ord_id: Option<Ustr>,
    pub account: i64,
    pub symbol: Ustr,
    pub side: Option<BitmexSide>,
    pub price: Option<f64>,
    pub currency: Option<Ustr>,
    pub text: Option<Ustr>,
    pub transact_time: Option<DateTime<Utc>>,
    pub timestamp: Option<DateTime<Utc>>,
    pub leaves_qty: Option<i64>,
    pub cum_qty: Option<i64>,
    pub ord_status: Option<BitmexOrderStatus>,
}

/// Represents a full order message from the WebSocket stream.
/// Used for `insert` and `partial` actions where all fields are present.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexOrderMsg {
    #[serde(rename = "orderID")]
    pub order_id: Uuid,
    #[serde(rename = "clOrdID")]
    pub cl_ord_id: Option<Ustr>,
    #[serde(rename = "clOrdLinkID")]
    pub cl_ord_link_id: Option<Ustr>,
    pub account: i64,
    pub symbol: Ustr,
    pub side: BitmexSide,
    pub order_qty: i64,
    pub price: Option<f64>,
    pub display_qty: Option<i64>,
    pub stop_px: Option<f64>,
    pub peg_offset_value: Option<f64>,
    pub peg_price_type: Option<BitmexPegPriceType>,
    pub currency: Ustr,
    pub settl_currency: Ustr,
    pub ord_type: Option<BitmexOrderType>,
    pub time_in_force: Option<BitmexTimeInForce>,
    #[serde(default, deserialize_with = "deserialize_exec_instructions")]
    pub exec_inst: Option<Vec<BitmexExecInstruction>>,
    pub contingency_type: Option<BitmexContingencyType>,
    pub ord_status: BitmexOrderStatus,
    pub triggered: Option<Ustr>,
    pub working_indicator: bool,
    pub ord_rej_reason: Option<Ustr>,
    pub leaves_qty: i64,
    pub cum_qty: i64,
    pub avg_px: Option<f64>,
    pub text: Option<Ustr>,
    pub transact_time: DateTime<Utc>,
    pub timestamp: DateTime<Utc>,
}

/// Wrapper enum for order data that can be either full or update messages.
#[derive(Clone, Debug)]
pub enum OrderData {
    Full(BitmexOrderMsg),
    Update(BitmexOrderUpdateMsg),
}

/// Custom deserializer for order data that tries to deserialize as full message first,
/// then falls back to update message if fields are missing.
fn deserialize_order_data<'de, D>(deserializer: D) -> Result<Vec<OrderData>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw_values: Vec<serde_json::Value> = Vec::deserialize(deserializer)?;
    let mut result = Vec::new();

    for value in raw_values {
        // Try to deserialize as full message first
        if let Ok(full_msg) = serde_json::from_value::<BitmexOrderMsg>(value.clone()) {
            result.push(OrderData::Full(full_msg));
        } else if let Ok(update_msg) = serde_json::from_value::<BitmexOrderUpdateMsg>(value) {
            result.push(OrderData::Update(update_msg));
        } else {
            return Err(de::Error::custom(
                "Failed to deserialize order data as either full or update message",
            ));
        }
    }

    Ok(result)
}

/// Raw Order and Balance Data.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexExecutionMsg {
    #[serde(rename = "execID")]
    pub exec_id: Option<Uuid>,
    #[serde(rename = "orderID")]
    pub order_id: Option<Uuid>,
    #[serde(rename = "clOrdID")]
    pub cl_ord_id: Option<Ustr>,
    #[serde(rename = "clOrdLinkID")]
    pub cl_ord_link_id: Option<Ustr>,
    pub account: Option<i64>,
    pub symbol: Option<Ustr>,
    pub side: Option<BitmexSide>,
    pub last_qty: Option<i64>,
    pub last_px: Option<f64>,
    pub underlying_last_px: Option<f64>,
    pub last_mkt: Option<Ustr>,
    pub last_liquidity_ind: Option<BitmexLiquidityIndicator>,
    pub order_qty: Option<i64>,
    pub price: Option<f64>,
    pub display_qty: Option<i64>,
    pub stop_px: Option<f64>,
    pub peg_offset_value: Option<f64>,
    pub peg_price_type: Option<BitmexPegPriceType>,
    pub currency: Option<Ustr>,
    pub settl_currency: Option<Ustr>,
    pub exec_type: Option<BitmexExecType>,
    pub ord_type: Option<BitmexOrderType>,
    pub time_in_force: Option<BitmexTimeInForce>,
    #[serde(default, deserialize_with = "deserialize_exec_instructions")]
    pub exec_inst: Option<Vec<BitmexExecInstruction>>,
    pub contingency_type: Option<BitmexContingencyType>,
    pub ex_destination: Option<Ustr>,
    pub ord_status: Option<BitmexOrderStatus>,
    pub triggered: Option<Ustr>,
    pub working_indicator: Option<bool>,
    pub ord_rej_reason: Option<Ustr>,
    pub leaves_qty: Option<i64>,
    pub cum_qty: Option<i64>,
    pub avg_px: Option<f64>,
    pub commission: Option<f64>,
    pub trade_publish_indicator: Option<Ustr>,
    pub multi_leg_reporting_type: Option<Ustr>,
    pub text: Option<Ustr>,
    #[serde(rename = "trdMatchID")]
    pub trd_match_id: Option<Uuid>,
    pub exec_cost: Option<i64>,
    pub exec_comm: Option<i64>,
    pub home_notional: Option<f64>,
    pub foreign_notional: Option<f64>,
    pub transact_time: Option<DateTime<Utc>>,
    pub timestamp: Option<DateTime<Utc>>,
}

/// Position status.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexPositionMsg {
    pub account: i64,
    pub symbol: Ustr,
    pub currency: Option<Ustr>,
    pub underlying: Option<Ustr>,
    pub quote_currency: Option<Ustr>,
    pub commission: Option<f64>,
    pub init_margin_req: Option<f64>,
    pub maint_margin_req: Option<f64>,
    pub risk_limit: Option<i64>,
    pub leverage: Option<f64>,
    pub cross_margin: Option<bool>,
    pub deleverage_percentile: Option<f64>,
    pub rebalanced_pnl: Option<i64>,
    pub prev_realised_pnl: Option<i64>,
    pub prev_unrealised_pnl: Option<i64>,
    pub prev_close_price: Option<f64>,
    pub opening_timestamp: Option<DateTime<Utc>>,
    pub opening_qty: Option<i64>,
    pub opening_cost: Option<i64>,
    pub opening_comm: Option<i64>,
    pub open_order_buy_qty: Option<i64>,
    pub open_order_buy_cost: Option<i64>,
    pub open_order_buy_premium: Option<i64>,
    pub open_order_sell_qty: Option<i64>,
    pub open_order_sell_cost: Option<i64>,
    pub open_order_sell_premium: Option<i64>,
    pub exec_buy_qty: Option<i64>,
    pub exec_buy_cost: Option<i64>,
    pub exec_sell_qty: Option<i64>,
    pub exec_sell_cost: Option<i64>,
    pub exec_qty: Option<i64>,
    pub exec_cost: Option<i64>,
    pub exec_comm: Option<i64>,
    pub current_timestamp: Option<DateTime<Utc>>,
    pub current_qty: Option<i64>,
    pub current_cost: Option<i64>,
    pub current_comm: Option<i64>,
    pub realised_cost: Option<i64>,
    pub unrealised_cost: Option<i64>,
    pub gross_open_cost: Option<i64>,
    pub gross_open_premium: Option<i64>,
    pub gross_exec_cost: Option<i64>,
    pub is_open: Option<bool>,
    pub mark_price: Option<f64>,
    pub mark_value: Option<i64>,
    pub risk_value: Option<i64>,
    pub home_notional: Option<f64>,
    pub foreign_notional: Option<f64>,
    pub pos_state: Option<Ustr>,
    pub pos_cost: Option<i64>,
    pub pos_cost2: Option<i64>,
    pub pos_cross: Option<i64>,
    pub pos_init: Option<i64>,
    pub pos_comm: Option<i64>,
    pub pos_loss: Option<i64>,
    pub pos_margin: Option<i64>,
    pub pos_maint: Option<i64>,
    pub pos_allowance: Option<i64>,
    pub taxable_margin: Option<i64>,
    pub init_margin: Option<i64>,
    pub maint_margin: Option<i64>,
    pub session_margin: Option<i64>,
    pub target_excess_margin: Option<i64>,
    pub var_margin: Option<i64>,
    pub realised_gross_pnl: Option<i64>,
    pub realised_tax: Option<i64>,
    pub realised_pnl: Option<i64>,
    pub unrealised_gross_pnl: Option<i64>,
    pub long_bankrupt: Option<i64>,
    pub short_bankrupt: Option<i64>,
    pub tax_base: Option<i64>,
    pub indicative_tax_rate: Option<f64>,
    pub indicative_tax: Option<i64>,
    pub unrealised_tax: Option<i64>,
    pub unrealised_pnl: Option<i64>,
    pub unrealised_pnl_pcnt: Option<f64>,
    pub unrealised_roe_pcnt: Option<f64>,
    pub avg_cost_price: Option<f64>,
    pub avg_entry_price: Option<f64>,
    pub break_even_price: Option<f64>,
    pub margin_call_price: Option<f64>,
    pub liquidation_price: Option<f64>,
    pub bankrupt_price: Option<f64>,
    pub timestamp: Option<DateTime<Utc>>,
    pub last_price: Option<f64>,
    pub last_value: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexWalletMsg {
    pub account: i64,
    pub currency: Ustr,
    pub prev_deposited: Option<i64>,
    pub prev_withdrawn: Option<i64>,
    pub prev_transfer_in: Option<i64>,
    pub prev_transfer_out: Option<i64>,
    pub prev_amount: Option<i64>,
    pub prev_timestamp: Option<DateTime<Utc>>,
    pub delta_deposited: Option<i64>,
    pub delta_withdrawn: Option<i64>,
    pub delta_transfer_in: Option<i64>,
    pub delta_transfer_out: Option<i64>,
    pub delta_amount: Option<i64>,
    pub deposited: Option<i64>,
    pub withdrawn: Option<i64>,
    pub transfer_in: Option<i64>,
    pub transfer_out: Option<i64>,
    pub amount: Option<i64>,
    pub pending_credit: Option<i64>,
    pub pending_debit: Option<i64>,
    pub confirmed_debit: Option<i64>,
    pub timestamp: Option<DateTime<Utc>>,
    pub addr: Option<Ustr>,
    pub script: Option<Ustr>,
    pub withdrawal_lock: Option<Vec<Ustr>>,
}

/// Represents margin account information
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexMarginMsg {
    /// Account identifier
    pub account: i64,
    /// Currency of the margin account
    pub currency: Ustr,
    /// Risk limit for the account
    pub risk_limit: Option<i64>,
    /// Current amount in the account
    pub amount: Option<i64>,
    /// Previously realized PnL
    pub prev_realised_pnl: Option<i64>,
    /// Gross commission
    pub gross_comm: Option<i64>,
    /// Gross open cost
    pub gross_open_cost: Option<i64>,
    /// Gross open premium
    pub gross_open_premium: Option<i64>,
    /// Gross execution cost
    pub gross_exec_cost: Option<i64>,
    /// Gross mark value
    pub gross_mark_value: Option<i64>,
    /// Risk value
    pub risk_value: Option<i64>,
    /// Initial margin requirement
    pub init_margin: Option<i64>,
    /// Maintenance margin requirement
    pub maint_margin: Option<i64>,
    /// Target excess margin
    pub target_excess_margin: Option<i64>,
    /// Realized profit and loss
    pub realised_pnl: Option<i64>,
    /// Unrealized profit and loss
    pub unrealised_pnl: Option<i64>,
    /// Wallet balance
    pub wallet_balance: Option<i64>,
    /// Margin balance
    pub margin_balance: Option<i64>,
    /// Margin leverage
    pub margin_leverage: Option<f64>,
    /// Margin used percentage
    pub margin_used_pcnt: Option<f64>,
    /// Excess margin
    pub excess_margin: Option<i64>,
    /// Available margin
    pub available_margin: Option<i64>,
    /// Withdrawable margin
    pub withdrawable_margin: Option<i64>,
    /// Maker fee discount
    pub maker_fee_discount: Option<f64>,
    /// Taker fee discount
    pub taker_fee_discount: Option<f64>,
    /// Timestamp of the margin update
    pub timestamp: DateTime<Utc>,
    /// Foreign margin balance
    pub foreign_margin_balance: Option<i64>,
    /// Foreign margin requirement
    pub foreign_requirement: Option<i64>,
}

/// Represents a funding rate update.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexFundingMsg {
    /// Timestamp of the funding update.
    pub timestamp: DateTime<Utc>,
    /// The instrument symbol the funding applies to.
    pub symbol: Ustr,
    /// The funding rate for this interval.
    pub funding_rate: f64,
    /// The daily funding rate.
    pub funding_rate_daily: f64,
}

/// Represents an insurance fund update.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexInsuranceMsg {
    /// The currency of the insurance fund.
    pub currency: Ustr,
    /// Timestamp of the update.
    pub timestamp: DateTime<Utc>,
    /// Current balance of the insurance wallet.
    pub wallet_balance: i64,
}

/// Represents a liquidation order.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexLiquidationMsg {
    /// Unique order ID of the liquidation.
    pub order_id: Ustr,
    /// The instrument symbol being liquidated.
    pub symbol: Ustr,
    /// Side of the liquidation ("Buy" or "Sell").
    pub side: BitmexSide,
    /// Price of the liquidation order.
    pub price: f64,
    /// Remaining quantity to be executed.
    pub leaves_qty: i64,
}
