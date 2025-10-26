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

//! Data transfer objects for deserializing Bybit HTTP API payloads.

use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::{
    enums::{
        BybitAccountType, BybitCancelType, BybitContractType, BybitExecType, BybitInnovationFlag,
        BybitInstrumentStatus, BybitMarginTrading, BybitOptionType, BybitOrderSide,
        BybitOrderStatus, BybitOrderType, BybitPositionIdx, BybitPositionSide, BybitProductType,
        BybitStopOrderType, BybitTimeInForce, BybitTpSlMode, BybitTriggerDirection,
        BybitTriggerType,
    },
    models::{
        BybitCursorListResponse, BybitListResponse, BybitResponse, LeverageFilter,
        LinearLotSizeFilter, LinearPriceFilter, OptionLotSizeFilter, SpotLotSizeFilter,
        SpotPriceFilter,
    },
};

/// Response payload returned by `GET /v5/market/server-time`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/server-time>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitServerTime {
    /// Server timestamp in seconds represented as string.
    pub time_second: String,
    /// Server timestamp in nanoseconds represented as string.
    pub time_nano: String,
}

/// Type alias for the server time response envelope.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/server-time>
pub type BybitServerTimeResponse = BybitResponse<BybitServerTime>;

/// Ticker payload for spot instruments.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/tickers>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitTickerSpot {
    pub symbol: Ustr,
    pub bid1_price: String,
    pub bid1_size: String,
    pub ask1_price: String,
    pub ask1_size: String,
    pub last_price: String,
    pub prev_price24h: String,
    pub price24h_pcnt: String,
    pub high_price24h: String,
    pub low_price24h: String,
    pub turnover24h: String,
    pub volume24h: String,
    pub usd_index_price: String,
}

/// Ticker payload for linear and inverse perpetual/futures instruments.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/tickers>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitTickerLinear {
    pub symbol: Ustr,
    pub last_price: String,
    pub index_price: String,
    pub mark_price: String,
    pub prev_price24h: String,
    pub price24h_pcnt: String,
    pub high_price24h: String,
    pub low_price24h: String,
    pub prev_price1h: String,
    pub open_interest: String,
    pub open_interest_value: String,
    pub turnover24h: String,
    pub volume24h: String,
    pub funding_rate: String,
    pub next_funding_time: String,
    pub predicted_delivery_price: String,
    pub basis_rate: String,
    pub delivery_fee_rate: String,
    pub delivery_time: String,
    pub ask1_size: String,
    pub bid1_price: String,
    pub ask1_price: String,
    pub bid1_size: String,
    pub basis: String,
}

/// Ticker payload for option instruments.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/tickers>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitTickerOption {
    pub symbol: Ustr,
    pub bid1_price: String,
    pub bid1_size: String,
    pub bid1_iv: String,
    pub ask1_price: String,
    pub ask1_size: String,
    pub ask1_iv: String,
    pub last_price: String,
    pub high_price24h: String,
    pub low_price24h: String,
    pub mark_price: String,
    pub index_price: String,
    pub mark_iv: String,
    pub underlying_price: String,
    pub open_interest: String,
    pub turnover24h: String,
    pub volume24h: String,
    pub total_volume: String,
    pub total_turnover: String,
    pub delta: String,
    pub gamma: String,
    pub vega: String,
    pub theta: String,
    pub predicted_delivery_price: String,
    pub change24h: String,
}

/// Response alias for spot ticker requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/tickers>
pub type BybitTickersSpotResponse = BybitListResponse<BybitTickerSpot>;
/// Response alias for linear/inverse ticker requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/tickers>
pub type BybitTickersLinearResponse = BybitListResponse<BybitTickerLinear>;
/// Response alias for option ticker requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/tickers>
pub type BybitTickersOptionResponse = BybitListResponse<BybitTickerOption>;

/// Unified ticker data structure containing common fields across all product types.
///
/// This simplified ticker structure is designed to work across SPOT, LINEAR, and OPTION products,
/// containing only the most commonly used fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit")
)]
pub struct BybitTickerData {
    pub symbol: Ustr,
    pub bid1_price: String,
    pub bid1_size: String,
    pub ask1_price: String,
    pub ask1_size: String,
    pub last_price: String,
    pub high_price24h: String,
    pub low_price24h: String,
    pub turnover24h: String,
    pub volume24h: String,
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BybitTickerData {
    #[getter]
    #[must_use]
    pub fn symbol(&self) -> &str {
        self.symbol.as_str()
    }

    #[getter]
    #[must_use]
    pub fn bid1_price(&self) -> &str {
        &self.bid1_price
    }

    #[getter]
    #[must_use]
    pub fn bid1_size(&self) -> &str {
        &self.bid1_size
    }

    #[getter]
    #[must_use]
    pub fn ask1_price(&self) -> &str {
        &self.ask1_price
    }

    #[getter]
    #[must_use]
    pub fn ask1_size(&self) -> &str {
        &self.ask1_size
    }

    #[getter]
    #[must_use]
    pub fn last_price(&self) -> &str {
        &self.last_price
    }

    #[getter]
    #[must_use]
    pub fn high_price24h(&self) -> &str {
        &self.high_price24h
    }

    #[getter]
    #[must_use]
    pub fn low_price24h(&self) -> &str {
        &self.low_price24h
    }

    #[getter]
    #[must_use]
    pub fn turnover24h(&self) -> &str {
        &self.turnover24h
    }

    #[getter]
    #[must_use]
    pub fn volume24h(&self) -> &str {
        &self.volume24h
    }
}

/// Kline/candlestick entry returned by `GET /v5/market/kline`.
///
/// Bybit returns klines as arrays with 7 elements:
/// [startTime, openPrice, highPrice, lowPrice, closePrice, volume, turnover]
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/kline>
#[derive(Clone, Debug, Serialize)]
pub struct BybitKline {
    pub start: String,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub turnover: String,
}

impl<'de> Deserialize<'de> for BybitKline {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let arr: [String; 7] = Deserialize::deserialize(deserializer)?;
        Ok(Self {
            start: arr[0].clone(),
            open: arr[1].clone(),
            high: arr[2].clone(),
            low: arr[3].clone(),
            close: arr[4].clone(),
            volume: arr[5].clone(),
            turnover: arr[6].clone(),
        })
    }
}

/// Kline list result returned by Bybit.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/kline>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitKlineResult {
    pub category: BybitProductType,
    pub symbol: Ustr,
    pub list: Vec<BybitKline>,
}

/// Response alias for kline history requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/kline>
pub type BybitKlinesResponse = BybitResponse<BybitKlineResult>;

/// Trade entry returned by `GET /v5/market/recent-trade`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/recent-trade>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitTrade {
    pub exec_id: String,
    pub symbol: Ustr,
    pub price: String,
    pub size: String,
    pub side: BybitOrderSide,
    pub time: String,
    pub is_block_trade: bool,
    #[serde(default)]
    pub m_p: Option<String>,
    #[serde(default)]
    pub i_p: Option<String>,
    #[serde(default)]
    pub mlv: Option<String>,
    #[serde(default)]
    pub iv: Option<String>,
}

/// Trade list result returned by Bybit.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/recent-trade>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitTradeResult {
    pub category: BybitProductType,
    pub list: Vec<BybitTrade>,
}

/// Response alias for recent trades requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/recent-trade>
pub type BybitTradesResponse = BybitResponse<BybitTradeResult>;

/// Instrument definition for spot symbols.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitInstrumentSpot {
    pub symbol: Ustr,
    pub base_coin: Ustr,
    pub quote_coin: Ustr,
    pub innovation: BybitInnovationFlag,
    pub status: BybitInstrumentStatus,
    pub margin_trading: BybitMarginTrading,
    pub lot_size_filter: SpotLotSizeFilter,
    pub price_filter: SpotPriceFilter,
}

/// Instrument definition for linear contracts.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitInstrumentLinear {
    pub symbol: Ustr,
    pub contract_type: BybitContractType,
    pub status: BybitInstrumentStatus,
    pub base_coin: Ustr,
    pub quote_coin: Ustr,
    pub launch_time: String,
    pub delivery_time: String,
    pub delivery_fee_rate: String,
    pub price_scale: String,
    pub leverage_filter: LeverageFilter,
    pub price_filter: LinearPriceFilter,
    pub lot_size_filter: LinearLotSizeFilter,
    pub unified_margin_trade: bool,
    pub funding_interval: i64,
    pub settle_coin: Ustr,
}

/// Instrument definition for inverse contracts.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitInstrumentInverse {
    pub symbol: Ustr,
    pub contract_type: BybitContractType,
    pub status: BybitInstrumentStatus,
    pub base_coin: Ustr,
    pub quote_coin: Ustr,
    pub launch_time: String,
    pub delivery_time: String,
    pub delivery_fee_rate: String,
    pub price_scale: String,
    pub leverage_filter: LeverageFilter,
    pub price_filter: LinearPriceFilter,
    pub lot_size_filter: LinearLotSizeFilter,
    pub unified_margin_trade: bool,
    pub funding_interval: i64,
    pub settle_coin: Ustr,
}

/// Instrument definition for option contracts.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitInstrumentOption {
    pub symbol: Ustr,
    pub status: BybitInstrumentStatus,
    pub base_coin: Ustr,
    pub quote_coin: Ustr,
    pub settle_coin: Ustr,
    pub options_type: BybitOptionType,
    pub launch_time: String,
    pub delivery_time: String,
    pub delivery_fee_rate: String,
    pub price_filter: LinearPriceFilter,
    pub lot_size_filter: OptionLotSizeFilter,
}

/// Response alias for instrument info requests that return spot instruments.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
pub type BybitInstrumentSpotResponse = BybitCursorListResponse<BybitInstrumentSpot>;
/// Response alias for instrument info requests that return linear contracts.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
pub type BybitInstrumentLinearResponse = BybitCursorListResponse<BybitInstrumentLinear>;
/// Response alias for instrument info requests that return inverse contracts.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
pub type BybitInstrumentInverseResponse = BybitCursorListResponse<BybitInstrumentInverse>;
/// Response alias for instrument info requests that return option contracts.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
pub type BybitInstrumentOptionResponse = BybitCursorListResponse<BybitInstrumentOption>;

/// Fee rate structure returned by `GET /v5/account/fee-rate`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/fee-rate>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct BybitFeeRate {
    pub symbol: Ustr,
    pub taker_fee_rate: String,
    pub maker_fee_rate: String,
    #[serde(default)]
    pub base_coin: Option<Ustr>,
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BybitFeeRate {
    #[getter]
    #[must_use]
    pub fn symbol(&self) -> &str {
        self.symbol.as_str()
    }

    #[getter]
    #[must_use]
    pub fn taker_fee_rate(&self) -> &str {
        &self.taker_fee_rate
    }

    #[getter]
    #[must_use]
    pub fn maker_fee_rate(&self) -> &str {
        &self.maker_fee_rate
    }

    #[getter]
    #[must_use]
    pub fn base_coin(&self) -> Option<&str> {
        self.base_coin.as_ref().map(|u| u.as_str())
    }
}

/// Response alias for fee rate requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/fee-rate>
pub type BybitFeeRateResponse = BybitListResponse<BybitFeeRate>;

/// Account balance snapshot coin entry.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/wallet-balance>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitCoinBalance {
    pub available_to_borrow: String,
    pub bonus: String,
    pub accrued_interest: String,
    pub available_to_withdraw: String,
    #[serde(default, rename = "totalOrderIM")]
    pub total_order_im: Option<String>,
    pub equity: String,
    pub usd_value: String,
    pub borrow_amount: String,
    #[serde(default, rename = "totalPositionMM")]
    pub total_position_mm: Option<String>,
    #[serde(default, rename = "totalPositionIM")]
    pub total_position_im: Option<String>,
    pub wallet_balance: String,
    pub unrealised_pnl: String,
    pub cum_realised_pnl: String,
    pub locked: String,
    pub collateral_switch: bool,
    pub margin_collateral: bool,
    pub coin: Ustr,
    #[serde(default)]
    pub spot_hedging_qty: Option<String>,
    #[serde(default)]
    pub spot_borrow: Option<String>,
}

/// Wallet balance snapshot containing per-coin balances.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/wallet-balance>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitWalletBalance {
    pub total_equity: String,
    #[serde(rename = "accountIMRate")]
    pub account_im_rate: String,
    pub total_margin_balance: String,
    pub total_initial_margin: String,
    pub account_type: BybitAccountType,
    pub total_available_balance: String,
    #[serde(rename = "accountMMRate")]
    pub account_mm_rate: String,
    #[serde(rename = "totalPerpUPL")]
    pub total_perp_upl: String,
    pub total_wallet_balance: String,
    #[serde(rename = "accountLTV")]
    pub account_ltv: String,
    pub total_maintenance_margin: String,
    pub coin: Vec<BybitCoinBalance>,
}

/// Response alias for wallet balance requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/wallet-balance>
pub type BybitWalletBalanceResponse = BybitListResponse<BybitWalletBalance>;

/// Order representation as returned by order-related endpoints.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/realtime>
/// - <https://bybit-exchange.github.io/docs/v5/order/order-list>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitOrder {
    pub order_id: Ustr,
    pub order_link_id: Ustr,
    pub block_trade_id: Option<Ustr>,
    pub symbol: Ustr,
    pub price: String,
    pub qty: String,
    pub side: BybitOrderSide,
    pub is_leverage: String,
    pub position_idx: i32,
    pub order_status: BybitOrderStatus,
    pub cancel_type: BybitCancelType,
    pub reject_reason: Ustr,
    pub avg_price: Option<String>,
    pub leaves_qty: String,
    pub leaves_value: String,
    pub cum_exec_qty: String,
    pub cum_exec_value: String,
    pub cum_exec_fee: String,
    pub time_in_force: BybitTimeInForce,
    pub order_type: BybitOrderType,
    pub stop_order_type: BybitStopOrderType,
    pub order_iv: Option<String>,
    pub trigger_price: String,
    pub take_profit: String,
    pub stop_loss: String,
    pub tp_trigger_by: BybitTriggerType,
    pub sl_trigger_by: BybitTriggerType,
    pub trigger_direction: BybitTriggerDirection,
    pub trigger_by: BybitTriggerType,
    pub last_price_on_created: String,
    pub reduce_only: bool,
    pub close_on_trigger: bool,
    pub smp_type: Ustr,
    pub smp_group: i32,
    pub smp_order_id: Ustr,
    pub tpsl_mode: Option<BybitTpSlMode>,
    pub tp_limit_price: String,
    pub sl_limit_price: String,
    pub place_type: Ustr,
    pub created_time: String,
    pub updated_time: String,
}

/// Response alias for open order queries.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/realtime>
pub type BybitOpenOrdersResponse = BybitListResponse<BybitOrder>;
/// Response alias for order history queries with pagination.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/order-list>
pub type BybitOrderHistoryResponse = BybitCursorListResponse<BybitOrder>;

/// Payload returned after placing a single order.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/create-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitPlaceOrderResult {
    pub order_id: Option<Ustr>,
    pub order_link_id: Option<Ustr>,
}

/// Response alias for order placement endpoints.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/create-order>
pub type BybitPlaceOrderResponse = BybitResponse<BybitPlaceOrderResult>;

/// Payload returned after cancelling a single order.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/cancel-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitCancelOrderResult {
    pub order_id: Option<Ustr>,
    pub order_link_id: Option<Ustr>,
}

/// Response alias for order cancellation endpoints.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/cancel-order>
pub type BybitCancelOrderResponse = BybitResponse<BybitCancelOrderResult>;

/// Execution/Fill payload returned by `GET /v5/execution/list`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/execution-list>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitExecution {
    pub symbol: Ustr,
    pub order_id: Ustr,
    pub order_link_id: Ustr,
    pub side: BybitOrderSide,
    pub order_price: String,
    pub order_qty: String,
    pub leaves_qty: String,
    pub create_type: Option<String>,
    pub order_type: BybitOrderType,
    pub stop_order_type: Option<BybitStopOrderType>,
    pub exec_fee: String,
    pub exec_id: String,
    pub exec_price: String,
    pub exec_qty: String,
    pub exec_type: BybitExecType,
    pub exec_value: String,
    pub exec_time: String,
    pub fee_currency: Ustr,
    pub is_maker: bool,
    pub fee_rate: String,
    pub trade_iv: String,
    pub mark_iv: String,
    pub mark_price: String,
    pub index_price: String,
    pub underlying_price: String,
    pub block_trade_id: String,
    pub closed_size: String,
    pub seq: i64,
}

/// Response alias for trade history requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/execution-list>
pub type BybitTradeHistoryResponse = BybitListResponse<BybitExecution>;

/// Represents a position returned by the Bybit API.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/position/position-info>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitPosition {
    pub position_idx: BybitPositionIdx,
    pub risk_id: i32,
    pub risk_limit_value: String,
    pub symbol: Ustr,
    pub side: BybitPositionSide,
    pub size: String,
    pub avg_price: String,
    pub position_value: String,
    pub trade_mode: i32,
    pub position_status: String,
    pub auto_add_margin: i32,
    pub adl_rank_indicator: i32,
    pub leverage: String,
    pub position_balance: String,
    pub mark_price: String,
    pub liq_price: String,
    pub bust_price: String,
    #[serde(rename = "positionMM")]
    pub position_mm: String,
    #[serde(rename = "positionIM")]
    pub position_im: String,
    pub tpsl_mode: String,
    pub take_profit: String,
    pub stop_loss: String,
    pub trailing_stop: String,
    pub unrealised_pnl: String,
    pub cur_realised_pnl: String,
    pub cum_realised_pnl: String,
    pub seq: i64,
    pub is_reduce_only: bool,
    pub mmr_sys_updated_time: String,
    pub leverage_sys_updated_time: String,
    pub created_time: String,
    pub updated_time: String,
}

/// Response alias for position list requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/position/position-info>
pub type BybitPositionListResponse = BybitCursorListResponse<BybitPosition>;

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::identifiers::AccountId;
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    #[rstest]
    fn deserialize_spot_instrument_uses_enums() {
        let json = load_test_json("http_get_instruments_spot.json");
        let response: BybitInstrumentSpotResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];

        assert_eq!(instrument.status, BybitInstrumentStatus::Trading);
        assert_eq!(instrument.innovation, BybitInnovationFlag::Standard);
        assert_eq!(instrument.margin_trading, BybitMarginTrading::UtaOnly);
    }

    #[rstest]
    fn deserialize_linear_instrument_status() {
        let json = load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];

        assert_eq!(instrument.status, BybitInstrumentStatus::Trading);
        assert_eq!(instrument.contract_type, BybitContractType::LinearPerpetual);
    }

    #[rstest]
    fn deserialize_order_response_maps_enums() {
        let json = load_test_json("http_get_orders_history.json");
        let response: BybitOrderHistoryResponse = serde_json::from_str(&json).unwrap();
        let order = &response.result.list[0];

        assert_eq!(order.cancel_type, BybitCancelType::CancelByUser);
        assert_eq!(order.tp_trigger_by, BybitTriggerType::MarkPrice);
        assert_eq!(order.sl_trigger_by, BybitTriggerType::LastPrice);
        assert_eq!(order.tpsl_mode, Some(BybitTpSlMode::Full));
        assert_eq!(order.order_type, BybitOrderType::Limit);
    }

    #[rstest]
    fn deserialize_wallet_balance_without_optional_fields() {
        let json = r#"{
            "retCode": 0,
            "retMsg": "OK",
            "result": {
                "list": [{
                    "totalEquity": "1000.00",
                    "accountIMRate": "0",
                    "totalMarginBalance": "1000.00",
                    "totalInitialMargin": "0",
                    "accountType": "UNIFIED",
                    "totalAvailableBalance": "1000.00",
                    "accountMMRate": "0",
                    "totalPerpUPL": "0",
                    "totalWalletBalance": "1000.00",
                    "accountLTV": "0",
                    "totalMaintenanceMargin": "0",
                    "coin": [{
                        "availableToBorrow": "0",
                        "bonus": "0",
                        "accruedInterest": "0",
                        "availableToWithdraw": "1000.00",
                        "equity": "1000.00",
                        "usdValue": "1000.00",
                        "borrowAmount": "0",
                        "totalPositionIM": "0",
                        "walletBalance": "1000.00",
                        "unrealisedPnl": "0",
                        "cumRealisedPnl": "0",
                        "locked": "0",
                        "collateralSwitch": true,
                        "marginCollateral": true,
                        "coin": "USDT"
                    }]
                }]
            }
        }"#;

        let response: BybitWalletBalanceResponse = serde_json::from_str(json)
            .expect("Failed to parse wallet balance without optional fields");

        assert_eq!(response.ret_code, 0);
        assert_eq!(response.result.list[0].coin[0].total_order_im, None);
        assert_eq!(response.result.list[0].coin[0].total_position_mm, None);
    }

    #[rstest]
    fn deserialize_wallet_balance_from_docs() {
        let json = include_str!("../../test_data/http_get_wallet_balance.json");

        let response: BybitWalletBalanceResponse = serde_json::from_str(json)
            .expect("Failed to parse wallet balance from Bybit docs example");

        assert_eq!(response.ret_code, 0);
        assert_eq!(response.ret_msg, "OK");

        let wallet = &response.result.list[0];
        assert_eq!(wallet.total_equity, "3.31216591");
        assert_eq!(wallet.account_im_rate, "0");
        assert_eq!(wallet.account_mm_rate, "0");
        assert_eq!(wallet.total_perp_upl, "0");
        assert_eq!(wallet.account_ltv, "0");

        // Check BTC coin
        let btc = &wallet.coin[0];
        assert_eq!(btc.coin.as_str(), "BTC");
        assert_eq!(btc.available_to_borrow, "3");
        assert_eq!(btc.total_order_im, Some("0".to_string()));
        assert_eq!(btc.total_position_mm, Some("0".to_string()));
        assert_eq!(btc.total_position_im, Some("0".to_string()));

        // Check USDT coin (without optional IM/MM fields)
        let usdt = &wallet.coin[1];
        assert_eq!(usdt.coin.as_str(), "USDT");
        assert_eq!(usdt.wallet_balance, "1000.50");
        assert_eq!(usdt.total_order_im, None);
        assert_eq!(usdt.total_position_mm, None);
        assert_eq!(usdt.total_position_im, None);
        assert_eq!(btc.spot_borrow, Some("0".to_string()));
        assert_eq!(usdt.spot_borrow, Some("0".to_string()));
    }

    #[rstest]
    fn test_parse_wallet_balance_with_spot_borrow() {
        let json = include_str!("../../test_data/http_get_wallet_balance_with_spot_borrow.json");
        let response: BybitWalletBalanceResponse =
            serde_json::from_str(json).expect("Failed to parse wallet balance with spotBorrow");

        let wallet = &response.result.list[0];
        let usdt = &wallet.coin[0];

        assert_eq!(usdt.coin.as_str(), "USDT");
        assert_eq!(usdt.wallet_balance, "1200.00");
        assert_eq!(usdt.spot_borrow, Some("200.00".to_string()));
        assert_eq!(usdt.borrow_amount, "200.00");

        // Verify calculation: actual_balance = walletBalance - spotBorrow = 1200 - 200 = 1000
        let account_id = crate::common::parse::parse_account_state(
            wallet,
            AccountId::new("BYBIT-001"),
            UnixNanos::default(),
        )
        .expect("Failed to parse account state");

        let balance = &account_id.balances[0];
        assert_eq!(balance.total.as_f64(), 1000.0);
    }
}
