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

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nautilus_model::{
    data::{Data, funding::FundingRateUpdate},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
};
use serde::Deserialize;
use strum::Display;
use uuid::Uuid;

use super::enums::{Action, Side, TickDirection};
use crate::enums::{
    ContingencyType, ExecInstruction, ExecType, LiquidityIndicator, OrderStatus, OrderType,
    PegPriceType, TimeInForce,
};

/// Unified WebSocket message type for BitMEX.
#[derive(Clone, Debug)]
pub enum NautilusWsMessage {
    Data(Vec<Data>),
    OrderStatusReport(Box<OrderStatusReport>),
    FillReports(Vec<FillReport>),
    PositionStatusReport(Box<PositionStatusReport>),
    FundingRateUpdates(Vec<FundingRateUpdate>),
}

/// Represents all possible message types from the `BitMEX` WebSocket API.
#[derive(Debug, Display, Deserialize)]
#[serde(untagged)]
pub enum WsMessage {
    /// Table websocket message.
    Table(TableMessage),
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
        limit: RateLimit,
    },
    /// Subscription response messages.
    Subscription {
        /// Whether the subscription request was successful.
        success: bool,
        /// The subscription topic if successful.
        subscribe: Option<String>,
        /// Error message if subscription failed.
        error: Option<String>,
    },
    Error {
        status: u16,
        error: String,
        meta: HashMap<String, String>,
        request: HttpRequest,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct HttpRequest {
    pub op: String,
    pub args: Vec<String>,
}

/// Rate limit information from `BitMEX` API.
#[derive(Debug, Deserialize)]
pub struct RateLimit {
    /// Number of requests remaining in the current time window.
    pub remaining: i32,
}

/// Represents table-based messages.
#[derive(Debug, Display, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "table")]
pub enum TableMessage {
    OrderBookL2 {
        action: Action,
        data: Vec<OrderBookMsg>,
    },
    OrderBookL2_25 {
        action: Action,
        data: Vec<OrderBookMsg>,
    },
    OrderBook10 {
        action: Action,
        data: Vec<OrderBook10Msg>,
    },
    Quote {
        action: Action,
        data: Vec<QuoteMsg>,
    },
    Trade {
        action: Action,
        data: Vec<TradeMsg>,
    },
    TradeBin1m {
        action: Action,
        data: Vec<TradeBinMsg>,
    },
    TradeBin5m {
        action: Action,
        data: Vec<TradeBinMsg>,
    },
    TradeBin1h {
        action: Action,
        data: Vec<TradeBinMsg>,
    },
    TradeBin1d {
        action: Action,
        data: Vec<TradeBinMsg>,
    },
    Instrument {
        action: Action,
        data: Vec<InstrumentMsg>,
    },
    Order {
        action: Action,
        data: Vec<OrderMsg>,
    },
    Execution {
        action: Action,
        data: Vec<ExecutionMsg>,
    },
    Position {
        action: Action,
        data: Vec<PositionMsg>,
    },
    Wallet {
        action: Action,
        data: Vec<WalletMsg>,
    },
    Margin {
        action: Action,
        data: Vec<MarginMsg>,
    },
    Funding {
        action: Action,
        data: Vec<FundingMsg>,
    },
    Insurance {
        action: Action,
        data: Vec<InsuranceMsg>,
    },
    Liquidation {
        action: Action,
        data: Vec<LiquidationMsg>,
    },
}

/// Represents a single order book entry in the `BitMEX` order book.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderBookMsg {
    /// The instrument symbol (e.g., "XBTUSD").
    pub symbol: String,
    /// Unique order ID.
    pub id: u64,
    /// Side of the order ("Buy" or "Sell").
    pub side: Side,
    /// Size of the order, can be None for deletes.
    pub size: Option<u64>,
    /// Price level of the order.
    pub price: f64,
    /// Timestamp of the update.
    pub timestamp: DateTime<Utc>,
    /// Timestamp of the transaction.
    pub transact_time: DateTime<Utc>,
}

/// Represents a single order book entry in the `BitMEX` order book.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderBook10Msg {
    /// The instrument symbol (e.g., "XBTUSD").
    pub symbol: String,
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
pub struct QuoteMsg {
    /// The instrument symbol (e.g., "XBTUSD").
    pub symbol: String,
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

/// Represents a single trade execution on `BitMEX`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeMsg {
    /// Timestamp of the trade.
    pub timestamp: DateTime<Utc>,
    /// The instrument symbol.
    pub symbol: String,
    /// Side of the trade ("Buy" or "Sell").
    pub side: Side,
    /// Size of the trade.
    pub size: u64,
    /// Price the trade executed at.
    pub price: f64,
    /// Direction of the tick ("`PlusTick`", "`MinusTick`", "`ZeroPlusTick`", "`ZeroMinusTick`").
    pub tick_direction: TickDirection,
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
    pub trade_type: String, // TODO: Add enum
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeBinMsg {
    /// Start time of the bin
    pub timestamp: DateTime<Utc>,
    /// Trading instrument symbol
    pub symbol: String,
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

/// Represents a single order book entry in the `BitMEX` order book.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentMsg {
    /// The instrument symbol (e.g., "XBTUSD").
    pub symbol: String,
    /// Last traded price for the instrument.
    pub last_price: Option<f64>,
    /// Last tick direction for the instrument.
    pub last_tick_direction: Option<TickDirection>,
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
    pub mark_method: Option<String>,
    /// Indicative tax rate.
    pub indicative_tax_rate: Option<f64>,
    /// Timestamp of the update.
    pub timestamp: DateTime<Utc>,
}

/// Represents an order update from the WebSocket stream
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderMsg {
    #[serde(rename = "orderID")]
    pub order_id: Uuid,
    #[serde(rename = "clOrdID")]
    pub cl_ord_id: Option<String>,
    #[serde(rename = "clOrdLinkID")]
    pub cl_ord_link_id: Option<String>,
    pub account: i64,
    pub symbol: String,
    pub side: Side,
    pub order_qty: i64,
    pub price: Option<f64>,
    pub display_qty: Option<i64>,
    pub stop_px: Option<f64>,
    pub peg_offset_value: Option<f64>,
    pub peg_price_type: Option<PegPriceType>,
    pub currency: String,
    pub settl_currency: String,
    pub ord_type: OrderType,
    pub time_in_force: TimeInForce,
    pub exec_inst: Option<ExecInstruction>,
    pub contingency_type: Option<ContingencyType>,
    pub ord_status: OrderStatus,
    pub triggered: Option<String>,
    pub working_indicator: bool,
    pub ord_rej_reason: Option<String>,
    pub leaves_qty: i64,
    pub cum_qty: i64,
    pub avg_px: Option<f64>,
    pub text: Option<String>,
    pub transact_time: DateTime<Utc>,
    pub timestamp: DateTime<Utc>,
}

/// Raw Order and Balance Data.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionMsg {
    #[serde(rename = "execID")]
    pub exec_id: Uuid,
    #[serde(rename = "orderID")]
    pub order_id: Option<Uuid>,
    #[serde(rename = "clOrdID")]
    pub cl_ord_id: Option<String>,
    #[serde(rename = "clOrdLinkID")]
    pub cl_ord_link_id: Option<String>,
    pub account: Option<i64>,
    pub symbol: Option<String>,
    pub side: Option<Side>,
    pub last_qty: Option<i64>,
    pub last_px: Option<f64>,
    pub underlying_last_px: Option<f64>,
    pub last_mkt: Option<String>,
    pub last_liquidity_ind: Option<LiquidityIndicator>,
    pub order_qty: Option<i64>,
    pub price: Option<f64>,
    pub display_qty: Option<i64>,
    pub stop_px: Option<f64>,
    pub peg_offset_value: Option<f64>,
    pub peg_price_type: Option<PegPriceType>,
    pub currency: Option<String>,
    pub settl_currency: Option<String>,
    pub exec_type: Option<ExecType>,
    pub ord_type: Option<OrderType>,
    pub time_in_force: Option<TimeInForce>,
    pub exec_inst: Option<ExecInstruction>,
    pub contingency_type: Option<ContingencyType>,
    pub ex_destination: Option<String>,
    pub ord_status: Option<String>,
    pub triggered: Option<String>,
    pub working_indicator: Option<bool>,
    pub ord_rej_reason: Option<String>,
    pub leaves_qty: Option<i64>,
    pub cum_qty: Option<i64>,
    pub avg_px: Option<f64>,
    pub commission: Option<f64>,
    pub trade_publish_indicator: Option<String>,
    pub multi_leg_reporting_type: Option<String>,
    pub text: Option<String>,
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
pub struct PositionMsg {
    pub account: i64,
    pub symbol: String,
    pub currency: Option<String>,
    pub underlying: Option<String>,
    pub quote_currency: Option<String>,
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
    pub pos_state: Option<String>,
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
pub struct WalletMsg {
    pub account: i64,
    pub currency: String,
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
    pub addr: Option<String>,
    pub script: Option<String>,
    pub withdrawal_lock: Option<Vec<String>>,
}

/// Represents margin account information
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarginMsg {
    /// Account identifier
    pub account: i64,
    /// Currency of the margin account
    pub currency: String,
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
pub struct FundingMsg {
    /// Timestamp of the funding update.
    pub timestamp: DateTime<Utc>,
    /// The instrument symbol the funding applies to.
    pub symbol: String,
    /// The funding rate for this interval.
    pub funding_rate: f64,
    /// The daily funding rate.
    pub funding_rate_daily: f64,
}

/// Represents an insurance fund update.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsuranceMsg {
    /// The currency of the insurance fund.
    pub currency: String,
    /// Timestamp of the update.
    pub timestamp: DateTime<Utc>,
    /// Current balance of the insurance wallet.
    pub wallet_balance: i64,
}

/// Represents a liquidation order.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiquidationMsg {
    /// Unique order ID of the liquidation.
    pub order_id: String,
    /// The instrument symbol being liquidated.
    pub symbol: String,
    /// Side of the liquidation ("Buy" or "Sell").
    pub side: Side,
    /// Price of the liquidation order.
    pub price: f64,
    /// Remaining quantity to be executed.
    pub leaves_qty: i64,
}
