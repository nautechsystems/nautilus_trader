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

//! Request parameter structures for the OKX **v5 REST API**.
//!
//! Each struct corresponds 1-to-1 with an OKX REST endpoint and is annotated
//! using `serde` so that it can be serialized directly into the query string
//! or request body expected by the exchange.
//!
//! The inline documentation repeats the required/optional fields described in
//! the [official OKX documentation](https://www.okx.com/docs-v5/en/) and, where
//! beneficial, links to the exact endpoint section.  All links point to the
//! English version.
//!
//! Example – building a request for historical trades:
//! ```rust
//! use nautilus_okx::http::query::{GetTradesParams, GetTradesParamsBuilder};
//!
//! let params = GetTradesParamsBuilder::default()
//!     .inst_id("BTC-USDT")
//!     .limit(200)
//!     .build()
//!     .unwrap();
//! ```
//!
//! Once built these parameter structs are passed to `OKXHttpClient::get`/`post`
//! where they are automatically serialized.

use derive_builder::Builder;
use serde::{self, Deserialize, Serialize};

use crate::{
    common::enums::{
        OKXInstrumentType, OKXOrderStatus, OKXOrderType, OKXPositionMode, OKXPositionSide,
        OKXTradeMode,
    },
    http::error::BuildError,
};

#[allow(dead_code, reason = "Under development")]
fn serialize_string_vec<S>(values: &Option<Vec<String>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match values {
        Some(vec) => serializer.serialize_str(&vec.join(",")),
        None => serializer.serialize_none(),
    }
}

/// Parameters for the POST /api/v5/account/set-position-mode endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct SetPositionModeParams {
    /// Position mode: "net_mode" or "long_short_mode".
    #[serde(rename = "posMode")]
    pub pos_mode: OKXPositionMode,
}

/// Parameters for the GET /api/v5/public/position-tiers endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetPositionTiersParams {
    /// Instrument type: MARGIN, SWAP, FUTURES, OPTION.
    pub inst_type: OKXInstrumentType,
    /// Trading mode, valid values: cross, isolated.
    pub td_mode: OKXTradeMode,
    /// Underlying, required for SWAP/FUTURES/OPTION
    /// Single underlying or multiple underlyings (no more than 3) separated with comma.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uly: Option<String>,
    /// Instrument family, required for SWAP/FUTURES/OPTION
    /// Single instrument family or multiple families (no more than 5) separated with comma.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_family: Option<String>,
    /// Specific instrument ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<String>,
    /// Margin currency, only applicable to cross MARGIN.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ccy: Option<String>,
    /// Tiers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

/// Parameters for the GET /api/v5/public/instruments endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetInstrumentsParams {
    /// Instrument type: SPOT, MARGIN, SWAP, FUTURES, OPTION.
    pub inst_type: OKXInstrumentType,
    /// Underlying. Only applicable to FUTURES/SWAP/OPTION.
    /// If instType is OPTION, either uly or instFamily is required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uly: Option<String>,
    /// Instrument family. Only applicable to FUTURES/SWAP/OPTION.
    /// If instType is OPTION, either uly or instFamily is required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_family: Option<String>,
    /// Instrument ID, e.g. BTC-USD-SWAP.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<String>,
}

/// Parameters for the GET /api/v5/market/history-trades endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetTradesParams {
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: String,
    /// Pagination: fetch records after this timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Pagination: fetch records before this timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Maximum number of records to return (default 100, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Parameters for the GET /api/v5/market/history-candles endpoint.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetCandlesticksParams {
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: String,
    /// Time interval, e.g. "1m", "5m", "1H".
    pub bar: String,
    /// Pagination: fetch records after this timestamp (milliseconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "after")]
    pub after_ms: Option<i64>,
    /// Pagination: fetch records before this timestamp (milliseconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "before")]
    pub before_ms: Option<i64>,
    /// Maximum number of records to return (default 100, max 300 for regular candles, max 100 for history).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Builder for GetCandlesticksParams with validation.
#[derive(Debug, Default)]
pub struct GetCandlesticksParamsBuilder {
    inst_id: Option<String>,
    bar: Option<String>,
    after_ms: Option<i64>,
    before_ms: Option<i64>,
    limit: Option<u32>,
}

impl GetCandlesticksParamsBuilder {
    /// Sets the instrument ID.
    pub fn inst_id(&mut self, inst_id: impl Into<String>) -> &mut Self {
        self.inst_id = Some(inst_id.into());
        self
    }

    /// Sets the bar interval.
    pub fn bar(&mut self, bar: impl Into<String>) -> &mut Self {
        self.bar = Some(bar.into());
        self
    }

    /// Sets the after timestamp (milliseconds).
    pub fn after_ms(&mut self, after_ms: i64) -> &mut Self {
        self.after_ms = Some(after_ms);
        self
    }

    /// Sets the before timestamp (milliseconds).
    pub fn before_ms(&mut self, before_ms: i64) -> &mut Self {
        self.before_ms = Some(before_ms);
        self
    }

    /// Sets the limit.
    pub fn limit(&mut self, limit: u32) -> &mut Self {
        self.limit = Some(limit);
        self
    }

    /// Builds the parameters with embedded invariant validation.
    ///
    /// # Errors
    ///
    /// Returns an error if the parameters are invalid.
    pub fn build(&mut self) -> Result<GetCandlesticksParams, BuildError> {
        // Extract values from builder
        let inst_id = self.inst_id.clone().ok_or(BuildError::MissingInstId)?;
        let bar = self.bar.clone().ok_or(BuildError::MissingBar)?;
        let after_ms = self.after_ms;
        let before_ms = self.before_ms;
        let limit = self.limit;

        // ───────── Both cursors validation
        // OKX API doesn't support both 'after' and 'before' parameters together
        if after_ms.is_some() && before_ms.is_some() {
            return Err(BuildError::BothCursors);
        }

        // ───────── Cursor chronological validation
        // When both after_ms and before_ms are provided as time bounds:
        // - after_ms represents the start time (older bound)
        // - before_ms represents the end time (newer bound)
        // Therefore: after_ms < before_ms for valid time ranges
        if let (Some(after), Some(before)) = (after_ms, before_ms)
            && after >= before
        {
            return Err(BuildError::InvalidTimeRange {
                after_ms: after,
                before_ms: before,
            });
        }

        // ───────── Cursor unit (≤ 13 digits ⇒ milliseconds)
        if let Some(nanos) = after_ms
            && nanos.abs() > 9_999_999_999_999
        {
            return Err(BuildError::CursorIsNanoseconds);
        }

        if let Some(nanos) = before_ms
            && nanos.abs() > 9_999_999_999_999
        {
            return Err(BuildError::CursorIsNanoseconds);
        }

        // ───────── Limit validation
        // Note: Regular endpoint supports up to 300, history endpoint up to 100
        // This validation is conservative for safety across both endpoints
        if let Some(limit) = limit
            && limit > 300
        {
            return Err(BuildError::LimitTooHigh);
        }

        Ok(GetCandlesticksParams {
            inst_id,
            bar,
            after_ms,
            before_ms,
            limit,
        })
    }
}

/// Parameters for the GET /api/v5/public/mark-price.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetMarkPriceParams {
    /// Instrument type: MARGIN, SWAP, FUTURES, OPTION.
    pub inst_type: OKXInstrumentType,
    /// Underlying, required for SWAP/FUTURES/OPTION
    /// Single underlying or multiple underlyings (no more than 3) separated with comma.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uly: Option<String>,
    /// Instrument family, required for SWAP/FUTURES/OPTION
    /// Single instrument family or multiple families (no more than 5) separated with comma.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_family: Option<String>,
    /// Specific instrument ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<String>,
}

/// Parameters for the GET /api/v5/market/index-tickers.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetIndexTickerParams {
    /// Specific instrument ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<String>,
    /// Quote currency.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_ccy: Option<String>,
}

/// Parameters for the GET /api/v5/trade/order-history endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetOrderHistoryParams {
    /// Instrument type: SPOT, MARGIN, SWAP, FUTURES, OPTION.
    pub inst_type: OKXInstrumentType,
    /// Underlying, for FUTURES, SWAP, OPTION (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uly: Option<String>,
    /// Instrument family, for FUTURES, SWAP, OPTION (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_family: Option<String>,
    /// Instrument ID, e.g. "BTC-USD-SWAP" (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<String>,
    /// Order type: limit, market, post_only, fok, ioc (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ord_type: Option<OKXOrderType>,
    /// Order state: live, filled, canceled (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Pagination parameter: fetch records after this order ID or timestamp (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Pagination parameter: fetch records before this order ID or timestamp (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Maximum number of records to return (default 100, max 100) (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Parameters for the GET /api/v5/trade/orders-pending endpoint.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetOrderListParams {
    /// Instrument type: SPOT, MARGIN, SWAP, FUTURES, OPTION.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_type: Option<OKXInstrumentType>,
    /// Instrument ID, e.g. "BTC-USDT" (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<String>,
    /// Instrument family, e.g. "BTC-USD" (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_family: Option<String>,
    /// State to filter for (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<OKXOrderStatus>,
    /// Pagination - fetch records **after** this order ID (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Pagination - fetch records **before** this order ID (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Number of results per request (default 100, max 100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Parameters for the GET /api/v5/trade/order-algo-* endpoints.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetAlgoOrdersParams {
    /// Algo order identifier assigned by OKX (optional).
    #[serde(rename = "algoId", skip_serializing_if = "Option::is_none")]
    pub algo_id: Option<String>,
    /// Client supplied algo order identifier (optional).
    #[serde(rename = "algoClOrdId", skip_serializing_if = "Option::is_none")]
    pub algo_cl_ord_id: Option<String>,
    /// Instrument type: SPOT, MARGIN, SWAP, FUTURES, OPTION.
    pub inst_type: OKXInstrumentType,
    /// Specific instrument identifier (optional).
    #[serde(rename = "instId", skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<String>,
    /// Order type filter (optional).
    #[serde(rename = "ordType", skip_serializing_if = "Option::is_none")]
    pub ord_type: Option<OKXOrderType>,
    /// State filter (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<OKXOrderStatus>,
    /// Pagination cursor – fetch records after this value (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Pagination cursor – fetch records before this value (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Maximum number of records to return (optional, default 100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Parameters for the GET /api/v5/trade/fills endpoint (transaction details).
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetTransactionDetailsParams {
    /// Instrument type: SPOT, MARGIN, SWAP, FUTURES, OPTION (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_type: Option<OKXInstrumentType>,
    /// Instrument ID, e.g. "BTC-USDT" (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<String>,
    /// Order ID (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ord_id: Option<String>,
    /// Pagination of data to return records earlier than the requested ID (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Pagination of data to return records newer than the requested ID (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Number of results per request (optional, default 100, max 100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Parameters for the GET /api/v5/public/positions endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetPositionsParams {
    /// Instrument type: MARGIN, SWAP, FUTURES, OPTION.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_type: Option<OKXInstrumentType>,
    /// Specific instrument ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<String>,
    /// Single position ID or multiple position IDs (no more than 20) separated with comma.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos_id: Option<String>,
}

/// Parameters for the GET /api/v5/account/positions-history endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetPositionsHistoryParams {
    /// Instrument type: MARGIN, SWAP, FUTURES, OPTION.
    pub inst_type: OKXInstrumentType,
    /// Instrument ID, e.g. "BTC-USD-SWAP" (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_id: Option<String>,
    /// One or more position IDs, separated by commas (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos_id: Option<String>,
    /// Pagination parameter - requests records **after** this ID or timestamp (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Pagination parameter - requests records **before** this ID or timestamp (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Number of results per request (default 100, max 100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Parameters for the GET /api/v5/trade/orders-pending endpoint.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetPendingOrdersParams {
    /// Instrument type: SPOT, MARGIN, SWAP, FUTURES, OPTION.
    pub inst_type: OKXInstrumentType,
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: String,
    /// Position side (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos_side: Option<OKXPositionSide>,
}

/// Parameters for the GET /api/v5/trade/order endpoint (fetch order details).
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetOrderParams {
    /// Instrument type: SPOT, MARGIN, SWAP, FUTURES, OPTION.
    pub inst_type: OKXInstrumentType,
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: String,
    /// Exchange-assigned order ID (optional if client order ID is provided).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ord_id: Option<String>,
    /// User-assigned client order ID (optional if order ID is provided).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// Position side (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos_side: Option<OKXPositionSide>,
}

/// Parameters for the GET /api/v5/account/trade-fee endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetTradeFeeParams {
    /// Instrument type: SPOT, MARGIN, SWAP, FUTURES, OPTION.
    pub inst_type: OKXInstrumentType,
    /// Underlying, required for SWAP/FUTURES/OPTION (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uly: Option<String>,
    /// Instrument family, required for SWAP/FUTURES/OPTION (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_family: Option<String>,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_optional_parameters_are_omitted_when_none() {
        let mut builder = GetCandlesticksParamsBuilder::default();
        builder.inst_id("BTC-USDT-SWAP");
        builder.bar("1m");

        let params = builder.build().unwrap();
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert_eq!(
            qs, "instId=BTC-USDT-SWAP&bar=1m",
            "unexpected optional parameters were serialized: {qs}",
        );
    }

    #[rstest]
    fn test_no_literal_none_strings_leak_into_query_string() {
        let mut builder = GetCandlesticksParamsBuilder::default();
        builder.inst_id("BTC-USDT-SWAP");
        builder.bar("1m");

        let params = builder.build().unwrap();
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert!(
            !qs.contains("None"),
            "found literal \"None\" in query string: {qs}",
        );
        assert!(
            !qs.contains("after=") && !qs.contains("before=") && !qs.contains("limit="),
            "empty optional parameters must be omitted entirely: {qs}",
        );
    }

    #[rstest]
    fn test_cursor_nanoseconds_rejected() {
        // 2025-07-01T00:00:00Z in *nanoseconds* on purpose.
        let after_nanos = 1_725_307_200_000_000_000i64;

        let mut builder = GetCandlesticksParamsBuilder::default();
        builder.inst_id("BTC-USDT-SWAP");
        builder.bar("1m");
        builder.after_ms(after_nanos);

        // This should fail because nanoseconds > 13 digits
        let result = builder.build();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nanoseconds"));
    }

    #[rstest]
    fn test_both_cursors_rejected() {
        let mut builder = GetCandlesticksParamsBuilder::default();
        builder.inst_id("BTC-USDT-SWAP");
        builder.bar("1m");
        builder.after_ms(1725307200000);
        builder.before_ms(1725393600000);

        // Both cursors should be rejected
        let result = builder.build();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("both"));
    }

    #[rstest]
    fn test_limit_exceeds_maximum_rejected() {
        let mut builder = GetCandlesticksParamsBuilder::default();
        builder.inst_id("BTC-USDT-SWAP");
        builder.bar("1m");
        builder.limit(301u32); // Exceeds maximum limit

        // Limit should be rejected
        let result = builder.build();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("300"));
    }

    #[rstest]
    #[case(1725307200000, "after=1725307200000")] // 13 digits = milliseconds
    #[case(1725307200, "after=1725307200")] // 10 digits = seconds
    #[case(1725307, "after=1725307")] // 7 digits = also valid
    fn test_valid_millisecond_cursor_passes(#[case] timestamp: i64, #[case] expected: &str) {
        let mut builder = GetCandlesticksParamsBuilder::default();
        builder.inst_id("BTC-USDT-SWAP");
        builder.bar("1m");
        builder.after_ms(timestamp);

        let params = builder.build().unwrap();
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert!(qs.contains(expected));
    }

    #[rstest]
    #[case(1, "limit=1")]
    #[case(50, "limit=50")]
    #[case(100, "limit=100")]
    #[case(300, "limit=300")] // Maximum allowed limit
    fn test_valid_limit_passes(#[case] limit: u32, #[case] expected: &str) {
        let mut builder = GetCandlesticksParamsBuilder::default();
        builder.inst_id("BTC-USDT-SWAP");
        builder.bar("1m");
        builder.limit(limit);

        let params = builder.build().unwrap();
        let qs = serde_urlencoded::to_string(&params).unwrap();
        assert!(qs.contains(expected));
    }
}
