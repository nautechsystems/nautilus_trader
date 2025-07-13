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

use derive_builder::Builder;
use serde::{self, Deserialize, Serialize};

use crate::common::enums::{
    OKXInstrumentType, OKXOrderStatus, OKXPositionMode, OKXPositionSide, OKXTradeMode,
};

#[allow(dead_code)] // Under development
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
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetCandlesticksParams {
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: String,
    /// Time interval, e.g. "1m", "5m", "1H".
    pub bar: String,
    /// Pagination: fetch records after this timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Pagination: fetch records before this timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// Maximum number of records to return (default 100, max 300 for regular candles, max 100 for history).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
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
    pub ord_type: Option<String>,
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
