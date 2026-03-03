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

use ahash::AHashMap;
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

/// Single cancel response from `DELETE /order`.
#[derive(Clone, Debug, Deserialize)]
pub struct CancelResponse {
    #[serde(default)]
    pub not_canceled: Option<String>,
}

/// Batch cancel response from `DELETE /orders`, `DELETE /cancel-all`, and
/// `DELETE /cancel-market-orders`.
#[derive(Clone, Debug, Deserialize)]
pub struct BatchCancelResponse {
    #[serde(default)]
    pub canceled: Vec<String>,
    #[serde(default)]
    pub not_canceled: AHashMap<String, Option<String>>,
}

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

/// Query parameters for Gamma API `GET /markets`.
#[derive(Clone, Debug, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetGammaMarketsParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ascending: Option<bool>,
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
        let ba: BalanceAllowance = load("http_balance_allowance_collateral.json");

        assert_eq!(ba.balance, dec!(1000.000000));
        assert_eq!(ba.allowance, Some(dec!(999999999.000000)));
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

        assert!(resp.not_canceled.is_none());
    }

    #[rstest]
    fn test_cancel_response_failed() {
        let resp: CancelResponse = load("http_cancel_response_failed.json");

        assert_eq!(
            resp.not_canceled.as_deref(),
            Some("already canceled or matched")
        );
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
