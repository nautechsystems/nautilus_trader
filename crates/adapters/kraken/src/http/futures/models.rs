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

//! Data models for Kraken Futures HTTP API responses.

use ahash::AHashMap;
use nautilus_core::{UnixNanos, datetime::unix_nanos_to_iso8601_millis};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    common::{
        enums::{
            KrakenApiResult, KrakenFillType, KrakenFuturesHistoryDirection,
            KrakenFuturesHistoryOrderType, KrakenFuturesOrderEventType,
            KrakenFuturesOrderLifecycleStatus, KrakenFuturesOrderStatus, KrakenFuturesOrderType,
            KrakenInstrumentType, KrakenOrderSide, KrakenPositionSide, KrakenSendStatus,
            KrakenTriggerSide, KrakenTriggerSignal,
        },
        serialization::{
            decimal, decimal_map, deserialize_decimal_pair, optional_decimal,
            optional_decimal_or_empty,
        },
    },
    http::error::KrakenHttpError,
};

// Futures Instruments Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesMarginLevel {
    /// Number of contracts (for inverse futures) or notional units (for flexible futures).
    /// The field name varies: `contracts` for inverse, `numNonContractUnits` for flexible.
    #[serde(alias = "numNonContractUnits", default, with = "decimal")]
    pub contracts: Decimal,
    #[serde(with = "decimal")]
    pub initial_margin: Decimal,
    #[serde(with = "decimal")]
    pub maintenance_margin: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesInstrument {
    pub symbol: String,
    #[serde(rename = "type")]
    pub instrument_type: KrakenInstrumentType,
    /// Only present for inverse futures, not for flexible futures.
    #[serde(default)]
    pub underlying: Option<String>,
    #[serde(with = "decimal")]
    pub tick_size: Decimal,
    #[serde(with = "decimal")]
    pub contract_size: Decimal,
    pub tradeable: bool,
    #[serde(default, with = "optional_decimal")]
    pub impact_mid_size: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub max_position_size: Option<Decimal>,
    pub opening_date: String,
    pub margin_levels: Vec<FuturesMarginLevel>,
    #[serde(default)]
    pub funding_rate_coefficient: Option<i32>,
    #[serde(default, with = "optional_decimal")]
    pub max_relative_funding_rate: Option<Decimal>,
    #[serde(default)]
    pub isin: Option<String>,
    pub contract_value_trade_precision: i32,
    pub post_only: bool,
    /// Maker Protection hold window in milliseconds for this market.
    ///
    /// Only present when the venue has Maker Protection configured for the
    /// market; absent means no hold (treat the same as zero).
    #[serde(default)]
    pub maker_protection_millis: Option<i64>,
    #[serde(default)]
    pub fee_schedule_uid: Option<String>,
    pub mtf: bool,
    pub base: String,
    pub quote: String,
    pub pair: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesInstrumentsResponse {
    pub result: KrakenApiResult,
    pub instruments: Vec<FuturesInstrument>,
}

// Futures Ticker Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesTicker {
    pub symbol: String,
    #[serde(default, with = "optional_decimal")]
    pub last: Option<Decimal>,
    #[serde(default)]
    pub last_time: Option<String>,
    pub tag: String,
    pub pair: String,
    #[serde(default, with = "optional_decimal")]
    pub mark_price: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub bid: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub bid_size: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub ask: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub ask_size: Option<Decimal>,
    #[serde(rename = "vol24h", default, with = "optional_decimal")]
    pub vol_24h: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub volume_quote: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub open_interest: Option<Decimal>,
    #[serde(rename = "open24h", default, with = "optional_decimal")]
    pub open_24h: Option<Decimal>,
    #[serde(rename = "high24h", default, with = "optional_decimal")]
    pub high_24h: Option<Decimal>,
    #[serde(rename = "low24h", default, with = "optional_decimal")]
    pub low_24h: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub last_size: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub funding_rate: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub funding_rate_prediction: Option<Decimal>,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default, with = "optional_decimal")]
    pub index_price: Option<Decimal>,
    #[serde(default)]
    pub post_only: bool,
    #[serde(rename = "change24h", default, with = "optional_decimal")]
    pub change_24h: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesTickersResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    pub tickers: Vec<FuturesTicker>,
}

// Futures Order Book Models

/// A `[price, qty]` pair from the Kraken Futures orderbook endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct FuturesOrderBookLevel {
    #[serde(serialize_with = "decimal::serialize")]
    pub price: Decimal,
    #[serde(serialize_with = "decimal::serialize")]
    pub qty: Decimal,
}

impl<'de> serde::Deserialize<'de> for FuturesOrderBookLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let arr = deserialize_decimal_pair(deserializer)?;
        Ok(Self {
            price: arr.0,
            qty: arr.1,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderBookData {
    pub bids: Vec<FuturesOrderBookLevel>,
    pub asks: Vec<FuturesOrderBookLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderBookResponse {
    pub result: KrakenApiResult,
    pub order_book: FuturesOrderBookData,
    #[serde(default)]
    pub server_time: Option<String>,
}

// Futures Historical Funding Rates Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesHistoricalFundingRate {
    pub timestamp: String,
    #[serde(with = "decimal")]
    pub relative_funding_rate: Decimal,
    #[serde(with = "decimal")]
    pub funding_rate: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesHistoricalFundingRatesResponse {
    pub result: KrakenApiResult,
    pub rates: Vec<FuturesHistoricalFundingRate>,
}

// Futures OHLC (Candles) Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesCandle {
    pub time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesCandlesResponse {
    pub candles: Vec<FuturesCandle>,
}

// Futures Open Orders Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOpenOrder {
    #[serde(rename = "order_id")]
    pub order_id: String,
    pub symbol: String,
    pub side: KrakenOrderSide,
    pub order_type: KrakenFuturesOrderType,
    #[serde(default, with = "optional_decimal")]
    pub limit_price: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub stop_price: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub unfilled_size: Option<Decimal>,
    pub received_time: String,
    pub status: KrakenFuturesOrderStatus,
    #[serde(with = "decimal")]
    pub filled_size: Decimal,
    #[serde(default)]
    pub reduce_only: Option<bool>,
    pub last_update_time: String,
    #[serde(default)]
    pub trigger_signal: Option<KrakenTriggerSignal>,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOpenOrdersResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub open_orders: Vec<FuturesOpenOrder>,
}

// Futures Orders Status Models (/orders/status)

/// Order body returned by the `/orders/status` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCachedOrder {
    pub order_id: String,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    pub symbol: String,
    pub side: KrakenOrderSide,
    #[serde(default, with = "optional_decimal")]
    pub quantity: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub filled: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub limit_price: Option<Decimal>,
    #[serde(default)]
    pub reduce_only: bool,
    pub timestamp: String,
    pub last_update_timestamp: String,
}

/// A single order status entry returned by the `/orders/status` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderStatusDetails {
    pub order: FuturesCachedOrder,
    pub status: KrakenFuturesOrderLifecycleStatus,
    #[serde(default)]
    pub update_reason: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Response from the Kraken Futures `/orders/status` endpoint, which reports
/// orders open or with a fill/cancel event in the last 5 seconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrdersStatusResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub orders: Vec<FuturesOrderStatusDetails>,
}

// Futures Order Events Models

/// Wrapper for an order event containing the order data and event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderEventWrapper {
    pub order: FuturesOrderEvent,
    #[serde(rename = "type")]
    pub event_type: KrakenFuturesOrderEventType,
    #[serde(default, with = "optional_decimal")]
    pub reduced_quantity: Option<Decimal>,
}

/// The actual order data within an order event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderEvent {
    pub order_id: String,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "type")]
    pub order_type: KrakenFuturesOrderType,
    pub symbol: String,
    pub side: KrakenOrderSide,
    #[serde(with = "decimal")]
    pub quantity: Decimal,
    #[serde(with = "decimal")]
    pub filled: Decimal,
    #[serde(default, with = "optional_decimal")]
    pub limit_price: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub stop_price: Option<Decimal>,
    pub timestamp: String,
    pub last_update_timestamp: String,
    #[serde(default)]
    pub reduce_only: bool,
}

/// Order events in the shape the readers consume, built from [`FuturesOrderHistoryResponse`].
///
/// This is a projection, not a wire type: a page is decoded as [`FuturesOrderHistoryResponse`]
/// and converted, so a body of another shape cannot read as an empty page.
#[derive(Debug, Clone)]
pub struct FuturesOrderEventsResponse {
    pub server_time: Option<String>,
    pub order_events: Vec<FuturesOrderEventWrapper>,
    pub continuation_token: Option<String>,
    /// Rows the projection could not represent, which leave the set incomplete.
    pub skipped_rows: usize,
}

// Futures Order History Models

/// Response from the Kraken Futures order history endpoint, `/api/history/v3/orders`.
///
/// Each element carries one lifecycle event keyed by its kind, and the order inside it names the
/// contract as `tradeable` and the side as `direction`, with millisecond timestamps. The venue
/// can answer with a success status and an error body, so `result` and `error` are read before
/// the shape is checked; see [`Self::into_order_events`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderHistoryResponse {
    #[serde(default)]
    pub result: Option<KrakenApiResult>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub server_time: Option<String>,
    #[serde(default)]
    pub elements: Option<Vec<FuturesOrderHistoryElement>>,
    #[serde(default)]
    pub continuation_token: Option<String>,
}

/// One order history element: the event and when it happened.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderHistoryElement {
    pub uid: String,
    pub timestamp: i64,
    pub event: FuturesOrderHistoryEvent,
}

/// An order history event, externally tagged by its kind.
///
/// The documented kinds are modeled; one the documentation does not list is kept by name so the
/// surrounding page still decodes.
#[derive(Debug, Clone)]
pub enum FuturesOrderHistoryEvent {
    OrderPlaced(FuturesOrderHistoryOrderEvent),
    OrderUpdated(FuturesOrderHistoryOrderUpdate),
    OrderCancelled(FuturesOrderHistoryOrderEvent),
    OrderRejected(FuturesOrderHistoryOrderEvent),
    /// An edit the venue refused; the order stands as `old_order`.
    OrderEditRejected(FuturesOrderHistoryEditRejected),
    /// A request naming an order the venue does not hold; it carries no order state.
    OrderNotFound(FuturesOrderHistoryOrderNotFound),
    Unknown(String),
}

impl<'de> Deserialize<'de> for FuturesOrderHistoryEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct EventVisitor;

        impl<'de> serde::de::Visitor<'de> for EventVisitor {
            type Value = FuturesOrderHistoryEvent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an order history event keyed by its kind")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                // The kind is the one key naming a documented event, wherever it sits among any
                // other keys; the first key is reported when no key does.
                let mut event = None;
                let mut first_key = None;

                while let Some(key) = map.next_key::<String>()? {
                    let known = match key.as_str() {
                        "OrderPlaced" => {
                            Some(FuturesOrderHistoryEvent::OrderPlaced(map.next_value()?))
                        }
                        "OrderUpdated" => {
                            Some(FuturesOrderHistoryEvent::OrderUpdated(map.next_value()?))
                        }
                        "OrderCancelled" => {
                            Some(FuturesOrderHistoryEvent::OrderCancelled(map.next_value()?))
                        }
                        "OrderRejected" => {
                            Some(FuturesOrderHistoryEvent::OrderRejected(map.next_value()?))
                        }
                        "OrderEditRejected" => Some(FuturesOrderHistoryEvent::OrderEditRejected(
                            map.next_value()?,
                        )),
                        "OrderNotFound" => {
                            Some(FuturesOrderHistoryEvent::OrderNotFound(map.next_value()?))
                        }
                        _ => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                            None
                        }
                    };

                    if event.is_none() && known.is_some() {
                        event = known;
                    }

                    if first_key.is_none() {
                        first_key = Some(key);
                    }
                }

                Ok(event.unwrap_or_else(|| {
                    FuturesOrderHistoryEvent::Unknown(first_key.unwrap_or_default())
                }))
            }
        }

        deserializer.deserialize_map(EventVisitor)
    }
}

/// An event that carries one order: placed, canceled or rejected.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderHistoryOrderEvent {
    pub order: FuturesOrderHistoryOrder,
}

/// An update event, carrying the order before and after the edit.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderHistoryOrderUpdate {
    pub new_order: FuturesOrderHistoryOrder,
}

/// A refused edit, carrying the order as it stands.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderHistoryEditRejected {
    pub old_order: FuturesOrderHistoryOrder,
}

/// A request for an order the venue does not hold.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderHistoryOrderNotFound {
    #[serde(default)]
    pub order_id: Option<String>,
}

/// An order as the history endpoint reports it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderHistoryOrder {
    pub uid: String,
    pub tradeable: String,
    pub direction: KrakenFuturesHistoryDirection,
    #[serde(with = "decimal")]
    pub quantity: Decimal,
    #[serde(with = "decimal")]
    pub filled: Decimal,
    /// Absent for a market order, which the venue reports as zero or as an empty string.
    #[serde(default, deserialize_with = "optional_decimal_or_empty::deserialize")]
    pub limit_price: Option<Decimal>,
    #[serde(default)]
    pub order_type: Option<KrakenFuturesHistoryOrderType>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub reduce_only: bool,
    pub timestamp: i64,
    pub last_update_timestamp: i64,
}

fn millis_to_rfc3339(millis: i64) -> Option<String> {
    let nanos = u64::try_from(millis).ok()?.checked_mul(1_000_000)?;
    Some(unix_nanos_to_iso8601_millis(UnixNanos::from(nanos)))
}

impl FuturesOrderHistoryResponse {
    /// Projects each element onto the event wrapper the readers consume.
    ///
    /// A venue error reported with a success status fails the read with the venue's reason, and a
    /// body without an `elements` array fails it as a parse error, so neither reads as an empty
    /// page. An update, and a refused edit, are represented by the order as it stands afterwards.
    /// A not-found event carries no order state and is skipped. A kind the documentation does not
    /// list, an order whose direction the venue could not decode, an order whose type is missing
    /// or reported as `Unknown`, and an order with a timestamp before the epoch may carry order
    /// state the projection cannot read, so each is skipped with a warning and counted in
    /// `skipped_rows`, which leaves the set incomplete. `Unknown` is how the venue reports a
    /// source type it could not decode, so it establishes no order type, and in particular not a
    /// market order. The venue fills the required `limitPrice` of a market order with a zero or
    /// an empty string and a missing client id with an empty string, so a market-like order (a
    /// market order or a venue-initiated kind) carries no price and an empty client id is
    /// `None`; a limit order keeps its price even at zero, since futures instruments allow
    /// non-positive prices. The contract name is kept as the venue spells it;
    /// the readers resolve it case-insensitively. The history order carries no trigger price, and
    /// the engine cannot materialize a stop order without one, so a stop row is reported as the
    /// limit or market order it executes as once triggered.
    ///
    /// # Errors
    ///
    /// Returns an error when the body carries a venue error or has no `elements` array.
    pub fn into_order_events(self) -> Result<FuturesOrderEventsResponse, KrakenHttpError> {
        if let Some(error) = self.error {
            return Err(KrakenHttpError::ApiError(vec![error]));
        }

        if self.result == Some(KrakenApiResult::Error) {
            return Err(KrakenHttpError::ApiError(vec![
                "order history request failed without a reason".to_string(),
            ]));
        }

        let Some(elements) = self.elements else {
            return Err(KrakenHttpError::ParseError(
                "order history page has no `elements` array".to_string(),
            ));
        };

        let mut skipped_rows = 0usize;
        let order_events = elements
            .into_iter()
            .filter_map(|element| {
                let (order, event_type) = match element.event {
                    FuturesOrderHistoryEvent::OrderPlaced(event) => {
                        (event.order, KrakenFuturesOrderEventType::Place)
                    }
                    FuturesOrderHistoryEvent::OrderUpdated(event) => {
                        (event.new_order, KrakenFuturesOrderEventType::Edit)
                    }
                    FuturesOrderHistoryEvent::OrderCancelled(event) => {
                        (event.order, KrakenFuturesOrderEventType::Cancel)
                    }
                    FuturesOrderHistoryEvent::OrderRejected(event) => {
                        (event.order, KrakenFuturesOrderEventType::Reject)
                    }
                    FuturesOrderHistoryEvent::OrderEditRejected(event) => {
                        (event.old_order, KrakenFuturesOrderEventType::Edit)
                    }
                    FuturesOrderHistoryEvent::OrderNotFound(event) => {
                        log::warn!(
                            "Skipping order history event {} (order not found {:?}): no order state",
                            element.uid,
                            event.order_id
                        );
                        return None;
                    }
                    FuturesOrderHistoryEvent::Unknown(kind) => {
                        log::warn!(
                            "Skipping order history event {} of undocumented kind {kind:?}; the set is incomplete",
                            element.uid
                        );
                        skipped_rows += 1;
                        return None;
                    }
                };

                // The event time is when this state became known; a refused edit, for one,
                // leaves the order's own stamp at its last change.
                let (Some(timestamp), Some(last_update_timestamp)) = (
                    millis_to_rfc3339(order.timestamp),
                    millis_to_rfc3339(order.last_update_timestamp.max(element.timestamp)),
                ) else {
                    log::warn!(
                        "Skipping order history event {} for order {} on {}: timestamp before the epoch; the set is incomplete",
                        element.uid,
                        order.uid,
                        order.tradeable
                    );
                    skipped_rows += 1;
                    return None;
                };

                let side = match order.direction {
                    KrakenFuturesHistoryDirection::Buy => KrakenOrderSide::Buy,
                    KrakenFuturesHistoryDirection::Sell => KrakenOrderSide::Sell,
                    KrakenFuturesHistoryDirection::Unknown => {
                        log::warn!(
                            "Skipping order history event {} for order {} on {}: direction unknown; the set is incomplete",
                            element.uid,
                            order.uid,
                            order.tradeable
                        );
                        skipped_rows += 1;
                        return None;
                    }
                };

                // `Unknown` is how the venue reports a source type it could not decode, so
                // neither it nor a missing type establishes what the order is.
                let kind = match order.order_type {
                    Some(KrakenFuturesHistoryOrderType::Unknown) | None => {
                        let situation = if order.order_type.is_none() {
                            "order type missing"
                        } else {
                            "order type reported as Unknown"
                        };
                        log::warn!(
                            "Skipping order history event {} for order {} (client id {:?}) on {}: {situation}; the set is incomplete",
                            element.uid,
                            order.uid,
                            order.client_id.as_deref().filter(|id| !id.is_empty()),
                            order.tradeable
                        );
                        skipped_rows += 1;
                        return None;
                    }
                    Some(kind) => kind,
                };

                let market_like = matches!(
                    kind,
                    KrakenFuturesHistoryOrderType::Market
                        | KrakenFuturesHistoryOrderType::Liquidation
                        | KrakenFuturesHistoryOrderType::PartialLiquidation
                        | KrakenFuturesHistoryOrderType::CoveredLiquidation
                        | KrakenFuturesHistoryOrderType::Assignment
                        | KrakenFuturesHistoryOrderType::HedgeAssignment
                        | KrakenFuturesHistoryOrderType::Unwind
                        | KrakenFuturesHistoryOrderType::Block
                        | KrakenFuturesHistoryOrderType::Rfq
                );
                let limit_price = match kind {
                    _ if market_like => None,
                    // A stop with the placeholder price has no limit leg.
                    KrakenFuturesHistoryOrderType::Stop => {
                        order.limit_price.filter(|price| !price.is_zero())
                    }
                    _ => order.limit_price,
                };
                let order_type = match (kind, limit_price) {
                    (KrakenFuturesHistoryOrderType::Stop, Some(_)) => KrakenFuturesOrderType::Limit,
                    (KrakenFuturesHistoryOrderType::Stop, None) => KrakenFuturesOrderType::Market,
                    (kind, _) => kind.into(),
                };

                Some(FuturesOrderEventWrapper {
                    order: FuturesOrderEvent {
                        order_id: order.uid,
                        cli_ord_id: order.client_id.filter(|id| !id.is_empty()),
                        order_type,
                        symbol: order.tradeable,
                        side,
                        quantity: order.quantity,
                        filled: order.filled,
                        limit_price,
                        stop_price: None,
                        timestamp,
                        last_update_timestamp,
                        reduce_only: order.reduce_only,
                    },
                    event_type,
                    reduced_quantity: None,
                })
            })
            .collect();

        Ok(FuturesOrderEventsResponse {
            server_time: self.server_time,
            order_events,
            continuation_token: self.continuation_token,
            skipped_rows,
        })
    }
}

// Futures Fills Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesFill {
    #[serde(rename = "fill_id")]
    pub fill_id: String,
    pub symbol: String,
    pub side: KrakenOrderSide,
    #[serde(rename = "order_id")]
    pub order_id: String,
    pub fill_time: String,
    #[serde(with = "decimal")]
    pub size: Decimal,
    #[serde(with = "decimal")]
    pub price: Decimal,
    pub fill_type: KrakenFillType,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "fee_paid", default, with = "optional_decimal")]
    pub fee_paid: Option<Decimal>,
    #[serde(rename = "fee_currency", default)]
    pub fee_currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesFillsResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub fills: Vec<FuturesFill>,
}

// Futures Positions Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPosition {
    pub side: KrakenPositionSide,
    pub symbol: String,
    #[serde(with = "decimal")]
    pub price: Decimal,
    pub fill_time: String,
    #[serde(with = "decimal")]
    pub size: Decimal,
    #[serde(default, with = "optional_decimal")]
    pub unrealized_funding: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOpenPositionsResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub open_positions: Vec<FuturesPosition>,
}

// Futures Order Execution Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesSendOrderResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    pub send_status: Option<FuturesSendStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesSendStatus {
    #[serde(rename = "order_id", default)]
    pub order_id: Option<String>,
    #[serde(rename = "order_tag", default)]
    pub order_tag: Option<String>,
    pub status: String,
    #[serde(default)]
    pub order_events: Option<Vec<FuturesSendOrderEvent>>,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "receivedTime", default)]
    pub received_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesSendOrderEvent {
    #[serde(rename = "type")]
    pub event_type: KrakenFuturesOrderEventType,
    #[serde(default)]
    pub order: Option<FuturesOrderEventData>,
    #[serde(default)]
    pub order_trigger: Option<FuturesOrderTriggerData>,
    #[serde(default, with = "optional_decimal")]
    pub reduced_quantity: Option<Decimal>,
    // Execution event fields
    #[serde(rename = "executionId", default)]
    pub execution_id: Option<String>,
    #[serde(default, with = "optional_decimal")]
    pub price: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub amount: Option<Decimal>,
    #[serde(rename = "orderPriorEdit", default)]
    pub order_prior_edit: Option<Box<FuturesOrderEventData>>,
    #[serde(rename = "orderPriorExecution", default)]
    pub order_prior_execution: Option<Box<FuturesOrderEventData>>,
    #[serde(rename = "takerReducedQuantity", default, with = "optional_decimal")]
    pub taker_reduced_quantity: Option<Decimal>,
    // Reject event fields
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub uid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderEventData {
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "cliOrdId", default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "type")]
    pub order_type: KrakenFuturesOrderType,
    pub symbol: String,
    pub side: KrakenOrderSide,
    #[serde(with = "decimal")]
    pub quantity: Decimal,
    #[serde(with = "decimal")]
    pub filled: Decimal,
    #[serde(rename = "limitPrice", default, with = "optional_decimal")]
    pub limit_price: Option<Decimal>,
    #[serde(rename = "stopPrice", default, with = "optional_decimal")]
    pub stop_price: Option<Decimal>,
    pub timestamp: String,
    #[serde(rename = "lastUpdateTimestamp")]
    pub last_update_timestamp: String,
    #[serde(rename = "reduceOnly", default)]
    pub reduce_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderTriggerData {
    pub uid: String,
    #[serde(rename = "clientId", default)]
    pub client_id: Option<String>,
    #[serde(rename = "type")]
    pub order_type: KrakenFuturesOrderType,
    pub symbol: String,
    pub side: KrakenOrderSide,
    #[serde(with = "decimal")]
    pub quantity: Decimal,
    #[serde(rename = "limitPrice", default, with = "optional_decimal")]
    pub limit_price: Option<Decimal>,
    #[serde(rename = "limitPriceOffsetValue", default, with = "optional_decimal")]
    pub limit_price_offset_value: Option<Decimal>,
    #[serde(rename = "limitPriceOffsetUnit", default)]
    pub limit_price_offset_unit: Option<String>,
    #[serde(rename = "triggerPrice")]
    #[serde(with = "decimal")]
    pub trigger_price: Decimal,
    #[serde(rename = "triggerSide")]
    pub trigger_side: KrakenTriggerSide,
    #[serde(rename = "triggerSignal")]
    pub trigger_signal: KrakenTriggerSignal,
    #[serde(rename = "reduceOnly", default)]
    pub reduce_only: bool,
    pub timestamp: String,
    #[serde(rename = "lastUpdateTimestamp")]
    pub last_update_timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelOrderResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    pub cancel_status: FuturesCancelStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelStatus {
    pub status: KrakenSendStatus,
    #[serde(rename = "order_id", default)]
    pub order_id: Option<String>,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesEditOrderResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    pub edit_status: FuturesEditStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesEditStatus {
    pub status: String,
    #[serde(rename = "order_id", default)]
    pub order_id: Option<String>,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesBatchOrderResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    pub batch_status: Vec<FuturesSendStatus>,
}

/// Response for batch cancel operations via `/derivatives/api/v3/batchorder`.
///
/// When sending only cancel operations, the response has a different format
/// with individual cancel status items.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesBatchCancelResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub batch_status: Vec<FuturesBatchCancelStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesBatchCancelStatus {
    #[serde(default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    #[serde(default)]
    pub status: Option<KrakenSendStatus>,
    #[serde(default)]
    pub cancel_status: Option<FuturesCancelStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelAllOrdersResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    pub cancel_status: FuturesCancelAllStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelAllStatus {
    pub status: KrakenSendStatus,
    #[serde(default)]
    pub cancelled_orders: Vec<CancelledOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelledOrder {
    #[serde(rename = "order_id", default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
}

// Futures Public Executions Models

/// Response from the Kraken Futures public executions endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPublicExecutionsResponse {
    pub elements: Vec<FuturesPublicExecutionElement>,
    #[serde(default)]
    pub len: Option<i64>,
    #[serde(default)]
    pub continuation_token: Option<String>,
}

/// A single execution element from the public executions response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesPublicExecutionElement {
    pub uid: String,
    pub timestamp: i64,
    pub event: FuturesPublicExecutionEvent,
}

/// The event wrapper containing the execution details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesPublicExecutionEvent {
    #[serde(rename = "Execution")]
    pub execution: FuturesPublicExecutionWrapper,
}

/// Wrapper containing the actual execution data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPublicExecutionWrapper {
    pub execution: FuturesPublicExecution,
    #[serde(default)]
    pub taker_reduced_quantity: Option<String>,
}

/// The actual execution/trade data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPublicExecution {
    pub uid: String,
    pub maker_order: FuturesPublicOrder,
    pub taker_order: FuturesPublicOrder,
    pub timestamp: i64,
    pub quantity: String,
    pub price: String,
    #[serde(default)]
    pub mark_price: Option<String>,
    #[serde(default)]
    pub limit_filled: Option<bool>,
    #[serde(default)]
    pub usd_value: Option<String>,
}

/// Order information within an execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPublicOrder {
    pub uid: String,
    pub tradeable: String,
    pub direction: String,
    pub quantity: String,
    pub timestamp: i64,
    #[serde(default)]
    pub limit_price: Option<String>,
    #[serde(default)]
    pub order_type: Option<String>,
    #[serde(default)]
    pub reduce_only: Option<bool>,
    #[serde(default)]
    pub last_update_timestamp: Option<i64>,
}

// Futures Accounts Models

/// Response from the Kraken Futures accounts endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesAccountsResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub accounts: AHashMap<String, FuturesAccount>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub server_time: Option<String>,
}

/// Kraken Futures account type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KrakenFuturesAccountType {
    /// Multi-collateral margin account (flex).
    MultiCollateralMarginAccount,
    /// Single-collateral margin account.
    MarginAccount,
    /// Cash account (no margin).
    CashAccount,
    /// Unknown account type.
    #[serde(other)]
    Unknown,
}

/// A Kraken Futures account (margin or multi-collateral).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesAccount {
    #[serde(rename = "type")]
    pub account_type: KrakenFuturesAccountType,
    /// Currency of a single-collateral margin account; its `auxiliary` and `marginRequirements`
    /// figures are in this currency. Absent on cash and flex accounts.
    #[serde(default)]
    pub currency: Option<String>,
    /// Balances for margin accounts (symbol -> amount).
    #[serde(default, with = "decimal_map")]
    pub balances: AHashMap<String, Decimal>,
    /// Currencies for flex/multi-collateral accounts.
    #[serde(default)]
    pub currencies: AHashMap<String, FuturesFlexCurrency>,
    /// Auxiliary info for margin accounts.
    #[serde(default)]
    pub auxiliary: Option<FuturesAuxiliary>,
    /// Margin requirements.
    #[serde(default)]
    pub margin_requirements: Option<FuturesMarginRequirements>,
    /// Portfolio value (for flex accounts).
    #[serde(default, with = "optional_decimal")]
    pub portfolio_value: Option<Decimal>,
    /// Available margin (for flex accounts).
    #[serde(default, with = "optional_decimal")]
    pub available_margin: Option<Decimal>,
    /// Initial margin (for flex accounts).
    #[serde(default, with = "optional_decimal")]
    pub initial_margin: Option<Decimal>,
    /// Total maintenance margin held for open positions (for flex accounts, in USD).
    #[serde(default, with = "optional_decimal")]
    pub maintenance_margin: Option<Decimal>,
    /// PnL (for flex accounts).
    #[serde(default, with = "optional_decimal")]
    pub pnl: Option<Decimal>,
}

/// Currency info for flex/multi-collateral accounts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesFlexCurrency {
    #[serde(with = "decimal")]
    pub quantity: Decimal,
    #[serde(default, with = "optional_decimal")]
    pub value: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub collateral: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub available: Option<Decimal>,
}

/// Auxiliary account info for margin accounts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesAuxiliary {
    #[serde(default, with = "optional_decimal")]
    pub usd: Option<Decimal>,
    /// Portfolio value.
    #[serde(default, with = "optional_decimal")]
    pub pv: Option<Decimal>,
    /// Profit/loss.
    #[serde(default, with = "optional_decimal")]
    pub pnl: Option<Decimal>,
    /// Available funds.
    #[serde(default, with = "optional_decimal")]
    pub af: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub funding: Option<Decimal>,
}

/// Margin requirements for an account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesMarginRequirements {
    /// Initial margin.
    #[serde(default, with = "optional_decimal")]
    pub im: Option<Decimal>,
    /// Maintenance margin.
    #[serde(default, with = "optional_decimal")]
    pub mm: Option<Decimal>,
    /// Liquidation threshold.
    #[serde(default, with = "optional_decimal")]
    pub lt: Option<Decimal>,
    /// Termination threshold.
    #[serde(default, with = "optional_decimal")]
    pub tt: Option<Decimal>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn load_test_data(filename: &str) -> String {
        let path = format!("test_data/{filename}");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to load test data from {path}: {e}"))
    }

    #[rstest]
    fn test_parse_futures_cancel_all_orders_with_no_orders_to_cancel_status() {
        // Regression for the venue response shape that broke parsing in production:
        // the `cancelStatus.status` field is `noOrdersToCancel` even when one or more
        // orders were canceled in the same call. The `cancelledOrders` array carries
        // the actual canceled order ids, so the parser must accept this status.
        let raw = r#"{
            "result": "success",
            "cancelStatus": {
                "receivedTime": "2026-04-10T13:17:23.291Z",
                "cancelOnly": "PF_XBTUSD",
                "status": "noOrdersToCancel",
                "cancelledOrders": [
                    {
                        "order_id": "a182b1c0-cd01-4d1c-853b-605e936f412b",
                        "cliOrdId": "5f173994-f660-4809-b97a-586221fe5926"
                    }
                ],
                "orderEvents": []
            },
            "serverTime": "2026-04-10T13:17:23.291Z"
        }"#;

        let response: FuturesCancelAllOrdersResponse =
            serde_json::from_str(raw).expect("Failed to parse cancel-all response");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(
            response.cancel_status.status,
            KrakenSendStatus::NoOrdersToCancel
        );
        assert_eq!(response.cancel_status.cancelled_orders.len(), 1);
        assert_eq!(
            response.cancel_status.cancelled_orders[0]
                .order_id
                .as_deref(),
            Some("a182b1c0-cd01-4d1c-853b-605e936f412b")
        );
        assert_eq!(
            response.cancel_status.cancelled_orders[0]
                .cli_ord_id
                .as_deref(),
            Some("5f173994-f660-4809-b97a-586221fe5926")
        );
    }

    #[rstest]
    fn test_parse_futures_open_orders() {
        let data = load_test_data("http_futures_open_orders.json");
        let response: FuturesOpenOrdersResponse =
            serde_json::from_str(&data).expect("Failed to parse futures open orders");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.open_orders.len(), 3);

        let order = &response.open_orders[0];
        assert_eq!(order.order_id, "2ce038ae-c144-4de7-a0f1-82f7f4fca864");
        assert_eq!(order.symbol, "PI_ETHUSD");
        assert_eq!(order.side, KrakenOrderSide::Buy);
        assert_eq!(order.order_type, KrakenFuturesOrderType::Limit);
        assert_eq!(order.limit_price, Some(dec!(1200)));
        assert_eq!(order.unfilled_size, Some(dec!(100)));
        assert_eq!(order.filled_size, dec!(0));

        let trigger_order = &response.open_orders[1];
        assert_eq!(
            trigger_order.order_id,
            "c8135f52-2a86-4e26-b629-43cc37da9dbf"
        );
        assert_eq!(trigger_order.order_type, KrakenFuturesOrderType::TakeProfit);
        assert_eq!(trigger_order.symbol, "PI_XBTUSD");
        assert_eq!(trigger_order.side, KrakenOrderSide::Buy);
        assert_eq!(trigger_order.limit_price, None);
        assert_eq!(trigger_order.stop_price, Some(dec!(1880.4)));
        assert_eq!(trigger_order.unfilled_size, None);
        assert_eq!(trigger_order.received_time, "2023-04-07T15:14:25.995Z");
        assert_eq!(trigger_order.status, KrakenFuturesOrderStatus::Untouched);
        assert_eq!(trigger_order.filled_size, dec!(0));
        assert_eq!(trigger_order.reduce_only, Some(true));
        assert_eq!(trigger_order.last_update_time, "2023-04-07T15:14:25.995Z");
        assert_eq!(
            trigger_order.trigger_signal,
            Some(KrakenTriggerSignal::Last)
        );
        assert_eq!(trigger_order.cli_ord_id, None);
    }

    #[rstest]
    fn test_parse_futures_orders_status() {
        let data = load_test_data("http_futures_orders_status.json");
        let response: FuturesOrdersStatusResponse =
            serde_json::from_str(&data).expect("Failed to parse futures orders status");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.orders.len(), 2);

        let part_filled = &response.orders[0];
        assert_eq!(
            part_filled.order.order_id,
            "5f6d15a5-8c9e-4b0a-9d3f-5a2b7c8d9e0f"
        );
        assert_eq!(
            part_filled.order.cli_ord_id.as_deref(),
            Some("uuid-mp-held-001")
        );
        assert_eq!(part_filled.order.side, KrakenOrderSide::Buy);
        assert_eq!(part_filled.order.quantity, Some(dec!(0.001)));
        assert_eq!(part_filled.order.filled, Some(dec!(0.0004)));
        assert_eq!(part_filled.order.limit_price, Some(dec!(70000)));
        assert!(!part_filled.order.reduce_only);
        assert_eq!(
            part_filled.status,
            KrakenFuturesOrderLifecycleStatus::Cancelled
        );
        assert_eq!(part_filled.update_reason.as_deref(), Some("PARTIAL_FILL"));

        let open = &response.orders[1];
        assert_eq!(open.order.quantity, Some(dec!(0.0002)));
        assert_eq!(open.order.filled, Some(dec!(0)));
        assert!(open.order.reduce_only);
        assert_eq!(open.status, KrakenFuturesOrderLifecycleStatus::EnteredBook);
        assert_eq!(open.update_reason, None);
    }

    #[rstest]
    fn test_parse_futures_instruments_maker_protection() {
        let data = load_test_data("http_futures_instruments_maker_protection.json");
        let response: FuturesInstrumentsResponse =
            serde_json::from_str(&data).expect("Failed to parse futures instruments");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.instruments.len(), 2);

        let protected = &response.instruments[0];
        assert_eq!(protected.symbol, "PF_ATOMUSD");
        assert_eq!(protected.maker_protection_millis, Some(20));

        // The venue omits makerProtectionMillis entirely on unprotected
        // markets; absent must decode the same as no protection configured.
        let unprotected = &response.instruments[1];
        assert_eq!(unprotected.symbol, "PF_ETHUSD");
        assert_eq!(unprotected.maker_protection_millis, None);
    }

    #[rstest]
    fn test_parse_futures_fills() {
        let data = load_test_data("http_futures_fills.json");
        let response: FuturesFillsResponse =
            serde_json::from_str(&data).expect("Failed to parse futures fills");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.fills.len(), 3);

        let fill = &response.fills[0];
        assert_eq!(fill.fill_id, "cad76f07-814e-4dc6-8478-7867407b6bff");
        assert_eq!(fill.symbol, "PI_XBTUSD");
        assert_eq!(fill.side, KrakenOrderSide::Buy);
        assert_eq!(fill.size, dec!(5000));
        assert_eq!(fill.price, dec!(27937.5));
        assert_eq!(fill.fill_type, KrakenFillType::Maker);
        assert_eq!(fill.fee_currency, Some("BTC".to_string()));
        assert_eq!(response.fills[1].fill_type, KrakenFillType::Taker);
        assert_eq!(response.fills[2].fill_type, KrakenFillType::Assignee);
    }

    #[rstest]
    fn test_parse_futures_open_positions() {
        let data = load_test_data("http_futures_open_positions.json");
        let response: FuturesOpenPositionsResponse =
            serde_json::from_str(&data).expect("Failed to parse futures open positions");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.open_positions.len(), 2);

        let position = &response.open_positions[0];
        assert_eq!(position.side, KrakenPositionSide::Short);
        assert_eq!(position.symbol, "PI_XBTUSD");
        assert_eq!(position.size, dec!(8000));
        assert!(position.unrealized_funding.is_some());
    }

    #[rstest]
    fn test_parse_futures_orderbook() {
        let data = load_test_data("http_futures_orderbook.json");
        let response: FuturesOrderBookResponse =
            serde_json::from_str(&data).expect("Failed to parse futures orderbook");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.order_book.bids.len(), 3);
        assert_eq!(response.order_book.asks.len(), 3);

        let best_bid = &response.order_book.bids[0];
        assert_eq!(best_bid.price, dec!(105900));
        assert_eq!(best_bid.qty, dec!(0.5));

        let best_ask = &response.order_book.asks[0];
        assert_eq!(best_ask.price, dec!(105950));
        assert_eq!(best_ask.qty, dec!(0.3));
    }

    #[rstest]
    fn test_parse_futures_historical_funding_rates() {
        let data = load_test_data("http_futures_historical_funding_rates.json");
        let response: FuturesHistoricalFundingRatesResponse =
            serde_json::from_str(&data).expect("Failed to parse historical funding rates");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.rates.len(), 3);

        let rate = &response.rates[0];
        assert_eq!(rate.timestamp, "2025-07-11T08:00:00.000Z");
        assert_eq!(rate.relative_funding_rate, dec!(0.0001));
        assert_eq!(rate.funding_rate, dec!(0.00005));

        let negative_rate = &response.rates[1];
        assert_eq!(negative_rate.relative_funding_rate, dec!(-0.00005));
    }

    #[rstest]
    fn test_parse_futures_orderbook_preserves_decimal_precision() {
        let data = load_test_data("http_futures_orderbook_precision.json");
        let level: FuturesOrderBookLevel = serde_json::from_str(&data).unwrap();

        assert_eq!(level.price, dec!(0.1234567890123456789012345678));
        assert_eq!(level.qty, dec!(123456789.123456789));
    }

    fn order_history_events(fixture: &str) -> FuturesOrderEventsResponse {
        let data = load_test_data(fixture);
        order_history_events_from(&data)
    }

    fn order_history_events_from(data: &str) -> FuturesOrderEventsResponse {
        let response: FuturesOrderHistoryResponse =
            serde_json::from_str(data).expect("Failed to parse futures order history");
        response
            .into_order_events()
            .expect("the page carries elements")
    }

    fn placed_element(order: &str) -> String {
        format!(
            r#"{{"elements":[{{"uid":"e1","timestamp":1680876930250,"event":{{"OrderPlaced":{{"order":{order}}}}}}}]}}"#
        )
    }

    /// Each documented event kind maps onto the event type the readers switch on.
    #[rstest]
    fn test_parse_futures_order_events_uses_enum_event_type() {
        let response = order_history_events("http_futures_order_events.json");

        assert_eq!(response.order_events.len(), 3);
        assert_eq!(
            response.order_events[0].event_type,
            KrakenFuturesOrderEventType::Place
        );
        assert_eq!(
            response.order_events[1].event_type,
            KrakenFuturesOrderEventType::Edit
        );
        assert_eq!(
            response.order_events[2].event_type,
            KrakenFuturesOrderEventType::Cancel
        );
        assert_eq!(response.continuation_token.as_deref(), Some("simb178"));
    }

    /// An update carries the order after the edit.
    #[rstest]
    fn test_parse_futures_order_events_update_uses_the_new_order() {
        let response = order_history_events("http_futures_order_events.json");
        let updated = &response.order_events[1].order;

        assert_eq!(updated.filled, dec!(10000));
        assert_eq!(updated.order_type, KrakenFuturesOrderType::Market);
        assert_eq!(updated.limit_price, None, "a zero limit price is no price");
        assert_eq!(updated.cli_ord_id, None, "an empty client id is no id");
    }

    /// The contract name is kept as the venue spells it, and the millisecond timestamps are
    /// formatted for the readers, which parse RFC 3339.
    #[rstest]
    fn test_parse_futures_order_events_keeps_the_tradeable_and_formats_timestamps() {
        let response = order_history_events("http_futures_order_events.json");
        let canceled = &response.order_events[2].order;

        assert_eq!(canceled.symbol, "pi_xbtusd");
        assert_eq!(canceled.timestamp, "2023-04-07T13:00:00.000Z");
        assert_eq!(canceled.last_update_timestamp, "2023-04-07T16:00:00.000Z");
        assert!(canceled.reduce_only);
        assert_eq!(canceled.limit_price, Some(dec!(26000.0)));
        assert_eq!(
            canceled.stop_price, None,
            "the history schema has no trigger price"
        );
    }

    /// A stop row carries no trigger price, which the engine needs to materialize a stop order,
    /// so it is reported as the limit or market order it executes as once triggered.
    #[rstest]
    #[case::with_limit("26000.0", KrakenFuturesOrderType::Limit, Some(dec!(26000.0)))]
    #[case::without_limit("0", KrakenFuturesOrderType::Market, None)]
    fn test_parse_futures_order_events_reports_a_stop_as_its_triggered_order(
        #[case] limit_price: &str,
        #[case] expected_type: KrakenFuturesOrderType,
        #[case] expected_price: Option<Decimal>,
    ) {
        let data = placed_element(&format!(
            r#"{{"uid":"o1","tradeable":"PI_XBTUSD","direction":"Sell","quantity":"2000","filled":"0","limitPrice":"{limit_price}","orderType":"Stop","clientId":"","reduceOnly":true,"timestamp":1680876930250,"lastUpdateTimestamp":1680876930250}}"#
        ));
        let response = order_history_events_from(&data);
        let order = &response.order_events[0].order;

        assert_eq!(order.order_type, expected_type);
        assert_eq!(order.limit_price, expected_price);
        assert_eq!(order.stop_price, None);
    }

    /// A market order's placeholder price, empty or zero, is no price; a limit order keeps its
    /// price even at zero, since futures instruments allow non-positive prices.
    #[rstest]
    #[case::market_empty("Market", "", None)]
    #[case::market_zero("Market", "0", None)]
    #[case::limit_zero("Limit", "0", Some(dec!(0)))]
    fn test_parse_futures_order_events_reads_the_limit_price_by_order_kind(
        #[case] order_type: &str,
        #[case] limit_price: &str,
        #[case] expected: Option<Decimal>,
    ) {
        let data = placed_element(&format!(
            r#"{{"uid":"o1","tradeable":"PI_XBTUSD","direction":"Buy","quantity":"1","filled":"1","limitPrice":"{limit_price}","orderType":"{order_type}","clientId":"","reduceOnly":false,"timestamp":1680876930250,"lastUpdateTimestamp":1680876930250}}"#
        ));
        let response = order_history_events_from(&data);

        assert_eq!(response.order_events[0].order.limit_price, expected);
    }

    /// A rejected order reports as rejected.
    #[rstest]
    fn test_parse_futures_order_events_maps_a_rejection() {
        let data = r#"{"elements":[{"uid":"e1","timestamp":1680876930250,"event":{"OrderRejected":{"order":{"uid":"o1","tradeable":"PI_XBTUSD","direction":"Buy","quantity":"1","filled":"0","limitPrice":"70000","orderType":"Limit","clientId":"","reduceOnly":false,"timestamp":1680876930250,"lastUpdateTimestamp":1680876930250},"reason":"insufficient_margin"}}}]}"#;
        let response = order_history_events_from(data);

        assert_eq!(
            response.order_events[0].event_type,
            KrakenFuturesOrderEventType::Reject
        );
    }

    /// An order whose type the venue reports as `Unknown` establishes no order type, so its row
    /// is skipped and counted, while the sibling rows of the page are kept with their fields
    /// intact. A not-found event carries no state and is skipped without counting; an
    /// undocumented kind may carry state and counts.
    #[rstest]
    fn test_parse_futures_order_events_skips_an_unknown_order_type_and_keeps_siblings() {
        let response = order_history_events("http_futures_order_events_unknown.json");

        assert_eq!(response.order_events.len(), 1);
        let sibling = &response.order_events[0];
        assert_eq!(sibling.event_type, KrakenFuturesOrderEventType::Edit);
        assert_eq!(sibling.reduced_quantity, None);
        assert_eq!(sibling.order.order_id, "abc");
        assert_eq!(sibling.order.cli_ord_id, None);
        assert_eq!(sibling.order.order_type, KrakenFuturesOrderType::Limit);
        assert_eq!(sibling.order.symbol, "PF_XBTUSD");
        assert_eq!(sibling.order.side, KrakenOrderSide::Buy);
        assert_eq!(sibling.order.quantity, dec!(1.0));
        assert_eq!(sibling.order.filled, dec!(0.5));
        assert_eq!(sibling.order.limit_price, Some(dec!(70000.0)));
        assert_eq!(sibling.order.stop_price, None);
        assert_eq!(sibling.order.timestamp, "2026-05-18T00:00:00.000Z");
        assert_eq!(
            sibling.order.last_update_timestamp,
            "2026-05-18T00:00:01.000Z"
        );
        assert!(!sibling.order.reduce_only);
        assert_eq!(
            response.server_time.as_deref(),
            Some("2026-05-18T00:00:03.000Z")
        );
        assert_eq!(response.continuation_token, None);
        assert_eq!(
            response.skipped_rows, 2,
            "the Unknown type and the undocumented kind count; the not-found event does not"
        );
    }

    /// `Unknown` is how the venue reports a source type it could not decode, so neither it nor a
    /// missing type establishes what the order is: the row is skipped with its price rather than
    /// reported as a market order, the sibling row is kept, and the set is incomplete.
    #[rstest]
    #[case::missing_type("")]
    #[case::undocumented_type(r#""orderType":"SomethingNew","#)]
    fn test_parse_futures_order_events_skips_an_unmappable_order_type(#[case] order_type: &str) {
        let data = format!(
            r#"{{"elements":[{{"uid":"e1","timestamp":1680876930250,"event":{{"OrderPlaced":{{"order":{{"uid":"o1","tradeable":"PF_XBTUSD","direction":"Sell","quantity":"3","filled":"1",{order_type}"limitPrice":"70000","clientId":"cl-1","reduceOnly":true,"timestamp":1680876930250,"lastUpdateTimestamp":1680876930250}}}}}}}},{{"uid":"e2","timestamp":1680877245500,"event":{{"OrderPlaced":{{"order":{{"uid":"o2","tradeable":"PF_XBTUSD","direction":"Buy","quantity":"2","filled":"0.5","limitPrice":"69500.5","orderType":"Limit","clientId":"cl-2","reduceOnly":false,"timestamp":1680877245500,"lastUpdateTimestamp":1680877245500}}}}}}}}]}}"#
        );
        let response = order_history_events_from(&data);

        assert_eq!(response.order_events.len(), 1);
        let sibling = &response.order_events[0];
        assert_eq!(sibling.event_type, KrakenFuturesOrderEventType::Place);
        assert_eq!(sibling.order.order_id, "o2");
        assert_eq!(sibling.order.cli_ord_id.as_deref(), Some("cl-2"));
        assert_eq!(sibling.order.order_type, KrakenFuturesOrderType::Limit);
        assert_eq!(sibling.order.symbol, "PF_XBTUSD");
        assert_eq!(sibling.order.side, KrakenOrderSide::Buy);
        assert_eq!(sibling.order.quantity, dec!(2));
        assert_eq!(sibling.order.filled, dec!(0.5));
        assert_eq!(sibling.order.limit_price, Some(dec!(69500.5)));
        assert_eq!(sibling.order.stop_price, None);
        assert_eq!(sibling.order.timestamp, "2023-04-07T14:20:45.500Z");
        assert_eq!(
            sibling.order.last_update_timestamp,
            "2023-04-07T14:20:45.500Z"
        );
        assert!(!sibling.order.reduce_only);
        assert_eq!(
            response.skipped_rows, 1,
            "the skip leaves the set incomplete"
        );
    }

    /// A timestamp before the epoch cannot be reported, so the row is skipped rather than dated
    /// at the epoch.
    #[rstest]
    fn test_parse_futures_order_events_skips_a_negative_timestamp() {
        let data = r#"{"elements":[{"uid":"e1","timestamp":-1,"event":{"OrderPlaced":{"order":{"uid":"o1","tradeable":"PF_XBTUSD","direction":"Buy","quantity":"1","filled":"0","limitPrice":"70000","orderType":"Limit","clientId":"","reduceOnly":false,"timestamp":-1,"lastUpdateTimestamp":-1}}}}]}"#;
        let response: FuturesOrderHistoryResponse = serde_json::from_str(data).unwrap();
        let response = response.into_order_events().unwrap();

        assert!(response.order_events.is_empty());
        assert_eq!(
            response.skipped_rows, 1,
            "the skip leaves the set incomplete"
        );
    }

    /// The kind is found wherever it sits among the event's keys.
    #[rstest]
    fn test_parse_futures_order_events_finds_the_kind_after_other_keys() {
        let data = r#"{"elements":[{"uid":"e1","timestamp":1680877245500,"event":{"version":1,"OrderCancelled":{"order":{"uid":"o1","tradeable":"PI_XBTUSD","direction":"Buy","quantity":"1","filled":"0","limitPrice":"70000","orderType":"Limit","clientId":"","reduceOnly":false,"timestamp":1680876930250,"lastUpdateTimestamp":1680876930250}}}}]}"#;
        let response = order_history_events_from(data);

        assert_eq!(
            response.order_events[0].event_type,
            KrakenFuturesOrderEventType::Cancel
        );
    }

    /// A report is dated at the event when that is later than the order's own update stamp, as
    /// for a refused edit.
    #[rstest]
    fn test_parse_futures_order_events_dates_a_report_at_the_event() {
        let data = r#"{"elements":[{"uid":"e1","timestamp":1680877245500,"event":{"OrderEditRejected":{"oldOrder":{"uid":"o1","tradeable":"PI_XBTUSD","direction":"Buy","quantity":"1","filled":"0","limitPrice":"70000","orderType":"Limit","clientId":"","reduceOnly":false,"timestamp":1680876930250,"lastUpdateTimestamp":1680876930250}}}}]}"#;
        let response = order_history_events_from(data);

        assert_eq!(
            response.order_events[0].order.last_update_timestamp,
            "2023-04-07T14:20:45.500Z"
        );
    }

    /// An empty event object is an undocumented kind, not a failed page.
    #[rstest]
    fn test_parse_futures_order_events_tolerates_an_empty_event() {
        let data = r#"{"elements":[{"uid":"e1","timestamp":1,"event":{}}]}"#;
        let response: FuturesOrderHistoryResponse = serde_json::from_str(data).unwrap();

        assert!(matches!(
            response.elements.as_deref(),
            Some([FuturesOrderHistoryElement { event: FuturesOrderHistoryEvent::Unknown(kind), .. }]) if kind.is_empty()
        ));
    }

    /// An order whose direction the venue could not decode has no side to report, so it is skipped.
    #[rstest]
    fn test_parse_futures_order_events_skips_an_unknown_direction() {
        let data = r#"{"elements":[{"uid":"e1","timestamp":1680876930250,"event":{"OrderPlaced":{"order":{"uid":"o1","tradeable":"PF_XBTUSD","direction":"Unknown","quantity":"1","filled":"0","limitPrice":"70000","orderType":"Limit","clientId":"","reduceOnly":false,"timestamp":1680876930250,"lastUpdateTimestamp":1680876930250},"reason":"","reducedQuantity":""}}}]}"#;
        let response: FuturesOrderHistoryResponse = serde_json::from_str(data).unwrap();
        let response = response.into_order_events().unwrap();

        assert!(response.order_events.is_empty());
        assert_eq!(
            response.skipped_rows, 1,
            "the skip leaves the set incomplete"
        );
    }

    /// A venue error reported with a success status fails the read with the venue's reason, not
    /// as an empty page.
    #[rstest]
    fn test_parse_futures_order_events_fails_on_a_venue_error_body() {
        let data = r#"{"result":"error","error":"apiLimitExceeded"}"#;
        let response: FuturesOrderHistoryResponse = serde_json::from_str(data).unwrap();

        let error = response
            .into_order_events()
            .expect_err("a venue error must fail the read");

        assert!(
            matches!(&error, KrakenHttpError::ApiError(reasons) if reasons == &["apiLimitExceeded"]),
            "unexpected error: {error}"
        );
    }

    /// A body without an `elements` array is not an order history page.
    #[rstest]
    fn test_parse_futures_order_events_requires_elements() {
        let data = r#"{"serverTime":"2023-04-07T16:30:45.678Z"}"#;
        let response: FuturesOrderHistoryResponse = serde_json::from_str(data).unwrap();

        assert!(matches!(
            response.into_order_events(),
            Err(KrakenHttpError::ParseError(_))
        ));
    }

    #[rstest]
    fn test_parse_futures_order_trigger_data_tolerates_unknown_enum_values() {
        // Trigger payload uses non-optional enums, so an `"unknown"` on
        // triggerSide / triggerSignal must not fail the sendStatus batch.
        let data = load_test_data("http_send_order_futures_unknown_trigger.json");
        let response: FuturesSendOrderResponse =
            serde_json::from_str(&data).expect("Failed to parse send-order response with unknown");

        let send_status = response.send_status.expect("sendStatus missing");
        let order_events = send_status.order_events.expect("orderEvents missing");
        let trigger = order_events[0]
            .order_trigger
            .as_ref()
            .expect("orderTrigger missing");

        assert_eq!(trigger.order_type, KrakenFuturesOrderType::Unknown);
        assert_eq!(trigger.trigger_side, KrakenTriggerSide::Unknown);
        assert_eq!(trigger.trigger_signal, KrakenTriggerSignal::Unknown);
    }

    #[rstest]
    fn test_parse_futures_send_order_execution_event_uses_enum_event_type() {
        let data = r#"
        {
          "result": "success",
          "sendStatus": {
            "status": "placed",
            "orderEvents": [
              {
                "type": "EXECUTION",
                "executionId": "c8a35168-8d52-4609-944f-3f32bb0d5c77",
                "price": 35000.5,
                "amount": 1.25,
                "orderPriorExecution": {
                  "orderId": "c8a35168-8d52-4609-944f-3f32bb0d5c77",
                  "cliOrdId": "test-order-001",
                  "type": "lmt",
                  "symbol": "PI_XBTUSD",
                  "side": "buy",
                  "quantity": 2.0,
                  "filled": 0.0,
                  "limitPrice": 35000.5,
                  "timestamp": "2024-01-15T10:30:45.123Z",
                  "lastUpdateTimestamp": "2024-01-15T10:30:45.123Z",
                  "reduceOnly": false
                }
              }
            ]
          }
        }
        "#;
        let response: FuturesSendOrderResponse =
            serde_json::from_str(data).expect("Failed to parse futures send order response");

        let send_status = response.send_status.expect("sendStatus missing");
        let order_events = send_status.order_events.expect("orderEvents missing");

        assert_eq!(order_events.len(), 1);
        assert_eq!(
            order_events[0].event_type,
            KrakenFuturesOrderEventType::Execution
        );
    }
}
