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

//! Query parameter builders for Lighter REST endpoints.

use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::common::enums::{
    LighterCandleResolution, LighterFundingResolution, LighterOrderBookFilter, LighterTradeType,
};

#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder, PartialEq, Eq)]
#[builder(setter(strip_option), default)]
pub struct LighterOrderBooksQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_id: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<LighterOrderBookFilter>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder, PartialEq, Eq)]
#[builder(setter(strip_option), default)]
pub struct LighterOrderBookDetailsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_id: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<LighterOrderBookFilter>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Builder, PartialEq, Eq)]
pub struct LighterOrderBookOrdersQuery {
    pub market_id: i16,
    pub limit: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, Builder, PartialEq, Eq)]
pub struct LighterRecentTradesQuery {
    pub market_id: i16,
    pub limit: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, Builder, PartialEq, Eq)]
#[builder(setter(strip_option))]
pub struct LighterCandlesQuery {
    pub market_id: i16,
    pub resolution: LighterCandleResolution,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub count_back: i64,
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_timestamp_to_end: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Builder, PartialEq, Eq)]
#[builder(setter(strip_option))]
pub struct LighterFundingsQuery {
    pub market_id: i16,
    pub resolution: LighterFundingResolution,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub count_back: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, Builder, PartialEq, Eq)]
#[builder(setter(strip_option), default)]
pub struct LighterTradesQuery {
    #[builder(setter(into, strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,
    #[builder(setter(into, strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_id: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_index: Option<i64>,
    pub sort_by: LighterTradeSortBy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_dir: Option<LighterSortDirection>,
    #[builder(setter(into, strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(rename = "from")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_timestamp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask_filter: Option<i8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<LighterTradeRole>,
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_type: Option<LighterTradeQueryType>,
    pub limit: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder, PartialEq, Eq)]
pub struct LighterNextNonceQuery {
    pub account_index: i64,
    pub api_key_index: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize, Builder, PartialEq, Eq)]
#[builder(setter(strip_option))]
pub struct LighterMakerOnlyApiKeysQuery {
    #[builder(default)]
    #[builder(setter(into, strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,
    #[builder(default)]
    #[builder(setter(into, strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    pub account_index: i64,
}

#[derive(Copy, Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LighterAccountLookup {
    #[default]
    Index,
    L1Address,
}

#[derive(Clone, Debug, Deserialize, Serialize, Builder, PartialEq, Eq)]
pub struct LighterAccountQuery {
    pub by: LighterAccountLookup,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Builder, PartialEq, Eq)]
#[builder(setter(strip_option))]
pub struct LighterAccountActiveOrdersQuery {
    #[builder(default)]
    #[builder(setter(into, strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,
    #[builder(default)]
    #[builder(setter(into, strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    pub account_index: i64,
    pub market_id: i16,
}

#[derive(Clone, Debug, Deserialize, Serialize, Builder, PartialEq, Eq)]
#[builder(setter(strip_option))]
pub struct LighterAccountInactiveOrdersQuery {
    #[builder(default)]
    #[builder(setter(into, strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,
    #[builder(default)]
    #[builder(setter(into, strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    pub account_index: i64,
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_id: Option<i16>,
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask_filter: Option<i8>,
    #[builder(default)]
    #[builder(setter(into, strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub between_timestamps: Option<String>,
    #[builder(default)]
    #[builder(setter(into, strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub limit: u16,
}

#[derive(Copy, Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LighterTradeSortBy {
    BlockHeight,
    Timestamp,
    #[default]
    TradeId,
}

#[derive(Copy, Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LighterSortDirection {
    #[default]
    Desc,
    Asc,
}

#[derive(Copy, Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LighterTradeRole {
    #[default]
    All,
    Maker,
    Taker,
}

#[derive(Copy, Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LighterTradeQueryType {
    #[default]
    All,
    Trade,
    Liquidation,
    Deleverage,
    MarketSettlement,
}

impl From<LighterTradeType> for LighterTradeQueryType {
    fn from(value: LighterTradeType) -> Self {
        match value {
            LighterTradeType::Trade => Self::Trade,
            LighterTradeType::Liquidation => Self::Liquidation,
            LighterTradeType::Deleverage => Self::Deleverage,
            LighterTradeType::MarketSettlement => Self::MarketSettlement,
        }
    }
}

impl Default for LighterTradesQuery {
    fn default() -> Self {
        Self {
            authorization: None,
            auth: None,
            market_id: None,
            account_index: None,
            order_index: None,
            sort_by: LighterTradeSortBy::default(),
            sort_dir: None,
            cursor: None,
            from_timestamp: None,
            ask_filter: None,
            role: None,
            trade_type: None,
            limit: 100,
            aggregate: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_order_books_query_serializes_filter() {
        let query = LighterOrderBooksQuery {
            market_id: Some(0),
            filter: Some(LighterOrderBookFilter::Perp),
        };

        let value = serde_json::to_value(query).unwrap();

        assert_eq!(value["market_id"], 0);
        assert_eq!(value["filter"], "perp");
    }

    #[rstest]
    fn test_order_book_details_query_serializes_filter() {
        let query = LighterOrderBookDetailsQueryBuilder::default()
            .market_id(1)
            .filter(LighterOrderBookFilter::Spot)
            .build()
            .unwrap();

        let value = serde_json::to_value(query).unwrap();

        assert_eq!(value["market_id"], 1);
        assert_eq!(value["filter"], "spot");
    }

    #[rstest]
    fn test_trades_query_renames_type_and_from() {
        let query = LighterTradesQuery {
            market_id: Some(0),
            from_timestamp: Some(1_700_000_000_000),
            trade_type: Some(LighterTradeQueryType::MarketSettlement),
            limit: 50,
            ..Default::default()
        };

        let value = serde_json::to_value(query).unwrap();

        assert_eq!(value["sort_by"], "trade_id");
        assert_eq!(value["from"], 1_700_000_000_000_i64);
        assert_eq!(value["type"], "market-settlement");
        assert_eq!(value["limit"], 50);
    }

    #[rstest]
    fn test_candles_query_serializes_resolution_and_timestamp_mode() {
        let query = LighterCandlesQueryBuilder::default()
            .market_id(0)
            .resolution(LighterCandleResolution::OneMinute)
            .start_timestamp(1_700_000_000_000)
            .end_timestamp(1_700_000_060_000)
            .count_back(500)
            .set_timestamp_to_end(false)
            .build()
            .unwrap();

        let value = serde_json::to_value(query).unwrap();

        assert_eq!(value["market_id"], 0);
        assert_eq!(value["resolution"], "1m");
        assert_eq!(value["start_timestamp"], 1_700_000_000_000_i64);
        assert_eq!(value["end_timestamp"], 1_700_000_060_000_i64);
        assert_eq!(value["count_back"], 500);
        assert_eq!(value["set_timestamp_to_end"], false);
    }

    #[rstest]
    fn test_fundings_query_serializes_resolution() {
        let query = LighterFundingsQueryBuilder::default()
            .market_id(0)
            .resolution(LighterFundingResolution::OneHour)
            .start_timestamp(1_700_000_000_000)
            .end_timestamp(1_700_003_600_000)
            .count_back(100)
            .build()
            .unwrap();

        let value = serde_json::to_value(query).unwrap();

        assert_eq!(value["market_id"], 0);
        assert_eq!(value["resolution"], "1h");
        assert_eq!(value["start_timestamp"], 1_700_000_000_000_i64);
        assert_eq!(value["end_timestamp"], 1_700_003_600_000_i64);
        assert_eq!(value["count_back"], 100);
    }

    #[rstest]
    fn test_order_book_orders_query_serializes_required_fields() {
        let query = LighterOrderBookOrdersQuery {
            market_id: 0,
            limit: 25,
        };

        let value = serde_json::to_value(query).unwrap();

        assert_eq!(value["market_id"], 0);
        assert_eq!(value["limit"], 25);
    }

    #[rstest]
    fn test_recent_trades_query_serializes_required_fields() {
        let query = LighterRecentTradesQueryBuilder::default()
            .market_id(1)
            .limit(10)
            .build()
            .unwrap();

        let value = serde_json::to_value(query).unwrap();

        assert_eq!(value["market_id"], 1);
        assert_eq!(value["limit"], 10);
    }

    #[rstest]
    fn test_trades_query_serializes_optional_filters() {
        let query = LighterTradesQuery {
            authorization: Some("bearer-token".to_string()),
            auth: Some("auth-token".to_string()),
            market_id: Some(0),
            account_index: Some(1234),
            order_index: Some(5678),
            sort_by: LighterTradeSortBy::Timestamp,
            sort_dir: Some(LighterSortDirection::Desc),
            cursor: Some("cursor-1".to_string()),
            from_timestamp: Some(1_700_000_000_000),
            ask_filter: Some(1),
            role: Some(LighterTradeRole::Maker),
            trade_type: Some(LighterTradeQueryType::from(LighterTradeType::Liquidation)),
            limit: 75,
            aggregate: Some(true),
        };

        let value = serde_json::to_value(query).unwrap();

        assert_eq!(value["authorization"], "bearer-token");
        assert_eq!(value["auth"], "auth-token");
        assert_eq!(value["market_id"], 0);
        assert_eq!(value["account_index"], 1234);
        assert_eq!(value["order_index"], 5678);
        assert_eq!(value["sort_by"], "timestamp");
        assert_eq!(value["sort_dir"], "desc");
        assert_eq!(
            serde_json::to_value(LighterSortDirection::Asc).unwrap(),
            "asc",
        );
        assert_eq!(value["cursor"], "cursor-1");
        assert_eq!(value["from"], 1_700_000_000_000_i64);
        assert_eq!(value["ask_filter"], 1);
        assert_eq!(value["role"], "maker");
        assert_eq!(value["type"], "liquidation");
        assert_eq!(value["limit"], 75);
        assert_eq!(value["aggregate"], true);
    }

    #[rstest]
    fn test_account_active_orders_query_serializes_required_fields() {
        let query = LighterAccountActiveOrdersQueryBuilder::default()
            .auth("auth-token")
            .account_index(123)
            .market_id(0)
            .build()
            .unwrap();

        let value = serde_json::to_value(query).unwrap();

        assert_eq!(value["auth"], "auth-token");
        assert_eq!(value["account_index"], 123);
        assert_eq!(value["market_id"], 0);
        assert!(value.get("authorization").is_none());
    }

    #[rstest]
    fn test_account_inactive_orders_query_serializes_optional_filters() {
        let query = LighterAccountInactiveOrdersQueryBuilder::default()
            .authorization("bearer-token")
            .account_index(123)
            .market_id(0)
            .ask_filter(1)
            .between_timestamps("1700000000-1700003600")
            .cursor("cursor-1")
            .limit(50)
            .build()
            .unwrap();

        let value = serde_json::to_value(query).unwrap();

        assert_eq!(value["authorization"], "bearer-token");
        assert_eq!(value["account_index"], 123);
        assert_eq!(value["market_id"], 0);
        assert_eq!(value["ask_filter"], 1);
        assert_eq!(value["between_timestamps"], "1700000000-1700003600",);
        assert_eq!(value["cursor"], "cursor-1");
        assert_eq!(value["limit"], 50);
        assert!(value.get("auth").is_none());
    }
}
