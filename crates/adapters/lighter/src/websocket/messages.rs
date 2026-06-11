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

//! Wire frames and handler-output message types for Lighter streams.

use std::fmt::Debug;

use ahash::AHashMap;
use nautilus_core::serialization::{
    deserialize_decimal, deserialize_decimal_from_str, deserialize_optional_decimal,
};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas,
        OrderBookDepth10, QuoteTick, TradeTick,
    },
    events::AccountState,
    reports::PositionStatusReport,
};
use rust_decimal::Decimal;
use serde::{
    Deserialize, Serialize,
    de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_json::value::RawValue;
use ustr::Ustr;

use crate::{
    common::enums::LighterCandleResolution,
    http::models::{LighterOrder, LighterPriceLevel, LighterTrade},
};

/// Inbound message produced by the Lighter feed handler and consumed by the
/// data and execution clients.
///
/// Account-stream variants carry typed Nautilus reports so that the execution
/// client can route them without re-parsing. Fills can arrive on both
/// `account_orders` (as the embedded fill quantity) and `account_all_trades`
/// (as discrete trade prints); the handler emits both untouched and the
/// execution-side consumer is responsible for cross-source dedup.
#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    Trades(Vec<TradeTick>),
    Quote(QuoteTick),
    Deltas(OrderBookDeltas),
    Depth10(Box<OrderBookDepth10>),
    Bar(Bar),
    MarkPrice(MarkPriceUpdate),
    IndexPrice(IndexPriceUpdate),
    FundingRate(FundingRateUpdate),
    ExecutionReports(Vec<ExecutionReport>),
    PositionSnapshot(Vec<PositionStatusReport>),
    AccountState(Box<AccountState>),
    SendTxAck {
        tx_hash: Option<String>,
        code: i64,
    },
    SendTxRejected {
        source: SendTxRejectionSource,
        code: Option<i64>,
        message: String,
    },
    Raw(serde_json::Value),
    Reconnected,
    /// Marker emitted by the feed handler right after each account stream
    /// has delivered its first frame. The execution consumption loop forwards
    /// any preceding typed reports first, then marks the corresponding
    /// readiness flag, keeping `connect()` blocked until applied state is
    /// observable to strategies.
    AccountStreamFirstFrame(AccountStream),
}

/// Identifier for one of the five account-scoped WebSocket streams the
/// execution client subscribes to on connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountStream {
    Orders,
    Trades,
    Positions,
    Assets,
    UserStats,
}

/// Origin of a Lighter `sendTx` rejection signal.
///
/// `Ack` is a direct non-200 response to our own `jsonapi/sendtx` request and
/// is always attributable to the most recent pending sendTx. `BareError` is a
/// standalone error frame that carries no correlation field, so attribution
/// relies on the FIFO pending queue plus a short attribution window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendTxRejectionSource {
    Ack,
    BareError,
}

/// Wrapper for the raw venue payloads emitted on Lighter account streams.
///
/// Carries unparsed [`LighterOrder`] / [`LighterTrade`] so the execution
/// consumption loop can decide between two paths:
///
/// - Tracked: build a typed `OrderEventAny` variant via the parsers in
///   [`crate::websocket::parse`].
/// - Untracked: convert to `OrderStatusReport` / `FillReport` and forward
///   for the engine's external-order reconciliation pipeline.
///
/// The handler produces these in batches per frame so that all reports
/// observed in one venue update are delivered atomically to the consumer.
#[derive(Debug, Clone)]
#[allow(
    clippy::large_enum_variant,
    reason = "payload variants are short-lived and consumed once on the venue-message channel"
)]
pub enum ExecutionReport {
    Order(LighterOrder),
    Fill(LighterTrade),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LighterWsRequest {
    #[serde(rename = "subscribe")]
    Subscribe {
        channel: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        auth: Option<String>,
    },
    #[serde(rename = "unsubscribe")]
    Unsubscribe { channel: String },
    #[serde(rename = "jsonapi/sendtx")]
    SendTx { data: LighterWsSendTx },
}

impl Debug for LighterWsRequest {
    /// Custom `Debug` that redacts the `auth` field of `Subscribe`. The
    /// serialized form of this enum is what hits the wire as a Lighter L2
    /// bearer token; deriving `Debug` would otherwise leak it via any
    /// `format!("{request:?}")` call in error or trace paths.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Subscribe { channel, auth } => f
                .debug_struct(stringify!(Subscribe))
                .field("channel", channel)
                .field("authed", &auth.is_some())
                .finish(),
            Self::Unsubscribe { channel } => f
                .debug_struct(stringify!(Unsubscribe))
                .field("channel", channel)
                .finish(),
            Self::SendTx { data } => f
                .debug_struct(stringify!(SendTx))
                .field("data", data)
                .finish(),
        }
    }
}

impl LighterWsRequest {
    #[must_use]
    pub fn subscribe(channel: impl Into<String>) -> Self {
        Self::Subscribe {
            channel: channel.into(),
            auth: None,
        }
    }

    #[must_use]
    pub fn subscribe_auth(channel: impl Into<String>, auth: impl Into<String>) -> Self {
        Self::Subscribe {
            channel: channel.into(),
            auth: Some(auth.into()),
        }
    }

    #[must_use]
    pub fn unsubscribe(channel: impl Into<String>) -> Self {
        Self::Unsubscribe {
            channel: channel.into(),
        }
    }
}

/// `tx_info` is carried as [`Box<RawValue>`] so the typed-tx renderer in
/// [`crate::signing::tx::TxInfoJson`] can hand the wrapper a pre-rendered
/// JSON string without paying for a parse-into-Value round-trip on every
/// exec command. The outer [`LighterWsRequest`] serialization emits the raw
/// source bytes inline. `PartialEq` is not derived because [`RawValue`]
/// doesn't implement it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterWsSendTx {
    pub tx_type: u8,
    pub tx_info: Box<RawValue>,
}

/// Wire labels for the Lighter WebSocket channel taxonomy.
///
/// Centralizes the channel name strings (`"order_book"`, `"trade"`, ...) so
/// outbound subscription payloads, topic keys, and inbound topic parsing all
/// share one source of truth.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum LighterWsChannelKind {
    OrderBook,
    Ticker,
    Trade,
    Candle,
    MarketStats,
    SpotMarketStats,
    AccountAll,
    AccountOrders,
    AccountAllOrders,
    AccountAllTrades,
    AccountAllPositions,
    AccountAllAssets,
    UserStats,
    Height,
}

impl LighterWsChannelKind {
    /// Returns the venue wire label for this channel kind.
    #[must_use]
    pub const fn as_wire_str(self) -> &'static str {
        match self {
            Self::OrderBook => "order_book",
            Self::Ticker => "ticker",
            Self::Trade => "trade",
            Self::Candle => "candle",
            Self::MarketStats => "market_stats",
            Self::SpotMarketStats => "spot_market_stats",
            Self::AccountAll => "account_all",
            Self::AccountOrders => "account_orders",
            Self::AccountAllOrders => "account_all_orders",
            Self::AccountAllTrades => "account_all_trades",
            Self::AccountAllPositions => "account_all_positions",
            Self::AccountAllAssets => "account_all_assets",
            Self::UserStats => "user_stats",
            Self::Height => "height",
        }
    }

    /// Returns the channel kind matching `wire_str`, or `None` if unknown.
    #[must_use]
    pub fn from_wire_str(wire_str: &str) -> Option<Self> {
        match wire_str {
            "order_book" => Some(Self::OrderBook),
            "ticker" => Some(Self::Ticker),
            "trade" => Some(Self::Trade),
            "candle" => Some(Self::Candle),
            "market_stats" => Some(Self::MarketStats),
            "spot_market_stats" => Some(Self::SpotMarketStats),
            "account_all" => Some(Self::AccountAll),
            "account_orders" => Some(Self::AccountOrders),
            "account_all_orders" => Some(Self::AccountAllOrders),
            "account_all_trades" => Some(Self::AccountAllTrades),
            "account_all_positions" => Some(Self::AccountAllPositions),
            "account_all_assets" => Some(Self::AccountAllAssets),
            "user_stats" => Some(Self::UserStats),
            "height" => Some(Self::Height),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LighterWsChannel {
    OrderBook(i16),
    Ticker(i16),
    MarketStats(LighterMarketSelection),
    SpotMarketStats(LighterMarketSelection),
    Trade(i16),
    Candle {
        market_index: i16,
        resolution: LighterCandleResolution,
    },
    AccountAll(i64),
    AccountOrders {
        market_index: i16,
        account_index: i64,
    },
    AccountAllOrders(i64),
    AccountAllTrades(i64),
    AccountAllPositions(i64),
    AccountAllAssets(i64),
    UserStats(i64),
    Height,
}

impl LighterWsChannel {
    /// Returns the kind of this channel.
    #[must_use]
    pub const fn kind(&self) -> LighterWsChannelKind {
        match self {
            Self::OrderBook(_) => LighterWsChannelKind::OrderBook,
            Self::Ticker(_) => LighterWsChannelKind::Ticker,
            Self::Trade(_) => LighterWsChannelKind::Trade,
            Self::Candle { .. } => LighterWsChannelKind::Candle,
            Self::MarketStats(_) => LighterWsChannelKind::MarketStats,
            Self::SpotMarketStats(_) => LighterWsChannelKind::SpotMarketStats,
            Self::AccountAll(_) => LighterWsChannelKind::AccountAll,
            Self::AccountOrders { .. } => LighterWsChannelKind::AccountOrders,
            Self::AccountAllOrders(_) => LighterWsChannelKind::AccountAllOrders,
            Self::AccountAllTrades(_) => LighterWsChannelKind::AccountAllTrades,
            Self::AccountAllPositions(_) => LighterWsChannelKind::AccountAllPositions,
            Self::AccountAllAssets(_) => LighterWsChannelKind::AccountAllAssets,
            Self::UserStats(_) => LighterWsChannelKind::UserStats,
            Self::Height => LighterWsChannelKind::Height,
        }
    }

    #[must_use]
    pub fn subscription_channel(&self) -> String {
        let kind = self.kind().as_wire_str();

        match self {
            Self::OrderBook(market_index)
            | Self::Ticker(market_index)
            | Self::Trade(market_index) => format!("{kind}/{market_index}"),
            Self::Candle {
                market_index,
                resolution,
            } => format!("{kind}/{market_index}/{}", resolution.as_str()),
            Self::MarketStats(selection) | Self::SpotMarketStats(selection) => {
                format!("{kind}/{}", selection.subscription_value())
            }
            Self::AccountAll(account_index)
            | Self::AccountAllOrders(account_index)
            | Self::AccountAllTrades(account_index)
            | Self::AccountAllPositions(account_index)
            | Self::AccountAllAssets(account_index)
            | Self::UserStats(account_index) => format!("{kind}/{account_index}"),
            Self::AccountOrders {
                market_index,
                account_index,
            } => format!("{kind}/{market_index}/{account_index}"),
            Self::Height => kind.to_string(),
        }
    }

    /// Returns the canonical topic key used to track this subscription.
    ///
    /// Lighter inbound frames carry a `channel` field formatted with `:`
    /// (e.g. `order_book:0`) while outbound subscribe payloads use `/`
    /// (e.g. `order_book/0`). The topic key matches the inbound form so the
    /// handler can correlate frame channel fields against the tracked
    /// subscription set.
    #[must_use]
    pub fn topic_key(&self) -> String {
        self.subscription_channel().replace('/', ":")
    }

    /// Returns `true` when subscribing to this channel requires an auth token.
    #[must_use]
    pub const fn requires_auth(&self) -> bool {
        matches!(
            self,
            Self::AccountAll(_)
                | Self::AccountOrders { .. }
                | Self::AccountAllOrders(_)
                | Self::AccountAllTrades(_)
                | Self::AccountAllPositions(_)
                | Self::AccountAllAssets(_)
                | Self::UserStats(_)
        )
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LighterMarketSelection {
    All,
    Market(i16),
}

impl LighterMarketSelection {
    fn subscription_value(self) -> String {
        match self {
            Self::All => "all".to_string(),
            Self::Market(market_index) => market_index.to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type")]
pub enum LighterWsFrame {
    #[serde(rename = "subscribed/order_book")]
    OrderBookSnapshot {
        channel: Ustr,
        #[serde(default)]
        last_updated_at: u64,
        offset: i64,
        order_book: LighterWsOrderBook,
        timestamp: u64,
    },
    #[serde(rename = "update/order_book")]
    OrderBook {
        channel: Ustr,
        last_updated_at: u64,
        offset: i64,
        order_book: LighterWsOrderBook,
        timestamp: u64,
    },
    #[serde(rename = "subscribed/ticker")]
    TickerSnapshot {
        channel: Ustr,
        #[serde(default)]
        last_updated_at: u64,
        nonce: i64,
        ticker: LighterTicker,
        timestamp: u64,
    },
    #[serde(rename = "update/ticker")]
    Ticker {
        channel: Ustr,
        last_updated_at: u64,
        nonce: i64,
        ticker: LighterTicker,
        timestamp: u64,
    },
    #[serde(rename = "update/market_stats", alias = "subscribed/market_stats")]
    MarketStats {
        channel: Ustr,
        market_stats: LighterMarketStatsPayload,
        timestamp: u64,
    },
    #[serde(
        rename = "update/spot_market_stats",
        alias = "subscribed/spot_market_stats"
    )]
    SpotMarketStats {
        channel: Ustr,
        spot_market_stats: LighterSpotMarketStatsPayload,
        timestamp: u64,
    },
    #[serde(rename = "subscribed/trade")]
    TradeSnapshot {
        channel: Ustr,
        #[serde(default, deserialize_with = "deserialize_trade_vec")]
        liquidation_trades: Vec<LighterTrade>,
        nonce: i64,
        #[serde(default, deserialize_with = "deserialize_trade_vec")]
        trades: Vec<LighterTrade>,
    },
    #[serde(rename = "update/trade")]
    Trade {
        channel: Ustr,
        #[serde(default, deserialize_with = "deserialize_trade_vec")]
        liquidation_trades: Vec<LighterTrade>,
        nonce: i64,
        #[serde(default, deserialize_with = "deserialize_trade_vec")]
        trades: Vec<LighterTrade>,
    },
    #[serde(rename = "update/account_orders")]
    AccountOrders {
        account: i64,
        channel: Ustr,
        nonce: i64,
        orders: AHashMap<Ustr, Vec<LighterOrder>>,
    },
    #[serde(
        rename = "update/account_all_orders",
        alias = "subscribed/account_all_orders"
    )]
    AccountAllOrders {
        channel: Ustr,
        orders: AHashMap<Ustr, Vec<LighterOrder>>,
    },
    #[serde(rename = "subscribed/account_all_trades")]
    AccountAllTradesSnapshot {
        channel: Ustr,
        #[serde(default, deserialize_with = "deserialize_trade_vec")]
        trades: Vec<LighterTrade>,
        #[serde(deserialize_with = "deserialize_decimal")]
        total_volume: Decimal,
        #[serde(deserialize_with = "deserialize_decimal")]
        monthly_volume: Decimal,
        #[serde(deserialize_with = "deserialize_decimal")]
        weekly_volume: Decimal,
        #[serde(deserialize_with = "deserialize_decimal")]
        daily_volume: Decimal,
    },
    #[serde(rename = "update/account_all_trades")]
    AccountAllTrades {
        channel: Ustr,
        trades: AHashMap<Ustr, Vec<LighterTrade>>,
    },
    #[serde(
        rename = "update/account_all_positions",
        alias = "subscribed/account_all_positions"
    )]
    AccountAllPositions {
        channel: Ustr,
        positions: AHashMap<Ustr, LighterPosition>,
        #[serde(default)]
        shares: Vec<LighterPoolShares>,
        last_funding_round: Option<AHashMap<Ustr, Decimal>>,
        last_funding_discount: Option<AHashMap<Ustr, Decimal>>,
    },
    #[serde(
        rename = "update/account_all_assets",
        alias = "subscribed/account_all_assets"
    )]
    AccountAllAssets {
        assets: AHashMap<Ustr, LighterAsset>,
        channel: Ustr,
        timestamp: u64,
    },
    #[serde(rename = "update/user_stats", alias = "subscribed/user_stats")]
    UserStats {
        channel: Ustr,
        stats: LighterUserStats,
        timestamp: u64,
    },
    #[serde(rename = "update/height")]
    Height {
        channel: Ustr,
        height: i64,
        timestamp: u64,
    },
    #[serde(rename = "subscribed/candle")]
    CandleSnapshot {
        channel: Ustr,
        candles: Vec<LighterWsCandle>,
        timestamp: u64,
    },
    #[serde(rename = "update/candle")]
    Candle {
        channel: Ustr,
        candles: Vec<LighterWsCandle>,
        timestamp: u64,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LighterWsCandle {
    pub t: i64,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub o: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub h: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub l: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub c: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub v: Decimal,
    #[serde(default, rename = "V", deserialize_with = "deserialize_decimal")]
    pub quote_volume: Decimal,
    #[serde(default)]
    pub i: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterWsOrderBook {
    pub code: i32,
    pub asks: Vec<LighterPriceLevel>,
    pub bids: Vec<LighterPriceLevel>,
    pub offset: i64,
    pub nonce: i64,
    pub last_updated_at: u64,
    pub begin_nonce: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct LighterTicker {
    pub s: Ustr,
    pub a: LighterPriceLevel,
    pub b: LighterPriceLevel,
    pub last_updated_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum LighterMarketStatsPayload {
    All(AHashMap<Ustr, LighterMarketStats>),
    One(Box<LighterMarketStats>),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LighterMarketStats {
    pub symbol: Ustr,
    pub market_id: i16,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub index_price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub mark_price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub mid_price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub open_interest: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub open_interest_limit: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub funding_clamp_small: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub funding_clamp_big: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub last_trade_price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub current_funding_rate: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub funding_rate: Decimal,
    pub funding_timestamp: u64,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_base_token_volume: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_quote_token_volume: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_low: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_high: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_change: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum LighterSpotMarketStatsPayload {
    All(AHashMap<Ustr, LighterSpotMarketStats>),
    One(Box<LighterSpotMarketStats>),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LighterSpotMarketStats {
    pub symbol: Ustr,
    pub market_id: i16,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub index_price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub mid_price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub last_trade_price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_base_token_volume: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_quote_token_volume: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_low: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_high: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub daily_price_change: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LighterPosition {
    pub market_id: i16,
    pub symbol: Ustr,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub initial_margin_fraction: Decimal,
    pub open_order_count: i64,
    pub pending_order_count: i64,
    pub position_tied_order_count: i64,
    pub sign: i8,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub position: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub avg_entry_price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub position_value: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub unrealized_pnl: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub realized_pnl: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub liquidation_price: Decimal,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_funding_paid_out: Option<Decimal>,
    pub margin_mode: i32,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub allocated_margin: Decimal,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_discount: Option<Decimal>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LighterPoolShares {
    pub public_pool_index: i64,
    pub shares_amount: i64,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub entry_usdc: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub principal_amount: Decimal,
    pub entry_timestamp: u64,
}

/// Inner shape of the `user_stats.stats.cross_stats` and `.total_stats`
/// substructs. Every field is a stringified decimal on the wire and
/// denominated in USDC.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LighterUserStatsScoped {
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub available_balance: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub buying_power: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub collateral: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub leverage: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub margin_usage: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub portfolio_value: Decimal,
}

/// Body of the `user_stats` frame. Top-level equity numbers mirror
/// `total_stats`; `cross_stats` reports cross-margin equity only.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LighterUserStats {
    #[serde(default)]
    pub account_trading_mode: i32,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub available_balance: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub buying_power: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub collateral: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub leverage: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub margin_usage: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub portfolio_value: Decimal,
    pub cross_stats: Option<LighterUserStatsScoped>,
    pub total_stats: Option<LighterUserStatsScoped>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LighterAsset {
    pub symbol: Ustr,
    pub asset_id: i16,
    /// Spot-side balance for this asset (USDC sitting in the wallet bucket,
    /// non-USDC spot holdings, etc).
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub balance: Decimal,
    /// Spot-side amount reserved by resting spot orders.
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub locked_balance: Decimal,
    /// Perp-side collateral for this asset. USDC on Lighter today; defaults
    /// to zero when the wire omits the field (spot-only frames).
    #[serde(default, deserialize_with = "deserialize_decimal_from_str")]
    pub margin_balance: Decimal,
    /// Per-asset margin treatment. Observed values: "disabled" (asset not
    /// pledged as collateral). Defaults to empty when the wire omits it.
    #[serde(default)]
    pub margin_mode: Ustr,
}

fn deserialize_trade_vec<'de, D>(deserializer: D) -> Result<Vec<LighterTrade>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct TradeVecVisitor;

    impl<'de> Visitor<'de> for TradeVecVisitor {
        type Value = Vec<LighterTrade>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("trade array, object keyed by market, or null")
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut trades = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            while let Some(trade) = seq.next_element::<LighterTrade>()? {
                trades.push(trade);
            }
            Ok(trades)
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut trades = Vec::new();
            while let Some((_, mut market_trades)) =
                map.next_entry::<IgnoredAny, Vec<LighterTrade>>()?
            {
                trades.append(&mut market_trades);
            }
            Ok(trades)
        }
    }

    deserializer.deserialize_any(TradeVecVisitor)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    const WS_ORDER_BOOK_UPDATE: &str = include_str!("../../test_data/ws_order_book_update.json");
    const WS_ORDER_BOOK_SUBSCRIBED: &str =
        include_str!("../../test_data/ws_order_book_subscribed.json");
    const WS_ORDER_BOOK_SUBSCRIBED_EMPTY: &str =
        include_str!("../../test_data/ws_order_book_subscribed_empty.json");
    const WS_TRADE_UPDATE: &str = include_str!("../../test_data/ws_trade_update.json");
    const WS_TRADE_SUBSCRIBED: &str = include_str!("../../test_data/ws_trade_subscribed.json");
    const WS_TICKER_UPDATE: &str = include_str!("../../test_data/ws_ticker_update.json");
    const WS_TICKER_SUBSCRIBED: &str = include_str!("../../test_data/ws_ticker_subscribed.json");
    const WS_TICKER_SUBSCRIBED_EMPTY: &str =
        include_str!("../../test_data/ws_ticker_subscribed_empty.json");
    const WS_MARKET_STATS_UPDATE_SINGLE: &str =
        include_str!("../../test_data/ws_market_stats_update_single.json");
    const WS_MARKET_STATS_SUBSCRIBED_SINGLE: &str =
        include_str!("../../test_data/ws_market_stats_subscribed_single.json");
    const WS_MARKET_STATS_UPDATE_ALL: &str =
        include_str!("../../test_data/ws_market_stats_update_all.json");
    const WS_SPOT_MARKET_STATS_UPDATE_SINGLE: &str =
        include_str!("../../test_data/ws_spot_market_stats_update_single.json");
    const WS_SPOT_MARKET_STATS_SUBSCRIBED_SINGLE: &str =
        include_str!("../../test_data/ws_spot_market_stats_subscribed_single.json");
    const WS_SPOT_MARKET_STATS_UPDATE_ALL: &str =
        include_str!("../../test_data/ws_spot_market_stats_update_all.json");
    const WS_ACCOUNT_ALL_ASSETS_UPDATE: &str =
        include_str!("../../test_data/ws_account_all_assets_update.json");
    const WS_ACCOUNT_ORDERS_UPDATE: &str =
        include_str!("../../test_data/ws_account_orders_update.json");
    const WS_ACCOUNT_ALL_TRADES_UPDATE: &str =
        include_str!("../../test_data/ws_account_all_trades_update.json");
    const WS_ACCOUNT_ALL_POSITIONS_UPDATE: &str =
        include_str!("../../test_data/ws_account_all_positions_update.json");
    const WS_HEIGHT_UPDATE: &str = include_str!("../../test_data/ws_height_update.json");
    const WS_CANDLE_SUBSCRIBED: &str = include_str!("../../test_data/ws_candle_subscribed.json");
    const WS_CANDLE_UPDATE: &str = include_str!("../../test_data/ws_candle_update.json");

    #[rstest]
    fn test_subscription_request_serializes_public_channel() {
        let channel = LighterWsChannel::OrderBook(0).subscription_channel();
        let request = LighterWsRequest::subscribe(channel);

        let json = serde_json::to_string(&request).unwrap();

        assert_eq!(
            serde_json::from_str::<Value>(&json).unwrap(),
            serde_json::json!({
                "type": "subscribe",
                "channel": "order_book/0",
            }),
        );
    }

    #[rstest]
    fn test_subscription_request_serializes_auth_channel() {
        let channel = LighterWsChannel::AccountOrders {
            market_index: 0,
            account_index: 1234,
        }
        .subscription_channel();
        let request = LighterWsRequest::subscribe_auth(channel, "token");

        let json = serde_json::to_string(&request).unwrap();

        assert_eq!(
            serde_json::from_str::<Value>(&json).unwrap(),
            serde_json::json!({
                "type": "subscribe",
                "channel": "account_orders/0/1234",
                "auth": "token",
            }),
        );
    }

    #[rstest]
    fn test_subscribe_request_debug_redacts_auth_token() {
        let token = "schnorr-signature-bytes-do-not-leak";
        let request = LighterWsRequest::subscribe_auth("account_all/123", token);

        let dbg = format!("{request:?}");

        assert!(
            !dbg.contains(token),
            "Debug output must not contain the auth token, found: {dbg}",
        );
        assert!(dbg.contains("authed"), "Debug should include authed flag");
    }

    #[rstest]
    #[case(LighterWsChannelKind::OrderBook)]
    #[case(LighterWsChannelKind::Ticker)]
    #[case(LighterWsChannelKind::Trade)]
    #[case(LighterWsChannelKind::Candle)]
    #[case(LighterWsChannelKind::MarketStats)]
    #[case(LighterWsChannelKind::SpotMarketStats)]
    #[case(LighterWsChannelKind::AccountAll)]
    #[case(LighterWsChannelKind::AccountOrders)]
    #[case(LighterWsChannelKind::AccountAllOrders)]
    #[case(LighterWsChannelKind::AccountAllTrades)]
    #[case(LighterWsChannelKind::AccountAllPositions)]
    #[case(LighterWsChannelKind::AccountAllAssets)]
    #[case(LighterWsChannelKind::Height)]
    fn test_channel_kind_wire_round_trip(#[case] kind: LighterWsChannelKind) {
        assert_eq!(
            LighterWsChannelKind::from_wire_str(kind.as_wire_str()),
            Some(kind),
        );
    }

    #[rstest]
    #[case("unknown_channel")]
    #[case("ORDER_BOOK")]
    #[case("")]
    #[case("order_book:0")]
    fn test_channel_kind_unknown_returns_none(#[case] input: &str) {
        assert_eq!(LighterWsChannelKind::from_wire_str(input), None);
    }

    #[rstest]
    fn test_order_book_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_ORDER_BOOK_UPDATE).unwrap();

        match frame {
            LighterWsFrame::OrderBook {
                channel,
                order_book,
                timestamp,
                ..
            } => {
                assert_eq!(channel, Ustr::from("order_book:0"));
                assert_eq!(order_book.asks.len(), 1);
                assert_eq!(
                    order_book.asks[0].price,
                    Decimal::from_str("2064.54").unwrap()
                );
                assert_eq!(timestamp, 1_774_884_082_326);
            }
            _ => panic!("expected order book frame"),
        }
    }

    #[rstest]
    fn test_trade_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_TRADE_UPDATE).unwrap();

        match frame {
            LighterWsFrame::Trade { trades, nonce, .. } => {
                assert_eq!(nonce, 8_630_448_841);
                assert_eq!(trades.len(), 1);
                assert_eq!(trades[0].trade_id_str.as_deref(), Some("16164557907"));
            }
            _ => panic!("expected trade frame"),
        }
    }

    #[rstest]
    fn test_trade_frame_deserializes_null_liquidations() {
        let payload = serde_json::json!({
            "type": "update/trade",
            "channel": "trade:1",
            "liquidation_trades": null,
            "nonce": 1,
            "trades": []
        });

        let frame: LighterWsFrame = serde_json::from_value(payload).unwrap();

        match frame {
            LighterWsFrame::Trade {
                liquidation_trades,
                trades,
                ..
            } => {
                assert!(liquidation_trades.is_empty());
                assert!(trades.is_empty());
            }
            _ => panic!("expected trade frame"),
        }
    }

    #[rstest]
    fn test_trade_frame_deserializes_object_trades() {
        let mut payload: Value = serde_json::from_str(WS_TRADE_UPDATE).unwrap();
        let trades = payload.get_mut("trades").unwrap().take();
        payload["trades"] = serde_json::json!({ "0": trades });

        let frame: LighterWsFrame = serde_json::from_value(payload).unwrap();

        match frame {
            LighterWsFrame::Trade { trades, .. } => {
                assert_eq!(trades.len(), 1);
                assert_eq!(trades[0].trade_id_str.as_deref(), Some("16164557907"));
            }
            _ => panic!("expected trade frame"),
        }
    }

    #[rstest]
    fn test_ticker_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_TICKER_UPDATE).unwrap();

        match frame {
            LighterWsFrame::Ticker {
                channel,
                nonce,
                ticker,
                timestamp,
                ..
            } => {
                assert_eq!(channel, Ustr::from("ticker:0"));
                assert_eq!(nonce, 9_182_390_020);
                assert_eq!(ticker.s, Ustr::from("ETH"));
                assert_eq!(ticker.a.price, Decimal::from_str("2064.48").unwrap());
                assert_eq!(ticker.b.size, Decimal::from_str("1.0392").unwrap());
                assert_eq!(timestamp, 1_774_883_844_933);
            }
            _ => panic!("expected ticker frame"),
        }
    }

    // The venue tags the initial state for each public stream as
    // `subscribed/<channel>` and only switches to `update/<channel>` for
    // incremental frames; the snapshot variants must round-trip even though
    // they share field shapes with their `update/*` counterparts.
    #[rstest]
    fn test_order_book_snapshot_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_ORDER_BOOK_SUBSCRIBED).unwrap();

        match frame {
            LighterWsFrame::OrderBookSnapshot {
                channel,
                order_book,
                timestamp,
                ..
            } => {
                assert_eq!(channel, Ustr::from("order_book:0"));
                assert_eq!(order_book.bids.len(), 1);
                assert_eq!(
                    order_book.bids[0].price,
                    Decimal::from_str("2000.00").unwrap()
                );
                assert_eq!(order_book.asks.len(), 2);
                assert_eq!(
                    order_book.asks[0].price,
                    Decimal::from_str("2325.00").unwrap()
                );
                assert_eq!(order_book.nonce, 904_845);
                assert_eq!(timestamp, 1_778_138_582_602);
            }
            _ => panic!("expected order book snapshot frame, was {frame:?}"),
        }
    }

    #[rstest]
    fn test_empty_order_book_snapshot_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_ORDER_BOOK_SUBSCRIBED_EMPTY).unwrap();

        match frame {
            LighterWsFrame::OrderBookSnapshot {
                channel,
                last_updated_at,
                order_book,
                timestamp,
                ..
            } => {
                assert_eq!(channel, Ustr::from("order_book:39"));
                assert_eq!(last_updated_at, 0);
                assert!(order_book.asks.is_empty());
                assert!(order_book.bids.is_empty());
                assert_eq!(order_book.offset, 1);
                assert_eq!(order_book.nonce, 0);
                assert_eq!(timestamp, 1_778_138_582_602);
            }
            _ => panic!("expected empty order book snapshot frame, was {frame:?}"),
        }
    }

    #[rstest]
    fn test_ticker_snapshot_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_TICKER_SUBSCRIBED).unwrap();

        match frame {
            LighterWsFrame::TickerSnapshot {
                channel,
                nonce,
                ticker,
                timestamp,
                ..
            } => {
                assert_eq!(channel, Ustr::from("ticker:0"));
                assert_eq!(nonce, 904_895);
                assert_eq!(ticker.s, Ustr::from("ETH"));
                assert_eq!(ticker.a.price, Decimal::from_str("2325.00").unwrap());
                assert_eq!(ticker.b.price, Decimal::from_str("2000.00").unwrap());
                assert_eq!(timestamp, 1_778_138_582_640);
            }
            _ => panic!("expected ticker snapshot frame, was {frame:?}"),
        }
    }

    #[rstest]
    fn test_empty_ticker_snapshot_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_TICKER_SUBSCRIBED_EMPTY).unwrap();

        match frame {
            LighterWsFrame::TickerSnapshot {
                channel,
                last_updated_at,
                nonce,
                ticker,
                timestamp,
                ..
            } => {
                assert_eq!(channel, Ustr::from("ticker:39"));
                assert_eq!(last_updated_at, 0);
                assert_eq!(nonce, 2_475_051);
                assert_eq!(ticker.s, Ustr::from("ADA"));
                assert_eq!(ticker.a.price, Decimal::ZERO);
                assert_eq!(ticker.a.size, Decimal::ZERO);
                assert_eq!(ticker.b.price, Decimal::ZERO);
                assert_eq!(ticker.b.size, Decimal::ZERO);
                assert_eq!(timestamp, 1_778_138_582_640);
            }
            _ => panic!("expected empty ticker snapshot frame, was {frame:?}"),
        }
    }

    #[rstest]
    fn test_trade_snapshot_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_TRADE_SUBSCRIBED).unwrap();

        match frame {
            LighterWsFrame::TradeSnapshot {
                channel,
                nonce,
                trades,
                ..
            } => {
                assert_eq!(channel, Ustr::from("trade:0"));
                assert_eq!(nonce, 8_630_448_841);
                assert_eq!(trades.len(), 1);
                assert_eq!(trades[0].trade_id_str.as_deref(), Some("16164557907"));
            }
            _ => panic!("expected trade snapshot frame, was {frame:?}"),
        }
    }

    #[rstest]
    fn test_market_stats_frame_deserializes_single_payload() {
        let frame: LighterWsFrame = serde_json::from_str(WS_MARKET_STATS_UPDATE_SINGLE).unwrap();

        match frame {
            LighterWsFrame::MarketStats {
                channel,
                market_stats: LighterMarketStatsPayload::One(stats),
                timestamp,
            } => {
                assert_eq!(channel, Ustr::from("market_stats:0"));
                assert_eq!(stats.symbol, Ustr::from("ETH"));
                assert_eq!(stats.market_id, 0);
                assert_eq!(stats.mark_price, Decimal::from_str("2064.47").unwrap());
                assert_eq!(
                    stats.daily_base_token_volume,
                    Decimal::new(1_999_586_931, 4),
                );
                assert_eq!(timestamp, 1_774_883_844_933);
            }
            _ => panic!("expected single market stats frame"),
        }
    }

    #[rstest]
    fn test_market_stats_subscribed_frame_deserializes_single_payload() {
        let frame: LighterWsFrame =
            serde_json::from_str(WS_MARKET_STATS_SUBSCRIBED_SINGLE).unwrap();

        match frame {
            LighterWsFrame::MarketStats {
                channel,
                market_stats: LighterMarketStatsPayload::One(stats),
                timestamp,
            } => {
                assert_eq!(channel, Ustr::from("market_stats:1"));
                assert_eq!(stats.symbol, Ustr::from("BTC"));
                assert_eq!(stats.market_id, 1);
                assert_eq!(stats.mark_price, Decimal::from_str("64356.3").unwrap());
                assert_eq!(timestamp, 1_780_546_209_291);
            }
            _ => panic!("expected subscribed market stats frame"),
        }
    }

    #[rstest]
    fn test_market_stats_frame_deserializes_all_payload() {
        let frame: LighterWsFrame = serde_json::from_str(WS_MARKET_STATS_UPDATE_ALL).unwrap();

        match frame {
            LighterWsFrame::MarketStats {
                market_stats: LighterMarketStatsPayload::All(stats),
                ..
            } => {
                assert_eq!(stats.len(), 1);
                let stats = stats.get(&Ustr::from("0")).unwrap();
                assert_eq!(stats.symbol, Ustr::from("ETH"));
                assert_eq!(
                    stats.open_interest,
                    Decimal::from_str("27250.8411").unwrap()
                );
            }
            _ => panic!("expected all market stats frame"),
        }
    }

    #[rstest]
    fn test_spot_market_stats_frame_deserializes_single_payload() {
        let frame: LighterWsFrame =
            serde_json::from_str(WS_SPOT_MARKET_STATS_UPDATE_SINGLE).unwrap();

        match frame {
            LighterWsFrame::SpotMarketStats {
                channel,
                spot_market_stats: LighterSpotMarketStatsPayload::One(stats),
                timestamp,
            } => {
                assert_eq!(channel, Ustr::from("spot_market_stats:2048"));
                assert_eq!(stats.symbol, Ustr::from("USDC"));
                assert_eq!(stats.market_id, 2048);
                assert_eq!(stats.mid_price, Decimal::from_str("1.000001").unwrap());
                assert_eq!(stats.daily_base_token_volume, Decimal::from(1000));
                assert_eq!(timestamp, 1_774_883_844_933);
            }
            _ => panic!("expected single spot market stats frame"),
        }
    }

    #[rstest]
    fn test_spot_market_stats_subscribed_frame_deserializes_single_payload() {
        let frame: LighterWsFrame =
            serde_json::from_str(WS_SPOT_MARKET_STATS_SUBSCRIBED_SINGLE).unwrap();

        match frame {
            LighterWsFrame::SpotMarketStats {
                channel,
                spot_market_stats: LighterSpotMarketStatsPayload::One(stats),
                timestamp,
            } => {
                assert_eq!(channel, Ustr::from("spot_market_stats:2048"));
                assert_eq!(stats.symbol, Ustr::from("USDC"));
                assert_eq!(stats.market_id, 2048);
                assert_eq!(stats.mid_price, Decimal::from_str("1.000001").unwrap());
                assert_eq!(timestamp, 1_774_883_844_933);
            }
            _ => panic!("expected subscribed spot market stats frame"),
        }
    }

    #[rstest]
    fn test_spot_market_stats_frame_deserializes_all_payload() {
        let frame: LighterWsFrame = serde_json::from_str(WS_SPOT_MARKET_STATS_UPDATE_ALL).unwrap();

        match frame {
            LighterWsFrame::SpotMarketStats {
                spot_market_stats: LighterSpotMarketStatsPayload::All(stats),
                ..
            } => {
                assert_eq!(stats.len(), 1);
                let stats = stats.get(&Ustr::from("2048")).unwrap();
                assert_eq!(stats.symbol, Ustr::from("USDC"));
                assert_eq!(
                    stats.last_trade_price,
                    Decimal::from_str("1.000002").unwrap()
                );
            }
            _ => panic!("expected all spot market stats frame"),
        }
    }

    #[rstest]
    fn test_account_all_assets_frame_deserializes() {
        // Fixture is the captured production no-position payload: USDC
        // sits at asset_id=3, balance=10 on spot, margin_balance=40 on
        // perp, margin_mode="disabled", no spot-order reservation.
        let frame: LighterWsFrame = serde_json::from_str(WS_ACCOUNT_ALL_ASSETS_UPDATE).unwrap();

        match frame {
            LighterWsFrame::AccountAllAssets {
                assets,
                channel,
                timestamp,
            } => {
                assert_eq!(channel, Ustr::from("account_all_assets:1234"));
                let asset = assets.get(&Ustr::from("3")).unwrap();
                assert_eq!(asset.symbol, Ustr::from("USDC"));
                assert_eq!(asset.asset_id, 3);
                assert_eq!(asset.balance, Decimal::from_str("10.000000").unwrap());
                assert_eq!(asset.locked_balance, Decimal::ZERO);
                assert_eq!(
                    asset.margin_balance,
                    Decimal::from_str("40.000000").unwrap()
                );
                assert_eq!(asset.margin_mode, Ustr::from("disabled"));
                assert_eq!(timestamp, 1_781_161_199_648);
            }
            _ => panic!("expected account all assets frame"),
        }
    }

    #[rstest]
    fn test_account_all_assets_subscribed_frame_deserializes() {
        let payload = serde_json::json!({
            "type": "subscribed/account_all_assets",
            "channel": "account_all_assets:1234",
            "timestamp": 1778751230509u64,
            "assets": {
                "3": {
                    "asset_id": 3,
                    "balance": "9.660200",
                    "locked_balance": "0.000000",
                    "margin_balance": "9.955800",
                    "margin_mode": "disabled",
                    "symbol": "USDC"
                }
            }
        });

        let frame: LighterWsFrame = serde_json::from_value(payload).unwrap();

        match frame {
            LighterWsFrame::AccountAllAssets { assets, .. } => {
                assert_eq!(
                    assets.get(&Ustr::from("3")).unwrap().symbol,
                    Ustr::from("USDC")
                );
            }
            _ => panic!("expected account all assets frame"),
        }
    }

    #[rstest]
    fn test_account_orders_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_ACCOUNT_ORDERS_UPDATE).unwrap();

        match frame {
            LighterWsFrame::AccountOrders {
                account,
                channel,
                orders,
                ..
            } => {
                assert_eq!(account, 1234);
                assert_eq!(channel, Ustr::from("account_orders:0:1234"));
                let market_orders = orders.get(&Ustr::from("0")).unwrap();
                assert_eq!(market_orders.len(), 1);
                assert_eq!(market_orders[0].order_id, "281476929510110");
                assert_eq!(
                    market_orders[0].filled_base_amount,
                    Decimal::from_str("0.0020").unwrap(),
                );
            }
            _ => panic!("expected account orders frame, was {frame:?}"),
        }
    }

    #[rstest]
    fn test_account_all_orders_subscribed_frame_deserializes_empty_side() {
        let frame: LighterWsFrame = serde_json::from_str(
            r#"{
                "type": "subscribed/account_all_orders",
                "channel": "account_all_orders:1234",
                "orders": {
                    "3": [{
                        "order_index": 1,
                        "client_order_index": 2,
                        "order_id": "1",
                        "client_order_id": "2",
                        "market_index": 3,
                        "owner_account_index": 1234,
                        "initial_base_amount": "100",
                        "price": "0.100000",
                        "nonce": 1,
                        "remaining_base_amount": "100",
                        "is_ask": false,
                        "base_size": 100,
                        "base_price": 100000,
                        "filled_base_amount": "0",
                        "filled_quote_amount": "0.000000",
                        "side": "",
                        "type": "limit",
                        "time_in_force": "good-till-time",
                        "reduce_only": false,
                        "trigger_price": "0.000000",
                        "order_expiry": 1781170441337,
                        "status": "open",
                        "trigger_status": "na",
                        "trigger_time": 0,
                        "parent_order_index": 0,
                        "parent_order_id": "0",
                        "to_trigger_order_id_0": "0",
                        "to_trigger_order_id_1": "0",
                        "to_cancel_order_id_0": "0",
                        "integrator_fee_collector_index": "",
                        "integrator_taker_fee": "",
                        "integrator_maker_fee": "",
                        "block_height": 1,
                        "timestamp": 1778751241,
                        "created_at": 1778751241,
                        "updated_at": 1778751241,
                        "transaction_time": 1778751241772524
                    }]
                }
            }"#,
        )
        .unwrap();

        match frame {
            LighterWsFrame::AccountAllOrders { orders, .. } => {
                let order = &orders.get(&Ustr::from("3")).unwrap()[0];
                assert_eq!(order.side, None);
                assert!(!order.is_ask);
            }
            _ => panic!("expected account all orders frame"),
        }
    }

    #[rstest]
    fn test_account_all_trades_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_ACCOUNT_ALL_TRADES_UPDATE).unwrap();

        match frame {
            LighterWsFrame::AccountAllTrades { channel, trades } => {
                assert_eq!(channel, Ustr::from("account_all_trades:1234"));
                let market_trades = trades.get(&Ustr::from("0")).unwrap();
                assert_eq!(market_trades.len(), 1);
                assert_eq!(market_trades[0].bid_account_id, 1234);
                assert_eq!(market_trades[0].taker_fee, Some(196));
            }
            _ => panic!("expected account all trades frame, was {frame:?}"),
        }
    }

    #[rstest]
    fn test_account_all_positions_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_ACCOUNT_ALL_POSITIONS_UPDATE).unwrap();

        match frame {
            LighterWsFrame::AccountAllPositions {
                channel, positions, ..
            } => {
                assert_eq!(channel, Ustr::from("account_all_positions:1234"));
                let position = positions.get(&Ustr::from("0")).unwrap();
                assert_eq!(position.market_id, 0);
                assert_eq!(position.position, Decimal::from_str("1.5000").unwrap());
                assert_eq!(position.sign, 1);
            }
            _ => panic!("expected account all positions frame, was {frame:?}"),
        }
    }

    #[rstest]
    fn test_height_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_HEIGHT_UPDATE).unwrap();

        match frame {
            LighterWsFrame::Height {
                channel,
                height,
                timestamp,
            } => {
                assert_eq!(channel, Ustr::from("height"));
                assert_eq!(height, 227_535_532);
                assert_eq!(timestamp, 1_774_883_844_933);
            }
            _ => panic!("expected height frame"),
        }
    }

    #[rstest]
    fn test_candle_channel_subscription_channel_uses_slash() {
        let channel = LighterWsChannel::Candle {
            market_index: 0,
            resolution: LighterCandleResolution::OneMinute,
        };

        assert_eq!(channel.subscription_channel(), "candle/0/1m");
    }

    #[rstest]
    fn test_candle_channel_topic_key_uses_colon() {
        let channel = LighterWsChannel::Candle {
            market_index: 7,
            resolution: LighterCandleResolution::FiveMinute,
        };

        assert_eq!(channel.topic_key(), "candle:7:5m");
    }

    #[rstest]
    fn test_candle_channel_does_not_require_auth() {
        let channel = LighterWsChannel::Candle {
            market_index: 0,
            resolution: LighterCandleResolution::OneMinute,
        };

        assert!(!channel.requires_auth());
    }

    #[rstest]
    fn test_candle_snapshot_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_CANDLE_SUBSCRIBED).unwrap();

        match frame {
            LighterWsFrame::CandleSnapshot {
                channel,
                candles,
                timestamp,
            } => {
                assert_eq!(channel, Ustr::from("candle:0:1m"));
                assert_eq!(timestamp, 1_778_821_471_842);
                assert_eq!(candles.len(), 1);
                let candle = &candles[0];
                assert_eq!(candle.t, 1_778_821_440_000);
                assert_eq!(candle.o, Decimal::from_str("2264.2").unwrap());
                assert_eq!(candle.h, Decimal::from_str("2264.34").unwrap());
                assert_eq!(candle.l, Decimal::from_str("2263.36").unwrap());
                assert_eq!(candle.c, Decimal::from_str("2263.97").unwrap());
                // f64 JSON numbers round-trip through `deserialize_decimal::visit_f64`
                // which converts via `Decimal::try_from(f64)`; the resulting value is the
                // nearest representable decimal to the float, not the JSON literal text.
                assert_eq!(candle.v, Decimal::from_str("13.2237").unwrap());
                assert_eq!(
                    candle.quote_volume,
                    Decimal::from_str("29934.60001199998").unwrap(),
                );
                assert_eq!(candle.i, 19_993_571_166);
            }
            _ => panic!("expected candle snapshot frame"),
        }
    }

    #[rstest]
    fn test_candle_update_frame_deserializes() {
        let frame: LighterWsFrame = serde_json::from_str(WS_CANDLE_UPDATE).unwrap();

        match frame {
            LighterWsFrame::Candle {
                channel,
                candles,
                timestamp,
            } => {
                assert_eq!(channel, Ustr::from("candle:0:1m"));
                assert_eq!(timestamp, 1_778_821_473_331);
                assert_eq!(candles.len(), 1);
                assert_eq!(candles[0].t, 1_778_821_440_000);
                assert_eq!(candles[0].c, Decimal::from_str("2263.89").unwrap());
            }
            _ => panic!("expected candle update frame"),
        }
    }
}
