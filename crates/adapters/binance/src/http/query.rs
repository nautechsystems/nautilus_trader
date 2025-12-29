// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Binance HTTP query parameter builders.
//!
//! This module provides builder types for constructing query parameters
//! for Binance REST API endpoints.

use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::common::enums::BinanceIncomeType;

/// Query parameters for `GET /api/v3/exchangeInfo` (Spot).
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/general-endpoints>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceSpotExchangeInfoParams {
    /// Filter by single symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Filter by multiple symbols (JSON array format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols: Option<String>,
    /// Filter by permissions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<String>,
}

/// Kline interval enumeration.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BinanceKlineInterval {
    /// 1 second (only for spot).
    #[serde(rename = "1s")]
    Second1,
    /// 1 minute.
    #[default]
    #[serde(rename = "1m")]
    Minute1,
    /// 3 minutes.
    #[serde(rename = "3m")]
    Minute3,
    /// 5 minutes.
    #[serde(rename = "5m")]
    Minute5,
    /// 15 minutes.
    #[serde(rename = "15m")]
    Minute15,
    /// 30 minutes.
    #[serde(rename = "30m")]
    Minute30,
    /// 1 hour.
    #[serde(rename = "1h")]
    Hour1,
    /// 2 hours.
    #[serde(rename = "2h")]
    Hour2,
    /// 4 hours.
    #[serde(rename = "4h")]
    Hour4,
    /// 6 hours.
    #[serde(rename = "6h")]
    Hour6,
    /// 8 hours.
    #[serde(rename = "8h")]
    Hour8,
    /// 12 hours.
    #[serde(rename = "12h")]
    Hour12,
    /// 1 day.
    #[serde(rename = "1d")]
    Day1,
    /// 3 days.
    #[serde(rename = "3d")]
    Day3,
    /// 1 week.
    #[serde(rename = "1w")]
    Week1,
    /// 1 month.
    #[serde(rename = "1M")]
    Month1,
}

impl BinanceKlineInterval {
    /// Returns the string representation for API requests.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Second1 => "1s",
            Self::Minute1 => "1m",
            Self::Minute3 => "3m",
            Self::Minute5 => "5m",
            Self::Minute15 => "15m",
            Self::Minute30 => "30m",
            Self::Hour1 => "1h",
            Self::Hour2 => "2h",
            Self::Hour4 => "4h",
            Self::Hour6 => "6h",
            Self::Hour8 => "8h",
            Self::Hour12 => "12h",
            Self::Day1 => "1d",
            Self::Day3 => "3d",
            Self::Week1 => "1w",
            Self::Month1 => "1M",
        }
    }

    /// Returns the interval duration in milliseconds.
    #[must_use]
    pub const fn as_millis(self) -> i64 {
        match self {
            Self::Second1 => 1_000,
            Self::Minute1 => 60_000,
            Self::Minute3 => 180_000,
            Self::Minute5 => 300_000,
            Self::Minute15 => 900_000,
            Self::Minute30 => 1_800_000,
            Self::Hour1 => 3_600_000,
            Self::Hour2 => 7_200_000,
            Self::Hour4 => 14_400_000,
            Self::Hour6 => 21_600_000,
            Self::Hour8 => 28_800_000,
            Self::Hour12 => 43_200_000,
            Self::Day1 => 86_400_000,
            Self::Day3 => 259_200_000,
            Self::Week1 => 604_800_000,
            Self::Month1 => 2_592_000_000, // Approximate 30 days
        }
    }
}

impl std::fmt::Display for BinanceKlineInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Query parameters for `GET /api/v3/klines` (Spot) or `GET /fapi/v1/klines` (Futures).
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceKlinesParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Kline interval (required).
    pub interval: BinanceKlineInterval,
    /// Start time in milliseconds.
    #[serde(rename = "startTime", skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// End time in milliseconds.
    #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    /// Timezone offset (spot only).
    #[serde(rename = "timeZone", skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    /// Number of results (default 500, max 1000 for spot, 1500 for futures).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl Default for BinanceKlinesParams {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            interval: BinanceKlineInterval::Minute1,
            start_time: None,
            end_time: None,
            time_zone: None,
            limit: None,
        }
    }
}

/// Query parameters for `GET /api/v3/trades` (Spot) or `GET /fapi/v1/trades` (Futures).
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceTradesParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Number of trades to return (default 500, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for `GET /api/v3/aggTrades` (Spot) or `GET /fapi/v1/aggTrades` (Futures).
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceAggTradesParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Trade ID to fetch from (inclusive).
    #[serde(rename = "fromId", skip_serializing_if = "Option::is_none")]
    pub from_id: Option<i64>,
    /// Start time in milliseconds.
    #[serde(rename = "startTime", skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// End time in milliseconds.
    #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    /// Number of trades to return (default 500, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for `GET /api/v3/depth` (Spot) or `GET /fapi/v1/depth` (Futures).
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceDepthParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Depth limit (default 100, max 5000 for spot, max 1000 for futures).
    /// Valid values for spot: 5, 10, 20, 50, 100, 500, 1000, 5000.
    /// Valid values for futures: 5, 10, 20, 50, 100, 500, 1000.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for `GET /api/v3/ticker/24hr` (Spot) or `GET /fapi/v1/ticker/24hr` (Futures).
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceTicker24hrParams {
    /// Filter by single symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Filter by multiple symbols (JSON array format, spot only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols: Option<String>,
    /// Response type: FULL or MINI (spot only).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub response_type: Option<String>,
}

/// Query parameters for `GET /api/v3/ticker/price` or `GET /fapi/v1/ticker/price`.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinancePriceTickerParams {
    /// Filter by single symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Filter by multiple symbols (JSON array format, spot only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols: Option<String>,
}

/// Query parameters for `GET /api/v3/ticker/bookTicker` or `GET /fapi/v1/ticker/bookTicker`.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceBookTickerParams {
    /// Filter by single symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Filter by multiple symbols (JSON array format, spot only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols: Option<String>,
}

/// Query parameters for `GET /fapi/v1/premiumIndex` or `GET /dapi/v1/premiumIndex`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Mark-Price>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceMarkPriceParams {
    /// Filter by single symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

/// Query parameters for `GET /fapi/v1/fundingRate` or `GET /dapi/v1/fundingRate`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Get-Funding-Rate-History>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceFundingRateParams {
    /// Trading symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Start time in milliseconds.
    #[serde(rename = "startTime", skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// End time in milliseconds.
    #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    /// Number of results (default 100, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for `GET /fapi/v1/openInterest` or `GET /dapi/v1/openInterest`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Open-Interest>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into))]
pub struct BinanceOpenInterestParams {
    /// Trading symbol (required).
    pub symbol: String,
}

/// Query parameters for `GET /fapi/v2/balance` or `GET /dapi/v1/balance`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data/account>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceFuturesBalanceParams {
    /// Filter by asset (e.g., "USDT").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `GET /fapi/v2/positionRisk` or `GET /dapi/v1/positionRisk`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data/account#position-information-v2-user_data>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinancePositionRiskParams {
    /// Filter by symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `GET /fapi/v1/income` or `GET /dapi/v1/income`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data/account#income-history-user_data>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceIncomeHistoryParams {
    /// Filter by symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Income type filter (e.g., FUNDING_FEE).
    #[serde(rename = "incomeType", skip_serializing_if = "Option::is_none")]
    pub income_type: Option<BinanceIncomeType>,
    /// Start time in milliseconds.
    #[serde(rename = "startTime", skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// End time in milliseconds.
    #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    /// Maximum number of rows (default 100, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `GET /fapi/v1/userTrades` or `GET /dapi/v1/userTrades`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data/trade#account-trade-list-user_data>
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceUserTradesParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Start time in milliseconds.
    #[serde(rename = "startTime", skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// End time in milliseconds.
    #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    /// Trade ID to fetch from (inclusive).
    #[serde(rename = "fromId", skip_serializing_if = "Option::is_none")]
    pub from_id: Option<i64>,
    /// Number of trades to return (default 500, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `GET /fapi/v1/openOrders` or `GET /dapi/v1/openOrders`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data/order#current-all-open-orders-user_data>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceOpenOrdersParams {
    /// Filter by symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `GET /fapi/v1/order` or `GET /dapi/v1/order`.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data/order#query-order-user_data>
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceOrderQueryParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Order ID.
    #[serde(rename = "orderId", skip_serializing_if = "Option::is_none")]
    pub order_id: Option<i64>,
    /// Orig client order ID.
    #[serde(rename = "origClientOrderId", skip_serializing_if = "Option::is_none")]
    pub orig_client_order_id: Option<String>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_kline_interval_as_str() {
        assert_eq!(BinanceKlineInterval::Second1.as_str(), "1s");
        assert_eq!(BinanceKlineInterval::Minute1.as_str(), "1m");
        assert_eq!(BinanceKlineInterval::Hour1.as_str(), "1h");
        assert_eq!(BinanceKlineInterval::Day1.as_str(), "1d");
        assert_eq!(BinanceKlineInterval::Week1.as_str(), "1w");
        assert_eq!(BinanceKlineInterval::Month1.as_str(), "1M");
    }

    #[rstest]
    fn test_kline_interval_as_millis() {
        assert_eq!(BinanceKlineInterval::Second1.as_millis(), 1_000);
        assert_eq!(BinanceKlineInterval::Minute1.as_millis(), 60_000);
        assert_eq!(BinanceKlineInterval::Hour1.as_millis(), 3_600_000);
        assert_eq!(BinanceKlineInterval::Day1.as_millis(), 86_400_000);
    }

    #[rstest]
    fn test_klines_params_builder() {
        let params = BinanceKlinesParamsBuilder::default()
            .symbol("BTCUSDT")
            .interval(BinanceKlineInterval::Hour1)
            .start_time(1_700_000_000_000i64)
            .limit(100u32)
            .build()
            .unwrap();

        assert_eq!(params.symbol, "BTCUSDT");
        assert_eq!(params.interval, BinanceKlineInterval::Hour1);
        assert_eq!(params.start_time, Some(1_700_000_000_000));
        assert_eq!(params.limit, Some(100));
    }

    #[rstest]
    fn test_trades_params_builder() {
        let params = BinanceTradesParamsBuilder::default()
            .symbol("ETHUSDT")
            .limit(500u32)
            .build()
            .unwrap();

        assert_eq!(params.symbol, "ETHUSDT");
        assert_eq!(params.limit, Some(500));
    }

    #[rstest]
    fn test_depth_params_builder() {
        let params = BinanceDepthParamsBuilder::default()
            .symbol("BTCUSDT")
            .limit(100u32)
            .build()
            .unwrap();

        assert_eq!(params.symbol, "BTCUSDT");
        assert_eq!(params.limit, Some(100));
    }

    #[rstest]
    fn test_ticker_params_serialization() {
        let params = BinanceTicker24hrParams {
            symbol: Some("BTCUSDT".to_string()),
            symbols: None,
            response_type: None,
        };

        let serialized = serde_urlencoded::to_string(&params).unwrap();
        assert_eq!(serialized, "symbol=BTCUSDT");
    }

    #[rstest]
    fn test_order_query_params_builder() {
        let params = BinanceOrderQueryParamsBuilder::default()
            .symbol("BTCUSDT")
            .order_id(12345_i64)
            .recv_window(5_000_u64)
            .build()
            .unwrap();

        assert_eq!(params.symbol, "BTCUSDT");
        assert_eq!(params.order_id, Some(12345));
        assert_eq!(params.recv_window, Some(5_000));
    }

    #[rstest]
    fn test_income_history_params_serialization() {
        let params = BinanceIncomeHistoryParamsBuilder::default()
            .symbol("ETHUSDT")
            .income_type(BinanceIncomeType::FundingFee)
            .limit(50_u32)
            .build()
            .unwrap();

        let serialized = serde_urlencoded::to_string(&params).unwrap();
        assert_eq!(serialized, "symbol=ETHUSDT&incomeType=FUNDING_FEE&limit=50");
    }

    #[rstest]
    fn test_open_orders_params_builder() {
        let params = BinanceOpenOrdersParamsBuilder::default()
            .symbol("BNBUSDT")
            .build()
            .unwrap();

        assert_eq!(params.symbol.as_deref(), Some("BNBUSDT"));
        assert!(params.recv_window.is_none());
    }
}
