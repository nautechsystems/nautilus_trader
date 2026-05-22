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

//! Deribit HTTP API query parameter builders.

use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use super::models::{DeribitCurrency, DeribitProductType};

/// Instrument kind filter for `/public/get_expirations` endpoint.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeribitExpirationKind {
    /// Future contract expirations.
    Future,
    /// Option contract expirations.
    Option,
    /// All supported instrument kinds.
    Any,
    /// Future combo expirations.
    FutureCombo,
    /// Option combo expirations.
    OptionCombo,
}

/// Query parameters for `/public/get_expirations` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetExpirationsParams {
    /// Settlement currency, `any`, or `grouped`.
    pub currency: String,
    /// Instrument kind filter.
    pub kind: DeribitExpirationKind,
    /// Optional currency pair filter (e.g., "btc_usd").
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub currency_pair: Option<String>,
}

impl GetExpirationsParams {
    /// Creates a new builder for [`GetExpirationsParams`].
    #[must_use]
    pub fn builder() -> GetExpirationsParamsBuilder {
        GetExpirationsParamsBuilder::default()
    }

    /// Creates parameters for a settlement currency and product kind.
    #[must_use]
    pub fn new(currency: impl Into<String>, kind: DeribitExpirationKind) -> Self {
        Self {
            currency: currency.into(),
            kind,
            currency_pair: None,
        }
    }
}

/// Query parameters for `/public/get_instruments` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetInstrumentsParams {
    /// Currency filter
    pub currency: DeribitCurrency,
    /// Optional product type filter
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub kind: Option<DeribitProductType>,
    /// Whether to include expired instruments
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub expired: Option<bool>,
}

impl GetInstrumentsParams {
    /// Creates a new builder for [`GetInstrumentsParams`].
    #[must_use]
    pub fn builder() -> GetInstrumentsParamsBuilder {
        GetInstrumentsParamsBuilder::default()
    }

    /// Creates parameters for a specific currency.
    #[must_use]
    pub fn new(currency: DeribitCurrency) -> Self {
        Self {
            currency,
            kind: None,
            expired: None,
        }
    }

    /// Creates parameters for a specific currency and product type.
    #[must_use]
    pub fn with_kind(currency: DeribitCurrency, kind: DeribitProductType) -> Self {
        Self {
            currency,
            kind: Some(kind),
            expired: None,
        }
    }
}

/// Query parameters for `/public/get_instrument` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
pub struct GetInstrumentParams {
    /// Instrument name (e.g., "BTC-PERPETUAL", "ETH-25MAR23-2000-C")
    pub instrument_name: String,
}

/// Query parameters for `/public/get_combos` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetCombosParams {
    /// Currency to query.
    pub currency: DeribitCurrency,
}

impl GetCombosParams {
    /// Creates a new builder for [`GetCombosParams`].
    #[must_use]
    pub fn builder() -> GetCombosParamsBuilder {
        GetCombosParamsBuilder::default()
    }

    /// Creates parameters for a specific currency.
    #[must_use]
    pub fn new(currency: DeribitCurrency) -> Self {
        Self { currency }
    }
}

/// Query parameters for `/private/get_account_summaries` endpoint.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct GetAccountSummariesParams {
    /// The user id for the subaccount.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subaccount_id: Option<String>,
    /// Include extended fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extended: Option<bool>,
}

impl GetAccountSummariesParams {
    /// Creates a new instance with both subaccount ID and extended flag.
    #[must_use]
    pub fn new(subaccount_id: String, extended: bool) -> Self {
        Self {
            subaccount_id: Some(subaccount_id),
            extended: Some(extended),
        }
    }
}

/// Query parameters for `/public/get_last_trades_by_instrument_and_time` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetLastTradesByInstrumentAndTimeParams {
    /// Instrument name (e.g., "BTC-PERPETUAL")
    pub instrument_name: String,
    /// The earliest timestamp to return result from (milliseconds since the UNIX epoch)
    pub start_timestamp: i64,
    /// The most recent timestamp to return result from (milliseconds since the UNIX epoch)
    pub end_timestamp: i64,
    /// Number of requested items, default - 10, maximum - 1000
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub count: Option<u32>,
    /// Direction of results sorting: "asc", "desc", or "default"
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub sorting: Option<String>,
}

impl GetLastTradesByInstrumentAndTimeParams {
    /// Creates a new instance with the required parameters.
    #[must_use]
    pub fn new(
        instrument_name: impl Into<String>,
        start_timestamp: i64,
        end_timestamp: i64,
        count: Option<u32>,
        sorting: Option<String>,
    ) -> Self {
        Self {
            instrument_name: instrument_name.into(),
            start_timestamp,
            end_timestamp,
            count,
            sorting,
        }
    }
}

/// Query parameters for `/public/get_last_trades_by_currency` endpoint.
///
/// Mirrors the per-instrument variant but selects trades by currency and
/// (optionally) product kind. Required to backfill combo trades, which are
/// not accessible via the instrument-scoped endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetLastTradesByCurrencyParams {
    /// Currency to query.
    pub currency: DeribitCurrency,
    /// Optional product kind filter (e.g., `option_combo`, `future_combo`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub kind: Option<DeribitProductType>,
    /// First trade ID (inclusive) of the range to fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub start_id: Option<String>,
    /// Last trade ID (inclusive) of the range to fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub end_id: Option<String>,
    /// Maximum number of trades to return (default 10, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub count: Option<u32>,
    /// Whether to include expired-instrument trades.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub include_old: Option<bool>,
    /// Direction of results sorting: `asc`, `desc`, or `default`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub sorting: Option<String>,
}

impl GetLastTradesByCurrencyParams {
    /// Creates a new builder for [`GetLastTradesByCurrencyParams`].
    #[must_use]
    pub fn builder() -> GetLastTradesByCurrencyParamsBuilder {
        GetLastTradesByCurrencyParamsBuilder::default()
    }

    /// Creates parameters for a specific currency.
    #[must_use]
    pub fn new(currency: DeribitCurrency) -> Self {
        Self {
            currency,
            kind: None,
            start_id: None,
            end_id: None,
            count: None,
            include_old: None,
            sorting: None,
        }
    }

    /// Creates parameters for a specific currency and product kind.
    #[must_use]
    pub fn with_kind(currency: DeribitCurrency, kind: DeribitProductType) -> Self {
        Self {
            currency,
            kind: Some(kind),
            start_id: None,
            end_id: None,
            count: None,
            include_old: None,
            sorting: None,
        }
    }
}

/// Query parameters for `/public/get_tradingview_chart_data` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetTradingViewChartDataParams {
    /// Instrument name (e.g., "BTC-PERPETUAL")
    pub instrument_name: String,
    /// The earliest timestamp to return result from (milliseconds since UNIX epoch)
    pub start_timestamp: i64,
    /// The most recent timestamp to return result from (milliseconds since UNIX epoch)
    pub end_timestamp: i64,
    /// Chart bars resolution given in full minutes or keyword "1D"
    /// Supported resolutions: 1, 3, 5, 10, 15, 30, 60, 120, 180, 360, 720, 1D
    pub resolution: String,
}

impl GetTradingViewChartDataParams {
    /// Creates new parameters for chart data request.
    #[must_use]
    pub fn new(
        instrument_name: String,
        start_timestamp: i64,
        end_timestamp: i64,
        resolution: String,
    ) -> Self {
        Self {
            instrument_name,
            start_timestamp,
            end_timestamp,
            resolution,
        }
    }
}

/// Query parameters for `/public/get_order_book` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetOrderBookParams {
    /// Instrument name (e.g., "BTC-PERPETUAL")
    pub instrument_name: String,
    /// The number of entries to return for bids and asks.
    /// Valid values: 1, 5, 10, 20, 50, 100, 1000, 10000
    /// Maximum: 10000
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub depth: Option<u32>,
}

impl GetOrderBookParams {
    /// Creates parameters with required fields.
    #[must_use]
    pub fn new(instrument_name: String, depth: Option<u32>) -> Self {
        Self {
            instrument_name,
            depth,
        }
    }
}

/// Query parameters for `/private/get_order_state` endpoint.
/// Retrieves a single order by its ID.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetOrderStateParams {
    /// The order ID to look up.
    pub order_id: String,
}

impl GetOrderStateParams {
    /// Creates parameters for a specific order ID.
    #[must_use]
    pub fn new(order_id: impl Into<String>) -> Self {
        Self {
            order_id: order_id.into(),
        }
    }
}

/// Query parameters for `/private/get_open_orders` endpoint.
/// Retrieves all open orders across all currencies and instruments.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct GetOpenOrdersParams {}

impl GetOpenOrdersParams {
    /// Creates parameters to get all open orders.
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

/// Query parameters for `/private/get_open_orders_by_instrument` endpoint.
/// Retrieves open orders for a specific instrument.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetOpenOrdersByInstrumentParams {
    /// Instrument name (e.g., "BTC-PERPETUAL")
    pub instrument_name: String,
    /// Order type filter: "all", "limit", "stop_all", "stop_limit", "stop_market",
    /// "take_all", "take_limit", "take_market", "trailing_all", "trailing_stop"
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub r#type: Option<String>,
}

impl GetOpenOrdersByInstrumentParams {
    /// Creates parameters for a specific instrument.
    #[must_use]
    pub fn new(instrument_name: impl Into<String>) -> Self {
        Self {
            instrument_name: instrument_name.into(),
            r#type: None,
        }
    }
}

/// Query parameters for `/private/get_order_history_by_instrument` endpoint.
/// Retrieves historical orders for a specific instrument.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetOrderHistoryByInstrumentParams {
    /// Instrument name (e.g., "BTC-PERPETUAL")
    pub instrument_name: String,
    /// Number of requested items, default - 20
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub count: Option<u32>,
    /// Offset for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub offset: Option<u32>,
    /// Include orders older than 3 days
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub include_old: Option<bool>,
    /// Include unfilled orders
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub include_unfilled: Option<bool>,
}

impl GetOrderHistoryByInstrumentParams {
    /// Creates parameters for a specific instrument.
    #[must_use]
    pub fn new(instrument_name: impl Into<String>) -> Self {
        Self {
            instrument_name: instrument_name.into(),
            count: None,
            offset: None,
            include_old: None,
            include_unfilled: None,
        }
    }

    /// Creates a new builder for [`GetOrderHistoryByInstrumentParams`].
    #[must_use]
    pub fn builder() -> GetOrderHistoryByInstrumentParamsBuilder {
        GetOrderHistoryByInstrumentParamsBuilder::default()
    }
}

/// Query parameters for `/private/get_order_history_by_currency` endpoint.
/// Retrieves historical orders for a specific currency.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetOrderHistoryByCurrencyParams {
    /// Currency filter
    pub currency: DeribitCurrency,
    /// Optional product type filter
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub kind: Option<DeribitProductType>,
    /// Number of requested items, default - 20, maximum - 1000
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub count: Option<u32>,
    /// Offset for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub offset: Option<u32>,
    /// Include orders older than 3 days
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub include_old: Option<bool>,
    /// Include unfilled orders
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub include_unfilled: Option<bool>,
}

impl GetOrderHistoryByCurrencyParams {
    /// Creates parameters for a specific currency.
    #[must_use]
    pub fn new(currency: DeribitCurrency) -> Self {
        Self {
            currency,
            kind: None,
            count: None,
            offset: None,
            include_old: None,
            include_unfilled: None,
        }
    }

    /// Creates a new builder for [`GetOrderHistoryByCurrencyParams`].
    #[must_use]
    pub fn builder() -> GetOrderHistoryByCurrencyParamsBuilder {
        GetOrderHistoryByCurrencyParamsBuilder::default()
    }
}

/// Query parameters for `/private/get_user_trades_by_instrument_and_time` endpoint.
/// Retrieves user trades for a specific instrument within a time range.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetUserTradesByInstrumentAndTimeParams {
    /// Instrument name (e.g., "BTC-PERPETUAL")
    pub instrument_name: String,
    /// Start timestamp in milliseconds since UNIX epoch
    pub start_timestamp: i64,
    /// End timestamp in milliseconds since UNIX epoch
    pub end_timestamp: i64,
    /// Number of requested items, default - 10, maximum - 1000
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub count: Option<u32>,
    /// Direction of results sorting: "asc", "desc", or "default"
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub sorting: Option<String>,
}

impl GetUserTradesByInstrumentAndTimeParams {
    /// Creates parameters with required fields.
    #[must_use]
    pub fn new(
        instrument_name: impl Into<String>,
        start_timestamp: i64,
        end_timestamp: i64,
    ) -> Self {
        Self {
            instrument_name: instrument_name.into(),
            start_timestamp,
            end_timestamp,
            count: None,
            sorting: None,
        }
    }

    /// Creates a new builder for [`GetUserTradesByInstrumentAndTimeParams`].
    #[must_use]
    pub fn builder() -> GetUserTradesByInstrumentAndTimeParamsBuilder {
        GetUserTradesByInstrumentAndTimeParamsBuilder::default()
    }
}

/// Query parameters for `/private/get_user_trades_by_currency_and_time` endpoint.
/// Retrieves user trades for a specific currency within a time range.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetUserTradesByCurrencyAndTimeParams {
    /// Currency filter
    pub currency: DeribitCurrency,
    /// Start timestamp in milliseconds since UNIX epoch
    pub start_timestamp: i64,
    /// End timestamp in milliseconds since UNIX epoch
    pub end_timestamp: i64,
    /// Optional product type filter
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub kind: Option<DeribitProductType>,
    /// Number of requested items, default - 10, maximum - 1000
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub count: Option<u32>,
    /// Direction of results sorting: "asc", "desc", or "default"
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub sorting: Option<String>,
}

impl GetUserTradesByCurrencyAndTimeParams {
    /// Creates parameters with required fields.
    #[must_use]
    pub fn new(currency: DeribitCurrency, start_timestamp: i64, end_timestamp: i64) -> Self {
        Self {
            currency,
            start_timestamp,
            end_timestamp,
            kind: None,
            count: None,
            sorting: None,
        }
    }

    /// Creates a new builder for [`GetUserTradesByCurrencyAndTimeParams`].
    #[must_use]
    pub fn builder() -> GetUserTradesByCurrencyAndTimeParamsBuilder {
        GetUserTradesByCurrencyAndTimeParamsBuilder::default()
    }
}

/// Query parameters for `/public/get_book_summary_by_currency` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetBookSummaryByCurrencyParams {
    /// Currency filter (e.g., "BTC", "ETH")
    pub currency: String,
    /// Optional product type filter (e.g., "option", "future")
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub kind: Option<String>,
}

impl GetBookSummaryByCurrencyParams {
    /// Creates parameters for options book summaries for a given currency.
    #[must_use]
    pub fn options(currency: impl Into<String>) -> Self {
        Self {
            currency: currency.into(),
            kind: Some("option".to_string()),
        }
    }
}

/// Query parameters for `/public/ticker` endpoint.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetTickerParams {
    /// Instrument name (e.g., "BTC-28FEB26-65000-C")
    pub instrument_name: String,
}

/// Query parameters for `/private/get_positions` endpoint.
/// Retrieves positions for a specific currency.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetPositionsParams {
    /// Currency filter
    pub currency: DeribitCurrency,
    /// Optional product type filter
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub kind: Option<DeribitProductType>,
}

impl GetPositionsParams {
    /// Creates parameters for a specific currency.
    #[must_use]
    pub fn new(currency: DeribitCurrency) -> Self {
        Self {
            currency,
            kind: None,
        }
    }

    /// Creates a new builder for [`GetPositionsParams`].
    #[must_use]
    pub fn builder() -> GetPositionsParamsBuilder {
        GetPositionsParamsBuilder::default()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::{Value, json};

    use super::*;

    #[rstest]
    fn test_get_expirations_params_default_payload() {
        let params = GetExpirationsParams::new("BTC", DeribitExpirationKind::Option);
        let value: Value = serde_json::to_value(&params).unwrap();
        assert_eq!(value, json!({"currency": "BTC", "kind": "option"}));
    }

    #[rstest]
    fn test_get_expirations_params_full_payload() {
        let params = GetExpirationsParams::builder()
            .currency("grouped")
            .kind(DeribitExpirationKind::Any)
            .currency_pair("btc_usd")
            .build()
            .unwrap();
        let value: Value = serde_json::to_value(&params).unwrap();
        assert_eq!(
            value,
            json!({
                "currency": "grouped",
                "kind": "any",
                "currency_pair": "btc_usd",
            }),
        );
    }

    #[rstest]
    fn test_get_expirations_params_combo_kind_serialization() {
        let params = GetExpirationsParams::new("BTC", DeribitExpirationKind::OptionCombo);
        let value: Value = serde_json::to_value(&params).unwrap();
        assert_eq!(value, json!({"currency": "BTC", "kind": "option_combo"}));
    }

    #[rstest]
    fn test_get_combos_params_serialization() {
        let params = GetCombosParams::new(DeribitCurrency::BTC);
        let value: Value = serde_json::to_value(params).unwrap();
        assert_eq!(value, json!({"currency": "BTC"}));
    }

    #[rstest]
    fn test_get_last_trades_by_currency_params_default_omits_optionals() {
        // Only `currency` should appear on the wire when no optional fields
        // are set; skip_serializing_if = Option::is_none must elide the rest.
        let params = GetLastTradesByCurrencyParams::new(DeribitCurrency::BTC);
        let value: Value = serde_json::to_value(&params).unwrap();
        assert_eq!(value, json!({"currency": "BTC"}));
    }

    #[rstest]
    fn test_get_last_trades_by_currency_params_full_payload() {
        // All fields populated. Pins Deribit wire key names and value
        // serialization (DeribitProductType uses serde rename for combos).
        let params = GetLastTradesByCurrencyParams {
            currency: DeribitCurrency::BTC,
            kind: Some(DeribitProductType::FutureCombo),
            start_id: Some("100".to_string()),
            end_id: Some("200".to_string()),
            count: Some(50),
            include_old: Some(true),
            sorting: Some("asc".to_string()),
        };
        let value: Value = serde_json::to_value(&params).unwrap();
        assert_eq!(
            value,
            json!({
                "currency": "BTC",
                "kind": "future_combo",
                "start_id": "100",
                "end_id": "200",
                "count": 50,
                "include_old": true,
                "sorting": "asc",
            }),
        );
    }

    #[rstest]
    fn test_get_last_trades_by_currency_params_with_kind_constructor() {
        let params = GetLastTradesByCurrencyParams::with_kind(
            DeribitCurrency::ETH,
            DeribitProductType::OptionCombo,
        );
        let value: Value = serde_json::to_value(&params).unwrap();
        assert_eq!(value, json!({"currency": "ETH", "kind": "option_combo"}));
    }

    #[rstest]
    fn test_get_last_trades_by_currency_params_builder_partial() {
        let params = GetLastTradesByCurrencyParams::builder()
            .currency(DeribitCurrency::BTC)
            .kind(DeribitProductType::FutureCombo)
            .count(25_u32)
            .include_old(true)
            .build()
            .unwrap();
        let value: Value = serde_json::to_value(&params).unwrap();
        assert_eq!(
            value,
            json!({
                "currency": "BTC",
                "kind": "future_combo",
                "count": 25,
                "include_old": true,
            }),
        );
    }
}
