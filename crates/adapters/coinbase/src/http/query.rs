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

//! Venue-shaped request bodies for the Coinbase Advanced Trade REST API.
//!
//! These types serialize to the exact JSON shape Coinbase expects on its
//! POST endpoints. The raw HTTP client takes one of these types per endpoint;
//! the domain HTTP client builds them from Nautilus types.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::{
    enums::{CoinbaseMarginType, CoinbaseOrderSide, CoinbaseStopDirection},
    parse::{
        deserialize_decimal_from_str, deserialize_optional_decimal_from_str,
        serialize_decimal_as_str, serialize_optional_decimal_as_str,
    },
};

/// Request body for `POST /api/v3/brokerage/orders` (Create Order).
///
/// # References
///
/// - <https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/create-order>
#[derive(Debug, Clone, Serialize)]
pub struct CreateOrderRequest {
    pub client_order_id: String,
    pub product_id: Ustr,
    pub side: CoinbaseOrderSide,
    pub order_configuration: OrderConfiguration,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_trade_prevention_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leverage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_type: Option<CoinbaseMarginType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retail_portfolio_id: Option<String>,
    /// Derivatives-only flag that marks the order as position-reducing only.
    ///
    /// Coinbase does not document `reduce_only` as an accepted create-order
    /// field; the venue's failure-reason enum acknowledges the concept but the
    /// order schema has no slot for it. The field is threaded through the
    /// request for API parity with other adapters and is omitted from the wire
    /// payload when `false`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reduce_only: bool,
}

/// Request body for `POST /api/v3/brokerage/orders/batch_cancel` (Cancel Orders).
///
/// # References
///
/// - <https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/cancel-order>
#[derive(Debug, Clone, Serialize)]
pub struct CancelOrdersRequest {
    pub order_ids: Vec<String>,
}

/// Filter parameters for `GET /api/v3/brokerage/orders/historical/batch`
/// (List Orders).
///
/// `client_order_id_filter` is a client-side filter applied during pagination
/// because Coinbase's batch endpoint does not accept a `client_order_id`
/// query parameter.
///
/// # References
///
/// - <https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/list-orders>
#[derive(Debug, Clone, Default)]
pub struct OrderListQuery {
    pub product_id: Option<String>,
    pub open_only: bool,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
    pub client_order_id_filter: Option<String>,
}

/// Filter parameters for `GET /api/v3/brokerage/orders/historical/fills`
/// (List Fills).
///
/// # References
///
/// - <https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/list-fills>
#[derive(Debug, Clone, Default)]
pub struct FillListQuery {
    pub product_id: Option<String>,
    pub venue_order_id: Option<String>,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}

/// Request body for `POST /api/v3/brokerage/orders/edit` (Edit Order).
///
/// Coinbase restricts edits to GTC variants of LIMIT (and limited STOP_LIMIT
/// configurations). Each field is optional so callers can edit a subset.
///
/// # References
///
/// - <https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/edit-order>
#[derive(Debug, Clone, Serialize)]
pub struct EditOrderRequest {
    pub order_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,
}

/// Order configuration for different order types.
///
/// Uses `#[serde(untagged)]` because Coinbase wraps each order type in a
/// uniquely-named key (e.g. `market_market_ioc`, `limit_limit_gtc`), which
/// serde matches by attempting each variant in declaration order. Error
/// messages on deserialization failure are opaque; prefer constructing
/// variants directly rather than deserializing from untrusted JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OrderConfiguration {
    MarketIoc(MarketIoc),
    MarketFok(MarketFok),
    LimitGtc(LimitGtc),
    LimitGtd(LimitGtd),
    LimitFok(LimitFok),
    StopLimitGtc(StopLimitGtc),
    StopLimitGtd(StopLimitGtd),
}

/// Market order with IOC fill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketIoc {
    pub market_market_ioc: MarketParams,
}

/// Market order with FOK fill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketFok {
    pub market_market_fok: MarketParams,
}

/// Market order parameters (shared by `market_market_ioc` and
/// `market_market_fok`; both wire shapes accept the same `base_size` /
/// `quote_size` body).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketParams {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub quote_size: Option<Decimal>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_decimal_from_str",
        serialize_with = "serialize_optional_decimal_as_str"
    )]
    pub base_size: Option<Decimal>,
}

/// Limit GTC order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitGtc {
    pub limit_limit_gtc: LimitGtcParams,
}

/// Limit GTC parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitGtcParams {
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub base_size: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub limit_price: Decimal,
    #[serde(default)]
    pub post_only: bool,
}

/// Limit GTD order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitGtd {
    pub limit_limit_gtd: LimitGtdParams,
}

/// Limit GTD parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitGtdParams {
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub base_size: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub limit_price: Decimal,
    pub end_time: String,
    #[serde(default)]
    pub post_only: bool,
}

/// Limit FOK order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitFok {
    pub limit_limit_fok: LimitFokParams,
}

/// Limit FOK parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitFokParams {
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub base_size: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub limit_price: Decimal,
}

/// Stop-limit GTC order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopLimitGtc {
    pub stop_limit_stop_limit_gtc: StopLimitGtcParams,
}

/// Stop-limit GTC parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopLimitGtcParams {
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub base_size: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub limit_price: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub stop_price: Decimal,
    pub stop_direction: CoinbaseStopDirection,
}

/// Stop-limit GTD order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopLimitGtd {
    pub stop_limit_stop_limit_gtd: StopLimitGtdParams,
}

/// Stop-limit GTD parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopLimitGtdParams {
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub base_size: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub limit_price: Decimal,
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_from_str"
    )]
    pub stop_price: Decimal,
    pub stop_direction: CoinbaseStopDirection,
    pub end_time: String,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;
    use rust_decimal::Decimal;
    use serde_json::json;

    use super::*;
    use crate::common::consts::{
        ORDER_CONFIG_BASE_SIZE, ORDER_CONFIG_LIMIT_GTC, ORDER_CONFIG_LIMIT_PRICE,
        ORDER_CONFIG_MARKET_IOC, ORDER_CONFIG_QUOTE_SIZE,
    };

    #[rstest]
    fn test_serialize_market_order() {
        let order = CreateOrderRequest {
            client_order_id: "test-123".to_string(),
            product_id: Ustr::from("BTC-USD"),
            side: CoinbaseOrderSide::Buy,
            order_configuration: OrderConfiguration::MarketIoc(MarketIoc {
                market_market_ioc: MarketParams {
                    quote_size: Some(Decimal::from_str("100").unwrap()),
                    base_size: None,
                },
            }),
            self_trade_prevention_id: None,
            leverage: None,
            margin_type: None,
            retail_portfolio_id: None,
            reduce_only: false,
        };

        let value = serde_json::to_value(&order).unwrap();
        assert_eq!(value["client_order_id"], "test-123");
        assert_eq!(value["product_id"], "BTC-USD");
        assert_eq!(value["side"], "BUY");
        assert_eq!(
            value["order_configuration"][ORDER_CONFIG_MARKET_IOC][ORDER_CONFIG_QUOTE_SIZE],
            "100"
        );
    }

    #[rstest]
    fn test_serialize_limit_gtc_order() {
        let order = CreateOrderRequest {
            client_order_id: "test-456".to_string(),
            product_id: Ustr::from("ETH-USD"),
            side: CoinbaseOrderSide::Sell,
            order_configuration: OrderConfiguration::LimitGtc(LimitGtc {
                limit_limit_gtc: LimitGtcParams {
                    base_size: Decimal::from_str("1.5").unwrap(),
                    limit_price: Decimal::from_str("3500.00").unwrap(),
                    post_only: true,
                },
            }),
            self_trade_prevention_id: None,
            leverage: None,
            margin_type: None,
            retail_portfolio_id: None,
            reduce_only: false,
        };

        let value = serde_json::to_value(&order).unwrap();
        assert_eq!(value["side"], "SELL");
        assert_eq!(
            value["order_configuration"][ORDER_CONFIG_LIMIT_GTC][ORDER_CONFIG_BASE_SIZE],
            "1.5"
        );
        assert_eq!(
            value["order_configuration"][ORDER_CONFIG_LIMIT_GTC][ORDER_CONFIG_LIMIT_PRICE],
            "3500.00"
        );
    }

    #[rstest]
    fn test_serialize_cancel_orders_request() {
        let request = CancelOrdersRequest {
            order_ids: vec!["abc".to_string(), "def".to_string()],
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({"order_ids": ["abc", "def"]})
        );
    }

    #[rstest]
    fn test_serialize_edit_order_request_omits_none_fields() {
        let request = EditOrderRequest {
            order_id: "venue-1".to_string(),
            price: Some("100.00".to_string()),
            size: None,
            stop_price: None,
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["order_id"], "venue-1");
        assert_eq!(value["price"], "100.00");
        assert!(value.get("size").is_none());
        assert!(value.get("stop_price").is_none());
    }
}
