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

//! Kalshi REST request and query builders.

use serde::{Deserialize, Serialize, Serializer};

use super::models::{BookSide, FixedPoint, SelfTradePrevention, TimeInForce};
use crate::common::enums::{
    CandlestickInterval, EventStatusFilter, MarketStatusFilter, MveFilter, MveHistoricalFilter,
    OrderStatusFilter, PositionCountFilter, PositionSettlementStatus,
};

/// Live market discovery filters; unset values are omitted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarketsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_created_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_created_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_updated_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_updated_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_close_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_close_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_settled_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_settled_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<MarketStatusFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tickers: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mve_filter: Option<MveFilter>,
}

/// Archived market discovery filters.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HistoricalMarketsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tickers: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mve_filter: Option<MveHistoricalFilter>,
}

/// Event discovery filters.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EventsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_nested_markets: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<EventStatusFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tickers: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_close_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_updated_ts: Option<i64>,
}

/// Optional market inclusion for one event.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EventQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_markets: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_nested_markets: Option<bool>,
}

/// Series discovery filters.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SeriesQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_volume: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_updated_ts: Option<i64>,
}

/// Shared live/archive public-trade filters.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TradesQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_block_trade: Option<bool>,
}

/// Candle window in Unix seconds and interval in minutes (1, 60, or 1440).
#[derive(Debug, Clone, Serialize)]
pub struct CandlesticksQuery {
    pub start_ts: i64,
    pub end_ts: i64,
    pub period_interval: CandlestickInterval,
}

/// Explicit primary-account shard selection.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AccountQuery {
    pub subaccount: u32,
    pub exchange_index: u32,
}

/// Live order report filters.
#[derive(Debug, Clone, Default, Serialize)]
pub struct OrdersQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<OrderStatusFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub subaccount: u32,
    pub exchange_index: u32,
}

/// Private fill report filters.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FillsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub subaccount: u32,
    pub exchange_index: u32,
}

/// Live position report filters.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PositionsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "serialize_count_filter")]
    pub count_filter: Option<Vec<PositionCountFilter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settlement_status: Option<PositionSettlementStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_ticker: Option<String>,
    pub subaccount: u32,
    pub exchange_index: u32,
}

/// Archived order/fill filters; archive endpoints do not accept a shard selector.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HistoryQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub subaccount: u32,
}

/// Archived positions; the caller filters record-level shard identity.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HistoricalPositionsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub subaccount: u32,
}

/// Settlements still in the live tier.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SettlementsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ts: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub subaccount: u32,
}

/// A V2 limit order using YES-price book-side semantics.
///
/// Prices serialize with four decimal places without rounding.
#[derive(Debug, Clone, Serialize)]
pub struct CreateOrderRequest {
    pub ticker: String,
    pub side: BookSide,
    pub count: FixedPoint,
    #[serde(serialize_with = "serialize_price")]
    pub price: FixedPoint,
    pub time_in_force: TimeInForce,
    pub self_trade_prevention_type: SelfTradePrevention,
    pub subaccount: u32,
    pub exchange_index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_order_on_pause: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
}

/// V2 amendment; count is total filled plus desired remaining contracts.
///
/// Prices serialize with four decimal places without rounding.
#[derive(Debug, Clone, Serialize)]
pub struct AmendOrderRequest {
    pub ticker: String,
    pub side: BookSide,
    #[serde(serialize_with = "serialize_price")]
    pub price: FixedPoint,
    pub count: FixedPoint,
    pub exchange_index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_client_order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration_time: Option<i64>,
}

/// V2 decrease with exactly one reduction selector.
#[derive(Debug, Clone, Serialize)]
pub struct DecreaseOrderRequest {
    pub exchange_index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_by: Option<FixedPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_to: Option<FixedPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_ticker: Option<String>,
}

/// One explicitly selected order for batch cancellation.
#[derive(Debug, Clone, Serialize)]
pub struct CancelOrderRequest {
    pub order_id: String,
    pub subaccount: u32,
    pub exchange_index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_ticker: Option<String>,
}

/// Independent V2 limit-order requests.
#[derive(Debug, Clone, Serialize)]
pub struct BatchCreateRequest {
    pub orders: Vec<CreateOrderRequest>,
}

/// Explicitly selected V2 order cancellations.
#[derive(Debug, Clone, Serialize)]
pub struct BatchCancelRequest {
    pub orders: Vec<CancelOrderRequest>,
}

fn serialize_count_filter<S: Serializer>(
    filters: &Option<Vec<PositionCountFilter>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match filters {
        Some(filters) => {
            let value = filters
                .iter()
                .map(AsRef::<str>::as_ref)
                .collect::<Vec<_>>()
                .join(",");
            serializer.serialize_str(&value)
        }
        None => serializer.serialize_none(),
    }
}

fn serialize_price<S: Serializer>(price: &FixedPoint, serializer: S) -> Result<S::Ok, S::Error> {
    let mut value = price.0;
    if value.scale() > 4 {
        return Err(serde::ser::Error::custom(
            "order price exceeds four decimal places",
        ));
    }

    value.rescale(4);
    serializer.serialize_str(&value.to_string())
}

pub(super) trait CursorQuery: Clone {
    fn cursor(&self) -> Option<&str>;

    fn set_cursor(&mut self, cursor: Option<String>);
}

macro_rules! cursor_query {
    ($($query:ty),+ $(,)?) => { $(
        impl CursorQuery for $query {
            fn cursor(&self) -> Option<&str> { self.cursor.as_deref() }
            fn set_cursor(&mut self, cursor: Option<String>) { self.cursor = cursor; }
        }
    )+ };
}

cursor_query!(
    MarketsQuery,
    HistoricalMarketsQuery,
    EventsQuery,
    TradesQuery,
    OrdersQuery,
    FillsQuery,
    PositionsQuery,
    HistoryQuery,
    HistoricalPositionsQuery,
    SettlementsQuery
);
