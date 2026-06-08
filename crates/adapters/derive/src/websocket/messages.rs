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

//! Wire payloads for the Derive WebSocket JSON-RPC transport.
//!
//! The transport reuses the [`crate::http::models::JsonRpcRequest`] /
//! [`crate::http::models::JsonRpcResponse`] envelope; this module covers only
//! the params payloads and the inbound notification frame.

use std::{collections::HashMap, fmt::Display, str::FromStr};

use nautilus_core::serialization::deserialize_decimal;
use nautilus_model::identifiers::InstrumentId;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use ustr::Ustr;

use crate::{
    common::{
        enums::{
            DeriveInstrumentType, DeriveOrderbookDepth, DeriveOrderbookGroup, DeriveTickerInterval,
        },
        parse::format_instrument_id,
        rate_limit::{DERIVE_MATCHING_RATE_KEY, DERIVE_NON_MATCHING_RATE_KEY},
    },
    http::models::{
        DeriveAggregateTradingStats, DeriveOptionPricing, DeriveOrder, DerivePublicTrade,
        DeriveTicker, DeriveTickerSnapshot, DeriveTrade, JsonRpcError,
    },
};

pub(crate) const DEFAULT_ORDERBOOK_GROUP: &str = "1";
pub(crate) const DEFAULT_ORDERBOOK_DEPTH: &str = "10";
pub(crate) const DEFAULT_TICKER_INTERVAL: &str = "1000";

/// Params payload for `public/login`.
///
/// The wallet/timestamp/signature triple comes from
/// [`crate::signing::auth::build_ws_login`]; the venue verifies the signature
/// recovers `wallet` over the millisecond timestamp string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WsLoginParams {
    /// Derive Chain smart-contract wallet address (`0x`-prefixed hex).
    pub wallet: String,
    /// Millisecond UNIX timestamp string (matches the bytes that were signed).
    pub timestamp: String,
    /// 0x-prefixed signature hex over `timestamp` under EIP-191.
    pub signature: String,
}

/// Params payload for `subscribe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WsSubscribeParams {
    /// Channel topics to subscribe to (e.g. `ticker_slim.ETH-PERP.1000`).
    pub channels: Vec<DeriveWsChannel>,
}

/// Params payload for `unsubscribe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WsUnsubscribeParams {
    /// Channel topics to drop.
    pub channels: Vec<DeriveWsChannel>,
}

/// Derive WebSocket subscription channel topic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeriveWsChannel {
    /// Public compact ticker channel.
    TickerSlim {
        /// Venue instrument name.
        instrument_name: Ustr,
        /// Update interval in milliseconds.
        interval: DeriveTickerInterval,
    },
    /// Public order book channel.
    Orderbook {
        /// Venue instrument name.
        instrument_name: Ustr,
        /// Venue grouping increment.
        group: DeriveOrderbookGroup,
        /// Requested depth.
        depth: DeriveOrderbookDepth,
    },
    /// Public trades channel.
    Trades {
        /// Venue instrument type.
        instrument_type: DeriveInstrumentType,
        /// Venue currency.
        currency: Ustr,
    },
    /// Private order updates channel.
    Orders {
        /// Subaccount id.
        subaccount_id: u64,
    },
    /// Private trade updates channel.
    PrivateTrades {
        /// Subaccount id.
        subaccount_id: u64,
    },
    /// Private balance updates channel.
    Balances {
        /// Subaccount id.
        subaccount_id: u64,
    },
    /// Passthrough topic for venue channels not yet modeled by the adapter.
    Raw(String),
}

impl DeriveWsChannel {
    /// Returns a compact ticker channel.
    #[must_use]
    pub fn ticker_slim(instrument_name: impl AsRef<str>, interval: impl AsRef<str>) -> Self {
        let instrument_name = instrument_name.as_ref();
        let interval = interval.as_ref();
        let Ok(interval) = DeriveTickerInterval::from_str(interval) else {
            return Self::Raw(ticker_channel(instrument_name, interval));
        };
        Self::TickerSlim {
            instrument_name: Ustr::from(instrument_name),
            interval,
        }
    }

    /// Returns an order book channel.
    #[must_use]
    pub fn orderbook(
        instrument_name: impl AsRef<str>,
        group: impl AsRef<str>,
        depth: impl AsRef<str>,
    ) -> Self {
        let instrument_name = instrument_name.as_ref();
        let group = group.as_ref();
        let depth = depth.as_ref();
        let Ok(group) = DeriveOrderbookGroup::from_str(group) else {
            return Self::Raw(orderbook_channel(instrument_name, group, depth));
        };
        let Ok(depth) = DeriveOrderbookDepth::from_str(depth) else {
            return Self::Raw(orderbook_channel(instrument_name, group.as_ref(), depth));
        };
        Self::Orderbook {
            instrument_name: Ustr::from(instrument_name),
            group,
            depth,
        }
    }

    /// Returns a public trades channel.
    #[must_use]
    pub fn trades(instrument_type: impl AsRef<str>, currency: impl AsRef<str>) -> Self {
        let instrument_type = instrument_type.as_ref();
        let currency = currency.as_ref();
        let Ok(instrument_type) = DeriveInstrumentType::from_str(instrument_type) else {
            return Self::Raw(trades_channel(instrument_type, currency));
        };
        Self::Trades {
            instrument_type,
            currency: Ustr::from(currency),
        }
    }

    /// Returns a private orders channel.
    #[must_use]
    pub const fn orders(subaccount_id: u64) -> Self {
        Self::Orders { subaccount_id }
    }

    /// Returns a private trades channel.
    #[must_use]
    pub const fn private_trades(subaccount_id: u64) -> Self {
        Self::PrivateTrades { subaccount_id }
    }

    /// Returns a private balances channel.
    #[must_use]
    pub const fn balances(subaccount_id: u64) -> Self {
        Self::Balances { subaccount_id }
    }

    /// Parses a topic string into the known channel family when possible.
    #[must_use]
    pub fn from_topic(topic: impl Into<String>) -> Self {
        let topic = topic.into();

        if let Some(rest) = topic.strip_prefix("ticker_slim.")
            && let Some((instrument_name, interval)) = rest.rsplit_once('.')
            && !instrument_name.is_empty()
            && !interval.is_empty()
        {
            return Self::ticker_slim(instrument_name, interval);
        }

        if let Some(rest) = topic.strip_prefix("orderbook.")
            && let Some((rest, depth)) = rest.rsplit_once('.')
            && let Some((instrument_name, group)) = rest.rsplit_once('.')
            && !instrument_name.is_empty()
            && !group.is_empty()
            && !depth.is_empty()
        {
            return Self::orderbook(instrument_name, group, depth);
        }

        if let Some(rest) = topic.strip_prefix("trades.")
            && let Some((instrument_type, currency)) = rest.split_once('.')
            && !instrument_type.is_empty()
            && !currency.is_empty()
        {
            return Self::trades(instrument_type, currency);
        }

        if let Some((subaccount_id, suffix)) = topic.split_once('.')
            && let Ok(subaccount_id) = subaccount_id.parse::<u64>()
        {
            return match suffix {
                "orders" => Self::orders(subaccount_id),
                "trades" => Self::private_trades(subaccount_id),
                "balances" => Self::balances(subaccount_id),
                _ => Self::Raw(topic),
            };
        }

        Self::Raw(topic)
    }
}

impl Display for DeriveWsChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TickerSlim {
                instrument_name,
                interval,
            } => f.write_str(&ticker_channel(instrument_name.as_str(), interval.as_ref())),
            Self::Orderbook {
                instrument_name,
                group,
                depth,
            } => f.write_str(&orderbook_channel(
                instrument_name.as_str(),
                group.as_ref(),
                depth.as_ref(),
            )),
            Self::Trades {
                instrument_type,
                currency,
            } => f.write_str(&trades_channel(instrument_type.as_ref(), currency.as_str())),
            Self::Orders { subaccount_id } => f.write_str(&orders_channel(*subaccount_id)),
            Self::PrivateTrades { subaccount_id } => {
                f.write_str(&private_trades_channel(*subaccount_id))
            }
            Self::Balances { subaccount_id } => f.write_str(&balances_channel(*subaccount_id)),
            Self::Raw(topic) => f.write_str(topic),
        }
    }
}

impl Serialize for DeriveWsChannel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DeriveWsChannel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::from_topic)
    }
}

impl From<String> for DeriveWsChannel {
    fn from(value: String) -> Self {
        Self::from_topic(value)
    }
}

impl From<&str> for DeriveWsChannel {
    fn from(value: &str) -> Self {
        Self::from_topic(value)
    }
}

/// Method-specific params accepted by the WebSocket JSON-RPC request path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WsRequestParams {
    /// Params for `public/login`.
    Login(WsLoginParams),
    /// Params for `subscribe`.
    Subscribe(WsSubscribeParams),
    /// Params for `unsubscribe`.
    Unsubscribe(WsUnsubscribeParams),
}

impl From<WsLoginParams> for WsRequestParams {
    fn from(value: WsLoginParams) -> Self {
        Self::Login(value)
    }
}

impl From<WsSubscribeParams> for WsRequestParams {
    fn from(value: WsSubscribeParams) -> Self {
        Self::Subscribe(value)
    }
}

impl From<WsUnsubscribeParams> for WsRequestParams {
    fn from(value: WsUnsubscribeParams) -> Self {
        Self::Unsubscribe(value)
    }
}

/// Result payload returned by `public/login`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum WsLoginResult {
    /// Mock and gateway acknowledgement shape.
    Success {
        /// Whether the venue accepted the login.
        #[serde(default)]
        success: bool,
    },
    /// Venue acknowledgement listing the authorized subaccount IDs.
    AuthorizedSubaccounts(Vec<u64>),
}

impl Default for WsLoginResult {
    fn default() -> Self {
        Self::Success { success: false }
    }
}

/// Result payload returned by `subscribe`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct WsSubscribeResult {
    /// Current subscriptions reported by the venue.
    #[serde(default, alias = "current_subscriptions")]
    pub channels: Vec<DeriveWsChannel>,
    /// Per-channel subscription status reported by the venue.
    #[serde(default)]
    pub status: HashMap<DeriveWsChannel, Ustr>,
}

/// Result payload returned by `unsubscribe`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct WsUnsubscribeResult {
    /// Whether the venue accepted the unsubscribe request.
    #[serde(default)]
    pub success: bool,
    /// Channels removed by the venue, when it echoes them.
    #[serde(default)]
    pub channels: Vec<DeriveWsChannel>,
}

/// Inbound notification frame pushed by the venue on a subscribed channel.
///
/// The venue tags the frame with `method = "subscription"` and inlines the
/// channel-specific payload under `params.data`.
#[derive(Debug, Clone, Deserialize)]
pub struct WsSubscriptionFrame {
    /// Routing key (`method` on the wire). Always `"subscription"`.
    #[serde(default)]
    pub method: Option<Ustr>,
    /// Subscription envelope.
    pub params: WsSubscriptionPayload,
}

/// Channel-tagged notification payload nested under [`WsSubscriptionFrame::params`].
///
/// The channel payload is held as a [`RawValue`] (the raw JSON bytes) rather
/// than a decoded [`Value`]; each channel parser decodes those bytes straight
/// into its typed struct, so the inbound path never materialises the payload
/// into an intermediate `Value` tree.
#[derive(Debug, Clone, Deserialize)]
pub struct WsSubscriptionPayload {
    /// Channel that produced the update (e.g. `"ticker_slim.ETH-PERP.1000"`).
    pub channel: Ustr,
    /// Opaque per-channel payload; specific channels decode this further.
    pub data: Box<RawValue>,
}

/// Price level in a Derive order book snapshot.
///
/// The venue sends levels as `[price, amount]` tuples with decimal strings.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub struct DeriveOrderbookLevel(
    /// Price level.
    #[serde(deserialize_with = "deserialize_decimal")]
    pub Decimal,
    /// Aggregated amount at the price level.
    #[serde(deserialize_with = "deserialize_decimal")]
    pub Decimal,
);

impl DeriveOrderbookLevel {
    /// Returns the level price.
    #[must_use]
    pub const fn price(&self) -> Decimal {
        self.0
    }

    /// Returns the level amount.
    #[must_use]
    pub const fn amount(&self) -> Decimal {
        self.1
    }
}

/// Order book snapshot pushed on `orderbook.{instrument_name}.{group}.{depth}`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DeriveOrderbookData {
    /// Instrument name on the Derive venue.
    pub instrument_name: Ustr,
    /// Snapshot timestamp in UNIX milliseconds.
    pub timestamp: i64,
    /// Bid price levels, best first.
    pub bids: Vec<DeriveOrderbookLevel>,
    /// Ask price levels, best first.
    pub asks: Vec<DeriveOrderbookLevel>,
}

impl DeriveOrderbookData {
    /// Returns the Nautilus instrument ID for this Derive symbol.
    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        format_instrument_id(self.instrument_name.as_str())
    }
}

/// Channel-tagged order book update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeriveOrderbookMsg {
    /// Channel that produced the update.
    pub channel: Ustr,
    /// Parsed order book data.
    pub data: DeriveOrderbookData,
}

/// Channel-tagged public trades update.
#[derive(Debug, Clone)]
pub struct DeriveTradesMsg {
    /// Channel that produced the update.
    pub channel: Ustr,
    /// Trades carried by the update.
    pub trades: Vec<DerivePublicTrade>,
}

/// Private `{subaccount_id}.orders` subscription payload.
#[derive(Debug, Clone)]
pub struct DeriveOrdersSubscriptionData {
    /// Orders carried by the update.
    pub orders: Vec<DeriveOrder>,
}

impl<'de> Deserialize<'de> for DeriveOrdersSubscriptionData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let orders = match value {
            Value::Array(values) => values
                .into_iter()
                .filter_map(|value| serde_json::from_value::<DeriveOrder>(value).ok())
                .collect(),
            value => vec![
                serde_json::from_value::<DeriveOrder>(value).map_err(serde::de::Error::custom)?,
            ],
        };
        Ok(Self { orders })
    }
}

/// Private `{subaccount_id}.trades` subscription payload.
#[derive(Debug, Clone)]
pub struct DeriveTradesSubscriptionData {
    /// Trades carried by the update.
    pub trades: Vec<DeriveTrade>,
}

impl<'de> Deserialize<'de> for DeriveTradesSubscriptionData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let trades = match value {
            Value::Array(values) => values
                .into_iter()
                .filter_map(|value| serde_json::from_value::<DeriveTrade>(value).ok())
                .collect(),
            value => vec![
                serde_json::from_value::<DeriveTrade>(value).map_err(serde::de::Error::custom)?,
            ],
        };
        Ok(Self { trades })
    }
}

/// Ticker payload shape pushed by the Derive ticker channels.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DeriveTickerData {
    /// Full ticker shape with a feed timestamp and nested ticker snapshot.
    Envelope {
        /// Feed snapshot timestamp in UNIX milliseconds.
        timestamp: i64,
        /// Full instrument ticker snapshot.
        instrument_ticker: DeriveTicker,
    },
    /// Compact ticker shape with a feed timestamp and nested ticker snapshot.
    SlimEnvelope {
        /// Feed snapshot timestamp in UNIX milliseconds.
        timestamp: i64,
        /// Compact instrument ticker snapshot.
        instrument_ticker: DeriveTickerSnapshot,
    },
    /// Legacy shape where `params.data` is the ticker object itself.
    Ticker(DeriveTicker),
}

impl DeriveTickerData {
    /// Returns the ticker timestamp in UNIX milliseconds.
    #[must_use]
    pub const fn timestamp(&self) -> i64 {
        match self {
            Self::Envelope { timestamp, .. } => *timestamp,
            Self::SlimEnvelope { timestamp, .. } => *timestamp,
            Self::Ticker(ticker) => ticker.timestamp,
        }
    }

    /// Returns the ticker instrument name.
    #[must_use]
    pub fn instrument_name(&self) -> &Ustr {
        match self {
            Self::Envelope {
                instrument_ticker, ..
            } => &instrument_ticker.instrument_name,
            Self::SlimEnvelope {
                instrument_ticker, ..
            } => &instrument_ticker.instrument_name,
            Self::Ticker(ticker) => &ticker.instrument_name,
        }
    }

    /// Returns the best ask price.
    #[must_use]
    pub fn best_ask_price(&self) -> Decimal {
        match self {
            Self::Envelope {
                instrument_ticker, ..
            } => instrument_ticker.best_ask_price,
            Self::SlimEnvelope {
                instrument_ticker, ..
            } => instrument_ticker.best_ask_price,
            Self::Ticker(ticker) => ticker.best_ask_price,
        }
    }

    /// Returns the best bid price.
    #[must_use]
    pub fn best_bid_price(&self) -> Decimal {
        match self {
            Self::Envelope {
                instrument_ticker, ..
            } => instrument_ticker.best_bid_price,
            Self::SlimEnvelope {
                instrument_ticker, ..
            } => instrument_ticker.best_bid_price,
            Self::Ticker(ticker) => ticker.best_bid_price,
        }
    }

    /// Returns the best ask amount.
    #[must_use]
    pub fn best_ask_amount(&self) -> Decimal {
        match self {
            Self::Envelope {
                instrument_ticker, ..
            } => instrument_ticker.best_ask_amount,
            Self::SlimEnvelope {
                instrument_ticker, ..
            } => instrument_ticker.best_ask_amount,
            Self::Ticker(ticker) => ticker.best_ask_amount,
        }
    }

    /// Returns the best bid amount.
    #[must_use]
    pub fn best_bid_amount(&self) -> Decimal {
        match self {
            Self::Envelope {
                instrument_ticker, ..
            } => instrument_ticker.best_bid_amount,
            Self::SlimEnvelope {
                instrument_ticker, ..
            } => instrument_ticker.best_bid_amount,
            Self::Ticker(ticker) => ticker.best_bid_amount,
        }
    }

    /// Returns the current mark price.
    #[must_use]
    pub fn mark_price(&self) -> Decimal {
        match self {
            Self::Envelope {
                instrument_ticker, ..
            } => instrument_ticker.mark_price,
            Self::SlimEnvelope {
                instrument_ticker, ..
            } => instrument_ticker.mark_price,
            Self::Ticker(ticker) => ticker.mark_price,
        }
    }

    /// Returns the current index price.
    #[must_use]
    pub fn index_price(&self) -> Decimal {
        match self {
            Self::Envelope {
                instrument_ticker, ..
            } => instrument_ticker.index_price,
            Self::SlimEnvelope {
                instrument_ticker, ..
            } => instrument_ticker.index_price,
            Self::Ticker(ticker) => ticker.index_price,
        }
    }

    /// Returns the current funding rate when the ticker carries one.
    #[must_use]
    pub fn funding_rate(&self) -> Option<Decimal> {
        match self {
            Self::Envelope {
                instrument_ticker, ..
            } => instrument_ticker
                .perp_details
                .as_ref()
                .map(|perp| perp.funding_rate),
            Self::SlimEnvelope {
                instrument_ticker, ..
            } => instrument_ticker.funding_rate,
            Self::Ticker(ticker) => ticker.perp_details.as_ref().map(|perp| perp.funding_rate),
        }
    }

    /// Returns option pricing fields when the ticker carries them.
    #[must_use]
    pub fn option_pricing(&self) -> Option<&DeriveOptionPricing> {
        match self {
            Self::Envelope {
                instrument_ticker, ..
            } => instrument_ticker.option_pricing.as_ref(),
            Self::SlimEnvelope {
                instrument_ticker, ..
            } => instrument_ticker.option_pricing.as_ref(),
            Self::Ticker(ticker) => ticker.option_pricing.as_ref(),
        }
    }

    /// Returns 24-hour aggregate statistics when the ticker carries them.
    #[must_use]
    pub fn stats(&self) -> Option<&DeriveAggregateTradingStats> {
        match self {
            Self::Envelope {
                instrument_ticker, ..
            } => instrument_ticker.stats.as_ref(),
            Self::SlimEnvelope {
                instrument_ticker, ..
            } => instrument_ticker.stats.as_ref(),
            Self::Ticker(ticker) => ticker.stats.as_ref(),
        }
    }

    /// Fills compact ticker context that the venue omits from `ticker_slim`.
    ///
    /// # Errors
    ///
    /// Returns an error when a compact ticker is received on an invalid
    /// channel.
    pub fn apply_channel_context(&mut self, channel: &str) -> Result<(), String> {
        let Self::SlimEnvelope {
            instrument_ticker, ..
        } = self
        else {
            return Ok(());
        };

        if !instrument_ticker.instrument_name.as_str().is_empty() {
            return Ok(());
        }

        let instrument_name = ticker_instrument_name_from_channel(channel)
            .ok_or_else(|| format!("invalid Derive ticker channel `{channel}`"))?;
        instrument_ticker.instrument_name = Ustr::from(instrument_name);
        Ok(())
    }

    /// Returns the Nautilus instrument ID for this Derive symbol.
    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        format_instrument_id(self.instrument_name().as_str())
    }
}

/// Channel-tagged ticker update.
#[derive(Debug, Clone)]
pub struct DeriveTickerMsg {
    /// Channel that produced the update.
    pub channel: Ustr,
    /// Parsed ticker data.
    pub data: DeriveTickerData,
}

/// Typed public market data update parsed from a Derive subscription frame.
#[derive(Debug, Clone)]
pub enum DerivePublicWsData {
    /// Order book snapshot update.
    Orderbook(DeriveOrderbookMsg),
    /// Public trades update.
    Trades(DeriveTradesMsg),
    /// Ticker update.
    Ticker(Box<DeriveTickerMsg>),
}

/// Inbound frame discriminated by whether it carries an `id` (response to a
/// client request) or a `method` (server-initiated notification).
#[derive(Debug, Clone)]
pub enum DeriveWsFrame {
    /// JSON-RPC response correlated with an outbound request `id`.
    Response {
        /// Echoed request id.
        id: u64,
        /// Result payload when the venue accepted the request.
        result: Option<Value>,
        /// Error payload when the venue rejected the request.
        error: Option<JsonRpcError>,
    },
    /// Server-initiated subscription update.
    Subscription(WsSubscriptionPayload),
    /// Frame we could decode as JSON but did not recognize; surfaced so logs
    /// can flag unknown server-initiated messages without dropping silently.
    Unknown(Value),
}

/// Single-pass deserialize target for an inbound frame.
///
/// `params` is captured as a raw [`RawValue`] span rather than eagerly decoded:
/// it is only parsed into a [`WsSubscriptionPayload`] once the method check
/// confirms a subscription, so a non-subscription notification carrying an
/// unrelated `params` object still classifies as `Unknown` instead of failing
/// the frame parse. `result` stays a `Value` because the lower-frequency
/// request/response path consumes it as one.
#[derive(Debug, Deserialize)]
struct InboundFrame {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    method: Option<Ustr>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
    #[serde(default)]
    params: Option<Box<RawValue>>,
}

impl DeriveWsFrame {
    /// Parses a raw text frame into the discriminated [`DeriveWsFrame`].
    ///
    /// Returns the original JSON error when the bytes are not valid JSON;
    /// callers log and drop the frame in that case.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] when `text` is not valid JSON.
    pub fn parse(text: &str) -> serde_json::Result<Self> {
        let frame: InboundFrame = serde_json::from_str(text)?;

        if let Some(id) = frame.id {
            return Ok(Self::Response {
                id,
                result: frame.result,
                error: frame.error,
            });
        }

        if frame
            .method
            .as_ref()
            .is_some_and(|method| method.as_str() == "subscription")
            && let Some(params) = frame.params
        {
            let payload: WsSubscriptionPayload = serde_json::from_str(params.get())?;
            return Ok(Self::Subscription(payload));
        }

        // Unrecognised frame: re-parse into a `Value` for diagnostic logging.
        // The live feed only sends responses and subscription notifications, so
        // this second parse never runs on a hot path.
        Ok(Self::Unknown(serde_json::from_str(text)?))
    }
}

/// Formats the topic for the public `ticker_slim.{instrument_name}.{interval}` channel.
///
/// `interval` is the millisecond cadence the venue exposes (e.g. `"100"`,
/// `"1000"`). The function does not validate the value; the venue rejects
/// unsupported intervals on subscribe.
#[must_use]
pub fn ticker_channel(instrument_name: &str, interval: &str) -> String {
    format!("ticker_slim.{instrument_name}.{interval}")
}

fn ticker_instrument_name_from_channel(channel: &str) -> Option<&str> {
    let rest = channel
        .strip_prefix("ticker_slim.")
        .or_else(|| channel.strip_prefix("ticker."))?;
    let (instrument_name, _) = rest.rsplit_once('.')?;
    (!instrument_name.is_empty()).then_some(instrument_name)
}

/// Formats the topic for `orderbook.{instrument_name}.{group}.{depth}`.
#[must_use]
pub fn orderbook_channel(instrument_name: &str, group: &str, depth: &str) -> String {
    format!("orderbook.{instrument_name}.{group}.{depth}")
}

/// Formats the topic for `trades.{instrument_type}.{currency}`.
#[must_use]
pub fn trades_channel(instrument_type: &str, currency: &str) -> String {
    format!("trades.{instrument_type}.{currency}")
}

/// Formats the topic for the private `{subaccount_id}.orders` channel.
#[must_use]
pub fn orders_channel(subaccount_id: u64) -> String {
    format!("{subaccount_id}.orders")
}

/// Formats the topic for the private `{subaccount_id}.trades` channel.
#[must_use]
pub fn private_trades_channel(subaccount_id: u64) -> String {
    format!("{subaccount_id}.trades")
}

/// Formats the topic for the private `{subaccount_id}.balances` channel.
#[must_use]
pub fn balances_channel(subaccount_id: u64) -> String {
    format!("{subaccount_id}.balances")
}

/// JSON-RPC method names exchanged on the Derive WebSocket transport.
///
/// The `private/*` trading methods mirror the REST endpoints exactly: the
/// signed EIP-712 params built in [`crate::http::query`] and the result
/// envelopes in [`crate::http::models`] are reused verbatim over the
/// WebSocket. The session is authorized once via `PUBLIC_LOGIN`; no
/// per-request auth headers are sent.
pub mod methods {
    /// Authenticated session login. Params: [`super::WsLoginParams`].
    pub const PUBLIC_LOGIN: &str = "public/login";
    /// Subscribe to a list of channels. Params: [`super::WsSubscribeParams`].
    pub const PUBLIC_SUBSCRIBE: &str = "subscribe";
    /// Unsubscribe from a list of channels. Params: [`super::WsUnsubscribeParams`].
    pub const PUBLIC_UNSUBSCRIBE: &str = "unsubscribe";
    /// Submit a signed order. Params: [`crate::http::query::DeriveOrderParams`].
    pub const PRIVATE_ORDER: &str = "private/order";
    /// Submit a signed trigger order. Params:
    /// [`crate::http::query::DeriveTriggerOrderParams`].
    pub const PRIVATE_TRIGGER_ORDER: &str = "private/trigger_order";
    /// Cancel a single order. Params: [`crate::http::query::DeriveCancelParams`].
    pub const PRIVATE_CANCEL: &str = "private/cancel";
    /// Cancel a single trigger order. Params:
    /// [`crate::http::query::DeriveCancelTriggerOrderParams`].
    pub const PRIVATE_CANCEL_TRIGGER_ORDER: &str = "private/cancel_trigger_order";
    /// List untriggered trigger orders. Params:
    /// [`crate::http::query::DeriveGetTriggerOrdersParams`].
    pub const PRIVATE_GET_TRIGGER_ORDERS: &str = "private/get_trigger_orders";
    /// Cancel every open order on the subaccount, optionally scoped to an
    /// instrument. Params: [`crate::http::query::DeriveCancelAllParams`].
    pub const PRIVATE_CANCEL_ALL: &str = "private/cancel_all";
    /// Atomically cancel one order and submit a replacement. Params:
    /// [`crate::http::query::DeriveReplaceParams`].
    pub const PRIVATE_REPLACE: &str = "private/replace";
}

/// Returns the rate-limit key for a JSON-RPC `method` sent over the WebSocket.
///
/// Matching-engine actions (order create/cancel/replace) draw on the venue's
/// per-account matching allowance; everything else (login, subscribe, reads)
/// draws on the non-matching allowance. See [`crate::common::rate_limit`].
#[must_use]
pub(crate) fn rate_limit_key_for(method: &str) -> Ustr {
    let key = if is_matching_method(method) {
        DERIVE_MATCHING_RATE_KEY
    } else {
        DERIVE_NON_MATCHING_RATE_KEY
    };
    Ustr::from(key)
}

fn is_matching_method(method: &str) -> bool {
    matches!(
        method,
        methods::PRIVATE_ORDER
            | methods::PRIVATE_TRIGGER_ORDER
            | methods::PRIVATE_REPLACE
            | methods::PRIVATE_CANCEL
            | methods::PRIVATE_CANCEL_TRIGGER_ORDER
            | methods::PRIVATE_CANCEL_ALL
    )
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::http::models::JsonRpcRequest;

    #[rstest]
    fn test_ticker_channel_joins_with_dots() {
        assert_eq!(
            ticker_channel("ETH-PERP", "1000"),
            "ticker_slim.ETH-PERP.1000",
        );
        assert_eq!(
            ticker_channel("BTC-20260627-100000-C", "100"),
            "ticker_slim.BTC-20260627-100000-C.100",
        );
    }

    #[rstest]
    fn test_orderbook_channel_joins_with_dots() {
        assert_eq!(
            orderbook_channel("ETH-PERP", "1", "10"),
            "orderbook.ETH-PERP.1.10",
        );
    }

    #[rstest]
    #[case(methods::PRIVATE_ORDER, DERIVE_MATCHING_RATE_KEY)]
    #[case(methods::PRIVATE_TRIGGER_ORDER, DERIVE_MATCHING_RATE_KEY)]
    #[case(methods::PRIVATE_REPLACE, DERIVE_MATCHING_RATE_KEY)]
    #[case(methods::PRIVATE_CANCEL, DERIVE_MATCHING_RATE_KEY)]
    #[case(methods::PRIVATE_CANCEL_TRIGGER_ORDER, DERIVE_MATCHING_RATE_KEY)]
    #[case(methods::PRIVATE_CANCEL_ALL, DERIVE_MATCHING_RATE_KEY)]
    #[case(methods::PUBLIC_LOGIN, DERIVE_NON_MATCHING_RATE_KEY)]
    #[case(methods::PUBLIC_SUBSCRIBE, DERIVE_NON_MATCHING_RATE_KEY)]
    #[case(methods::PUBLIC_UNSUBSCRIBE, DERIVE_NON_MATCHING_RATE_KEY)]
    #[case(methods::PRIVATE_GET_TRIGGER_ORDERS, DERIVE_NON_MATCHING_RATE_KEY)]
    fn test_rate_limit_key_for(#[case] method: &str, #[case] expected: &str) {
        assert_eq!(rate_limit_key_for(method), Ustr::from(expected));
    }

    #[rstest]
    fn test_trades_channel_joins_with_dots() {
        assert_eq!(trades_channel("perp", "ETH"), "trades.perp.ETH");
    }

    #[rstest]
    #[case(0_u64, "0.orders", "0.trades", "0.balances")]
    #[case(1_u64, "1.orders", "1.trades", "1.balances")]
    #[case(30769_u64, "30769.orders", "30769.trades", "30769.balances")]
    fn test_private_channel_formatters_emit_subaccount_prefix(
        #[case] subaccount: u64,
        #[case] expected_orders: &str,
        #[case] expected_trades: &str,
        #[case] expected_balances: &str,
    ) {
        assert_eq!(orders_channel(subaccount), expected_orders);
        assert_eq!(private_trades_channel(subaccount), expected_trades);
        assert_eq!(balances_channel(subaccount), expected_balances);
    }

    #[rstest]
    fn test_ws_channel_formats_known_topics() {
        assert_eq!(
            DeriveWsChannel::ticker_slim("ETH-PERP", DeriveTickerInterval::Ms1000).to_string(),
            "ticker_slim.ETH-PERP.1000",
        );
        assert_eq!(
            DeriveWsChannel::orderbook(
                "ETH-PERP",
                DeriveOrderbookGroup::G1,
                DeriveOrderbookDepth::D10,
            )
            .to_string(),
            "orderbook.ETH-PERP.1.10",
        );
        assert_eq!(
            DeriveWsChannel::trades(DeriveInstrumentType::Perp, "ETH").to_string(),
            "trades.perp.ETH",
        );
        assert_eq!(DeriveWsChannel::orders(30769).to_string(), "30769.orders");
        assert_eq!(
            DeriveWsChannel::private_trades(30769).to_string(),
            "30769.trades",
        );
        assert_eq!(
            DeriveWsChannel::balances(30769).to_string(),
            "30769.balances",
        );
    }

    #[rstest]
    fn test_ws_channel_deserializes_known_and_raw_topics() {
        let ticker: DeriveWsChannel =
            serde_json::from_value(json!("ticker_slim.ETH.TEST-PERP.1000")).unwrap();
        let orderbook: DeriveWsChannel =
            serde_json::from_value(json!("orderbook.ETH.TEST-PERP.1.10")).unwrap();
        let private_trades: DeriveWsChannel =
            serde_json::from_value(json!("30769.trades")).unwrap();
        let raw: DeriveWsChannel = serde_json::from_value(json!("trades.ETH-USDC")).unwrap();

        assert_eq!(
            ticker,
            DeriveWsChannel::ticker_slim("ETH.TEST-PERP", "1000"),
        );
        assert_eq!(
            orderbook,
            DeriveWsChannel::orderbook("ETH.TEST-PERP", "1", "10"),
        );
        assert_eq!(private_trades, DeriveWsChannel::private_trades(30769));
        assert_eq!(raw, DeriveWsChannel::Raw("trades.ETH-USDC".to_string()));
    }

    #[rstest]
    fn test_ws_channel_uses_typed_known_topic_fields() {
        let ticker = DeriveWsChannel::from_topic("ticker_slim.ETH-PERP.1000");
        let orderbook = DeriveWsChannel::from_topic("orderbook.ETH-PERP.1.10");
        let trades = DeriveWsChannel::from_topic("trades.perp.ETH");

        match ticker {
            DeriveWsChannel::TickerSlim {
                instrument_name,
                interval,
            } => {
                assert_eq!(instrument_name.as_str(), "ETH-PERP");
                assert_eq!(interval, DeriveTickerInterval::Ms1000);
            }
            other => panic!("expected TickerSlim, was {other:?}"),
        }

        match orderbook {
            DeriveWsChannel::Orderbook {
                instrument_name,
                group,
                depth,
            } => {
                assert_eq!(instrument_name.as_str(), "ETH-PERP");
                assert_eq!(group, DeriveOrderbookGroup::G1);
                assert_eq!(depth, DeriveOrderbookDepth::D10);
            }
            other => panic!("expected Orderbook, was {other:?}"),
        }

        match trades {
            DeriveWsChannel::Trades {
                instrument_type,
                currency,
            } => {
                assert_eq!(instrument_type, DeriveInstrumentType::Perp);
                assert_eq!(currency.as_str(), "ETH");
            }
            other => panic!("expected Trades, was {other:?}"),
        }
    }

    #[rstest]
    fn test_subscribe_request_serializes_as_jsonrpc_envelope() {
        let req = JsonRpcRequest::new(
            1,
            methods::PUBLIC_SUBSCRIBE,
            WsSubscribeParams {
                channels: vec![DeriveWsChannel::ticker_slim("ETH-PERP", "1000")],
            },
        );
        let wire = serde_json::to_value(&req).unwrap();
        assert_eq!(wire["jsonrpc"], "2.0");
        assert_eq!(wire["id"], 1);
        assert_eq!(wire["method"], "subscribe");
        assert_eq!(wire["params"]["channels"][0], "ticker_slim.ETH-PERP.1000");
    }

    #[rstest]
    fn test_ws_request_params_preserve_jsonrpc_wire_output() {
        let login = JsonRpcRequest::new(
            1,
            methods::PUBLIC_LOGIN,
            WsRequestParams::from(WsLoginParams {
                wallet: "0xWALLET".to_string(),
                timestamp: "1700000000000".to_string(),
                signature: "0xSIG".to_string(),
            }),
        );
        let subscribe = JsonRpcRequest::new(
            2,
            methods::PUBLIC_SUBSCRIBE,
            WsRequestParams::from(WsSubscribeParams {
                channels: vec![DeriveWsChannel::ticker_slim("ETH-PERP", "1000")],
            }),
        );
        let unsubscribe = JsonRpcRequest::new(
            3,
            methods::PUBLIC_UNSUBSCRIBE,
            WsRequestParams::from(WsUnsubscribeParams {
                channels: vec![DeriveWsChannel::ticker_slim("ETH-PERP", "1000")],
            }),
        );

        assert_eq!(
            serde_json::to_string(&login).unwrap(),
            concat!(
                r#"{"jsonrpc":"2.0","id":1,"method":"public/login","params":{"#,
                r#""wallet":"0xWALLET","timestamp":"1700000000000","signature":"0xSIG"}}"#,
            ),
        );
        assert_eq!(
            serde_json::to_string(&subscribe).unwrap(),
            concat!(
                r#"{"jsonrpc":"2.0","id":2,"method":"subscribe","params":{"#,
                r#""channels":["ticker_slim.ETH-PERP.1000"]}}"#,
            ),
        );
        assert_eq!(
            serde_json::to_string(&unsubscribe).unwrap(),
            concat!(
                r#"{"jsonrpc":"2.0","id":3,"method":"unsubscribe","params":{"#,
                r#""channels":["ticker_slim.ETH-PERP.1000"]}}"#,
            ),
        );
    }

    #[rstest]
    fn test_ws_response_results_decode_known_shapes() {
        let login_object: WsLoginResult = serde_json::from_value(json!({"success": true})).unwrap();
        let login_array: WsLoginResult = serde_json::from_value(json!([30769])).unwrap();
        let subscribe: WsSubscribeResult = serde_json::from_value(json!({
            "channels": ["ticker_slim.ETH-PERP.1000"],
        }))
        .unwrap();
        let unsubscribe: WsUnsubscribeResult =
            serde_json::from_value(json!({"success": true})).unwrap();

        assert_eq!(login_object, WsLoginResult::Success { success: true });
        assert_eq!(
            login_array,
            WsLoginResult::AuthorizedSubaccounts(vec![30769]),
        );
        assert_eq!(
            subscribe.channels,
            vec![DeriveWsChannel::ticker_slim("ETH-PERP", "1000")],
        );
        assert!(unsubscribe.success);
    }

    #[rstest]
    fn test_subscribe_result_decodes_recorded_venue_ack() {
        let ack: Value =
            serde_json::from_str(include_str!("../../test_data/spot/ws_subscribe_ack.json"))
                .unwrap();
        let result: WsSubscribeResult =
            serde_json::from_value(ack["result"].clone()).expect("subscribe ack parses");

        assert!(
            result
                .channels
                .contains(&DeriveWsChannel::ticker_slim("ETH-USDC", "1000"))
        );
        assert_eq!(
            result
                .status
                .get(&DeriveWsChannel::orderbook("ETH-USDC", "1", "10"))
                .map(|status| status.as_str()),
            Some("ok"),
        );
    }

    #[rstest]
    fn test_login_params_round_trip() {
        let params = WsLoginParams {
            wallet: "0xWALLET".to_string(),
            timestamp: "1700000000000".to_string(),
            signature: "0xDEAD".to_string(),
        };
        let wire = serde_json::to_value(&params).unwrap();
        assert_eq!(wire["wallet"], "0xWALLET");
        assert_eq!(wire["timestamp"], "1700000000000");
        assert_eq!(wire["signature"], "0xDEAD");
        let back: WsLoginParams = serde_json::from_value(wire).unwrap();
        assert_eq!(back, params);
    }

    #[rstest]
    fn test_parse_response_with_result() {
        let text = json!({"id": 42, "result": {"ok": true}}).to_string();
        let frame = DeriveWsFrame::parse(&text).unwrap();
        match frame {
            DeriveWsFrame::Response { id, result, error } => {
                assert_eq!(id, 42);
                assert_eq!(result, Some(json!({"ok": true})));
                assert!(error.is_none());
            }
            other => panic!("expected Response, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_response_with_error_payload() {
        let text = json!({
            "id": 7,
            "error": {"code": -32602, "message": "bad params", "data": {"field": "channels"}},
        })
        .to_string();
        let frame = DeriveWsFrame::parse(&text).unwrap();
        match frame {
            DeriveWsFrame::Response { id, result, error } => {
                assert_eq!(id, 7);
                assert!(result.is_none());
                let err = error.expect("error present");
                assert_eq!(err.code, -32602);
                assert_eq!(err.data, Some(json!({"field": "channels"})));
            }
            other => panic!("expected Response, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_subscription_notification() {
        let text = json!({
            "method": "subscription",
            "params": {
                "channel": "ticker.ETH-PERP.1000",
                "data": {"instrument_name": "ETH-PERP", "mark_price": "3500.5"},
            },
        })
        .to_string();
        let frame = DeriveWsFrame::parse(&text).unwrap();
        match frame {
            DeriveWsFrame::Subscription(payload) => {
                assert_eq!(payload.channel.as_str(), "ticker.ETH-PERP.1000");
                let data: Value = serde_json::from_str(payload.data.get()).unwrap();
                assert_eq!(data["mark_price"], "3500.5");
            }
            other => panic!("expected Subscription, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_unknown_frame_preserves_value() {
        let text = json!({"hello": "world"}).to_string();
        let frame = DeriveWsFrame::parse(&text).unwrap();
        match frame {
            DeriveWsFrame::Unknown(v) => {
                assert_eq!(v["hello"], "world");
                assert!(v.get("id").is_none(), "unknown frame must not carry id");
                let method = v.get("method").and_then(Value::as_str);
                assert_ne!(method, Some("subscription"));
            }
            other => panic!("expected Unknown, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_non_subscription_notification_with_params_is_unknown() {
        // A non-subscription notification that carries an unrelated `params`
        // object must classify as Unknown, not fail the frame parse: the
        // params shape is only checked once the method confirms a subscription.
        let text = json!({"method": "heartbeat", "params": {"interval": 30}}).to_string();
        let frame = DeriveWsFrame::parse(&text).unwrap();
        match frame {
            DeriveWsFrame::Unknown(v) => {
                assert_eq!(v["method"], "heartbeat");
                assert_eq!(v["params"]["interval"], 30);
            }
            other => panic!("expected Unknown, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_response_with_both_result_and_error_prefers_error() {
        // FeedHandler dispatch treats error as winning when both are present.
        let text = json!({
            "id": 11,
            "result": {"should_not_win": true},
            "error": {"code": -1, "message": "wins"},
        })
        .to_string();
        let frame = DeriveWsFrame::parse(&text).unwrap();
        match frame {
            DeriveWsFrame::Response { id, result, error } => {
                assert_eq!(id, 11);
                assert!(result.is_some(), "result is preserved on the frame");
                let err = error.expect("error present");
                assert_eq!(err.code, -1);
                assert_eq!(err.message, "wins");
            }
            other => panic!("expected Response, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_rejects_malformed_json() {
        let err = DeriveWsFrame::parse("{not json").expect_err("must reject");
        // Pin the variant so a future refactor swallowing parse errors into Ok(Unknown) fails.
        assert_eq!(err.classify(), serde_json::error::Category::Syntax);
    }

    #[rstest]
    fn test_unsubscribe_params_round_trip() {
        let params = WsUnsubscribeParams {
            channels: vec![
                DeriveWsChannel::ticker_slim("ETH-PERP", "1000"),
                DeriveWsChannel::ticker_slim("BTC-PERP", "100"),
            ],
        };
        let wire = serde_json::to_value(&params).unwrap();
        assert_eq!(wire["channels"][0], "ticker_slim.ETH-PERP.1000");
        assert_eq!(wire["channels"][1], "ticker_slim.BTC-PERP.100");
        let back: WsUnsubscribeParams = serde_json::from_value(wire).unwrap();
        assert_eq!(back, params);
    }

    #[rstest]
    fn test_private_orders_subscription_data_decodes_single_and_array_payloads() {
        let order: Value = serde_json::from_str(include_str!(
            "../../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        let single: DeriveOrdersSubscriptionData = serde_json::from_value(order.clone()).unwrap();
        let array: DeriveOrdersSubscriptionData =
            serde_json::from_value(json!([order, {"not": "an order"}])).unwrap();

        assert_eq!(single.orders.len(), 1);
        assert_eq!(single.orders[0].order_id, "abc-123");
        assert_eq!(array.orders.len(), 1);
        assert_eq!(array.orders[0].order_id, "abc-123");
    }

    #[rstest]
    fn test_private_trades_subscription_data_decodes_single_and_array_payloads() {
        let trade: Value = serde_json::from_str(include_str!(
            "../../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        let single: DeriveTradesSubscriptionData = serde_json::from_value(trade.clone()).unwrap();
        let array: DeriveTradesSubscriptionData =
            serde_json::from_value(json!([trade, {"not": "a trade"}])).unwrap();

        assert_eq!(single.trades.len(), 1);
        assert_eq!(single.trades[0].trade_id, "trade-xyz");
        assert_eq!(array.trades.len(), 1);
        assert_eq!(array.trades[0].trade_id, "trade-xyz");
    }
}
