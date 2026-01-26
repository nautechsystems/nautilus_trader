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

//! Binance Futures WebSocket message types.
//!
//! Futures streams use standard JSON encoding (not SBE like Spot).
//!
//! Message types are separated into data (public market data) and execution
//! (private user data stream) concerns:
//! - [`NautilusDataWsMessage`] - Market data for data clients.
//! - [`NautilusExecWsMessage`] - User data for execution clients.
//! - [`NautilusWsMessage`] - Wrapper enum containing both.

use nautilus_model::{
    data::{Data, OrderBookDeltas},
    events::{
        AccountState, OrderAccepted, OrderCanceled, OrderFilled, OrderRejected, OrderUpdated,
    },
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::InstrumentAny,
};
use nautilus_network::websocket::WebSocketClient;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    common::enums::{
        BinanceFuturesOrderType, BinanceKlineInterval, BinanceMarginType, BinanceOrderStatus,
        BinancePositionSide, BinanceSide, BinanceTimeInForce, BinanceWorkingType, BinanceWsMethod,
    },
    futures::http::BinanceFuturesInstrument,
};

/// Output message from the Futures WebSocket handler.
///
/// Wraps data and execution message types, allowing the single handler to
/// produce messages for both data and execution clients.
#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    /// Public market data message (normalized Nautilus types).
    Data(NautilusDataWsMessage),
    /// Private user data stream message (normalized Nautilus events).
    Exec(Box<NautilusExecWsMessage>),
    /// Raw user data stream message (for internal processing by exec handler).
    ExecRaw(BinanceFuturesExecWsMessage),
    /// Error from the server.
    Error(BinanceFuturesWsErrorMsg),
    /// WebSocket reconnected - subscriptions should be restored.
    Reconnected,
}

/// Market data message from Binance Futures WebSocket.
///
/// These are public messages that don't require authentication.
#[derive(Debug, Clone)]
pub enum NautilusDataWsMessage {
    /// Market data (trades, quotes, etc.).
    Data(Vec<Data>),
    /// Order book deltas with Binance-specific sequence metadata for validation.
    DepthUpdate {
        deltas: OrderBookDeltas,
        first_update_id: u64,
        prev_final_update_id: u64,
    },
    /// Instrument update.
    Instrument(Box<InstrumentAny>),
    /// Raw JSON message (for debugging or unhandled types).
    RawJson(serde_json::Value),
}

/// Normalized execution event from Binance Futures.
///
/// These are normalized Nautilus events produced by the execution handler
/// from raw WebSocket messages. The handler correlates updates with
/// the original order context (strategy_id, etc.) using pending order maps.
#[derive(Debug, Clone)]
pub enum NautilusExecWsMessage {
    /// Account state update (balance changes).
    AccountUpdate(AccountState),
    /// Order accepted by the exchange.
    OrderAccepted(OrderAccepted),
    /// Order canceled.
    OrderCanceled(OrderCanceled),
    /// Order rejected.
    OrderRejected(OrderRejected),
    /// Order filled (partial or full).
    OrderFilled(OrderFilled),
    /// Order modified/amended.
    OrderUpdated(OrderUpdated),
    /// Listen key expired - need to reconnect user data stream.
    ListenKeyExpired,
    /// WebSocket reconnected - subscriptions should be restored.
    Reconnected,
}

/// Raw user data stream message from Binance Futures WebSocket.
///
/// These are raw messages from the user data stream that require
/// a listen key for authentication. The execution handler processes these
/// and emits normalized Nautilus events via [`NautilusExecWsMessage`].
#[derive(Debug, Clone)]
pub enum BinanceFuturesExecWsMessage {
    /// Account update (balance/position changes).
    AccountUpdate(BinanceFuturesAccountUpdateMsg),
    /// Order/trade update.
    OrderUpdate(Box<BinanceFuturesOrderUpdateMsg>),
    /// Margin call warning.
    MarginCall(BinanceFuturesMarginCallMsg),
    /// Account configuration change (leverage, etc.).
    AccountConfigUpdate(BinanceFuturesAccountConfigMsg),
    /// Listen key expired - need to reconnect user data stream.
    ListenKeyExpired,
}

/// Error message from Binance Futures WebSocket.
#[derive(Debug, Clone)]
pub struct BinanceFuturesWsErrorMsg {
    /// Error code from Binance.
    pub code: i64,
    /// Error message.
    pub msg: String,
}

/// Handler command for data client-handler communication.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum DataHandlerCommand {
    /// Set the WebSocket client reference.
    SetClient(WebSocketClient),
    /// Disconnect from the WebSocket.
    Disconnect,
    /// Initialize instruments in the handler cache.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the handler cache.
    UpdateInstrument(InstrumentAny),
    /// Subscribe to streams.
    Subscribe { streams: Vec<String> },
    /// Unsubscribe from streams.
    Unsubscribe { streams: Vec<String> },
}

/// Handler command for execution client-handler communication.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum ExecHandlerCommand {
    /// Set the WebSocket client reference.
    SetClient(WebSocketClient),
    /// Disconnect from the WebSocket.
    Disconnect,
    /// Initialize instruments in the handler cache.
    InitializeInstruments(Vec<BinanceFuturesInstrument>),
    /// Update a single instrument in the handler cache.
    UpdateInstrument(BinanceFuturesInstrument),
    /// Subscribe to user data stream.
    Subscribe { streams: Vec<String> },
    /// Register an order for context tracking.
    RegisterOrder {
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    },
    /// Register a cancel request for context tracking.
    RegisterCancel {
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
    },
    /// Register a modify request for context tracking.
    RegisterModify {
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
    },
}

/// Aggregate trade stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesAggTradeMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Aggregate trade ID.
    #[serde(rename = "a")]
    pub agg_trade_id: u64,
    /// Price.
    #[serde(rename = "p")]
    pub price: String,
    /// Quantity.
    #[serde(rename = "q")]
    pub quantity: String,
    /// First trade ID.
    #[serde(rename = "f")]
    pub first_trade_id: u64,
    /// Last trade ID.
    #[serde(rename = "l")]
    pub last_trade_id: u64,
    /// Trade time in milliseconds.
    #[serde(rename = "T")]
    pub trade_time: i64,
    /// Is buyer the market maker.
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

/// Trade stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesTradeMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Trade ID.
    #[serde(rename = "t")]
    pub trade_id: u64,
    /// Price.
    #[serde(rename = "p")]
    pub price: String,
    /// Quantity.
    #[serde(rename = "q")]
    pub quantity: String,
    /// Trade time in milliseconds.
    #[serde(rename = "T")]
    pub trade_time: i64,
    /// Is buyer the market maker.
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

/// Order book depth update stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesDepthUpdateMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Transaction time in milliseconds.
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// First update ID.
    #[serde(rename = "U")]
    pub first_update_id: u64,
    /// Final update ID.
    #[serde(rename = "u")]
    pub final_update_id: u64,
    /// Previous final update ID.
    #[serde(rename = "pu")]
    pub prev_final_update_id: u64,
    /// Bids [price, quantity].
    #[serde(rename = "b")]
    pub bids: Vec<[String; 2]>,
    /// Asks [price, quantity].
    #[serde(rename = "a")]
    pub asks: Vec<[String; 2]>,
}

/// Mark price stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesMarkPriceMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Mark price.
    #[serde(rename = "p")]
    pub mark_price: String,
    /// Index price.
    #[serde(rename = "i")]
    pub index_price: String,
    /// Estimated settle price.
    #[serde(rename = "P")]
    pub estimated_settle_price: String,
    /// Funding rate.
    #[serde(rename = "r")]
    pub funding_rate: String,
    /// Next funding time in milliseconds.
    #[serde(rename = "T")]
    pub next_funding_time: i64,
}

/// Book ticker stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesBookTickerMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Update ID.
    #[serde(rename = "u")]
    pub update_id: u64,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Transaction time in milliseconds.
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Best bid price.
    #[serde(rename = "b")]
    pub best_bid_price: String,
    /// Best bid quantity.
    #[serde(rename = "B")]
    pub best_bid_qty: String,
    /// Best ask price.
    #[serde(rename = "a")]
    pub best_ask_price: String,
    /// Best ask quantity.
    #[serde(rename = "A")]
    pub best_ask_qty: String,
}

/// Kline/candlestick stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesKlineMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Kline data.
    #[serde(rename = "k")]
    pub kline: BinanceFuturesKlineData,
}

/// Kline data within kline message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesKlineData {
    /// Kline start time.
    #[serde(rename = "t")]
    pub start_time: i64,
    /// Kline close time.
    #[serde(rename = "T")]
    pub close_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Kline interval.
    #[serde(rename = "i")]
    pub interval: BinanceKlineInterval,
    /// First trade ID.
    #[serde(rename = "f")]
    pub first_trade_id: i64,
    /// Last trade ID.
    #[serde(rename = "L")]
    pub last_trade_id: i64,
    /// Open price.
    #[serde(rename = "o")]
    pub open: String,
    /// Close price.
    #[serde(rename = "c")]
    pub close: String,
    /// High price.
    #[serde(rename = "h")]
    pub high: String,
    /// Low price.
    #[serde(rename = "l")]
    pub low: String,
    /// Base asset volume.
    #[serde(rename = "v")]
    pub volume: String,
    /// Number of trades.
    #[serde(rename = "n")]
    pub num_trades: i64,
    /// Is this kline closed.
    #[serde(rename = "x")]
    pub is_closed: bool,
    /// Quote asset volume.
    #[serde(rename = "q")]
    pub quote_volume: String,
    /// Taker buy base asset volume.
    #[serde(rename = "V")]
    pub taker_buy_volume: String,
    /// Taker buy quote asset volume.
    #[serde(rename = "Q")]
    pub taker_buy_quote_volume: String,
}

/// Liquidation order stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesLiquidationMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Order data.
    #[serde(rename = "o")]
    pub order: BinanceFuturesLiquidationOrder,
}

/// Liquidation order details.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesLiquidationOrder {
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Order side.
    #[serde(rename = "S")]
    pub side: BinanceSide,
    /// Order type.
    #[serde(rename = "o")]
    pub order_type: BinanceFuturesOrderType,
    /// Time in force.
    #[serde(rename = "f")]
    pub time_in_force: BinanceTimeInForce,
    /// Original quantity.
    #[serde(rename = "q")]
    pub original_qty: String,
    /// Price.
    #[serde(rename = "p")]
    pub price: String,
    /// Average price.
    #[serde(rename = "ap")]
    pub average_price: String,
    /// Order status.
    #[serde(rename = "X")]
    pub status: BinanceOrderStatus,
    /// Last filled quantity.
    #[serde(rename = "l")]
    pub last_filled_qty: String,
    /// Accumulated filled quantity.
    #[serde(rename = "z")]
    pub accumulated_qty: String,
    /// Trade time in milliseconds.
    #[serde(rename = "T")]
    pub trade_time: i64,
}

/// WebSocket subscription request.
#[derive(Debug, Clone, Serialize)]
pub struct BinanceFuturesWsSubscribeRequest {
    /// Request method.
    pub method: BinanceWsMethod,
    /// Stream names to subscribe.
    pub params: Vec<String>,
    /// Request ID.
    pub id: u64,
}

/// WebSocket subscription response.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesWsSubscribeResponse {
    /// Response result (null on success).
    pub result: Option<serde_json::Value>,
    /// Request ID echoed back.
    pub id: u64,
}

/// WebSocket error response.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesWsErrorResponse {
    /// Error code.
    pub code: i64,
    /// Error message.
    pub msg: String,
    /// Request ID if available.
    pub id: Option<u64>,
}

/// Account update event from user data stream.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesAccountUpdateMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Transaction time in milliseconds.
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Account update data.
    #[serde(rename = "a")]
    pub account: AccountUpdateData,
}

/// Account update data payload.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountUpdateData {
    /// Reason for account update.
    #[serde(rename = "m")]
    pub reason: AccountUpdateReason,
    /// Balance updates.
    #[serde(rename = "B", default)]
    pub balances: Vec<BalanceUpdate>,
    /// Position updates.
    #[serde(rename = "P", default)]
    pub positions: Vec<PositionUpdate>,
}

/// Account update reason type.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccountUpdateReason {
    Deposit,
    Withdraw,
    Order,
    FundingFee,
    WithdrawReject,
    Adjustment,
    InsuranceClear,
    AdminDeposit,
    AdminWithdraw,
    MarginTransfer,
    MarginTypeChange,
    AssetTransfer,
    OptionsPremiumFee,
    OptionsSettleProfit,
    AutoExchange,
    CoinSwapDeposit,
    CoinSwapWithdraw,
    #[serde(other)]
    Unknown,
}

/// Balance update within account update.
#[derive(Debug, Clone, Deserialize)]
pub struct BalanceUpdate {
    /// Asset name.
    #[serde(rename = "a")]
    pub asset: Ustr,
    /// Wallet balance.
    #[serde(rename = "wb")]
    pub wallet_balance: String,
    /// Cross wallet balance.
    #[serde(rename = "cw")]
    pub cross_wallet_balance: String,
    /// Balance change (except for PnL and commission).
    #[serde(rename = "bc", default)]
    pub balance_change: Option<String>,
}

/// Position update within account update.
#[derive(Debug, Clone, Deserialize)]
pub struct PositionUpdate {
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Position amount.
    #[serde(rename = "pa")]
    pub position_amount: String,
    /// Entry price.
    #[serde(rename = "ep")]
    pub entry_price: String,
    /// Break-even price.
    #[serde(rename = "bep", default)]
    pub break_even_price: Option<String>,
    /// Accumulated realized (pre-fee).
    #[serde(rename = "cr")]
    pub accumulated_realized: String,
    /// Unrealized PnL.
    #[serde(rename = "up")]
    pub unrealized_pnl: String,
    /// Margin type.
    #[serde(rename = "mt")]
    pub margin_type: BinanceMarginType,
    /// Isolated wallet (if isolated position).
    #[serde(rename = "iw")]
    pub isolated_wallet: String,
    /// Position side.
    #[serde(rename = "ps")]
    pub position_side: BinancePositionSide,
}

/// Order/trade update event from user data stream.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesOrderUpdateMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Transaction time in milliseconds.
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Order data.
    #[serde(rename = "o")]
    pub order: OrderUpdateData,
}

/// Order update data payload.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderUpdateData {
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Client order ID.
    #[serde(rename = "c")]
    pub client_order_id: String,
    /// Order side.
    #[serde(rename = "S")]
    pub side: BinanceSide,
    /// Order type.
    #[serde(rename = "o")]
    pub order_type: BinanceFuturesOrderType,
    /// Time in force.
    #[serde(rename = "f")]
    pub time_in_force: BinanceTimeInForce,
    /// Original quantity.
    #[serde(rename = "q")]
    pub original_qty: String,
    /// Original price.
    #[serde(rename = "p")]
    pub original_price: String,
    /// Average price.
    #[serde(rename = "ap")]
    pub average_price: String,
    /// Stop price.
    #[serde(rename = "sp")]
    pub stop_price: String,
    /// Execution type.
    #[serde(rename = "x")]
    pub execution_type: BinanceExecutionType,
    /// Order status.
    #[serde(rename = "X")]
    pub order_status: BinanceOrderStatus,
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
    /// Commission asset.
    #[serde(rename = "N", default)]
    pub commission_asset: Option<Ustr>,
    /// Commission amount.
    #[serde(rename = "n", default)]
    pub commission: Option<String>,
    /// Order trade time.
    #[serde(rename = "T")]
    pub trade_time: i64,
    /// Trade ID.
    #[serde(rename = "t")]
    pub trade_id: i64,
    /// Bids notional.
    #[serde(rename = "b", default)]
    pub bids_notional: Option<String>,
    /// Asks notional.
    #[serde(rename = "a", default)]
    pub asks_notional: Option<String>,
    /// Is maker.
    #[serde(rename = "m")]
    pub is_maker: bool,
    /// Is reduce only.
    #[serde(rename = "R")]
    pub is_reduce_only: bool,
    /// Working type.
    #[serde(rename = "wt")]
    pub working_type: BinanceWorkingType,
    /// Original order type.
    #[serde(rename = "ot")]
    pub original_order_type: BinanceFuturesOrderType,
    /// Position side.
    #[serde(rename = "ps")]
    pub position_side: BinancePositionSide,
    /// Close all (for stop orders).
    #[serde(rename = "cp", default)]
    pub close_position: Option<bool>,
    /// Activation price (for trailing stop).
    #[serde(rename = "AP", default)]
    pub activation_price: Option<String>,
    /// Callback rate (for trailing stop).
    #[serde(rename = "cr", default)]
    pub callback_rate: Option<String>,
    /// Price protection.
    #[serde(rename = "pP", default)]
    pub price_protect: Option<bool>,
    /// Realized profit.
    #[serde(rename = "rp")]
    pub realized_profit: String,
    /// Self-trade prevention mode.
    #[serde(rename = "V", default)]
    pub stp_mode: Option<String>,
    /// Price match mode.
    #[serde(rename = "pm", default)]
    pub price_match: Option<String>,
    /// Good till date for GTD orders.
    #[serde(rename = "gtd", default)]
    pub good_till_date: Option<i64>,
}

impl OrderUpdateData {
    /// Returns true if this is a liquidation order.
    #[must_use]
    pub fn is_liquidation(&self) -> bool {
        self.client_order_id.starts_with("autoclose-")
    }

    /// Returns true if this is an ADL (Auto-Deleveraging) order.
    #[must_use]
    pub fn is_adl(&self) -> bool {
        self.client_order_id.starts_with("adl_autoclose")
    }

    /// Returns true if this is a settlement order.
    #[must_use]
    pub fn is_settlement(&self) -> bool {
        self.client_order_id.starts_with("settlement_autoclose-")
    }

    /// Returns true if this is an exchange-generated order.
    #[must_use]
    pub fn is_exchange_generated(&self) -> bool {
        self.is_liquidation() || self.is_adl() || self.is_settlement()
    }
}

/// Execution type for order updates.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceExecutionType {
    /// New order accepted.
    New,
    /// Order canceled.
    Canceled,
    /// Calculated (liquidation, ADL).
    Calculated,
    /// Order expired.
    Expired,
    /// Trade (partial or full fill).
    Trade,
    /// Amendment (order modified).
    Amendment,
}

/// Margin call event from user data stream.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesMarginCallMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Cross wallet balance.
    #[serde(rename = "cw")]
    pub cross_wallet_balance: String,
    /// Positions at risk.
    #[serde(rename = "p")]
    pub positions: Vec<MarginCallPosition>,
}

/// Position at risk in margin call.
#[derive(Debug, Clone, Deserialize)]
pub struct MarginCallPosition {
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Position side.
    #[serde(rename = "ps")]
    pub position_side: BinancePositionSide,
    /// Position amount.
    #[serde(rename = "pa")]
    pub position_amount: String,
    /// Margin type.
    #[serde(rename = "mt")]
    pub margin_type: BinanceMarginType,
    /// Isolated wallet (if any).
    #[serde(rename = "iw")]
    pub isolated_wallet: String,
    /// Mark price.
    #[serde(rename = "mp")]
    pub mark_price: String,
    /// Unrealized PnL.
    #[serde(rename = "up")]
    pub unrealized_pnl: String,
    /// Maintenance margin required.
    #[serde(rename = "mm")]
    pub maintenance_margin: String,
}

/// Account configuration update event.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesAccountConfigMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Transaction time in milliseconds.
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Leverage configuration data.
    #[serde(rename = "ac", default)]
    pub leverage_config: Option<LeverageConfig>,
    /// Asset index price data (for multi-assets mode).
    #[serde(rename = "ai", default)]
    pub asset_index: Option<AssetIndexConfig>,
}

/// Leverage configuration change.
#[derive(Debug, Clone, Deserialize)]
pub struct LeverageConfig {
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// New leverage value.
    #[serde(rename = "l")]
    pub leverage: u32,
}

/// Asset index configuration (multi-assets mode).
#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndexConfig {
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
}

/// Listen key expired event.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesListenKeyExpiredMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
}
