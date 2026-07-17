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

//! HTTP query and response model types for the Polymarket CLOB API.

use ahash::{AHashMap, AHashSet};
use derive_builder::Builder;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    common::{
        enums::{PolymarketOrderType, SignatureType},
        parse::{deserialize_decimal_from_str, deserialize_optional_decimal_from_str},
    },
    http::models::PolymarketOrder,
};

/// Query parameters for `GET /data/orders`.
#[derive(Clone, Debug, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetOrdersParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Query parameters for `GET /data/trades`.
#[derive(Clone, Debug, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetTradesParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maker_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Query parameters for `GET /balance-allowance`.
#[derive(Clone, Debug, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetBalanceAllowanceParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_type: Option<AssetType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_type: Option<SignatureType>,
}

/// Body parameters for `DELETE /cancel-market-orders`.
#[derive(Clone, Debug, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct CancelMarketOrdersParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
}

/// Asset type for balance and allowance requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssetType {
    Collateral,
    Conditional,
}

/// Balance and allowance response from `GET /balance-allowance`.
#[derive(Clone, Debug, Deserialize)]
pub struct BalanceAllowance {
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub balance: Decimal,
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub allowance: Option<Decimal>,
}

/// Order submission response from `POST /order` and `POST /orders`.
#[derive(Clone, Debug, Deserialize)]
pub struct OrderResponse {
    pub success: bool,
    #[serde(rename = "orderID")]
    pub order_id: Option<String>,
    #[serde(rename = "errorMsg")]
    pub error_msg: Option<String>,
}

/// Cancel response from all cancel endpoints (`DELETE /order`, `/orders`,
/// `/cancel-all`, `/cancel-market-orders`).
///
/// All endpoints return the same format:
/// `{ "canceled": ["0x..."], "not_canceled": {"0x...": "reason"} }`
#[derive(Clone, Debug, Deserialize)]
pub struct CancelResponse {
    #[serde(default)]
    pub canceled: Vec<String>,
    #[serde(default)]
    pub not_canceled: AHashMap<String, Option<String>>,
}

/// Type alias for backwards compatibility.
pub type BatchCancelResponse = CancelResponse;

/// Parameters for `POST /order`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostOrderParams {
    pub order_type: PolymarketOrderType,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub post_only: bool,
}

/// One order entry for `POST /orders`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderSubmission {
    pub order: PolymarketOrder,
    pub order_type: PolymarketOrderType,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub post_only: bool,
}

/// Query parameters for Gamma API `GET /markets/keyset`.
#[derive(Clone, Debug, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetGammaMarketsParams {
    /// Compatibility filter retained from the legacy Gamma market query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// Compatibility filter retained from the legacy Gamma market query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Vec<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Client-side initial offset. Keyset requests never send this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascending: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clob_token_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_maker_address: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidity_num_min: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidity_num_max: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_num_min: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_num_max: Option<Decimal>,
    /// ISO 8601 date or RFC 3339 date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date_min: Option<String>,
    /// ISO 8601 date or RFC 3339 date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date_max: Option<String>,
    /// ISO 8601 date or RFC 3339 date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date_min: Option<String>,
    /// ISO 8601 date or RFC 3339 date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date_max: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_id: Option<Vec<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_tags: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_match: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimalized: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cyom: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rfq_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uma_resolution_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sports_market_types: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_tag: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// Client-side cap on total markets to fetch across all pages.
    /// Not sent to the API, only used by the paginator to stop early.
    /// Each market produces 2 instruments (Yes/No outcomes).
    #[serde(skip)]
    pub max_markets: Option<u32>,
}

/// Query parameters for Gamma API `GET /events/keyset`.
#[derive(Clone, Debug, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetGammaEventsParams {
    /// Compatibility filter retained from the legacy Gamma event query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// Compatibility filter retained from the legacy Gamma event query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Vec<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub featured: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cyom: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_search: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidity_min: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidity_max: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_min: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_max: Option<Decimal>,
    /// ISO 8601 date or RFC 3339 date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date_min: Option<String>,
    /// ISO 8601 date or RFC 3339 date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date_max: Option<String>,
    /// ISO 8601 date or RFC 3339 date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date_min: Option<String>,
    /// ISO 8601 date or RFC 3339 date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date_max: Option<String>,
    /// ISO 8601 date or RFC 3339 date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time_min: Option<String>,
    /// ISO 8601 date or RFC 3339 date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time_max: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_id: Option<Vec<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_tag_id: Option<Vec<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_tags: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_match: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_id: Option<Vec<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_id: Option<Vec<u64>>,
    /// ISO 8601 date or RFC 3339 date-time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_week: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub featured_order: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_children: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partner_slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_chat: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_template: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_best_lines: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascending: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Client-side initial offset. Keyset requests never send this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// Client-side cap on total events to fetch across all pages.
    #[serde(skip)]
    pub max_events: Option<u32>,
}

impl GetGammaMarketsParams {
    /// Validates values and combinations used by the Gamma market keyset endpoint.
    pub fn validate_keyset(&self) -> Result<(), String> {
        validate_limit(self.limit, 100, "market")?;
        validate_non_empty_values(self.id.as_deref(), "id")?;
        validate_non_empty_list(self.slug.as_deref(), "slug")?;
        validate_non_empty_list(self.clob_token_ids.as_deref(), "clob_token_ids")?;
        validate_non_empty_list(self.condition_ids.as_deref(), "condition_ids")?;
        validate_non_empty_list(self.question_ids.as_deref(), "question_ids")?;
        validate_non_empty_list(self.market_maker_address.as_deref(), "market_maker_address")?;
        validate_non_empty_values(self.tag_id.as_deref(), "tag_id")?;
        validate_non_empty_list(self.sports_market_types.as_deref(), "sports_market_types")?;

        if self
            .condition_ids
            .as_ref()
            .is_some_and(|ids| ids.len() > 100)
        {
            return Err("condition_ids accepts at most 100 values".to_string());
        }

        validate_decimal_bounds(
            self.liquidity_num_min,
            self.liquidity_num_max,
            "liquidity_num",
        )?;
        validate_decimal_bounds(self.volume_num_min, self.volume_num_max, "volume_num")?;
        validate_date_bounds(
            self.start_date_min.as_deref(),
            self.start_date_max.as_deref(),
            "start_date",
        )?;
        validate_date_bounds(
            self.end_date_min.as_deref(),
            self.end_date_max.as_deref(),
            "end_date",
        )?;
        validate_non_empty_string(self.order.as_deref(), "order")?;
        validate_non_empty_string(self.tag_match.as_deref(), "tag_match")?;
        validate_non_empty_string(
            self.uma_resolution_status.as_deref(),
            "uma_resolution_status",
        )?;
        validate_non_empty_string(self.game_id.as_deref(), "game_id")?;
        validate_non_empty_string(self.locale.as_deref(), "locale")
    }
}

impl GetGammaEventsParams {
    /// Validates values and combinations used by the Gamma event keyset endpoint.
    pub fn validate_keyset(&self) -> Result<(), String> {
        validate_limit(self.limit, 500, "event")?;
        validate_non_empty_values(self.id.as_deref(), "id")?;
        validate_non_empty_list(self.slug.as_deref(), "slug")?;
        validate_non_empty_values(self.tag_id.as_deref(), "tag_id")?;
        validate_non_empty_values(self.exclude_tag_id.as_deref(), "exclude_tag_id")?;
        validate_non_empty_values(self.series_id.as_deref(), "series_id")?;
        validate_non_empty_values(self.game_id.as_deref(), "game_id")?;
        validate_non_empty_list(self.created_by.as_deref(), "created_by")?;

        if let (Some(tag_ids), Some(excluded_ids)) = (&self.tag_id, &self.exclude_tag_id) {
            let tag_ids: AHashSet<u64> = tag_ids.iter().copied().collect();
            if excluded_ids.iter().any(|id| tag_ids.contains(id)) {
                return Err("tag_id and exclude_tag_id cannot overlap".to_string());
            }
        }

        validate_decimal_bounds(self.liquidity_min, self.liquidity_max, "liquidity")?;
        validate_decimal_bounds(self.volume_min, self.volume_max, "volume")?;
        validate_date_bounds(
            self.start_date_min.as_deref(),
            self.start_date_max.as_deref(),
            "start_date",
        )?;
        validate_date_bounds(
            self.end_date_min.as_deref(),
            self.end_date_max.as_deref(),
            "end_date",
        )?;
        validate_date_bounds(
            self.start_time_min.as_deref(),
            self.start_time_max.as_deref(),
            "start_time",
        )?;
        validate_date_value(self.event_date.as_deref(), "event_date")?;
        validate_non_empty_string(self.order.as_deref(), "order")?;
        validate_non_empty_string(self.title_search.as_deref(), "title_search")?;
        validate_non_empty_string(self.tag_slug.as_deref(), "tag_slug")?;
        validate_non_empty_string(self.tag_match.as_deref(), "tag_match")?;
        validate_non_empty_string(self.recurrence.as_deref(), "recurrence")?;
        validate_non_empty_string(self.partner_slug.as_deref(), "partner_slug")?;
        validate_non_empty_string(self.locale.as_deref(), "locale")
    }
}

fn validate_limit(limit: Option<u32>, ceiling: u32, endpoint: &str) -> Result<(), String> {
    if let Some(limit) = limit
        && !(1..=ceiling).contains(&limit)
    {
        return Err(format!(
            "{endpoint} limit must be between 1 and {ceiling}, was {limit}"
        ));
    }
    Ok(())
}

fn validate_non_empty_list(values: Option<&[String]>, name: &str) -> Result<(), String> {
    if let Some(values) = values
        && (values.is_empty() || values.iter().any(|value| value.trim().is_empty()))
    {
        return Err(format!("{name} must contain non-empty values"));
    }
    Ok(())
}

fn validate_non_empty_values<T>(values: Option<&[T]>, name: &str) -> Result<(), String> {
    if values.is_some_and(<[T]>::is_empty) {
        return Err(format!("{name} must contain at least one value"));
    }
    Ok(())
}

fn validate_decimal_bounds(
    min: Option<Decimal>,
    max: Option<Decimal>,
    name: &str,
) -> Result<(), String> {
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        return Err(format!("{name}_min cannot exceed {name}_max"));
    }
    Ok(())
}

fn validate_date_bounds(min: Option<&str>, max: Option<&str>, name: &str) -> Result<(), String> {
    let min = parse_date_value(min, &format!("{name}_min"))?;
    let max = parse_date_value(max, &format!("{name}_max"))?;
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        return Err(format!("{name}_min cannot exceed {name}_max"));
    }
    Ok(())
}

fn validate_date_value(value: Option<&str>, name: &str) -> Result<(), String> {
    parse_date_value(value, name).map(|_| ())
}

fn parse_date_value(
    value: Option<&str>,
    name: &str,
) -> Result<Option<chrono::DateTime<chrono::FixedOffset>>, String> {
    value
        .map(|value| {
            chrono::DateTime::parse_from_rfc3339(value)
                .or_else(|_| {
                    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").map(|date| {
                        date.and_hms_opt(0, 0, 0)
                            .expect("midnight is a valid time")
                            .and_utc()
                            .fixed_offset()
                    })
                })
                .map_err(|_| format!("{name} must be an ISO 8601 date or RFC 3339 date-time"))
        })
        .transpose()
}

fn validate_non_empty_string(value: Option<&str>, name: &str) -> Result<(), String> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        return Err(format!("{name} cannot be empty"));
    }
    Ok(())
}

/// Query parameters for Gamma API `GET /public-search`.
#[derive(Clone, Debug, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetSearchParams {
    /// Free-text search query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    /// Filter events by status ("active", "closed", etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events_status: Option<String>,
    /// Filter by event tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events_tag: Option<String>,
    /// Sort field ("volume", "liquidity", etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascending: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_per_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_closed_markets: Option<bool>,
}

/// Paginated response wrapper for CLOB list endpoints.
#[derive(Clone, Debug, Deserialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub next_cursor: String,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        common::enums::{PolymarketOrderSide, PolymarketOrderType},
        http::models::{PolymarketOpenOrder, PolymarketTradeReport},
    };

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    #[rstest]
    fn test_paginated_orders_page() {
        let page: PaginatedResponse<PolymarketOpenOrder> = load("http_open_orders_page.json");

        assert_eq!(page.data.len(), 2);
        assert_eq!(page.next_cursor, "LTE=");
        assert_eq!(page.data[0].side, PolymarketOrderSide::Buy);
        assert_eq!(page.data[1].side, PolymarketOrderSide::Sell);
    }

    #[rstest]
    fn test_paginated_trades_page() {
        let page: PaginatedResponse<PolymarketTradeReport> = load("http_trades_page.json");

        assert_eq!(page.data.len(), 1);
        assert_eq!(page.next_cursor, "LTE=");
        assert_eq!(page.data[0].id, "trade-0x001");
    }

    #[rstest]
    fn test_balance_allowance_with_allowance() {
        // The Polymarket API returns balances and allowances as integer
        // micro-pUSD strings (e.g. `"1000000000"` == 1000 pUSD).
        let ba: BalanceAllowance = load("http_balance_allowance_collateral.json");

        assert_eq!(ba.balance, dec!(1_000_000_000));
        assert_eq!(ba.allowance, Some(dec!(999_999_999_000_000)));
    }

    #[rstest]
    fn test_balance_allowance_no_allowance() {
        let ba: BalanceAllowance = load("http_balance_allowance_no_allowance.json");

        assert_eq!(ba.balance, dec!(250.500000));
        assert!(ba.allowance.is_none());
    }

    #[rstest]
    fn test_order_response_success() {
        let resp: OrderResponse = load("http_order_response_ok.json");

        assert!(resp.success);
        assert_eq!(
            resp.order_id.as_deref(),
            Some("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12")
        );
        assert!(resp.error_msg.is_none());
    }

    #[rstest]
    fn test_order_response_failure() {
        let resp: OrderResponse = load("http_order_response_failed.json");

        assert!(!resp.success);
        assert!(resp.order_id.is_none());
        assert_eq!(resp.error_msg.as_deref(), Some("Insufficient balance"));
    }

    #[rstest]
    fn test_cancel_response_ok() {
        let resp: CancelResponse = load("http_cancel_response_ok.json");

        assert_eq!(resp.canceled.len(), 1);
        assert!(resp.not_canceled.is_empty());
    }

    #[rstest]
    fn test_cancel_response_failed() {
        let resp: CancelResponse = load("http_cancel_response_failed.json");

        assert!(resp.canceled.is_empty());
        assert_eq!(resp.not_canceled.len(), 1);
        let reason = resp.not_canceled.values().next().and_then(|v| v.as_deref());
        assert_eq!(reason, Some("already canceled or matched"));
    }

    #[rstest]
    fn test_batch_cancel_response() {
        let resp: BatchCancelResponse = load("http_batch_cancel_response.json");

        assert_eq!(resp.canceled.len(), 2);
        assert!(resp.canceled[0].contains("1111"));
        assert!(resp.canceled[1].contains("2222"));
        assert_eq!(resp.not_canceled.len(), 1);
        let reason = resp.not_canceled.values().next().and_then(|v| v.as_deref());
        assert_eq!(reason, Some("already canceled or matched"));
    }

    #[rstest]
    fn test_asset_type_serializes_screaming_snake() {
        assert_eq!(
            serde_json::to_string(&AssetType::Collateral).unwrap(),
            "\"COLLATERAL\""
        );
        assert_eq!(
            serde_json::to_string(&AssetType::Conditional).unwrap(),
            "\"CONDITIONAL\""
        );
    }

    #[rstest]
    fn test_asset_type_deserializes() {
        assert_eq!(
            serde_json::from_str::<AssetType>("\"COLLATERAL\"").unwrap(),
            AssetType::Collateral
        );
        assert_eq!(
            serde_json::from_str::<AssetType>("\"CONDITIONAL\"").unwrap(),
            AssetType::Conditional
        );
    }

    #[rstest]
    fn test_get_orders_params_skips_none() {
        let params = GetOrdersParams::default();
        let json = serde_json::to_string(&params).unwrap();
        assert_eq!(json, "{}");
    }

    #[rstest]
    fn test_get_orders_params_serializes_set_fields() {
        let params = GetOrdersParams {
            market: Some("0xmarket".to_string()),
            asset_id: None,
            next_cursor: Some("MA==".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"market\""));
        assert!(json.contains("\"next_cursor\""));
        assert!(!json.contains("\"asset_id\""));
    }

    #[rstest]
    fn test_get_orders_params_id_filter() {
        let params = GetOrdersParams {
            id: Some("0xorder123".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"id\""));
        assert!(json.contains("0xorder123"));
    }

    #[rstest]
    fn test_get_gamma_markets_params_slug() {
        let params = GetGammaMarketsParams {
            slug: Some(vec!["btc-updown-15m-1741500000".to_string()]),
            ..Default::default()
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"slug\""));
        assert!(json.contains("btc-updown-15m-1741500000"));
        assert!(!json.contains("\"active\""));
    }

    #[rstest]
    fn test_get_gamma_markets_params_skips_none_slug() {
        let params = GetGammaMarketsParams {
            active: Some(true),
            ..Default::default()
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(!json.contains("\"slug\""));
        assert!(json.contains("\"active\""));
    }

    #[rstest]
    fn test_get_gamma_markets_params_new_filter_fields() {
        let params = GetGammaMarketsParams {
            volume_num_min: Some(dec!(1000.0)),
            tag_id: Some(vec![123]),
            end_date_min: Some("2025-06-01T00:00:00Z".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"volume_num_min\":\"1000.0\""));
        assert!(json.contains("\"tag_id\":[123]"));
        assert!(json.contains("\"end_date_min\":\"2025-06-01T00:00:00Z\""));
        assert!(!json.contains("\"active\""));
        assert!(!json.contains("\"archived\""));
    }

    #[rstest]
    fn test_get_gamma_markets_params_condition_ids() {
        let params = GetGammaMarketsParams {
            condition_ids: Some(vec!["0xcond1".to_string(), "0xcond2".to_string()]),
            liquidity_num_min: Some(dec!(500.0)),
            ..Default::default()
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"condition_ids\":[\"0xcond1\",\"0xcond2\"]"));
        assert!(json.contains("\"liquidity_num_min\":\"500.0\""));
    }

    #[rstest]
    fn test_get_trades_params_skips_none() {
        let params = GetTradesParams::default();
        let json = serde_json::to_string(&params).unwrap();
        assert_eq!(json, "{}");
    }

    #[rstest]
    fn test_post_order_params_skips_post_only_when_false() {
        let params = PostOrderParams {
            order_type: PolymarketOrderType::GTC,
            post_only: false,
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(!json.contains("post_only"));
        assert!(!json.contains("postOnly"));
    }

    #[rstest]
    fn test_post_order_params_includes_post_only_when_true() {
        let params = PostOrderParams {
            order_type: PolymarketOrderType::GTC,
            post_only: true,
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("postOnly"));
    }
}
