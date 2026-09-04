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

//! WebSocket message types for the Polymarket CLOB API.

use nautilus_core::{
    serialization::deserialize_empty_string_as_none, string::secret::SecretString,
};
use rust_decimal::Decimal;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{
        DeserializeSeed, MapAccess, Visitor,
        value::{BorrowedStrDeserializer, MapAccessDeserializer},
    },
};
use serde_json::value::RawValue;
use ustr::Ustr;
use zeroize::Zeroize;

use crate::common::{
    enums::{
        PolymarketEventType, PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOrderStatus,
        PolymarketOrderType, PolymarketOutcome, PolymarketTradeStatus,
    },
    models::PolymarketMakerOrder,
    parse::{
        deserialize_decimal_from_str, deserialize_optional_decimal_from_str,
        serialize_decimal_as_str, serialize_optional_decimal_as_str,
    },
};

/// A user-channel order status and its optional venue reason suffix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolymarketUserOrderStatus {
    pub status: PolymarketOrderStatus,
    pub reason: Option<String>,
}

impl PolymarketUserOrderStatus {
    pub(crate) fn new(status: PolymarketOrderStatus, reason: Option<&str>) -> Self {
        Self {
            status,
            reason: reason
                .filter(|reason| !reason.trim().is_empty())
                .map(str::to_string),
        }
    }
}

impl From<PolymarketOrderStatus> for PolymarketUserOrderStatus {
    fn from(status: PolymarketOrderStatus) -> Self {
        Self::new(status, None)
    }
}

impl<'de> Deserialize<'de> for PolymarketUserOrderStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;

        PolymarketOrderStatus::parse_wire(&raw)
            .map(|(status, reason)| Self::new(status, reason))
            .ok_or_else(|| {
                serde::de::Error::custom(format!("Unknown PolymarketOrderStatus: {raw}"))
            })
    }
}

impl Serialize for PolymarketUserOrderStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.reason.as_deref() {
            Some(reason) => serializer.serialize_str(&format!("{}_{reason}", self.status)),
            None => self.status.serialize(serializer),
        }
    }
}

/// A user order status update from the WebSocket user channel.
///
/// References: <https://docs.polymarket.com/developers/CLOB/websocket/user-channel#order-message>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketUserOrder {
    pub asset_id: Ustr,
    pub associate_trades: Option<Vec<String>>,
    pub created_at: Option<String>,
    pub expiration: Option<String>,
    pub id: String,
    pub maker_address: Option<Ustr>,
    pub market: Ustr,
    pub order_owner: Option<Ustr>,
    pub order_type: Option<PolymarketOrderType>,
    pub original_size: String,
    pub outcome: Option<PolymarketOutcome>,
    pub owner: Ustr,
    pub price: String,
    pub side: PolymarketOrderSide,
    pub size_matched: String,
    pub status: Option<PolymarketUserOrderStatus>,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub event_type: PolymarketEventType,
}

/// A user trade update from the WebSocket user channel.
///
/// References: <https://docs.polymarket.com/developers/CLOB/websocket/user-channel>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketUserTrade {
    pub asset_id: Ustr,
    pub bucket_index: u64,
    pub fee_rate_bps: String,
    pub id: String,
    pub last_update: String,
    pub maker_address: Ustr,
    pub maker_orders: Vec<PolymarketMakerOrder>,
    pub market: Ustr,
    pub match_time: String,
    pub outcome: PolymarketOutcome,
    pub owner: Ustr,
    pub price: String,
    pub side: PolymarketOrderSide,
    pub size: String,
    pub status: PolymarketTradeStatus,
    pub taker_order_id: String,
    pub timestamp: String,
    pub trade_owner: Ustr,
    #[serde(
        default,
        deserialize_with = "deserialize_empty_string_as_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub transaction_hash: Option<String>,
    pub trader_side: PolymarketLiquiditySide,
    #[serde(rename = "type")]
    pub event_type: PolymarketEventType,
}

/// A single price level in an order book snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketBookLevel {
    pub price: String,
    pub size: String,
}

/// An order book snapshot from the WebSocket market channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketBookSnapshot {
    pub market: Ustr,
    pub asset_id: Ustr,
    pub bids: Vec<PolymarketBookLevel>,
    pub asks: Vec<PolymarketBookLevel>,
    pub timestamp: String,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub min_order_size: Option<String>,
    #[serde(default)]
    pub tick_size: Option<String>,
    #[serde(default)]
    pub neg_risk: Option<bool>,
    #[serde(default)]
    pub last_trade_price: Option<String>,
}

/// A single price change entry within a quotes message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketQuote {
    pub asset_id: Ustr,
    pub price: String,
    pub side: PolymarketOrderSide,
    pub size: String,
    pub hash: String,
    #[serde(default)]
    pub best_bid: Option<String>,
    #[serde(default)]
    pub best_ask: Option<String>,
}

/// A price change (quotes) message from the WebSocket market channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketQuotes {
    pub market: Ustr,
    pub price_changes: Vec<PolymarketQuote>,
    pub timestamp: String,
}

/// A last trade price message from the WebSocket market channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketTrade {
    pub market: Ustr,
    pub asset_id: Ustr,
    pub fee_rate_bps: String,
    pub price: String,
    pub side: PolymarketOrderSide,
    pub size: String,
    pub timestamp: String,
    #[serde(default)]
    pub transaction_hash: Option<String>,
}

/// A tick size change notification from the WebSocket market channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketTickSizeChange {
    pub market: Ustr,
    pub asset_id: Ustr,
    pub new_tick_size: String,
    pub old_tick_size: String,
    pub timestamp: String,
}

/// Event metadata embedded in a new market notification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketNewMarketEvent {
    pub id: String,
    pub ticker: String,
    pub slug: String,
    pub title: String,
    pub description: String,
}

/// Fee configuration observed in a new market notification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketNewMarketFeeSchedule {
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub exponent: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub rate: Decimal,
    pub taker_only: bool,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub rebate_rate: Decimal,
}

/// A new market notification from the WebSocket market channel.
///
/// Only received when `subscribe_new_markets` is enabled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketNewMarket {
    pub id: String,
    pub question: String,
    pub market: Ustr,
    pub slug: String,
    pub description: String,
    pub assets_ids: Vec<String>,
    pub outcomes: Vec<String>,
    pub timestamp: String,
    pub tags: Vec<String>,
    pub condition_id: String,
    pub active: bool,
    pub clob_token_ids: Vec<String>,
    #[serde(default)]
    pub order_price_min_tick_size: Option<String>,
    #[serde(default)]
    pub group_item_title: Option<String>,
    #[serde(default)]
    pub event_message: Option<PolymarketNewMarketEvent>,
    #[serde(default)]
    pub sports_market_type: Option<String>,
    #[serde(default)]
    pub line: Option<String>,
    #[serde(default)]
    pub game_start_time: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_decimal_as_str",
        deserialize_with = "deserialize_optional_decimal_from_str"
    )]
    pub taker_base_fee: Option<Decimal>,
    #[serde(default)]
    pub fees_enabled: Option<bool>,
    #[serde(default)]
    pub fee_schedule: Option<PolymarketNewMarketFeeSchedule>,
}

/// A market resolved notification from the WebSocket market channel.
///
/// Only received when `subscribe_new_markets` is enabled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketMarketResolved {
    pub id: String,
    pub market: Ustr,
    pub assets_ids: Vec<String>,
    pub winning_asset_id: String,
    pub winning_outcome: String,
    pub timestamp: String,
    pub tags: Vec<String>,
}

/// A best bid/ask notification from the WebSocket market channel.
///
/// Only received when `subscribe_new_markets` is enabled.
/// The data adapter emits these events as quote ticks for active quote subscriptions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketBestBidAsk {
    pub market: Ustr,
    pub asset_id: Ustr,
    pub best_bid: String,
    pub best_ask: String,
    pub spread: String,
    pub timestamp: String,
}

/// An envelope for tagged WebSocket market channel messages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum MarketWsMessage {
    #[serde(rename = "book")]
    Book(PolymarketBookSnapshot),
    #[serde(rename = "price_change")]
    PriceChange(PolymarketQuotes),
    #[serde(rename = "last_trade_price")]
    LastTradePrice(PolymarketTrade),
    #[serde(rename = "tick_size_change")]
    TickSizeChange(PolymarketTickSizeChange),
    #[serde(rename = "new_market")]
    NewMarket(Box<PolymarketNewMarket>),
    #[serde(rename = "market_resolved")]
    MarketResolved(PolymarketMarketResolved),
    #[serde(rename = "best_bid_ask")]
    BestBidAsk(PolymarketBestBidAsk),
}

struct PayloadMapAccess<A> {
    inner: A,
}

impl<A> PayloadMapAccess<A> {
    const fn new(inner: A) -> Self {
        Self { inner }
    }
}

impl<'de, A> MapAccess<'de> for PayloadMapAccess<A>
where
    A: MapAccess<'de>,
{
    type Error = A::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        let Some(key) = self.inner.next_key::<&'de str>()? else {
            return Ok(None);
        };

        if key == "event_type" {
            return Err(serde::de::Error::duplicate_field("event_type"));
        }

        seed.deserialize(BorrowedStrDeserializer::new(key))
            .map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        self.inner.next_value_seed(seed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

struct MarketWsMessageVisitor;

impl<'de> Visitor<'de> for MarketWsMessageVisitor {
    type Value = MarketWsMessage;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a Polymarket market-channel message with event_type first")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let Some(key) = map.next_key::<&str>()? else {
            return Err(serde::de::Error::custom("expected event_type field"));
        };

        if key != "event_type" {
            return Err(serde::de::Error::custom(
                "event_type was not the first field",
            ));
        }

        let event_type = map.next_value::<&str>()?;
        let remaining = MapAccessDeserializer::new(PayloadMapAccess::new(map));
        match event_type {
            "book" => PolymarketBookSnapshot::deserialize(remaining).map(Self::Value::Book),
            "price_change" => {
                PolymarketQuotes::deserialize(remaining).map(Self::Value::PriceChange)
            }
            "last_trade_price" => {
                PolymarketTrade::deserialize(remaining).map(Self::Value::LastTradePrice)
            }
            "tick_size_change" => {
                PolymarketTickSizeChange::deserialize(remaining).map(Self::Value::TickSizeChange)
            }
            "new_market" => PolymarketNewMarket::deserialize(remaining)
                .map(Box::new)
                .map(Self::Value::NewMarket),
            "market_resolved" => {
                PolymarketMarketResolved::deserialize(remaining).map(Self::Value::MarketResolved)
            }
            "best_bid_ask" => {
                PolymarketBestBidAsk::deserialize(remaining).map(Self::Value::BestBidAsk)
            }
            other => Err(serde::de::Error::unknown_variant(
                other,
                &[
                    "book",
                    "price_change",
                    "last_trade_price",
                    "tick_size_change",
                    "new_market",
                    "market_resolved",
                    "best_bid_ask",
                ],
            )),
        }
    }
}

impl MarketWsMessage {
    /// Parses a market-channel JSON message.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] when `text` is not a valid market message.
    pub fn parse(text: &str) -> serde_json::Result<Self> {
        let mut deserializer = serde_json::Deserializer::from_str(text);
        serde::Deserializer::deserialize_map(&mut deserializer, MarketWsMessageVisitor)
            .and_then(|message| {
                deserializer.end()?;
                Ok(message)
            })
            .or_else(|_| Self::parse_reordered(text))
            .or_else(|_| serde_json::from_str(text))
    }

    fn parse_reordered(text: &str) -> serde_json::Result<Self> {
        let tag = serde_json::from_str::<MarketWsTag>(text)?;
        match tag.event_type {
            MarketWsEventTag::Book => serde_json::from_str(text).map(Self::Book),
            MarketWsEventTag::PriceChange => serde_json::from_str(text).map(Self::PriceChange),
            MarketWsEventTag::LastTradePrice => {
                serde_json::from_str(text).map(Self::LastTradePrice)
            }
            MarketWsEventTag::TickSizeChange => {
                serde_json::from_str(text).map(Self::TickSizeChange)
            }
            MarketWsEventTag::NewMarket => serde_json::from_str(text)
                .map(Box::new)
                .map(Self::NewMarket),
            MarketWsEventTag::MarketResolved => {
                serde_json::from_str(text).map(Self::MarketResolved)
            }
            MarketWsEventTag::BestBidAsk => serde_json::from_str(text).map(Self::BestBidAsk),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum MarketWsEventTag {
    Book,
    PriceChange,
    LastTradePrice,
    TickSizeChange,
    NewMarket,
    MarketResolved,
    BestBidAsk,
}

#[derive(Deserialize)]
struct MarketWsTag {
    event_type: MarketWsEventTag,
}

/// An envelope for tagged WebSocket user channel messages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum UserWsMessage {
    #[serde(rename = "order")]
    Order(PolymarketUserOrder),
    #[serde(rename = "trade")]
    Trade(PolymarketUserTrade),
}

struct UserWsMessageVisitor;

impl<'de> Visitor<'de> for UserWsMessageVisitor {
    type Value = UserWsMessage;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a Polymarket user-channel message with event_type first")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let Some(key) = map.next_key::<&str>()? else {
            return Err(serde::de::Error::custom("expected event_type field"));
        };

        if key != "event_type" {
            return Err(serde::de::Error::custom(
                "event_type was not the first field",
            ));
        }

        let event_type = map.next_value::<&str>()?;
        let remaining = MapAccessDeserializer::new(PayloadMapAccess::new(map));
        match event_type {
            "order" => PolymarketUserOrder::deserialize(remaining).map(Self::Value::Order),
            "trade" => PolymarketUserTrade::deserialize(remaining).map(Self::Value::Trade),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["order", "trade"],
            )),
        }
    }
}

impl UserWsMessage {
    /// Parses a user-channel JSON message.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] when `text` is not a valid user message.
    pub fn parse(text: &str) -> serde_json::Result<Self> {
        let mut deserializer = serde_json::Deserializer::from_str(text);
        serde::Deserializer::deserialize_map(&mut deserializer, UserWsMessageVisitor)
            .and_then(|message| {
                deserializer.end()?;
                Ok(message)
            })
            .or_else(|_| Self::parse_reordered(text))
            .or_else(|_| serde_json::from_str(text))
    }

    /// Parses a batch of user-channel JSON messages.
    ///
    /// Elements carrying an unrecognized `event_type` are skipped so that a single unknown
    /// message cannot discard the valid `order` and `trade` messages batched alongside it.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] when `text` is not a valid user-message batch.
    pub fn parse_batch(text: &str) -> serde_json::Result<Vec<Self>> {
        /// Reads only the tag, to classify an element before deserializing it.
        #[derive(Deserialize)]
        struct EventTypeTag {
            event_type: Option<String>,
        }

        // Elements stay raw so the derived impl parses each one and rejects a duplicated
        // `event_type`; `serde_json::Value` would silently keep the last occurrence.
        let elements: Vec<&RawValue> = serde_json::from_str(text)?;
        let mut messages = Vec::with_capacity(elements.len());
        let mut skipped = 0usize;

        for element in elements {
            let tag: EventTypeTag = serde_json::from_str(element.get())?;
            match tag.event_type.as_deref() {
                Some(event_type) if !matches!(event_type, "order" | "trade") => skipped += 1,
                _ => messages.push(serde_json::from_str(element.get())?),
            }
        }

        if skipped > 0 {
            log::debug!("Skipped {skipped} user WS message(s) with an unrecognized event_type");
        }

        Ok(messages)
    }

    fn parse_reordered(text: &str) -> serde_json::Result<Self> {
        let tag = serde_json::from_str::<UserWsTag>(text)?;
        match tag.event_type {
            UserWsEventTag::Order => serde_json::from_str(text).map(Self::Order),
            UserWsEventTag::Trade => serde_json::from_str(text).map(Self::Trade),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum UserWsEventTag {
    Order,
    Trade,
}

#[derive(Deserialize)]
struct UserWsTag {
    event_type: UserWsEventTag,
}

/// Output message type from the Polymarket WebSocket handler.
#[derive(Debug)]
pub enum PolymarketWsMessage {
    Market(MarketWsMessage),
    User(UserWsMessage),
    /// Emitted when the underlying WebSocket reconnects.
    Reconnected,
}

/// Auth payload embedded in user-channel subscribe messages.
#[derive(Debug, Serialize, Zeroize)]
pub struct PolymarketWsAuth {
    #[serde(rename = "apiKey")]
    pub api_key: SecretString,
    pub secret: SecretString,
    pub passphrase: SecretString,
}

/// Initial market-channel subscribe request sent for a fresh WebSocket session.
///
/// Wire format: `{"assets_ids": [...], "type": "market", "initial_dump": true}`
/// When `custom_feature_enabled` is true, enables new-market, market-resolved, and best-bid/ask
/// events.
#[derive(Debug, Serialize)]
pub struct MarketInitialSubscribeRequest {
    pub assets_ids: Vec<String>,
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub initial_dump: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub custom_feature_enabled: bool,
}

/// Incremental market-channel subscribe request sent after the initial session subscribe.
///
/// Wire format: `{"assets_ids": [...], "operation": "subscribe", "initial_dump": true}`
/// When `custom_feature_enabled` is true, enables new-market, market-resolved, and best-bid/ask
/// events.
#[derive(Debug, Serialize)]
pub struct MarketSubscribeRequest {
    pub assets_ids: Vec<String>,
    pub operation: &'static str,
    pub initial_dump: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub custom_feature_enabled: bool,
}

/// Market-channel dynamic unsubscribe request sent during an active session.
///
/// Wire format: `{"assets_ids": [...], "operation": "unsubscribe"}`
#[derive(Debug, Serialize)]
pub struct MarketUnsubscribeRequest {
    pub assets_ids: Vec<String>,
    pub operation: &'static str,
}

/// User-channel subscribe request sent on connect.
///
/// Wire format: `{"auth": {...}, "type": "user"}`
#[derive(Debug, Serialize, Zeroize)]
pub struct UserSubscribeRequest {
    pub auth: PolymarketWsAuth,
    #[serde(rename = "type")]
    #[zeroize(skip)]
    pub msg_type: &'static str,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::enums::{
        PolymarketEventType, PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOrderStatus,
        PolymarketOrderType, PolymarketOutcome, PolymarketTradeStatus,
    };

    #[rstest]
    fn user_subscribe_request_matches_all_markets_wire_format() {
        let request = UserSubscribeRequest {
            auth: PolymarketWsAuth {
                api_key: SecretString::from("fixture-key"),
                secret: SecretString::from("fixture-secret"),
                passphrase: SecretString::from("fixture-passphrase"),
            },
            msg_type: "user",
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "auth": {
                    "apiKey": "fixture-key",
                    "secret": "fixture-secret",
                    "passphrase": "fixture-passphrase",
                },
                "type": "user",
            }),
        );
    }

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    fn load_text(filename: &str) -> String {
        let path = format!("test_data/{filename}");
        std::fs::read_to_string(path).expect("Failed to read test data")
    }

    /// An `auto_redeem` user-channel event as observed from the venue, which is undocumented and
    /// not modelled by [`UserWsMessage`].
    fn auto_redeem_element() -> serde_json::Value {
        serde_json::json!({
            "event_type": "auto_redeem",
            "proxy_wallet": "0x0000000000000000000000000000000000000000",
            "txn_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "amount": "70",
            "condition_id": "0xb88862256916cb0c82e72667bd43f3e6cfe94cd7d5cda0bf763ec82394021e07",
            "question": "Bitcoin Up or Down - August 17, 9:35AM-9:40AM ET",
            "slug": "btc-updown-5m-1786973700",
            "neg_risk": false,
            "timestamp": "1786974414920",
            "position_id": "",
            "outcome_index": 0,
            "legs": 0,
            "owner": "00000000-0000-0000-0000-000000000000",
        })
    }

    #[rstest]
    fn test_book_snapshot() {
        let snap: PolymarketBookSnapshot = load("ws_book_snapshot.json");

        assert_eq!(
            snap.asset_id.as_str(),
            "71321045679252212594626385532706912750332728571942532289631379312455583992563"
        );
        assert_eq!(snap.bids.len(), 3);
        assert_eq!(snap.asks.len(), 3);
        assert_eq!(snap.bids[0].price, "0.48");
        assert_eq!(snap.bids[0].size, "500.0");
        assert_eq!(snap.asks[0].price, "0.53");
        assert_eq!(snap.timestamp, "1703875200000");
        assert!(snap.hash.is_none());
        assert!(snap.min_order_size.is_none());
        assert!(snap.tick_size.is_none());
        assert!(snap.neg_risk.is_none());
        assert!(snap.last_trade_price.is_none());
    }

    #[rstest]
    fn test_book_snapshot_roundtrip() {
        let snap: PolymarketBookSnapshot = load("ws_book_snapshot.json");
        let json = serde_json::to_string(&snap).unwrap();
        let snap2: PolymarketBookSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, snap2);
    }

    #[rstest]
    fn test_quotes() {
        let quotes: PolymarketQuotes = load("ws_quotes.json");

        assert_eq!(quotes.price_changes.len(), 2);
        assert_eq!(quotes.price_changes[0].side, PolymarketOrderSide::Buy);
        assert_eq!(quotes.price_changes[0].price, "0.51");
        assert_eq!(quotes.price_changes[0].best_bid.as_deref(), Some("0.51"));
        assert_eq!(quotes.price_changes[0].best_ask.as_deref(), Some("0.52"));
        assert_eq!(quotes.price_changes[1].side, PolymarketOrderSide::Sell);
        assert_eq!(quotes.timestamp, "1703875201000");
    }

    #[rstest]
    fn test_last_trade() {
        let trade: PolymarketTrade = load("ws_last_trade.json");

        assert_eq!(trade.price, "0.51");
        assert_eq!(trade.size, "25.0");
        assert_eq!(trade.side, PolymarketOrderSide::Buy);
        assert_eq!(trade.fee_rate_bps, "0");
        assert_eq!(trade.timestamp, "1703875202000");
        assert_eq!(
            trade.transaction_hash.as_deref(),
            Some("0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab")
        );
    }

    #[rstest]
    fn test_optional_market_hash_fields_default() {
        let snap: PolymarketBookSnapshot = load("ws_book_snapshot_missing_hash.json");
        let trade: PolymarketTrade = load("ws_last_trade_missing_transaction_hash.json");

        assert!(snap.hash.is_none());
        assert!(snap.min_order_size.is_none());
        assert!(snap.tick_size.is_none());
        assert!(snap.neg_risk.is_none());
        assert!(snap.last_trade_price.is_none());
        assert!(trade.transaction_hash.is_none());
    }

    #[rstest]
    fn test_tick_size_change() {
        let msg: PolymarketTickSizeChange = load("ws_tick_size_change.json");

        assert_eq!(msg.new_tick_size, "0.01");
        assert_eq!(msg.old_tick_size, "0.1");
        assert_eq!(msg.timestamp, "1703875210000");
    }

    #[rstest]
    fn test_user_order_placement() {
        let order: PolymarketUserOrder = load("ws_user_order_placement.json");

        assert_eq!(order.event_type, PolymarketEventType::Placement);
        assert_eq!(
            order.status.as_ref().map(|status| status.status),
            Some(PolymarketOrderStatus::Live)
        );
        assert_eq!(order.side, PolymarketOrderSide::Buy);
        assert_eq!(order.order_type, Some(PolymarketOrderType::GTC));
        assert_eq!(order.outcome, Some(PolymarketOutcome::yes()));
        assert_eq!(order.original_size, "100.0");
        assert_eq!(order.size_matched, "0.0");
        assert!(order.associate_trades.is_none());
        assert!(order.expiration.is_none());
    }

    #[rstest]
    fn test_user_order_update() {
        let order: PolymarketUserOrder = load("ws_user_order_update.json");

        assert_eq!(order.event_type, PolymarketEventType::Update);
        assert_eq!(order.size_matched, "25.0");
        assert_eq!(
            order.associate_trades.as_deref(),
            Some(&["trade-0xabcdef1234".to_string()][..])
        );
    }

    #[rstest]
    fn test_user_order_cancellation() {
        let order: PolymarketUserOrder = load("ws_user_order_cancellation.json");

        assert_eq!(order.event_type, PolymarketEventType::Cancellation);
        assert_eq!(
            order.status.as_ref().map(|status| status.status),
            Some(PolymarketOrderStatus::Canceled)
        );
        assert_eq!(order.size_matched, "0.0");
    }

    #[rstest]
    fn test_user_order_status_preserves_rejection_reason() {
        let raw = "UNMATCHED_invalid post-only order: order crosses book";
        let status: PolymarketUserOrderStatus =
            serde_json::from_str(&format!("\"{raw}\"")).unwrap();

        assert_eq!(status.status, PolymarketOrderStatus::Unmatched);
        assert_eq!(
            status.reason.as_deref(),
            Some("invalid post-only order: order crosses book")
        );
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            format!("\"{raw}\"")
        );
    }

    /// Repro for issue #3987: venue cancels a FOK order with a status field
    /// containing a trailing reason ("CANCELED_<reason>") and empty fields
    /// on `size_matched`, `outcome`, and `created_at`.
    #[rstest]
    fn test_user_order_fok_killed() {
        let msg: UserWsMessage = load("ws_user_order_fok_killed.json");

        let UserWsMessage::Order(order) = msg else {
            panic!("Expected UserWsMessage::Order");
        };
        assert_eq!(order.event_type, PolymarketEventType::Cancellation);
        assert_eq!(
            order.status.as_ref().map(|status| status.status),
            Some(PolymarketOrderStatus::Canceled)
        );
        assert_eq!(
            order
                .status
                .as_ref()
                .and_then(|status| status.reason.as_deref()),
            Some("order couldn't be fully filled. FOK orders are fully filled or killed.")
        );
        assert_eq!(order.order_type, Some(PolymarketOrderType::FOK));
        assert_eq!(order.size_matched, "");
        assert_eq!(order.created_at.as_deref(), Some(""));
        assert_eq!(
            order.outcome.as_ref().map(PolymarketOutcome::as_str),
            Some("")
        );
    }

    #[rstest]
    fn test_user_trade() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");

        assert_eq!(trade.event_type, PolymarketEventType::Trade);
        assert_eq!(trade.status, PolymarketTradeStatus::Confirmed);
        assert_eq!(trade.side, PolymarketOrderSide::Buy);
        assert_eq!(trade.trader_side, PolymarketLiquiditySide::Taker);
        assert_eq!(trade.price, "0.5");
        assert_eq!(trade.size, "25.0");
        assert_eq!(trade.fee_rate_bps, "0");
        assert_eq!(trade.bucket_index, 1);
        assert_eq!(trade.maker_orders.len(), 1);
        assert_eq!(
            trade.transaction_hash.as_deref(),
            Some("0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab")
        );
        assert_eq!(
            trade.taker_order_id,
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12"
        );
    }

    #[rstest]
    fn test_user_trade_missing_transaction_hash() {
        let mut value: serde_json::Value = load("ws_user_trade.json");
        value
            .as_object_mut()
            .expect("trade fixture should be an object")
            .remove("transaction_hash");

        let trade: PolymarketUserTrade =
            serde_json::from_value(value).expect("trade fixture should deserialize");

        assert!(trade.transaction_hash.is_none());
    }

    #[rstest]
    fn test_market_ws_message_book() {
        let msg: MarketWsMessage = load("ws_market_book_msg.json");

        assert!(matches!(msg, MarketWsMessage::Book(_)));
        if let MarketWsMessage::Book(snap) = msg {
            assert_eq!(snap.bids.len(), 2);
            assert_eq!(snap.asks.len(), 2);
            assert_eq!(snap.timestamp, "1703875200000");
        }
    }

    #[rstest]
    #[case("ws_market_book_msg.json")]
    #[case("ws_market_price_change_msg.json")]
    #[case("ws_market_last_trade_msg.json")]
    #[case("ws_market_tick_size_msg.json")]
    #[case("ws_market_new_market_msg.json")]
    #[case("ws_market_resolved_msg.json")]
    #[case("ws_market_best_bid_ask_msg.json")]
    fn test_market_ws_message_parse(#[case] filename: &str) {
        let text = load_text(filename);
        let expected: MarketWsMessage =
            serde_json::from_str(&text).expect("market fixture should deserialize");

        let actual = MarketWsMessage::parse(&text).expect("market fixture should parse");

        assert_eq!(actual, expected);
    }

    #[rstest]
    fn test_market_ws_message_parse_with_reordered_event_type() {
        let expected: MarketWsMessage = load("ws_market_book_msg.json");
        let mut value: serde_json::Value = load("ws_market_book_msg.json");
        let object = value
            .as_object_mut()
            .expect("market fixture should be an object");
        let event_type = object
            .remove("event_type")
            .expect("market fixture should contain event_type");
        object.insert("event_type".to_string(), event_type);
        let text = serde_json::to_string(&value).expect("market fixture should serialize");

        assert!(!text.starts_with(r#"{"event_type":"#));
        assert_eq!(
            MarketWsMessage::parse(&text).expect("reordered market fixture should parse"),
            expected
        );
    }

    #[rstest]
    fn test_market_ws_message_parse_rejects_duplicate_event_type() {
        let text = load_text("ws_market_book_msg.json").replacen(
            r#""event_type": "book","#,
            r#""event_type": "book", "event_type": "book","#,
            1,
        );
        let expected = serde_json::from_str::<MarketWsMessage>(&text)
            .expect_err("derived parser should reject a duplicate event_type");
        let actual = MarketWsMessage::parse(&text)
            .expect_err("optimized parser should reject a duplicate event_type");

        assert_eq!(actual.to_string(), expected.to_string());
    }

    #[rstest]
    fn test_market_ws_message_price_change() {
        let msg: MarketWsMessage = load("ws_market_price_change_msg.json");

        assert!(matches!(msg, MarketWsMessage::PriceChange(_)));
        if let MarketWsMessage::PriceChange(quotes) = msg {
            assert_eq!(quotes.price_changes.len(), 1);
        }
    }

    #[rstest]
    fn test_market_ws_message_last_trade_price() {
        let msg: MarketWsMessage = load("ws_market_last_trade_msg.json");

        assert!(matches!(msg, MarketWsMessage::LastTradePrice(_)));
        if let MarketWsMessage::LastTradePrice(trade) = msg {
            assert_eq!(trade.price, "0.51");
        }
    }

    #[rstest]
    fn test_market_ws_message_tick_size_change() {
        let msg: MarketWsMessage = load("ws_market_tick_size_msg.json");

        assert!(matches!(msg, MarketWsMessage::TickSizeChange(_)));
        if let MarketWsMessage::TickSizeChange(change) = msg {
            assert_eq!(change.new_tick_size, "0.01");
            assert_eq!(change.old_tick_size, "0.1");
        }
    }

    #[rstest]
    fn test_user_ws_message_order() {
        let msg: UserWsMessage = load("ws_user_order_msg.json");

        let UserWsMessage::Order(order) = msg else {
            panic!("expected order message");
        };
        assert_eq!(
            order.asset_id.as_str(),
            "10000000000000000000000000000000000000000000000000000000000000000000000000001"
        );
        assert_eq!(order.associate_trades, Some(Vec::new()));
        assert_eq!(order.created_at.as_deref(), Some(""));
        assert_eq!(order.expiration.as_deref(), Some("0"));
        assert_eq!(
            order.id,
            "0x1111111111111111111111111111111111111111111111111111111111111111"
        );
        assert_eq!(
            order.maker_address.as_ref().map(Ustr::as_str),
            Some("0x1111111111111111111111111111111111111111")
        );
        assert_eq!(
            order.market.as_str(),
            "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        );
        assert_eq!(
            order.order_owner.as_ref().map(Ustr::as_str),
            Some("11111111-2222-3333-4444-555555555555")
        );
        assert_eq!(order.order_type, Some(PolymarketOrderType::FOK));
        assert_eq!(order.original_size, "1");
        assert_eq!(
            order.outcome.as_ref().map(PolymarketOutcome::as_str),
            Some("")
        );
        assert_eq!(order.owner.as_str(), "11111111-2222-3333-4444-555555555555");
        assert_eq!(order.price, "0.01");
        assert_eq!(order.side, PolymarketOrderSide::Buy);
        assert_eq!(order.size_matched, "");
        assert_eq!(
            order.status.as_ref().map(|status| status.status),
            Some(PolymarketOrderStatus::Canceled)
        );
        assert_eq!(order.timestamp, "1786179547007");
        assert_eq!(order.event_type, PolymarketEventType::Cancellation);
    }

    #[rstest]
    fn test_user_ws_message_order_optional_fields_absent() {
        // Constructed from the documented required field set
        let json = r#"{
            "event_type":"order",
            "id":"order-1",
            "owner":"owner-1",
            "market":"market-1",
            "asset_id":"asset-1",
            "side":"SELL",
            "original_size":"2",
            "size_matched":"0",
            "price":"0.5",
            "type":"PLACEMENT",
            "timestamp":"1786179547008"
        }"#;
        let UserWsMessage::Order(order) = serde_json::from_str(json).unwrap() else {
            panic!("expected order message");
        };

        assert_eq!(order.asset_id.as_str(), "asset-1");
        assert!(order.associate_trades.is_none());
        assert!(order.created_at.is_none());
        assert!(order.expiration.is_none());
        assert_eq!(order.id, "order-1");
        assert!(order.maker_address.is_none());
        assert_eq!(order.market.as_str(), "market-1");
        assert!(order.order_owner.is_none());
        assert!(order.order_type.is_none());
        assert_eq!(order.original_size, "2");
        assert!(order.outcome.is_none());
        assert_eq!(order.owner.as_str(), "owner-1");
        assert_eq!(order.price, "0.5");
        assert_eq!(order.side, PolymarketOrderSide::Sell);
        assert_eq!(order.size_matched, "0");
        assert!(order.status.is_none());
        assert_eq!(order.timestamp, "1786179547008");
        assert_eq!(order.event_type, PolymarketEventType::Placement);
    }

    #[rstest]
    fn test_user_ws_message_trade() {
        let msg: UserWsMessage = load("ws_user_trade_msg.json");

        assert!(matches!(msg, UserWsMessage::Trade(_)));
        if let UserWsMessage::Trade(trade) = msg {
            assert_eq!(trade.event_type, PolymarketEventType::Trade);
            assert_eq!(trade.status, PolymarketTradeStatus::Confirmed);
            assert!(trade.transaction_hash.is_none());
        }
    }

    #[rstest]
    #[case("ws_user_order_msg.json")]
    #[case("ws_user_order_fok_killed.json")]
    #[case("ws_user_trade_msg.json")]
    fn test_user_ws_message_parse(#[case] filename: &str) {
        let text = load_text(filename);
        let expected: UserWsMessage =
            serde_json::from_str(&text).expect("user fixture should deserialize");

        let actual = UserWsMessage::parse(&text).expect("user fixture should parse");

        assert_eq!(actual, expected);
    }

    #[rstest]
    fn test_user_ws_message_parse_with_reordered_event_type() {
        let expected: UserWsMessage = load("ws_user_trade_msg.json");
        let mut value: serde_json::Value = load("ws_user_trade_msg.json");
        let object = value
            .as_object_mut()
            .expect("user fixture should be an object");
        let event_type = object
            .remove("event_type")
            .expect("user fixture should contain event_type");
        object.insert("event_type".to_string(), event_type);
        let text = serde_json::to_string(&value).expect("user fixture should serialize");

        assert!(!text.starts_with(r#"{"event_type":"#));
        assert_eq!(
            UserWsMessage::parse(&text).expect("reordered user fixture should parse"),
            expected
        );
    }

    #[rstest]
    fn test_user_ws_message_parse_rejects_duplicate_event_type() {
        let text = load_text("ws_user_order_msg.json").replacen(
            r#""event_type": "order""#,
            r#""event_type": "order", "event_type": "order""#,
            1,
        );
        let expected = serde_json::from_str::<UserWsMessage>(&text)
            .expect_err("derived parser should reject a duplicate event_type");
        let actual = UserWsMessage::parse(&text)
            .expect_err("optimized parser should reject a duplicate event_type");

        assert_eq!(actual.to_string(), expected.to_string());
    }

    #[rstest]
    fn test_user_ws_message_parse_batch() {
        let text = load_text("ws_user_batch_msg.json");
        let expected: Vec<UserWsMessage> =
            serde_json::from_str(&text).expect("user batch fixture should deserialize");

        let actual = UserWsMessage::parse_batch(&text).expect("user batch fixture should parse");

        assert_eq!(actual, expected);
    }

    #[rstest]
    fn test_user_ws_message_parse_batch_with_reordered_event_type() {
        let expected: Vec<UserWsMessage> = load("ws_user_batch_msg.json");
        let mut value: serde_json::Value = load("ws_user_batch_msg.json");
        let first = value
            .as_array_mut()
            .expect("user batch fixture should be an array")[0]
            .as_object_mut()
            .expect("user batch element should be an object");
        let event_type = first
            .remove("event_type")
            .expect("user batch element should contain event_type");
        first.insert("event_type".to_string(), event_type);
        let text = serde_json::to_string(&value).expect("user batch fixture should serialize");

        assert_eq!(
            UserWsMessage::parse_batch(&text).expect("reordered user batch should parse"),
            expected
        );
    }

    #[rstest]
    fn test_user_ws_message_parse_batch_rejects_invalid_element() {
        let mut value: serde_json::Value = load("ws_user_batch_msg.json");
        value
            .as_array_mut()
            .expect("user batch fixture should be an array")[1]
            .as_object_mut()
            .expect("user batch element should be an object")
            .remove("event_type");
        let text = serde_json::to_string(&value).expect("user batch fixture should serialize");

        assert!(UserWsMessage::parse_batch(&text).is_err());
    }

    /// A duplicated `event_type` is malformed and must be rejected, matching
    /// [`UserWsMessage::parse`].
    #[rstest]
    fn test_user_ws_message_parse_batch_rejects_duplicate_event_type() {
        let element = load_text("ws_user_order_msg.json").replacen(
            r#""event_type": "order""#,
            r#""event_type": "order", "event_type": "order""#,
            1,
        );
        let text = format!("[{element}]");

        assert!(UserWsMessage::parse_batch(&text).is_err());
    }

    /// Polymarket emits undocumented user-channel events such as `auto_redeem`, which must not
    /// discard the `order` and `trade` messages batched alongside them.
    #[rstest]
    fn test_user_ws_message_parse_batch_skips_unknown_event_type() {
        let expected: Vec<UserWsMessage> = load("ws_user_batch_msg.json");
        let mut value: serde_json::Value = load("ws_user_batch_msg.json");
        let elements = value
            .as_array_mut()
            .expect("user batch fixture should be an array");
        elements.insert(1, auto_redeem_element());
        let text = serde_json::to_string(&value).expect("user batch fixture should serialize");

        let actual =
            UserWsMessage::parse_batch(&text).expect("batch with unknown event_type should parse");

        assert_eq!(actual, expected);
    }

    #[rstest]
    fn test_user_ws_message_parse_batch_all_unknown_event_types() {
        let text = serde_json::to_string(&serde_json::Value::Array(vec![
            auto_redeem_element(),
            auto_redeem_element(),
        ]))
        .expect("unknown batch should serialize");

        let actual =
            UserWsMessage::parse_batch(&text).expect("batch of unknown event types should parse");

        assert!(actual.is_empty());
    }

    #[rstest]
    fn test_market_ws_message_new_market() {
        let msg: MarketWsMessage = load("ws_market_new_market_msg.json");
        let raw: serde_json::Value = load("ws_market_new_market_msg.json");

        let MarketWsMessage::NewMarket(nm) = msg else {
            panic!("expected new market message");
        };
        assert_eq!(nm.id, "market-001");
        assert_eq!(
            nm.question,
            "Map 1 Rounds Handicap: Sangal (-6.5) vs zeste (+6.5)"
        );
        assert_eq!(
            nm.market.as_str(),
            "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        );
        assert_eq!(nm.slug, "sanitized-new-market");
        assert_eq!(nm.description, raw["description"].as_str().unwrap());
        assert_eq!(
            nm.assets_ids,
            vec![
                "10000000000000000000000000000000000000000000000000000000000000000000000000001",
                "10000000000000000000000000000000000000000000000000000000000000000000000000002",
            ]
        );
        assert_eq!(nm.outcomes, vec!["Sangal", "zeste"]);
        assert_eq!(nm.timestamp, "1786179115414");
        assert!(nm.tags.is_empty());
        assert_eq!(
            nm.condition_id,
            "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        );
        assert!(!nm.active);
        assert_eq!(nm.clob_token_ids, nm.assets_ids);
        assert_eq!(nm.order_price_min_tick_size.as_deref(), Some("0.01"));
        assert_eq!(
            nm.group_item_title.as_deref(),
            Some("Map 1 Rounds Handicap: Sangal (-6.5) vs zeste (+6.5)")
        );
        assert_eq!(
            nm.sports_market_type.as_deref(),
            Some("round_handicap_game_1")
        );
        assert_eq!(nm.line.as_deref(), Some("-6.5"));
        assert_eq!(
            nm.game_start_time.as_deref(),
            Some("2026-08-08 09:00:00+00")
        );
        assert_eq!(nm.taker_base_fee, Some(Decimal::from(1000)));
        assert_eq!(nm.fees_enabled, Some(true));
        let schedule = nm.fee_schedule.as_ref().expect("captured fee schedule");
        assert_eq!(schedule.exponent, Decimal::ONE);
        assert_eq!(schedule.rate, Decimal::new(5, 2));
        assert!(schedule.taker_only);
        assert_eq!(schedule.rebate_rate, Decimal::new(15, 2));
        let event = nm.event_message.as_ref().expect("captured event metadata");
        assert_eq!(event.id, "event-001");
        assert_eq!(event.ticker, "sanitized-event");
        assert_eq!(event.slug, "sanitized-event");
        assert_eq!(
            event.title,
            "Counter-Strike: Sangal vs zeste (BO3) - Esports World Cup Open Qualifier Group 16"
        );
        assert_eq!(
            event.description,
            raw["event_message"]["description"].as_str().unwrap()
        );
    }

    #[rstest]
    fn test_market_ws_message_resolved() {
        let msg: MarketWsMessage = load("ws_market_resolved_msg.json");

        assert!(matches!(msg, MarketWsMessage::MarketResolved(_)));
        if let MarketWsMessage::MarketResolved(mr) = msg {
            assert_eq!(mr.id, "1031769");
            assert_eq!(mr.winning_outcome, "Yes");
            assert_eq!(mr.assets_ids.len(), 2);
            assert_eq!(
                mr.winning_asset_id,
                "76043073756653678226373981964075571318267289248134717369284518995922789326425"
            );
        }
    }

    #[rstest]
    fn test_market_ws_message_best_bid_ask() {
        let msg: MarketWsMessage = load("ws_market_best_bid_ask_msg.json");

        assert!(matches!(msg, MarketWsMessage::BestBidAsk(_)));
        if let MarketWsMessage::BestBidAsk(bba) = msg {
            assert_eq!(bba.best_bid, "0.73");
            assert_eq!(bba.best_ask, "0.77");
            assert_eq!(bba.spread, "0.04");
        }
    }
}
