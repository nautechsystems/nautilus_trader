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

//! Data transfer objects for deserializing OKX HTTP API payloads.

use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::parse::{
    deserialize_empty_string_as_none, deserialize_empty_ustr_as_none,
    deserialize_target_currency_as_none,
};

/// Represents a trade tick from the GET /api/v5/market/trades endpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXTrade {
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Trade price.
    pub px: String,
    /// Trade size.
    pub sz: String,
    /// Trade side: buy or sell.
    pub side: OKXSide,
    /// Trade ID assigned by OKX.
    pub trade_id: Ustr,
    /// Trade timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Represents a candlestick from the GET /api/v5/market/history-candles endpoint.
/// The tuple contains [timestamp(ms), open, high, low, close, volume, turnover, base_volume, count].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OKXCandlestick(
    /// Timestamp in milliseconds.
    pub String,
    /// Open price.
    pub String,
    /// High price.
    pub String,
    /// Low price.
    pub String,
    /// Close price.
    pub String,
    /// Volume.
    pub String,
    /// Turnover in quote currency.
    pub String,
    /// Base volume.
    pub String,
    /// Record count.
    pub String,
);

use crate::common::{
    enums::{
        OKXAlgoOrderType, OKXExecType, OKXInstrumentType, OKXMarginMode, OKXOrderCategory,
        OKXOrderStatus, OKXOrderType, OKXPositionSide, OKXSide, OKXTargetCurrency, OKXTradeMode,
        OKXTriggerType, OKXVipLevel,
    },
    parse::deserialize_string_to_u64,
};

/// Represents a mark price from the GET /api/v5/public/mark-price endpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXMarkPrice {
    /// Underlying.
    pub uly: Option<Ustr>,
    /// Instrument ID.
    pub inst_id: Ustr,
    /// The mark price.
    pub mark_px: String,
    /// The timestamp for the mark price.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Represents an index price from the GET /api/v5/public/index-tickers endpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXIndexTicker {
    /// Instrument ID.
    pub inst_id: Ustr,
    /// The index price.
    pub idx_px: String,
    /// The timestamp for the index price.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Represents a position tier from the GET /api/v5/public/position-tiers endpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXPositionTier {
    /// Underlying.
    pub uly: Ustr,
    /// Instrument family.
    pub inst_family: String,
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Tier level.
    pub tier: String,
    /// Minimum size/amount for the tier.
    pub min_sz: String,
    /// Maximum size/amount for the tier.
    pub max_sz: String,
    /// Maintenance margin requirement rate.
    pub mmr: String,
    /// Initial margin requirement rate.
    pub imr: String,
    /// Maximum available leverage.
    pub max_lever: String,
    /// Option Margin Coefficient (only applicable to options).
    pub opt_mgn_factor: String,
    /// Quote currency borrowing amount.
    pub quote_max_loan: String,
    /// Base currency borrowing amount.
    pub base_max_loan: String,
}

/// Represents an account balance snapshot from `GET /api/v5/account/balance`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXAccount {
    /// Adjusted/Effective equity in USD.
    pub adj_eq: String,
    /// Borrow frozen amount.
    pub borrow_froz: String,
    /// Account details by currency.
    pub details: Vec<OKXBalanceDetail>,
    /// Initial margin requirement.
    pub imr: String,
    /// Isolated margin equity.
    pub iso_eq: String,
    /// Margin ratio.
    pub mgn_ratio: String,
    /// Maintenance margin requirement.
    pub mmr: String,
    /// Notional value in USD for borrow.
    pub notional_usd_for_borrow: String,
    /// Notional value in USD for futures.
    pub notional_usd_for_futures: String,
    /// Notional value in USD for option.
    pub notional_usd_for_option: String,
    /// Notional value in USD for swap.
    pub notional_usd_for_swap: String,
    /// Notional value in USD.
    pub notional_usd: String,
    /// Order frozen.
    pub ord_froz: String,
    /// Total equity in USD.
    pub total_eq: String,
    /// Last update time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub u_time: u64,
    /// Unrealized profit and loss.
    pub upl: String,
}

/// Represents a balance detail for a single currency in an OKX account.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "python", pyo3::pyclass)]
pub struct OKXBalanceDetail {
    /// Available balance.
    pub avail_bal: String,
    /// Available equity.
    pub avail_eq: String,
    /// Borrow frozen amount.
    pub borrow_froz: String,
    /// Cash balance.
    pub cash_bal: String,
    /// Currency.
    pub ccy: Ustr,
    /// Cross liability.
    pub cross_liab: String,
    /// Discount equity in USD.
    pub dis_eq: String,
    /// Equity.
    pub eq: String,
    /// Equity in USD.
    pub eq_usd: String,
    /// Same-token equity.
    pub smt_sync_eq: String,
    /// Copy trading equity.
    pub spot_copy_trading_eq: String,
    /// Fixed balance.
    pub fixed_bal: String,
    /// Frozen balance.
    pub frozen_bal: String,
    /// Initial margin requirement.
    pub imr: String,
    /// Interest.
    pub interest: String,
    /// Isolated margin equity.
    pub iso_eq: String,
    /// Isolated margin liability.
    pub iso_liab: String,
    /// Isolated unrealized profit and loss.
    pub iso_upl: String,
    /// Liability.
    pub liab: String,
    /// Maximum loan amount.
    pub max_loan: String,
    /// Margin ratio.
    pub mgn_ratio: String,
    /// Maintenance margin requirement.
    pub mmr: String,
    /// Notional leverage.
    pub notional_lever: String,
    /// Order frozen.
    pub ord_frozen: String,
    /// Reward balance.
    pub reward_bal: String,
    /// Spot in use amount.
    #[serde(alias = "spotInUse")]
    pub spot_in_use_amt: String,
    /// Cross liability spot in use amount.
    #[serde(alias = "clSpotInUse")]
    pub cl_spot_in_use_amt: String,
    /// Maximum spot in use amount.
    #[serde(alias = "maxSpotInUse")]
    pub max_spot_in_use_amt: String,
    /// Spot isolated balance.
    pub spot_iso_bal: String,
    /// Strategy equity.
    pub stgy_eq: String,
    /// Time-weighted average price.
    pub twap: String,
    /// Last update time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub u_time: u64,
    /// Unrealized profit and loss.
    pub upl: String,
    /// Unrealized profit and loss liability.
    pub upl_liab: String,
    /// Spot balance.
    pub spot_bal: String,
    /// Open average price.
    pub open_avg_px: String,
    /// Accumulated average price.
    pub acc_avg_px: String,
    /// Spot unrealized profit and loss.
    pub spot_upl: String,
    /// Spot unrealized profit and loss ratio.
    pub spot_upl_ratio: String,
    /// Total profit and loss.
    pub total_pnl: String,
    /// Total profit and loss ratio.
    pub total_pnl_ratio: String,
}

/// Represents a single open position from `GET /api/v5/account/positions`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXPosition {
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Instrument type.
    pub inst_type: OKXInstrumentType,
    /// Margin mode: isolated/cross.
    pub mgn_mode: OKXMarginMode,
    /// Position ID.
    #[serde(default, deserialize_with = "deserialize_empty_ustr_as_none")]
    pub pos_id: Option<Ustr>,
    /// Position side: long/short.
    pub pos_side: OKXPositionSide,
    /// Position size.
    pub pos: String,
    /// Base currency balance.
    pub base_bal: String,
    /// Position currency.
    pub ccy: String,
    /// Trading fee.
    pub fee: String,
    /// Position leverage.
    pub lever: String,
    /// Last traded price.
    pub last: String,
    /// Mark price.
    pub mark_px: String,
    /// Liquidation price.
    pub liq_px: String,
    /// Maintenance margin requirement.
    pub mmr: String,
    /// Interest.
    pub interest: String,
    /// Trade ID.
    pub trade_id: Ustr,
    /// Notional value of position in USD.
    pub notional_usd: String,
    /// Average entry price.
    pub avg_px: String,
    /// Unrealized profit and loss.
    pub upl: String,
    /// Unrealized profit and loss ratio.
    pub upl_ratio: String,
    /// Last update time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub u_time: u64,
    /// Position margin.
    pub margin: String,
    /// Margin ratio.
    pub mgn_ratio: String,
    /// Auto-deleveraging (ADL) ranking.
    pub adl: String,
    /// Creation time, Unix timestamp in milliseconds.
    pub c_time: String,
    /// Realized profit and loss.
    pub realized_pnl: String,
    /// Unrealized profit and loss at last price.
    pub upl_last_px: String,
    /// Unrealized profit and loss ratio at last price.
    pub upl_ratio_last_px: String,
    /// Available position that can be closed.
    pub avail_pos: String,
    /// Breakeven price.
    pub be_px: String,
    /// Funding fee.
    pub funding_fee: String,
    /// Index price.
    pub idx_px: String,
    /// Liquidation penalty.
    pub liq_penalty: String,
    /// Option value.
    pub opt_val: String,
    /// Pending close order liability value.
    pub pending_close_ord_liab_val: String,
    /// Total profit and loss.
    pub pnl: String,
    /// Position currency.
    pub pos_ccy: String,
    /// Quote currency balance.
    pub quote_bal: String,
    /// Borrowed amount in quote currency.
    pub quote_borrowed: String,
    /// Interest on quote currency.
    pub quote_interest: String,
    /// Amount in use for spot trading.
    #[serde(alias = "spotInUse")]
    pub spot_in_use_amt: String,
    /// Currency in use for spot trading.
    pub spot_in_use_ccy: String,
    /// USD price.
    pub usd_px: String,
}

/// Represents the response from `POST /api/v5/trade/order` (place order).
/// This model is designed to be flexible and handle the minimal fields that the API returns.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXPlaceOrderResponse {
    /// Order ID.
    #[serde(default)]
    pub ord_id: Option<Ustr>,
    /// Client order ID.
    #[serde(default)]
    pub cl_ord_id: Option<Ustr>,
    /// Order tag.
    #[serde(default)]
    pub tag: Option<String>,
    /// Instrument ID (optional - might not be in response).
    #[serde(default)]
    pub inst_id: Option<Ustr>,
    /// Order side (optional).
    #[serde(default)]
    pub side: Option<OKXSide>,
    /// Order type (optional).
    #[serde(default)]
    pub ord_type: Option<OKXOrderType>,
    /// Order size (optional).
    #[serde(default)]
    pub sz: Option<String>,
    /// Order state (optional).
    pub state: Option<OKXOrderStatus>,
    /// Price (optional).
    #[serde(default)]
    pub px: Option<String>,
    /// Average price (optional).
    #[serde(default)]
    pub avg_px: Option<String>,
    /// Accumulated filled size.
    #[serde(default)]
    pub acc_fill_sz: Option<String>,
    /// Fill size (optional).
    #[serde(default)]
    pub fill_sz: Option<String>,
    /// Fill price (optional).
    #[serde(default)]
    pub fill_px: Option<String>,
    /// Trade ID (optional).
    #[serde(default)]
    pub trade_id: Option<Ustr>,
    /// Fill time (optional).
    #[serde(default)]
    pub fill_time: Option<String>,
    /// Fee (optional).
    #[serde(default)]
    pub fee: Option<String>,
    /// Fee currency (optional).
    #[serde(default)]
    pub fee_ccy: Option<String>,
    /// Request ID (optional).
    #[serde(default)]
    pub req_id: Option<Ustr>,
    /// Position side (optional).
    #[serde(default)]
    pub pos_side: Option<OKXPositionSide>,
    /// Reduce-only flag (optional).
    #[serde(default)]
    pub reduce_only: Option<String>,
    /// Target currency (optional).
    #[serde(default, deserialize_with = "deserialize_target_currency_as_none")]
    pub tgt_ccy: Option<OKXTargetCurrency>,
    /// Creation time.
    #[serde(default)]
    pub c_time: Option<String>,
    /// Last update time (optional).
    #[serde(default)]
    pub u_time: Option<String>,
}

/// Represents a single historical order record from `GET /api/v5/trade/orders-history`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXOrderHistory {
    /// Order ID.
    pub ord_id: Ustr,
    /// Client order ID.
    pub cl_ord_id: Ustr,
    /// Algo order ID (for conditional orders).
    #[serde(default)]
    pub algo_id: Option<Ustr>,
    /// Client-supplied algo order ID (for conditional orders).
    #[serde(default)]
    pub algo_cl_ord_id: Option<Ustr>,
    /// Client account ID (may be omitted by OKX).
    #[serde(default)]
    pub cl_act_id: Option<Ustr>,
    /// Order tag.
    pub tag: String,
    /// Instrument type.
    pub inst_type: OKXInstrumentType,
    /// Underlying (optional).
    pub uly: Option<Ustr>,
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Order type.
    pub ord_type: OKXOrderType,
    /// Order size.
    pub sz: String,
    /// Price (optional).
    pub px: String,
    /// Side.
    pub side: OKXSide,
    /// Position side.
    pub pos_side: OKXPositionSide,
    /// Trade mode.
    pub td_mode: OKXTradeMode,
    /// Reduce-only flag.
    pub reduce_only: String,
    /// Target currency (optional).
    #[serde(default, deserialize_with = "deserialize_target_currency_as_none")]
    pub tgt_ccy: Option<OKXTargetCurrency>,
    /// Order state.
    pub state: OKXOrderStatus,
    /// Average price (optional).
    pub avg_px: String,
    /// Execution fee.
    pub fee: String,
    /// Fee currency.
    pub fee_ccy: String,
    /// Filled size (optional).
    pub fill_sz: String,
    /// Fill price (optional).
    pub fill_px: String,
    /// Trade ID (optional).
    pub trade_id: Ustr,
    /// Fill time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub fill_time: u64,
    /// Accumulated filled size.
    pub acc_fill_sz: String,
    /// Fill fee (optional, may be omitted).
    #[serde(default)]
    pub fill_fee: Option<String>,
    /// Request ID (optional).
    #[serde(default)]
    pub req_id: Option<Ustr>,
    /// Cancelled filled size (optional).
    #[serde(default)]
    pub cancel_fill_sz: Option<String>,
    /// Cancelled total size (optional).
    #[serde(default)]
    pub cancel_total_sz: Option<String>,
    /// Fee discount (optional).
    #[serde(default)]
    pub fee_discount: Option<String>,
    /// Order category (normal, liquidation, ADL, etc.).
    pub category: OKXOrderCategory,
    /// Last update time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub u_time: u64,
    /// Creation time.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub c_time: u64,
}

/// Represents an algo order response from `/trade/order-algo-*` endpoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXOrderAlgo {
    /// Algo order ID assigned by OKX.
    pub algo_id: String,
    /// Client-specified algo order ID.
    #[serde(default)]
    pub algo_cl_ord_id: String,
    /// Client order ID (empty until triggered).
    #[serde(default)]
    pub cl_ord_id: String,
    /// Venue order ID (empty until triggered).
    #[serde(default)]
    pub ord_id: String,
    /// Instrument ID, e.g. `ETH-USDT-SWAP`.
    pub inst_id: Ustr,
    /// Instrument type.
    pub inst_type: OKXInstrumentType,
    /// Algo order type.
    pub ord_type: OKXOrderType,
    /// Current order state.
    pub state: OKXOrderStatus,
    /// Order side.
    pub side: OKXSide,
    /// Position side.
    pub pos_side: OKXPositionSide,
    /// Submitted size.
    pub sz: String,
    /// Trigger price (empty for certain algo styles).
    #[serde(default)]
    pub trigger_px: String,
    /// Trigger price type (last/mark/index).
    #[serde(default)]
    pub trigger_px_type: Option<OKXTriggerType>,
    /// Order price (-1 indicates market execution once triggered).
    #[serde(default)]
    pub ord_px: String,
    /// Trade mode (cash/cross/isolated).
    pub td_mode: OKXTradeMode,
    /// Algo leverage configuration.
    #[serde(default)]
    pub lever: String,
    /// Reduce-only flag.
    #[serde(default)]
    pub reduce_only: String,
    /// Executed price (if triggered).
    #[serde(default)]
    pub actual_px: String,
    /// Executed size (if triggered).
    #[serde(default)]
    pub actual_sz: String,
    /// Notional value in USD.
    #[serde(default)]
    pub notional_usd: String,
    /// Creation time (milliseconds).
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub c_time: u64,
    /// Last update time (milliseconds).
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub u_time: u64,
    /// Trigger timestamp (if triggered).
    #[serde(default)]
    pub trigger_time: String,
    /// Optional tag supplied during submission.
    #[serde(default)]
    pub tag: String,
}

/// Represents a transaction detail (fill) from `GET /api/v5/trade/fills`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXTransactionDetail {
    /// Product type (SPOT, MARGIN, SWAP, FUTURES, OPTION).
    pub inst_type: OKXInstrumentType,
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: Ustr,
    /// Trade ID.
    pub trade_id: Ustr,
    /// Order ID.
    pub ord_id: Ustr,
    /// Client order ID.
    pub cl_ord_id: Ustr,
    /// Bill ID.
    pub bill_id: Ustr,
    /// Last filled price.
    pub fill_px: String,
    /// Last filled quantity.
    pub fill_sz: String,
    /// Trade side: buy or sell.
    pub side: OKXSide,
    /// Execution type.
    pub exec_type: OKXExecType,
    /// Fee currency.
    pub fee_ccy: String,
    /// Fee amount.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fee: Option<String>,
    /// Timestamp, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Represents a single historical position record from `GET /api/v5/account/positions-history`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXPositionHistory {
    /// Instrument type (e.g. "SWAP", "FUTURES", etc.).
    pub inst_type: OKXInstrumentType,
    /// Instrument ID (e.g. "BTC-USD-SWAP").
    pub inst_id: Ustr,
    /// Margin mode: e.g. "cross", "isolated".
    pub mgn_mode: OKXMarginMode,
    /// The type of the last close, e.g. "1" (close partially), "2" (close all), etc.
    /// See OKX docs for the meaning of each numeric code.
    #[serde(rename = "type")]
    pub r#type: Ustr,
    /// Creation time of the position (Unix timestamp in milliseconds).
    pub c_time: String,
    /// Last update time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub u_time: u64,
    /// Average price of opening position.
    pub open_avg_px: String,
    /// Average price of closing position (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_avg_px: Option<String>,
    /// The position ID.
    #[serde(default, deserialize_with = "deserialize_empty_ustr_as_none")]
    pub pos_id: Option<Ustr>,
    /// Max quantity of the position at open time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_max_pos: Option<String>,
    /// Cumulative closed volume of the position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_total_pos: Option<String>,
    /// Realized profit and loss (only for FUTURES/SWAP/OPTION).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realized_pnl: Option<String>,
    /// Accumulated fee for the position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<String>,
    /// Accumulated funding fee (for perpetual swaps).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub funding_fee: Option<String>,
    /// Accumulated liquidation penalty. Negative if there was a penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liq_penalty: Option<String>,
    /// Profit and loss (realized or unrealized depending on status).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pnl: Option<String>,
    /// PnL ratio.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pnl_ratio: Option<String>,
    /// Position side: "long" / "short" / "net".
    pub pos_side: OKXPositionSide,
    /// Leverage used (the JSON field is "lev", but we rename it in Rust).
    pub lever: String,
    /// Direction: "long" or "short" (only for MARGIN/FUTURES/SWAP/OPTION).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    /// Trigger mark price. Populated if `type` indicates liquidation or ADL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_px: Option<String>,
    /// The underlying (e.g. "BTC-USD" for futures or swap).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uly: Option<String>,
    /// Currency (e.g. "BTC"). May or may not appear in all responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ccy: Option<String>,
}

/// Represents the request body for `POST /api/v5/trade/order-algo` (place algo order).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXPlaceAlgoOrderRequest {
    /// Instrument ID.
    #[serde(rename = "instId")]
    pub inst_id: String,
    /// Trade mode (isolated, cross, cash).
    #[serde(rename = "tdMode")]
    pub td_mode: OKXTradeMode,
    /// Order side (buy, sell).
    pub side: OKXSide,
    /// Algo order type (trigger).
    #[serde(rename = "ordType")]
    pub ord_type: OKXAlgoOrderType,
    /// Order size.
    pub sz: String,
    /// Client-supplied algo order ID.
    #[serde(rename = "algoClOrdId", skip_serializing_if = "Option::is_none")]
    pub algo_cl_ord_id: Option<String>,
    /// Trigger price.
    #[serde(rename = "triggerPx", skip_serializing_if = "Option::is_none")]
    pub trigger_px: Option<String>,
    /// Order price (for limit orders).
    #[serde(rename = "orderPx", skip_serializing_if = "Option::is_none")]
    pub order_px: Option<String>,
    /// Trigger type (last, mark, index).
    #[serde(rename = "triggerPxType", skip_serializing_if = "Option::is_none")]
    pub trigger_px_type: Option<OKXTriggerType>,
    /// Target currency (base_ccy or quote_ccy).
    #[serde(rename = "tgtCcy", skip_serializing_if = "Option::is_none")]
    pub tgt_ccy: Option<OKXTargetCurrency>,
    /// Position side (net, long, short).
    #[serde(rename = "posSide", skip_serializing_if = "Option::is_none")]
    pub pos_side: Option<OKXPositionSide>,
    /// Whether to close position.
    #[serde(rename = "closePosition", skip_serializing_if = "Option::is_none")]
    pub close_position: Option<bool>,
    /// Order tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Whether it's a reduce-only order.
    #[serde(rename = "reduceOnly", skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
}

/// Represents the response from `POST /api/v5/trade/order-algo` (place algo order).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXPlaceAlgoOrderResponse {
    /// Algo order ID.
    pub algo_id: String,
    /// Client-supplied algo order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algo_cl_ord_id: Option<String>,
    /// The result of the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s_code: Option<String>,
    /// Error message if the request failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s_msg: Option<String>,
    /// Request ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub req_id: Option<String>,
}

/// Represents the request body for `POST /api/v5/trade/cancel-algos` (cancel algo order).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXCancelAlgoOrderRequest {
    /// Instrument ID.
    pub inst_id: String,
    /// Algo order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algo_id: Option<String>,
    /// Client-supplied algo order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algo_cl_ord_id: Option<String>,
}

/// Represents the response from `POST /api/v5/trade/cancel-algos` (cancel algo order).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXCancelAlgoOrderResponse {
    /// Algo order ID.
    pub algo_id: String,
    /// The result of the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s_code: Option<String>,
    /// Error message if the request failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s_msg: Option<String>,
}

/// Represents the response from `GET /api/v5/public/time` (get system time).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXServerTime {
    /// Server timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Represents a fee rate entry from `GET /api/v5/account/trade-fee`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXFeeRate {
    /// Fee level (VIP tier) - indicates the user's VIP tier (0-9).
    #[serde(deserialize_with = "crate::common::parse::deserialize_vip_level")]
    pub level: OKXVipLevel,
    /// Taker fee rate for crypto-margined contracts.
    pub taker: String,
    /// Maker fee rate for crypto-margined contracts.
    pub maker: String,
    /// Taker fee rate for USDT-margined contracts.
    pub taker_u: String,
    /// Maker fee rate for USDT-margined contracts.
    pub maker_u: String,
    /// Delivery fee rate.
    #[serde(default)]
    pub delivery: String,
    /// Option exercise fee rate.
    #[serde(default)]
    pub exercise: String,
    /// Instrument type (SPOT, MARGIN, SWAP, FUTURES, OPTION).
    pub inst_type: OKXInstrumentType,
    /// Fee schedule category (being deprecated).
    #[serde(default)]
    pub category: String,
    /// Data return timestamp (Unix timestamp in milliseconds).
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json;

    use super::*;

    #[rstest]
    fn test_algo_order_request_serialization() {
        let request = OKXPlaceAlgoOrderRequest {
            inst_id: "ETH-USDT-SWAP".to_string(),
            td_mode: OKXTradeMode::Isolated,
            side: OKXSide::Buy,
            ord_type: OKXAlgoOrderType::Trigger,
            sz: "0.01".to_string(),
            algo_cl_ord_id: Some("test123".to_string()),
            trigger_px: Some("3000".to_string()),
            order_px: Some("-1".to_string()),
            trigger_px_type: Some(OKXTriggerType::Last),
            tgt_ccy: None,
            pos_side: None,
            close_position: None,
            tag: None,
            reduce_only: None,
        };

        let json = serde_json::to_string(&request).unwrap();

        // Verify that fields are serialized with correct camelCase names
        assert!(json.contains("\"instId\":\"ETH-USDT-SWAP\""));
        assert!(json.contains("\"tdMode\":\"isolated\""));
        assert!(json.contains("\"ordType\":\"trigger\""));
        assert!(json.contains("\"algoClOrdId\":\"test123\""));
        assert!(json.contains("\"triggerPx\":\"3000\""));
        assert!(json.contains("\"orderPx\":\"-1\""));
        assert!(json.contains("\"triggerPxType\":\"last\""));

        // Verify that None fields are not included
        assert!(!json.contains("tgtCcy"));
        assert!(!json.contains("posSide"));
        assert!(!json.contains("closePosition"));
    }

    #[rstest]
    fn test_algo_order_request_array_serialization() {
        let request = OKXPlaceAlgoOrderRequest {
            inst_id: "BTC-USDT".to_string(),
            td_mode: OKXTradeMode::Cross,
            side: OKXSide::Sell,
            ord_type: OKXAlgoOrderType::Trigger,
            sz: "0.1".to_string(),
            algo_cl_ord_id: None,
            trigger_px: Some("50000".to_string()),
            order_px: Some("49900".to_string()),
            trigger_px_type: Some(OKXTriggerType::Mark),
            tgt_ccy: Some(OKXTargetCurrency::BaseCcy),
            pos_side: Some(OKXPositionSide::Net),
            close_position: None,
            tag: None,
            reduce_only: Some(true),
        };

        // OKX expects an array of requests
        let json = serde_json::to_string(&[request]).unwrap();

        // Verify array format
        assert!(json.starts_with('['));
        assert!(json.ends_with(']'));

        // Verify correct field names
        assert!(json.contains("\"instId\":\"BTC-USDT\""));
        assert!(json.contains("\"tdMode\":\"cross\""));
        assert!(json.contains("\"triggerPx\":\"50000\""));
        assert!(json.contains("\"orderPx\":\"49900\""));
        assert!(json.contains("\"triggerPxType\":\"mark\""));
        assert!(json.contains("\"tgtCcy\":\"base_ccy\""));
        assert!(json.contains("\"posSide\":\"net\""));
        assert!(json.contains("\"reduceOnly\":true"));
    }

    #[rstest]
    fn test_cancel_algo_order_request_serialization() {
        let request = OKXCancelAlgoOrderRequest {
            inst_id: "ETH-USDT-SWAP".to_string(),
            algo_id: Some("123456".to_string()),
            algo_cl_ord_id: None,
        };

        let json = serde_json::to_string(&request).unwrap();

        // Verify correct field names
        assert!(json.contains("\"instId\":\"ETH-USDT-SWAP\""));
        assert!(json.contains("\"algoId\":\"123456\""));
        assert!(!json.contains("algoClOrdId"));
    }

    #[rstest]
    fn test_cancel_algo_order_with_client_id_serialization() {
        let request = OKXCancelAlgoOrderRequest {
            inst_id: "BTC-USDT".to_string(),
            algo_id: None,
            algo_cl_ord_id: Some("client123".to_string()),
        };

        // OKX expects an array of requests
        let json = serde_json::to_string(&[request]).unwrap();

        // Verify array format and field names
        assert!(json.starts_with('['));
        assert!(json.contains("\"instId\":\"BTC-USDT\""));
        assert!(json.contains("\"algoClOrdId\":\"client123\""));
        assert!(!json.contains("\"algoId\""));
    }
}
