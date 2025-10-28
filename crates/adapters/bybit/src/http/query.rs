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

//! Builder types for Bybit REST query parameters and filters.

use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::common::enums::{
    BybitAccountType, BybitExecType, BybitInstrumentStatus, BybitKlineInterval, BybitMarginMode,
    BybitOptionType, BybitOrderSide, BybitOrderStatus, BybitOrderType, BybitPositionIdx,
    BybitPositionMode, BybitProductType, BybitTimeInForce, BybitTpSlMode, BybitTriggerDirection,
    BybitTriggerType,
};

/// Query parameters for `GET /v5/market/instruments-info`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(default)]
#[builder(setter(into))]
pub struct BybitInstrumentsInfoParams {
    pub category: BybitProductType,
    #[builder(setter(strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[builder(setter(strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<BybitInstrumentStatus>,
    #[builder(setter(strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_coin: Option<String>,
    #[builder(setter(strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[builder(setter(strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Query parameters for `GET /v5/market/tickers`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/tickers>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitTickersParams {
    pub category: BybitProductType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp_date: Option<String>,
}

/// Query parameters for `GET /v5/market/kline`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/kline>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(setter(into, strip_option), default)]
pub struct BybitKlinesParams {
    pub category: BybitProductType,
    pub symbol: String,
    pub interval: BybitKlineInterval,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl Default for BybitKlinesParams {
    fn default() -> Self {
        Self {
            category: BybitProductType::Linear,
            symbol: String::new(),
            interval: BybitKlineInterval::Minute1,
            start: None,
            end: None,
            limit: None,
        }
    }
}

/// Query parameters for `GET /v5/market/recent-trade`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/recent-trade>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(setter(into, strip_option), default)]
pub struct BybitTradesParams {
    pub category: BybitProductType,
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_type: Option<BybitOptionType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl Default for BybitTradesParams {
    fn default() -> Self {
        Self {
            category: BybitProductType::Linear,
            symbol: String::new(),
            base_coin: None,
            option_type: None,
            limit: None,
        }
    }
}

/// Query parameters for `GET /v5/asset/coin/query-info`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/asset/coin/query-info>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitCoinInfoParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
}

/// Query parameters for `GET /v5/account/fee-rate`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/fee-rate>
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(default)]
#[builder(setter(into))]
pub struct BybitFeeRateParams {
    pub category: BybitProductType,
    #[builder(setter(strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[builder(setter(strip_option))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_coin: Option<String>,
}

/// Query parameters for `GET /v5/account/wallet-balance`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/wallet-balance>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitWalletBalanceParams {
    pub account_type: BybitAccountType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
}

/// Query parameters for `GET /v5/position/list`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/position/position>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitPositionListParams {
    pub category: BybitProductType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle_coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Body parameters for `POST /v5/account/set-margin-mode`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/set-margin-mode>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitSetMarginModeParams {
    pub set_margin_mode: BybitMarginMode,
}

/// Body parameters for `POST /v5/position/set-leverage`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/position/leverage>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitSetLeverageParams {
    pub category: BybitProductType,
    pub symbol: String,
    pub buy_leverage: String,
    pub sell_leverage: String,
}

/// Body parameters for `POST /v5/position/switch-mode`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/position/position-mode>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitSwitchModeParams {
    pub category: BybitProductType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    pub mode: BybitPositionMode,
}

/// Body parameters for `POST /v5/position/trading-stop`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/position/trading-stop>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitSetTradingStopParams {
    pub category: BybitProductType,
    pub symbol: String,
    pub position_idx: BybitPositionIdx,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trailing_stop: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_price: Option<String>,
    pub tpsl_mode: BybitTpSlMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_limit_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_limit_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_order_type: Option<BybitOrderType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_order_type: Option<BybitOrderType>,
}

/// Order entry payload for `POST /v5/order/create-batch`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/batch-place>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitBatchPlaceOrderEntry {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_leverage: Option<i32>,
    pub side: BybitOrderSide,
    pub order_type: BybitOrderType,
    pub qty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_direction: Option<BybitTriggerDirection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_iv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<BybitTimeInForce>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_idx: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option))]
    pub order_link_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_on_trigger: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smp_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mmp: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tpsl_mode: Option<BybitTpSlMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_limit_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_limit_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_order_type: Option<BybitOrderType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_order_type: Option<BybitOrderType>,
}

/// Body parameters for `POST /v5/order/create-batch`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/batch-place>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitBatchPlaceOrderParams {
    pub category: BybitProductType,
    pub request: Vec<BybitBatchPlaceOrderEntry>,
}

/// Body parameters for `POST /v5/order/create`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/create-order>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitPlaceOrderParams {
    #[serde(flatten)]
    pub order: BybitBatchPlaceOrderEntry,
    pub category: BybitProductType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slippage_tolerance_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slippage_tolerance: Option<String>,
}

/// Amend entry for `POST /v5/order/amend-batch`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/batch-amend>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitBatchAmendOrderEntry {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option))]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option))]
    pub order_link_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_iv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tpsl_mode: Option<BybitTpSlMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_by: Option<BybitTriggerType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_limit_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_limit_price: Option<String>,
}

/// Body parameters for `POST /v5/order/amend-batch`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/batch-amend>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitBatchAmendOrderParams {
    pub category: BybitProductType,
    pub request: Vec<BybitBatchAmendOrderEntry>,
}

/// Body parameters for `POST /v5/order/amend`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/amend-order>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitAmendOrderParams {
    #[serde(flatten)]
    pub order: BybitBatchAmendOrderEntry,
    pub category: BybitProductType,
}

/// Cancel entry for `POST /v5/order/cancel-batch`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/cancel-batch>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitBatchCancelOrderEntry {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option))]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option))]
    pub order_link_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_filter: Option<String>,
}

/// Body parameters for `POST /v5/order/cancel-batch`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/cancel-batch>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitBatchCancelOrderParams {
    pub category: BybitProductType,
    pub request: Vec<BybitBatchCancelOrderEntry>,
}

/// Body parameters for `POST /v5/order/cancel`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/cancel-order>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitCancelOrderParams {
    #[serde(flatten)]
    pub order: BybitBatchCancelOrderEntry,
    pub category: BybitProductType,
}

/// Body parameters for `POST /v5/order/cancel-all`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/cancel-all>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitCancelAllOrdersParams {
    pub category: BybitProductType,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option))]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle_coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_order_type: Option<String>,
}

/// Query parameters for `GET /v5/order/realtime`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/realtime>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitOpenOrdersParams {
    pub category: BybitProductType,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option))]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle_coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option))]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option))]
    pub order_link_id: Option<String>,
}

/// Query parameters for `GET /v5/order/history`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/order-list>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitOrderHistoryParams {
    pub category: BybitProductType,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option))]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle_coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_link_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_only: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_status: Option<BybitOrderStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "endTime")]
    pub end_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option))]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Query parameters for `GET /v5/execution/list`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/execution-list>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitTradeHistoryParams {
    pub category: BybitProductType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_link_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "endTime")]
    pub end_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec_type: Option<BybitExecType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Body parameters for `POST /v5/user/update-sub-api`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/user/modify-sub-apikey>
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BybitUpdateSubApiParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ips: Option<String>,
}
