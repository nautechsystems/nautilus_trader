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

//! Data transfer objects for deserializing Bybit HTTP API payloads.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::{
    enums::{
        BybitAccountType, BybitApiKeyType, BybitCancelType, BybitContractType, BybitCreateType,
        BybitExecType, BybitInnovationFlag, BybitInstrumentStatus, BybitMarginMode,
        BybitMarginTrading, BybitOptionType, BybitOrderSide, BybitOrderStatus, BybitOrderType,
        BybitPositionIdx, BybitPositionSide, BybitPositionStatus, BybitProductType, BybitSmpType,
        BybitStopOrderType, BybitTimeInForce, BybitTpSlMode, BybitTriggerDirection,
        BybitTriggerType, BybitUnifiedMarginStatus,
    },
    models::{
        BybitCursorList, BybitCursorListResponse, BybitListResponse, BybitResponse, LeverageFilter,
        LinearLotSizeFilter, LinearPriceFilter, OptionLotSizeFilter, SpotLotSizeFilter,
        SpotPriceFilter,
    },
    parse::{
        bool_or_int, deserialize_decimal_or_zero, deserialize_optional_decimal_or_zero,
        deserialize_string_to_u8, masked_secret, on_off_bool,
    },
};

/// Cursor-paginated list of orders for Python bindings.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
)]
pub struct BybitOrderCursorList {
    /// Collection of orders returned by the endpoint.
    pub list: Vec<BybitOrder>,
    /// Pagination cursor for the next page.
    pub next_page_cursor: Option<String>,
    /// Optional product category when the API includes it.
    #[serde(default)]
    pub category: Option<BybitProductType>,
}

impl From<BybitCursorList<BybitOrder>> for BybitOrderCursorList {
    fn from(cursor_list: BybitCursorList<BybitOrder>) -> Self {
        Self {
            list: cursor_list.list,
            next_page_cursor: cursor_list.next_page_cursor,
            category: cursor_list.category,
        }
    }
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BybitOrderCursorList {
    #[getter]
    #[must_use]
    pub fn list(&self) -> Vec<BybitOrder> {
        self.list.clone()
    }

    #[getter]
    #[must_use]
    pub fn next_page_cursor(&self) -> Option<&str> {
        self.next_page_cursor.as_deref()
    }

    #[getter]
    #[must_use]
    pub fn category(&self) -> Option<BybitProductType> {
        self.category
    }
}

/// Response payload returned by `GET /v5/market/time`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/time>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
)]
#[serde(rename_all = "camelCase")]
pub struct BybitServerTime {
    /// Server timestamp in seconds represented as string.
    pub time_second: String,
    /// Server timestamp in nanoseconds represented as string.
    pub time_nano: String,
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BybitServerTime {
    #[getter]
    #[must_use]
    pub fn time_second(&self) -> &str {
        &self.time_second
    }

    #[getter]
    #[must_use]
    pub fn time_nano(&self) -> &str {
        &self.time_nano
    }
}

/// Type alias for the server time response envelope.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/time>
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
    #[serde(default)]
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
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
    #[serde(default)]
    pub open_interest: Option<String>,
    #[serde(default)]
    pub funding_rate: Option<String>,
    #[serde(default)]
    pub next_funding_time: Option<String>,
    #[serde(default)]
    pub mark_price: Option<String>,
    #[serde(default)]
    pub index_price: Option<String>,
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

    #[getter]
    #[must_use]
    pub fn open_interest(&self) -> Option<&str> {
        self.open_interest.as_deref()
    }

    #[getter]
    #[must_use]
    pub fn funding_rate(&self) -> Option<&str> {
        self.funding_rate.as_deref()
    }

    #[getter]
    #[must_use]
    pub fn next_funding_time(&self) -> Option<&str> {
        self.next_funding_time.as_deref()
    }

    #[getter]
    #[must_use]
    pub fn mark_price(&self) -> Option<&str> {
        self.mark_price.as_deref()
    }

    #[getter]
    #[must_use]
    pub fn index_price(&self) -> Option<&str> {
        self.index_price.as_deref()
    }
}

impl From<BybitTickerSpot> for BybitTickerData {
    fn from(ticker: BybitTickerSpot) -> Self {
        Self {
            symbol: ticker.symbol,
            bid1_price: ticker.bid1_price,
            bid1_size: ticker.bid1_size,
            ask1_price: ticker.ask1_price,
            ask1_size: ticker.ask1_size,
            last_price: ticker.last_price,
            high_price24h: ticker.high_price24h,
            low_price24h: ticker.low_price24h,
            turnover24h: ticker.turnover24h,
            volume24h: ticker.volume24h,
            open_interest: None,
            funding_rate: None,
            next_funding_time: None,
            mark_price: None,
            index_price: None,
        }
    }
}

impl From<BybitTickerLinear> for BybitTickerData {
    fn from(ticker: BybitTickerLinear) -> Self {
        Self {
            symbol: ticker.symbol,
            bid1_price: ticker.bid1_price,
            bid1_size: ticker.bid1_size,
            ask1_price: ticker.ask1_price,
            ask1_size: ticker.ask1_size,
            last_price: ticker.last_price,
            high_price24h: ticker.high_price24h,
            low_price24h: ticker.low_price24h,
            turnover24h: ticker.turnover24h,
            volume24h: ticker.volume24h,
            open_interest: Some(ticker.open_interest),
            funding_rate: Some(ticker.funding_rate),
            next_funding_time: Some(ticker.next_funding_time),
            mark_price: Some(ticker.mark_price),
            index_price: Some(ticker.index_price),
        }
    }
}

impl From<BybitTickerOption> for BybitTickerData {
    fn from(ticker: BybitTickerOption) -> Self {
        Self {
            symbol: ticker.symbol,
            bid1_price: ticker.bid1_price,
            bid1_size: ticker.bid1_size,
            ask1_price: ticker.ask1_price,
            ask1_size: ticker.ask1_size,
            last_price: ticker.last_price,
            high_price24h: ticker.high_price24h,
            low_price24h: ticker.low_price24h,
            turnover24h: ticker.turnover24h,
            volume24h: ticker.volume24h,
            open_interest: Some(ticker.open_interest),
            funding_rate: None,
            next_funding_time: None,
            mark_price: Some(ticker.mark_price),
            index_price: Some(ticker.index_price),
        }
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
        let [start, open, high, low, close, volume, turnover]: [String; 7] =
            Deserialize::deserialize(deserializer)?;
        Ok(Self {
            start,
            open,
            high,
            low,
            close,
            volume,
            turnover,
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

/// Funding entry returned by `GET /v5/market/funding/history`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/history-fund-rate>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitFunding {
    pub symbol: Ustr,
    pub funding_rate: String,
    pub funding_rate_timestamp: String,
}

/// Funding list result returned by Bybit.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/history-fund-rate>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitFundingResult {
    pub category: BybitProductType,
    pub list: Vec<BybitFunding>,
}

/// Response alias for historical funding requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/history-fund-rate>
pub type BybitFundingResponse = BybitResponse<BybitFundingResult>;

/// Orderbook result returned by Bybit.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/orderbook>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitOrderbookResult {
    /// Symbol.
    pub s: Ustr,
    /// Bid levels represented as `[price, size]` string pairs.
    pub b: Vec<[String; 2]>,
    /// Ask levels represented as `[price, size]` string pairs.
    pub a: Vec<[String; 2]>,
    pub ts: i64,
    /// Update identifier.
    pub u: i64,
    /// Cross sequence number.
    pub seq: i64,
    pub cts: i64,
}

/// Response alias for orderbook requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/orderbook>
pub type BybitOrderbookResponse = BybitResponse<BybitOrderbookResult>;

/// Instrument definition for spot symbols.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
/// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
/// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
/// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
/// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
pub type BybitInstrumentSpotResponse = BybitCursorListResponse<BybitInstrumentSpot>;
/// Response alias for instrument info requests that return linear contracts.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
pub type BybitInstrumentLinearResponse = BybitCursorListResponse<BybitInstrumentLinear>;
/// Response alias for instrument info requests that return inverse contracts.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
pub type BybitInstrumentInverseResponse = BybitCursorListResponse<BybitInstrumentInverse>;
/// Response alias for instrument info requests that return option contracts.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
pub type BybitInstrumentOptionResponse = BybitCursorListResponse<BybitInstrumentOption>;

/// Fee rate structure returned by `GET /v5/account/fee-rate`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/fee-rate>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
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
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub wallet_balance: Decimal,
    pub unrealised_pnl: String,
    pub cum_realised_pnl: String,
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub locked: Decimal,
    pub collateral_switch: bool,
    pub margin_collateral: bool,
    pub coin: Ustr,
    #[serde(default)]
    pub spot_hedging_qty: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal_or_zero")]
    pub spot_borrow: Decimal,
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

/// Account-level configuration returned by `GET /v5/account/info`.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/account-info>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitAccountInfo {
    pub unified_margin_status: BybitUnifiedMarginStatus,
    pub margin_mode: BybitMarginMode,
    pub is_master_trader: bool,
    #[serde(with = "on_off_bool")]
    pub spot_hedging_status: bool,
    pub updated_time: String,
    // `dcp_status`, `time_window`, and `smp_group` are absent from responses
    // for accounts that predate the disconnection-protection feature.
    #[serde(default, with = "on_off_bool")]
    pub dcp_status: bool,
    #[serde(default)]
    pub time_window: i32,
    #[serde(default)]
    pub smp_group: i32,
}

/// Response alias for account info requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/account-info>
pub type BybitAccountInfoResponse = BybitResponse<BybitAccountInfo>;

/// Order representation as returned by order-related endpoints.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/order-list>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
)]
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
    pub smp_type: BybitSmpType,
    pub smp_group: i32,
    pub smp_order_id: Ustr,
    pub tpsl_mode: Option<BybitTpSlMode>,
    pub tp_limit_price: String,
    pub sl_limit_price: String,
    pub place_type: Ustr,
    pub created_time: String,
    pub updated_time: String,
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BybitOrder {
    #[getter]
    #[must_use]
    pub fn order_id(&self) -> &str {
        self.order_id.as_str()
    }

    #[getter]
    #[must_use]
    pub fn order_link_id(&self) -> &str {
        self.order_link_id.as_str()
    }

    #[getter]
    #[must_use]
    pub fn block_trade_id(&self) -> Option<&str> {
        self.block_trade_id.as_ref().map(|s| s.as_str())
    }

    #[getter]
    #[must_use]
    pub fn symbol(&self) -> &str {
        self.symbol.as_str()
    }

    #[getter]
    #[must_use]
    pub fn price(&self) -> &str {
        &self.price
    }

    #[getter]
    #[must_use]
    pub fn qty(&self) -> &str {
        &self.qty
    }

    #[getter]
    #[must_use]
    pub fn side(&self) -> BybitOrderSide {
        self.side
    }

    #[getter]
    #[must_use]
    pub fn is_leverage(&self) -> &str {
        &self.is_leverage
    }

    #[getter]
    #[must_use]
    pub fn position_idx(&self) -> i32 {
        self.position_idx
    }

    #[getter]
    #[must_use]
    pub fn order_status(&self) -> BybitOrderStatus {
        self.order_status
    }

    #[getter]
    #[must_use]
    pub fn cancel_type(&self) -> BybitCancelType {
        self.cancel_type
    }

    #[getter]
    #[must_use]
    pub fn reject_reason(&self) -> &str {
        self.reject_reason.as_str()
    }

    #[getter]
    #[must_use]
    pub fn avg_price(&self) -> Option<&str> {
        self.avg_price.as_deref()
    }

    #[getter]
    #[must_use]
    pub fn leaves_qty(&self) -> &str {
        &self.leaves_qty
    }

    #[getter]
    #[must_use]
    pub fn leaves_value(&self) -> &str {
        &self.leaves_value
    }

    #[getter]
    #[must_use]
    pub fn cum_exec_qty(&self) -> &str {
        &self.cum_exec_qty
    }

    #[getter]
    #[must_use]
    pub fn cum_exec_value(&self) -> &str {
        &self.cum_exec_value
    }

    #[getter]
    #[must_use]
    pub fn cum_exec_fee(&self) -> &str {
        &self.cum_exec_fee
    }

    #[getter]
    #[must_use]
    pub fn time_in_force(&self) -> BybitTimeInForce {
        self.time_in_force
    }

    #[getter]
    #[must_use]
    pub fn order_type(&self) -> BybitOrderType {
        self.order_type
    }

    #[getter]
    #[must_use]
    pub fn stop_order_type(&self) -> BybitStopOrderType {
        self.stop_order_type
    }

    #[getter]
    #[must_use]
    pub fn order_iv(&self) -> Option<&str> {
        self.order_iv.as_deref()
    }

    #[getter]
    #[must_use]
    pub fn trigger_price(&self) -> &str {
        &self.trigger_price
    }

    #[getter]
    #[must_use]
    pub fn take_profit(&self) -> &str {
        &self.take_profit
    }

    #[getter]
    #[must_use]
    pub fn stop_loss(&self) -> &str {
        &self.stop_loss
    }

    #[getter]
    #[must_use]
    pub fn tp_trigger_by(&self) -> BybitTriggerType {
        self.tp_trigger_by
    }

    #[getter]
    #[must_use]
    pub fn sl_trigger_by(&self) -> BybitTriggerType {
        self.sl_trigger_by
    }

    #[getter]
    #[must_use]
    pub fn trigger_direction(&self) -> BybitTriggerDirection {
        self.trigger_direction
    }

    #[getter]
    #[must_use]
    pub fn trigger_by(&self) -> BybitTriggerType {
        self.trigger_by
    }

    #[getter]
    #[must_use]
    pub fn last_price_on_created(&self) -> &str {
        &self.last_price_on_created
    }

    #[getter]
    #[must_use]
    pub fn reduce_only(&self) -> bool {
        self.reduce_only
    }

    #[getter]
    #[must_use]
    pub fn close_on_trigger(&self) -> bool {
        self.close_on_trigger
    }

    #[getter]
    #[must_use]
    #[expect(
        clippy::missing_panics_doc,
        reason = "serialization of a simple enum cannot fail"
    )]
    pub fn smp_type(&self) -> String {
        serde_json::to_string(&self.smp_type)
            .expect("Failed to serialize BybitSmpType")
            .trim_matches('"')
            .to_string()
    }

    #[getter]
    #[must_use]
    pub fn smp_group(&self) -> i32 {
        self.smp_group
    }

    #[getter]
    #[must_use]
    pub fn smp_order_id(&self) -> &str {
        self.smp_order_id.as_str()
    }

    #[getter]
    #[must_use]
    pub fn tpsl_mode(&self) -> Option<BybitTpSlMode> {
        self.tpsl_mode
    }

    #[getter]
    #[must_use]
    pub fn tp_limit_price(&self) -> &str {
        &self.tp_limit_price
    }

    #[getter]
    #[must_use]
    pub fn sl_limit_price(&self) -> &str {
        &self.sl_limit_price
    }

    #[getter]
    #[must_use]
    pub fn place_type(&self) -> &str {
        self.place_type.as_str()
    }

    #[getter]
    #[must_use]
    pub fn created_time(&self) -> &str {
        &self.created_time
    }

    #[getter]
    #[must_use]
    pub fn updated_time(&self) -> &str {
        &self.updated_time
    }
}

/// Response alias for open order queries.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/order/order-list>
pub type BybitOpenOrdersResponse = BybitCursorListResponse<BybitOrder>;
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
/// - <https://bybit-exchange.github.io/docs/v5/order/execution>
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
    pub create_type: Option<BybitCreateType>,
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
/// - <https://bybit-exchange.github.io/docs/v5/order/execution>
pub type BybitTradeHistoryResponse = BybitCursorListResponse<BybitExecution>;

/// Represents a position returned by the Bybit API.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/position>
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
    pub position_status: BybitPositionStatus,
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
    pub tpsl_mode: BybitTpSlMode,
    pub take_profit: String,
    pub stop_loss: String,
    pub trailing_stop: String,
    pub unrealised_pnl: String,
    pub cur_realised_pnl: String,
    pub cum_realised_pnl: String,
    #[serde(default = "default_position_seq")]
    pub seq: i64,
    #[serde(default)]
    pub is_reduce_only: bool,
    #[serde(default)]
    pub mmr_sys_updated_time: String,
    #[serde(default)]
    pub leverage_sys_updated_time: String,
    pub created_time: String,
    pub updated_time: String,
}

const fn default_position_seq() -> i64 {
    -1
}

/// Response alias for position list requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/position>
pub type BybitPositionListResponse = BybitCursorListResponse<BybitPosition>;

/// Reason detail for set margin mode failures.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/set-margin-mode>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitSetMarginModeReason {
    pub reason_code: String,
    pub reason_msg: String,
}

/// Result payload for set margin mode operation.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/set-margin-mode>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitSetMarginModeResult {
    #[serde(default)]
    pub reasons: Vec<BybitSetMarginModeReason>,
}

/// Response alias for set margin mode requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/account/set-margin-mode>
pub type BybitSetMarginModeResponse = BybitResponse<BybitSetMarginModeResult>;

/// Empty result for set leverage operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BybitSetLeverageResult {}

/// Response alias for set leverage requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/position/leverage>
pub type BybitSetLeverageResponse = BybitResponse<BybitSetLeverageResult>;

/// Empty result for switch mode operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BybitSwitchModeResult {}

/// Response alias for switch mode requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/position/position-mode>
pub type BybitSwitchModeResponse = BybitResponse<BybitSwitchModeResult>;

/// Empty result for set trading stop operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BybitSetTradingStopResult {}

/// Response alias for set trading stop requests.
///
/// # References
/// - <https://bybit-exchange.github.io/docs/v5/position/trading-stop>
pub type BybitSetTradingStopResponse = BybitResponse<BybitSetTradingStopResult>;

/// Result from manual borrow operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitBorrowResult {
    pub coin: Ustr,
    pub amount: String,
}

/// Response alias for manual borrow requests.
///
/// # References
///
/// - <https://bybit-exchange.github.io/docs/v5/account/borrow>
pub type BybitBorrowResponse = BybitResponse<BybitBorrowResult>;

/// Result from no-convert repay operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitNoConvertRepayResult {
    pub result_status: String,
}

/// Response alias for no-convert repay requests.
///
/// # References
///
/// - <https://bybit-exchange.github.io/docs/v5/account/no-convert-repay>
pub type BybitNoConvertRepayResponse = BybitResponse<BybitNoConvertRepayResult>;

/// API key permissions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
)]
#[serde(rename_all = "PascalCase")]
pub struct BybitApiKeyPermissions {
    #[serde(default)]
    pub contract_trade: Vec<String>,
    #[serde(default)]
    pub spot: Vec<String>,
    #[serde(default)]
    pub wallet: Vec<String>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub derivatives: Vec<String>,
    #[serde(default)]
    pub exchange: Vec<String>,
    #[serde(default)]
    pub copy_trading: Vec<String>,
    #[serde(default)]
    pub block_trade: Vec<String>,
    // Bybit ships this key uppercase (`"NFT"`); the struct-level PascalCase
    // rule would otherwise serialize it as `"Nft"` and silently drop values.
    #[serde(rename = "NFT", default)]
    pub nft: Vec<String>,
    #[serde(default)]
    pub affiliate: Vec<String>,
    // Newer permission buckets. Master-account responses populate them, sub-key
    // responses typically omit or return empty arrays — both cases deserialize
    // to an empty `Vec` via `serde(default)`.
    #[serde(default)]
    pub earn: Vec<String>,
    // Bybit uses `"FiatP2P"` — PascalCase rename would emit `"FiatP2p"`.
    #[serde(rename = "FiatP2P", default)]
    pub fiat_p2p: Vec<String>,
    #[serde(default)]
    pub fiat_bybit_pay: Vec<String>,
    #[serde(default)]
    pub fiat_bit_pay: Vec<String>,
    #[serde(default)]
    pub fiat_global_pay: Vec<String>,
    #[serde(default)]
    pub fiat_convert_broker: Vec<String>,
    #[serde(default)]
    pub bit_card: Vec<String>,
    // Bybit uses `"ByXPost"` — PascalCase rename would emit `"ByxPost"`.
    #[serde(rename = "ByXPost", default)]
    pub byx_post: Vec<String>,
}

/// Account details from API key info.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
)]
#[serde(rename_all = "camelCase")]
pub struct BybitAccountDetails {
    pub id: String,
    pub note: String,
    pub api_key: String,
    pub read_only: u8,
    pub secret: String,
    #[serde(rename = "type")]
    pub key_type: u8,
    pub permissions: BybitApiKeyPermissions,
    pub ips: Vec<String>,
    #[serde(default)]
    pub user_id: Option<u64>,
    #[serde(default)]
    pub inviter_id: Option<u64>,
    pub vip_level: String,
    #[serde(deserialize_with = "deserialize_string_to_u8", default)]
    pub mkt_maker_level: u8,
    #[serde(default)]
    pub affiliate_id: Option<u64>,
    pub rsa_public_key: String,
    pub is_master: bool,
    pub parent_uid: String,
    pub uta: u8,
    pub kyc_level: String,
    pub kyc_region: String,
    #[serde(default)]
    pub unified: Option<i32>,
    #[serde(default)]
    pub deadline_day: i64,
    #[serde(default)]
    pub expired_at: Option<String>,
    pub created_at: String,
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BybitAccountDetails {
    #[getter]
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[getter]
    #[must_use]
    pub fn note(&self) -> &str {
        &self.note
    }

    #[getter]
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    #[getter]
    #[must_use]
    pub fn read_only(&self) -> u8 {
        self.read_only
    }

    #[getter]
    #[must_use]
    pub fn key_type(&self) -> u8 {
        self.key_type
    }

    #[getter]
    #[must_use]
    pub fn user_id(&self) -> Option<u64> {
        self.user_id
    }

    #[getter]
    #[must_use]
    pub fn inviter_id(&self) -> Option<u64> {
        self.inviter_id
    }

    #[getter]
    #[must_use]
    pub fn vip_level(&self) -> &str {
        &self.vip_level
    }

    #[getter]
    #[must_use]
    pub fn mkt_maker_level(&self) -> u8 {
        self.mkt_maker_level
    }

    #[getter]
    #[must_use]
    pub fn affiliate_id(&self) -> Option<u64> {
        self.affiliate_id
    }

    #[getter]
    #[must_use]
    pub fn rsa_public_key(&self) -> &str {
        &self.rsa_public_key
    }

    #[getter]
    #[must_use]
    pub fn is_master(&self) -> bool {
        self.is_master
    }

    #[getter]
    #[must_use]
    pub fn parent_uid(&self) -> &str {
        &self.parent_uid
    }

    #[getter]
    #[must_use]
    pub fn uta(&self) -> u8 {
        self.uta
    }

    #[getter]
    #[must_use]
    pub fn kyc_level(&self) -> &str {
        &self.kyc_level
    }

    #[getter]
    #[must_use]
    pub fn kyc_region(&self) -> &str {
        &self.kyc_region
    }

    #[getter]
    #[must_use]
    pub fn deadline_day(&self) -> i64 {
        self.deadline_day
    }

    #[getter]
    #[must_use]
    pub fn expired_at(&self) -> Option<&str> {
        self.expired_at.as_deref()
    }

    #[getter]
    #[must_use]
    pub fn created_at(&self) -> &str {
        &self.created_at
    }
}

/// Response alias for API key info requests.
///
/// # References
///
/// - <https://bybit-exchange.github.io/docs/v5/user/apikey-info>
pub type BybitAccountDetailsResponse = BybitResponse<BybitAccountDetails>;

/// Basic information about a sub-account member.
///
/// `member_type`, `status`, and `account_mode` use raw integer codes whose valid
/// ranges differ per endpoint; values are kept as-is rather than mapped to Rust
/// enums, consistent with other venue-raw fields in this module.
///
/// # References
///
/// - <https://bybit-exchange.github.io/docs/v5/user/subuid-list>
/// - <https://bybit-exchange.github.io/docs/v5/user/page-subuid>
/// - <https://bybit-exchange.github.io/docs/v5/user/fund-subuid-list>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitSubMember {
    pub uid: String,
    pub username: String,
    pub member_type: i32,
    pub status: i32,
    pub account_mode: i32,
    #[serde(default)]
    pub remark: String,
}

/// Result payload for `GET /v5/user/query-sub-members`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitSubMembersResult {
    #[serde(default)]
    pub sub_members: Vec<BybitSubMember>,
}

/// Response alias for the non-paginated sub-UID list.
///
/// # References
///
/// - <https://bybit-exchange.github.io/docs/v5/user/subuid-list>
pub type BybitSubMembersResponse = BybitResponse<BybitSubMembersResult>;

/// Result payload for cursor-paginated sub-account listings.
///
/// The inner array is named `subMembers` and the cursor field is `nextCursor`
/// (with `"0"` as the end-of-pages sentinel), so the standard
/// `BybitCursorListResponse<T>` (which expects `list` / `nextPageCursor`)
/// cannot be reused here. Callers treat `"0"` or an empty string as the
/// termination sentinel.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitSubMembersPagedResult {
    #[serde(default)]
    pub sub_members: Vec<BybitSubMember>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

impl BybitSubMembersPagedResult {
    /// Returns the cursor to use for the next page, or `None` when the final
    /// page has been fetched.
    ///
    /// Bybit signals end-of-pages either by omitting the cursor or returning
    /// `"0"`/`""`; both cases collapse to `None` here so callers can treat any
    /// non-`None` return value as a live cursor.
    #[must_use]
    pub fn continuation_cursor(&self) -> Option<&str> {
        match self.next_cursor.as_deref() {
            None | Some("" | "0") => None,
            Some(cursor) => Some(cursor),
        }
    }

    /// Returns `true` when the result has more pages to fetch.
    #[must_use]
    pub fn has_more_pages(&self) -> bool {
        self.continuation_cursor().is_some()
    }
}

/// Response alias for paginated sub-UID list (`/v5/user/submembers`).
///
/// # References
///
/// - <https://bybit-exchange.github.io/docs/v5/user/page-subuid>
pub type BybitSubMembersPagedResponse = BybitResponse<BybitSubMembersPagedResult>;

/// Response alias for the escrow (fund-custodial) sub-account list
/// (`/v5/user/escrow_sub_members`); shares the paginated sub-member shape.
///
/// # References
///
/// - <https://bybit-exchange.github.io/docs/v5/user/fund-subuid-list>
pub type BybitEscrowSubMembersResponse = BybitResponse<BybitSubMembersPagedResult>;

/// Information about a single sub-account API key.
///
/// Deliberately not shared with [`BybitAccountDetails`]: master-level fields
/// such as `is_master`, `parent_uid`, `uta`, and the KYC block are absent.
///
/// # References
///
/// - <https://bybit-exchange.github.io/docs/v5/user/list-sub-apikeys>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitSubApiKeyInfo {
    pub id: String,
    #[serde(default)]
    pub ips: Vec<String>,
    pub api_key: String,
    #[serde(default)]
    pub note: String,
    pub status: i32,
    #[serde(default)]
    pub expired_at: Option<String>,
    pub created_at: String,
    #[serde(rename = "type")]
    pub key_type: BybitApiKeyType,
    #[serde(with = "masked_secret")]
    pub secret: Option<String>,
    #[serde(with = "bool_or_int")]
    pub read_only: bool,
    #[serde(default)]
    pub deadline_day: Option<i64>,
    #[serde(default)]
    pub flag: String,
    pub permissions: BybitApiKeyPermissions,
}

/// Result payload for `GET /v5/user/sub-apikeys`.
///
/// The inner array field is named `result` (nested inside the outer
/// `retCode/retMsg/result` envelope) rather than the usual `list`, so the
/// standard `BybitCursorListResponse<T>` cannot be reused here.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitSubApiKeysResult {
    #[serde(rename = "result", default)]
    pub keys: Vec<BybitSubApiKeyInfo>,
    #[serde(default)]
    pub next_page_cursor: Option<String>,
}

impl BybitSubApiKeysResult {
    /// Returns the cursor to use for the next page, or `None` when the final
    /// page has been fetched.
    ///
    /// The end-of-pages sentinel on this endpoint is an empty string rather
    /// than `"0"`; both that and a missing cursor collapse to `None`.
    #[must_use]
    pub fn continuation_cursor(&self) -> Option<&str> {
        match self.next_page_cursor.as_deref() {
            None | Some("") => None,
            Some(cursor) => Some(cursor),
        }
    }

    /// Returns `true` when the result has more pages to fetch.
    #[must_use]
    pub fn has_more_pages(&self) -> bool {
        self.continuation_cursor().is_some()
    }
}

/// Response alias for sub-account API keys list.
///
/// # References
///
/// - <https://bybit-exchange.github.io/docs/v5/user/list-sub-apikeys>
pub type BybitSubApiKeysResponse = BybitResponse<BybitSubApiKeysResult>;

/// Shared result payload for API-key update endpoints (sub or master).
///
/// `/v5/user/update-sub-api` and `/v5/user/update-api` return the same field
/// set; only the number of permission buckets populated inside `permissions`
/// differs. Because [`BybitApiKeyPermissions`] covers the superset of both,
/// the two endpoints reuse a single DTO.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitApiKeyUpdateResult {
    pub id: String,
    #[serde(default)]
    pub note: String,
    pub api_key: String,
    #[serde(with = "bool_or_int")]
    pub read_only: bool,
    #[serde(with = "masked_secret")]
    pub secret: Option<String>,
    pub permissions: BybitApiKeyPermissions,
    #[serde(default)]
    pub ips: Vec<String>,
}

/// Response alias for `POST /v5/user/update-sub-api`.
///
/// # References
///
/// - <https://bybit-exchange.github.io/docs/v5/user/modify-sub-apikey>
pub type BybitUpdateSubApiResponse = BybitResponse<BybitApiKeyUpdateResult>;

/// Response alias for `POST /v5/user/update-api`.
///
/// # References
///
/// - <https://bybit-exchange.github.io/docs/v5/user/modify-master-apikey>
pub type BybitUpdateMasterApiResponse = BybitResponse<BybitApiKeyUpdateResult>;

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::identifiers::AccountId;
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

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
    fn deserialize_account_info_response() {
        let json = load_test_json("http_get_account_info.json");
        let response: BybitAccountInfoResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(response.result.margin_mode, BybitMarginMode::RegularMargin);
        assert_eq!(
            response.result.unified_margin_status,
            BybitUnifiedMarginStatus::UnifiedTradingAccount10Pro
        );
        assert!(!response.result.is_master_trader);
        assert!(!response.result.spot_hedging_status);
        assert!(!response.result.dcp_status);
        assert_eq!(response.result.time_window, 10);
        assert_eq!(response.result.smp_group, 0);
    }

    #[rstest]
    fn deserialize_account_info_without_deprecated_fields() {
        let json = r#"{
            "retCode": 0,
            "retMsg": "OK",
            "result": {
                "marginMode": "PORTFOLIO_MARGIN",
                "updatedTime": "1697078946000",
                "unifiedMarginStatus": 5,
                "isMasterTrader": true,
                "spotHedgingStatus": "ON"
            }
        }"#;
        let response: BybitAccountInfoResponse = serde_json::from_str(json).unwrap();

        assert_eq!(
            response.result.margin_mode,
            BybitMarginMode::PortfolioMargin
        );
        assert_eq!(
            response.result.unified_margin_status,
            BybitUnifiedMarginStatus::UnifiedTradingAccount20
        );
        assert!(response.result.is_master_trader);
        assert!(response.result.spot_hedging_status);
        assert!(!response.result.dcp_status);
        assert_eq!(response.result.time_window, 0);
        assert_eq!(response.result.smp_group, 0);
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
        assert_eq!(order.smp_type, BybitSmpType::None);
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
        assert_eq!(usdt.wallet_balance, dec!(1000.50));
        assert_eq!(usdt.total_order_im, None);
        assert_eq!(usdt.total_position_mm, None);
        assert_eq!(usdt.total_position_im, None);
        assert_eq!(btc.spot_borrow, Decimal::ZERO);
        assert_eq!(usdt.spot_borrow, Decimal::ZERO);
    }

    #[rstest]
    fn test_parse_wallet_balance_with_spot_borrow() {
        let json = include_str!("../../test_data/http_get_wallet_balance_with_spot_borrow.json");
        let response: BybitWalletBalanceResponse =
            serde_json::from_str(json).expect("Failed to parse wallet balance with spotBorrow");

        let wallet = &response.result.list[0];
        let usdt = &wallet.coin[0];

        assert_eq!(usdt.coin.as_str(), "USDT");
        assert_eq!(usdt.wallet_balance, dec!(1200.00));
        assert_eq!(usdt.spot_borrow, dec!(200.00));
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

    #[rstest]
    fn test_parse_wallet_balance_spot_short() {
        let json = include_str!("../../test_data/http_get_wallet_balance_spot_short.json");
        let response: BybitWalletBalanceResponse = serde_json::from_str(json)
            .expect("Failed to parse wallet balance with SHORT SPOT position");

        let wallet = &response.result.list[0];
        let eth = &wallet.coin[0];

        assert_eq!(eth.coin.as_str(), "ETH");
        assert_eq!(eth.wallet_balance, dec!(0));
        assert_eq!(eth.spot_borrow, dec!(0.06142));
        assert_eq!(eth.borrow_amount, "0.06142");

        let account_state = crate::common::parse::parse_account_state(
            wallet,
            AccountId::new("BYBIT-001"),
            UnixNanos::default(),
        )
        .expect("Failed to parse account state");

        let eth_balance = account_state
            .balances
            .iter()
            .find(|b| b.currency.code.as_str() == "ETH")
            .expect("ETH balance not found");

        // Negative balance represents SHORT position (borrowed ETH)
        assert_eq!(eth_balance.total.as_f64(), -0.06142);
    }

    #[rstest]
    fn deserialize_borrow_response() {
        let json = r#"{
            "retCode": 0,
            "retMsg": "success",
            "result": {
                "coin": "BTC",
                "amount": "0.01"
            },
            "retExtInfo": {},
            "time": 1756197991955
        }"#;

        let response: BybitBorrowResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.ret_code, 0);
        assert_eq!(response.ret_msg, "success");
        assert_eq!(response.result.coin, "BTC");
        assert_eq!(response.result.amount, "0.01");
    }

    #[rstest]
    fn deserialize_no_convert_repay_response() {
        let json = r#"{
            "retCode": 0,
            "retMsg": "OK",
            "result": {
                "resultStatus": "SU"
            },
            "retExtInfo": {},
            "time": 1234567890
        }"#;

        let response: BybitNoConvertRepayResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.ret_code, 0);
        assert_eq!(response.ret_msg, "OK");
        assert_eq!(response.result.result_status, "SU");
    }

    #[rstest]
    fn deserialize_position_without_conditional_fields() {
        // Bybit v5 docs mark `isReduceOnly`, `mmrSysUpdatedTime`, `leverageSysUpdatedTime`
        // and `seq` as conditional fields that may be absent, e.g. once a position has been
        // closed through the UI (see issue #3836).
        let json = r#"{
            "retCode": 0,
            "retMsg": "OK",
            "result": {
                "list": [{
                    "positionIdx": 0,
                    "riskId": 1,
                    "riskLimitValue": "150",
                    "symbol": "LTCUSDT",
                    "side": "",
                    "size": "0",
                    "avgPrice": "0",
                    "positionValue": "0",
                    "tradeMode": 0,
                    "positionStatus": "Normal",
                    "autoAddMargin": 0,
                    "adlRankIndicator": 0,
                    "leverage": "10",
                    "positionBalance": "0",
                    "markPrice": "70.00",
                    "liqPrice": "",
                    "bustPrice": "",
                    "positionMM": "0",
                    "positionIM": "0",
                    "tpslMode": "Full",
                    "takeProfit": "0",
                    "stopLoss": "0",
                    "trailingStop": "0",
                    "unrealisedPnl": "0",
                    "curRealisedPnl": "0",
                    "cumRealisedPnl": "0",
                    "createdTime": "1676538056258",
                    "updatedTime": "1697673600012"
                }],
                "nextPageCursor": "",
                "category": "linear"
            },
            "retExtInfo": {},
            "time": 1697673900000
        }"#;

        let response: BybitPositionListResponse = serde_json::from_str(json)
            .expect("Failed to parse position list with missing conditional fields");

        let position = &response.result.list[0];
        assert!(!position.is_reduce_only);
        assert_eq!(position.seq, -1);
        assert_eq!(position.mmr_sys_updated_time, "");
        assert_eq!(position.leverage_sys_updated_time, "");
    }

    #[rstest]
    fn deserialize_sub_members_response() {
        let json = load_test_json("http_get_user_sub_members.json");
        let response: BybitSubMembersResponse =
            serde_json::from_str(&json).expect("parse sub members");
        assert_eq!(response.ret_code, 0);
        assert_eq!(response.result.sub_members.len(), 2);
        let first = &response.result.sub_members[0];
        assert_eq!(first.uid, "106314365");
        assert_eq!(first.username, "xxxx02");
        assert_eq!(first.member_type, 1);
        assert_eq!(first.status, 1);
        assert_eq!(first.account_mode, 5);
        assert_eq!(first.remark, "");
        let second = &response.result.sub_members[1];
        assert_eq!(second.uid, "106279879");
        assert_eq!(second.account_mode, 6);
    }

    #[rstest]
    fn deserialize_sub_members_paged_response() {
        // The final-page sentinel is `"0"`; both `"0"` and `None` collapse to
        // `continuation_cursor() == None` via the helper.
        let json = load_test_json("http_get_user_sub_members_paged.json");
        let response: BybitSubMembersPagedResponse =
            serde_json::from_str(&json).expect("parse paged sub members");
        assert_eq!(response.result.sub_members.len(), 2);
        assert_eq!(response.result.next_cursor.as_deref(), Some("0"));
        assert!(!response.result.has_more_pages());
        assert_eq!(response.result.continuation_cursor(), None);
    }

    #[rstest]
    fn deserialize_escrow_sub_members_response_uses_same_shape() {
        // The escrow alias must decode into the same shape as the paginated
        // sub-member list; a non-`"0"` cursor indicates more pages to fetch.
        let json = load_test_json("http_get_user_escrow_sub_members.json");
        let response: BybitEscrowSubMembersResponse =
            serde_json::from_str(&json).expect("parse escrow sub members");
        assert_eq!(response.result.sub_members.len(), 2);
        assert_eq!(response.result.sub_members[0].member_type, 12);
        assert_eq!(response.result.sub_members[0].remark, "earn fund");
        assert_eq!(response.result.next_cursor.as_deref(), Some("344"));
        assert!(response.result.has_more_pages());
        assert_eq!(response.result.continuation_cursor(), Some("344"));
    }

    #[rstest]
    fn deserialize_sub_api_keys_response() {
        // `readOnly` arrives as a bool here; the masked `"******"` secret
        // collapses to `None` through the `masked_secret` helper.
        let json = load_test_json("http_get_user_sub_apikeys.json");
        let response: BybitSubApiKeysResponse =
            serde_json::from_str(&json).expect("parse sub apikeys");
        assert_eq!(response.result.keys.len(), 1);
        let key = &response.result.keys[0];
        assert!(!key.read_only);
        assert_eq!(key.secret, None);
        assert_eq!(key.key_type, BybitApiKeyType::Hmac);
        assert_eq!(key.flag, "hmac");
        assert_eq!(key.deadline_day, Some(21));
        assert_eq!(key.permissions.contract_trade, vec!["Order", "Position"]);
        assert_eq!(key.permissions.spot, vec!["SpotTrade"]);
        assert!(key.permissions.earn.is_empty());
        assert_eq!(response.result.next_page_cursor.as_deref(), Some(""));
        assert!(!response.result.has_more_pages());
    }

    #[rstest]
    fn deserialize_update_sub_api_response() {
        let json = load_test_json("http_post_user_update_sub_api.json");
        let response: BybitUpdateSubApiResponse =
            serde_json::from_str(&json).expect("parse update sub api");
        assert!(!response.result.read_only);
        assert_eq!(response.result.secret, None);
        assert_eq!(response.result.ips, vec!["*"]);
        assert_eq!(response.result.permissions.spot, vec!["SpotTrade"]);
        assert_eq!(response.result.permissions.wallet, vec!["AccountTransfer"]);
    }

    #[rstest]
    fn deserialize_update_master_api_response() {
        // Asserts on non-empty permission buckets so the test actually verifies
        // deserialisation (an empty `Vec` would be indistinguishable from a
        // `#[serde(default)]` fallback). In particular, `nft` exercises the
        // explicit `#[serde(rename = "NFT")]` attribute.
        let json = load_test_json("http_post_user_update_master_api.json");
        let response: BybitUpdateMasterApiResponse =
            serde_json::from_str(&json).expect("parse update master api");
        assert!(!response.result.read_only);
        assert_eq!(response.result.ips, vec!["*"]);
        let perms = &response.result.permissions;
        assert_eq!(perms.contract_trade, vec!["Order", "Position"]);
        assert_eq!(perms.copy_trading, vec!["CopyTrading"]);
        assert!(perms.earn.is_empty());
        assert_eq!(perms.nft, vec!["NFTQueryProductList"]);
    }

    #[rstest]
    fn deserialize_permissions_renamed_buckets_preserve_values() {
        // Regression guard for `#[serde(rename = ...)]` on permission keys
        // whose Bybit casing (`NFT`, `FiatP2P`, `ByXPost`) differs from
        // serde's `PascalCase` default (`Nft`, `FiatP2p`, `ByxPost`). Using
        // non-empty values ensures a rename regression causes a failure
        // rather than silently falling through to `serde(default)`.
        let json = r#"{
            "NFT": ["NFTQueryProductList"],
            "FiatP2P": ["P2PDeposit"],
            "ByXPost": ["PostContent"]
        }"#;
        let perms: BybitApiKeyPermissions =
            serde_json::from_str(json).expect("parse renamed buckets");
        assert_eq!(perms.nft, vec!["NFTQueryProductList"]);
        assert_eq!(perms.fiat_p2p, vec!["P2PDeposit"]);
        assert_eq!(perms.byx_post, vec!["PostContent"]);
    }

    #[rstest]
    fn deserialize_account_details_response_with_current_docs_example() {
        let json = load_test_json("http_get_user_query_api.json");
        let response: BybitAccountDetailsResponse =
            serde_json::from_str(&json).expect("parse account details");

        assert_eq!(
            response.result.permissions.fiat_global_pay,
            Vec::<String>::new()
        );
        assert_eq!(
            response.result.permissions.fiat_bit_pay,
            vec!["FaitPayOrder"]
        );
        assert_eq!(response.result.permissions.bit_card, vec!["BitCard"]);
        assert_eq!(response.result.permissions.byx_post, vec!["ByXPost"]);
        assert_eq!(response.result.unified, Some(0));
    }
}
