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
    instruments::InstrumentAny,
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
    Instruments(Vec<InstrumentAny>),
    OrderStatusReports(Vec<OrderStatusReport>),
    OrderUpdated(OrderUpdated),
    FillReports(Vec<FillReport>),
    PositionStatusReport(PositionStatusReport),
    FundingRateUpdates(Vec<FundingRateUpdate>),
    AccountState(AccountState),
    Reconnected,
    Authenticated,
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
    /// Indicates a WebSocket reconnection has completed.
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
    pub symbol: Ustr,
    pub root_symbol: Option<Ustr>,
    pub state: Option<Ustr>,
    #[serde(rename = "typ")]
    pub instrument_type: Option<Ustr>,
    pub listing: Option<DateTime<Utc>>,
    pub front: Option<DateTime<Utc>>,
    pub expiry: Option<DateTime<Utc>>,
    pub settle: Option<DateTime<Utc>>,
    pub listed_settle: Option<DateTime<Utc>>,
    pub position_currency: Option<Ustr>,
    pub underlying: Option<Ustr>,
    pub quote_currency: Option<Ustr>,
    pub underlying_symbol: Option<Ustr>,
    pub reference: Option<Ustr>,
    pub reference_symbol: Option<Ustr>,
    pub max_order_qty: Option<f64>,
    pub max_price: Option<f64>,
    pub lot_size: Option<f64>,
    pub tick_size: Option<f64>,
    pub multiplier: Option<f64>,
    pub settl_currency: Option<Ustr>,
    pub underlying_to_position_multiplier: Option<f64>,
    pub underlying_to_settle_multiplier: Option<f64>,
    pub quote_to_settle_multiplier: Option<f64>,
    pub is_quanto: Option<bool>,
    pub is_inverse: Option<bool>,
    pub init_margin: Option<f64>,
    pub maint_margin: Option<f64>,
    pub risk_limit: Option<f64>,
    pub risk_step: Option<f64>,
    pub maker_fee: Option<f64>,
    pub taker_fee: Option<f64>,
    pub settlement_fee: Option<f64>,
    pub funding_base_symbol: Option<Ustr>,
    pub funding_quote_symbol: Option<Ustr>,
    pub funding_premium_symbol: Option<Ustr>,
    pub funding_timestamp: Option<DateTime<Utc>>,
    pub funding_interval: Option<DateTime<Utc>>,
    pub funding_rate: Option<f64>,
    pub indicative_funding_rate: Option<f64>,
    pub last_price: Option<f64>,
    pub last_tick_direction: Option<BitmexTickDirection>,
    pub mark_price: Option<f64>,
    pub mark_method: Option<Ustr>,
    pub index_price: Option<f64>,
    pub indicative_settle_price: Option<f64>,
    pub indicative_tax_rate: Option<f64>,
    pub open_interest: Option<i64>,
    pub open_value: Option<i64>,
    pub fair_basis: Option<f64>,
    pub fair_basis_rate: Option<f64>,
    pub fair_price: Option<f64>,
    pub timestamp: DateTime<Utc>,
}

impl TryFrom<BitmexInstrumentMsg> for crate::http::models::BitmexInstrument {
    type Error = anyhow::Error;

    fn try_from(msg: BitmexInstrumentMsg) -> Result<Self, Self::Error> {
        use crate::common::enums::{BitmexInstrumentState, BitmexInstrumentType};

        // Required fields
        let root_symbol = msg
            .root_symbol
            .ok_or_else(|| anyhow::anyhow!("Missing root_symbol for {}", msg.symbol))?;
        let underlying = msg
            .underlying
            .ok_or_else(|| anyhow::anyhow!("Missing underlying for {}", msg.symbol))?;
        let quote_currency = msg
            .quote_currency
            .ok_or_else(|| anyhow::anyhow!("Missing quote_currency for {}", msg.symbol))?;
        let tick_size = msg
            .tick_size
            .ok_or_else(|| anyhow::anyhow!("Missing tick_size for {}", msg.symbol))?;
        let multiplier = msg
            .multiplier
            .ok_or_else(|| anyhow::anyhow!("Missing multiplier for {}", msg.symbol))?;
        let is_quanto = msg
            .is_quanto
            .ok_or_else(|| anyhow::anyhow!("Missing is_quanto for {}", msg.symbol))?;
        let is_inverse = msg
            .is_inverse
            .ok_or_else(|| anyhow::anyhow!("Missing is_inverse for {}", msg.symbol))?;

        // Parse state - default to Open if not present
        let state = msg
            .state
            .and_then(|s| serde_json::from_str::<BitmexInstrumentState>(&format!("\"{s}\"")).ok())
            .unwrap_or(BitmexInstrumentState::Open);

        // Parse instrument type - default to PerpetualContract if not present
        let instrument_type = msg
            .instrument_type
            .and_then(|t| serde_json::from_str::<BitmexInstrumentType>(&format!("\"{t}\"")).ok())
            .unwrap_or(BitmexInstrumentType::PerpetualContract);

        Ok(Self {
            symbol: msg.symbol,
            root_symbol,
            state,
            instrument_type,
            listing: msg.listing,
            front: msg.front,
            expiry: msg.expiry,
            settle: msg.settle,
            listed_settle: msg.listed_settle,
            position_currency: msg.position_currency,
            underlying,
            quote_currency,
            underlying_symbol: msg.underlying_symbol,
            reference: msg.reference,
            reference_symbol: msg.reference_symbol,
            calc_interval: None,
            publish_interval: None,
            publish_time: None,
            max_order_qty: msg.max_order_qty,
            max_price: msg.max_price,
            lot_size: msg.lot_size,
            tick_size,
            multiplier,
            settl_currency: msg.settl_currency,
            underlying_to_position_multiplier: msg.underlying_to_position_multiplier,
            underlying_to_settle_multiplier: msg.underlying_to_settle_multiplier,
            quote_to_settle_multiplier: msg.quote_to_settle_multiplier,
            is_quanto,
            is_inverse,
            init_margin: msg.init_margin,
            maint_margin: msg.maint_margin,
            risk_limit: msg.risk_limit,
            risk_step: msg.risk_step,
            limit: None,
            taxed: None,
            deleverage: None,
            maker_fee: msg.maker_fee,
            taker_fee: msg.taker_fee,
            settlement_fee: msg.settlement_fee,
            funding_base_symbol: msg.funding_base_symbol,
            funding_quote_symbol: msg.funding_quote_symbol,
            funding_premium_symbol: msg.funding_premium_symbol,
            funding_timestamp: msg.funding_timestamp,
            funding_interval: msg.funding_interval,
            funding_rate: msg.funding_rate,
            indicative_funding_rate: msg.indicative_funding_rate,
            rebalance_timestamp: None,
            rebalance_interval: None,
            prev_close_price: None,
            limit_down_price: None,
            limit_up_price: None,
            prev_total_volume: None,
            total_volume: None,
            volume: None,
            volume_24h: None,
            prev_total_turnover: None,
            total_turnover: None,
            turnover: None,
            turnover_24h: None,
            home_notional_24h: None,
            foreign_notional_24h: None,
            prev_price_24h: None,
            vwap: None,
            high_price: None,
            low_price: None,
            last_price: msg.last_price,
            last_price_protected: None,
            last_tick_direction: None, // WebSocket uses different enum, skip for now
            last_change_pcnt: None,
            bid_price: None,
            mid_price: None,
            ask_price: None,
            impact_bid_price: None,
            impact_mid_price: None,
            impact_ask_price: None,
            has_liquidity: None,
            open_interest: msg.open_interest.map(|v| v as f64),
            open_value: msg.open_value.map(|v| v as f64),
            fair_method: None,
            fair_basis_rate: msg.fair_basis_rate,
            fair_basis: msg.fair_basis,
            fair_price: msg.fair_price,
            mark_method: None,
            mark_price: msg.mark_price,
            indicative_settle_price: msg.indicative_settle_price,
            settled_price_adjustment_rate: None,
            settled_price: None,
            instant_pnl: false,
            min_tick: None,
            funding_base_rate: None,
            funding_quote_rate: None,
            capped: None,
            opening_timestamp: None,
            closing_timestamp: None,
            timestamp: msg.timestamp,
        })
    }
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_try_from_instrument_msg_with_full_data_success() {
        let json_data = r#"{
            "symbol": "XBTUSD",
            "rootSymbol": "XBT",
            "state": "Open",
            "typ": "FFWCSX",
            "listing": "2016-05-13T12:00:00.000Z",
            "front": "2016-05-13T12:00:00.000Z",
            "positionCurrency": "USD",
            "underlying": "XBT",
            "quoteCurrency": "USD",
            "underlyingSymbol": "XBT=",
            "reference": "BMEX",
            "referenceSymbol": ".BXBT",
            "maxOrderQty": 10000000,
            "maxPrice": 1000000,
            "lotSize": 100,
            "tickSize": 0.1,
            "multiplier": -100000000,
            "settlCurrency": "XBt",
            "underlyingToSettleMultiplier": -100000000,
            "isQuanto": false,
            "isInverse": true,
            "initMargin": 0.01,
            "maintMargin": 0.005,
            "riskLimit": 20000000000,
            "riskStep": 15000000000,
            "taxed": true,
            "deleverage": true,
            "makerFee": 0.0005,
            "takerFee": 0.0005,
            "settlementFee": 0,
            "fundingBaseSymbol": ".XBTBON8H",
            "fundingQuoteSymbol": ".USDBON8H",
            "fundingPremiumSymbol": ".XBTUSDPI8H",
            "fundingTimestamp": "2024-11-25T04:00:00.000Z",
            "fundingInterval": "2000-01-01T08:00:00.000Z",
            "fundingRate": 0.00011,
            "indicativeFundingRate": 0.000125,
            "prevClosePrice": 97409.63,
            "limitDownPrice": null,
            "limitUpPrice": null,
            "prevTotalVolume": 3868480147789,
            "totalVolume": 3868507398889,
            "volume": 27251100,
            "volume24h": 419742700,
            "prevTotalTurnover": 37667656761390205,
            "totalTurnover": 37667684492745237,
            "turnover": 27731355032,
            "turnover24h": 431762899194,
            "homeNotional24h": 4317.62899194,
            "foreignNotional24h": 419742700,
            "prevPrice24h": 97655,
            "vwap": 97216.6863,
            "highPrice": 98743.5,
            "lowPrice": 95802.9,
            "lastPrice": 97893.7,
            "lastPriceProtected": 97912.5054,
            "lastTickDirection": "PlusTick",
            "lastChangePcnt": 0.0024,
            "bidPrice": 97882.5,
            "midPrice": 97884.8,
            "askPrice": 97887.1,
            "impactBidPrice": 97882.7951,
            "impactMidPrice": 97884.7,
            "impactAskPrice": 97886.6277,
            "hasLiquidity": true,
            "openInterest": 411647400,
            "openValue": 420691293378,
            "fairMethod": "FundingRate",
            "fairBasisRate": 0.12045,
            "fairBasis": 5.99,
            "fairPrice": 97849.76,
            "markMethod": "FairPrice",
            "markPrice": 97849.76,
            "indicativeSettlePrice": 97843.77,
            "instantPnl": true,
            "timestamp": "2024-11-24T23:33:19.034Z",
            "minTick": 0.01,
            "fundingBaseRate": 0.0003,
            "fundingQuoteRate": 0.0006,
            "capped": false
        }"#;

        let ws_msg: BitmexInstrumentMsg =
            serde_json::from_str(json_data).expect("Failed to deserialize instrument message");

        let result = crate::http::models::BitmexInstrument::try_from(ws_msg);
        assert!(
            result.is_ok(),
            "TryFrom should succeed with full instrument data"
        );

        let instrument = result.unwrap();
        assert_eq!(instrument.symbol.as_str(), "XBTUSD");
        assert_eq!(instrument.root_symbol.as_str(), "XBT");
        assert_eq!(instrument.quote_currency.as_str(), "USD");
        assert_eq!(instrument.tick_size, 0.1);
    }

    #[rstest]
    fn test_try_from_instrument_msg_with_partial_data_fails() {
        let json_data = r#"{
            "symbol": "XBTUSD",
            "lastPrice": 95123.5,
            "lastTickDirection": "ZeroPlusTick",
            "markPrice": 95125.7,
            "indexPrice": 95124.3,
            "indicativeSettlePrice": 95126.0,
            "openInterest": 123456789,
            "openValue": 1234567890,
            "fairBasis": 1.4,
            "fairBasisRate": 0.00001,
            "fairPrice": 95125.0,
            "markMethod": "FairPrice",
            "indicativeTaxRate": 0.00075,
            "timestamp": "2024-11-25T12:00:00.000Z"
        }"#;

        let ws_msg: BitmexInstrumentMsg =
            serde_json::from_str(json_data).expect("Failed to deserialize instrument message");

        let result = crate::http::models::BitmexInstrument::try_from(ws_msg);
        assert!(
            result.is_err(),
            "TryFrom should fail with partial instrument data (update action)"
        );

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Missing"),
            "Error should indicate missing required fields"
        );
    }
}
