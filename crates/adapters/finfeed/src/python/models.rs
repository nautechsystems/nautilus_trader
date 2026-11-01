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

//! Typed Python views of raw provider results, with exact decimals and RFC 3339 timestamps.

use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use nautilus_model::instruments::BinaryOption;
use pyo3::{prelude::*, types::PyDict};
use rust_decimal::Decimal;

use crate::http::{
    models::{
        ActivityBatch, ActivityCurrent, Book, BookEvent, BookLevel, Exchange, Market, MarketStatus,
        MarketsPage, Ohlcv, OhlcvPeriod, OutcomeType, Quote, Trade, TradeSide,
    },
    parse::parse_instrument,
};

/// Provider exchange identity and metadata, retaining nullable fields.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedExchange",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedExchange {
    inner: Exchange,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedExchange {
    /// Provider exchange identifier, retained verbatim.
    #[getter(exchange_id)]
    fn py_exchange_id(&self) -> Option<String> {
        self.inner.exchange_id.map(|value| value.to_string())
    }

    /// Provider description of the exchange or institution.
    #[getter(market_name_institution_description)]
    fn py_market_name_institution_description(&self) -> Option<String> {
        self.inner.market_name_institution_description.clone()
    }

    /// Provider-supplied website URL.
    #[getter(website)]
    fn py_website(&self) -> Option<String> {
        self.inner.website.clone()
    }
}

impl From<Exchange> for PyFinFeedExchange {
    fn from(inner: Exchange) -> Self {
        Self { inner }
    }
}

/// Provider outcome metadata, with exact decimals and a separate checked Nautilus mapping.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedMarket",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedMarket {
    inner: Market,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedMarket {
    /// Provider exchange identifier for this outcome.
    #[getter(exchange_id)]
    fn py_exchange_id(&self) -> Option<String> {
        self.inner.exchange_id.map(|value| value.to_string())
    }

    /// Provider outcome identifier, retained verbatim.
    #[getter(market_id)]
    fn py_market_id(&self) -> Option<String> {
        self.inner.market_id.map(|value| value.to_string())
    }

    /// Provider contract classification, independent of native mapping support.
    #[getter(outcome_type)]
    fn py_outcome_type(&self) -> FinFeedOutcomeType {
        self.inner.outcome_type.into()
    }

    /// Provider outcome label, such as `Yes` or `No`.
    #[getter(outcome_name)]
    fn py_outcome_name(&self) -> Option<String> {
        self.inner.outcome_name.clone()
    }

    /// Provider market mechanism label.
    #[getter(mechanism)]
    fn py_mechanism(&self) -> Option<String> {
        self.inner.mechanism.map(|value| value.to_string())
    }

    /// Provider lifecycle label; unknown labels decode as `Unknown`.
    #[getter(status)]
    fn py_status(&self) -> FinFeedMarketStatus {
        self.inner.status.into()
    }

    /// Provider-reported outcome price, decoded without floating-point conversion.
    #[getter(price)]
    fn py_price(&self) -> Decimal {
        self.inner.price
    }

    /// Provider market title.
    #[getter(title)]
    fn py_title(&self) -> Option<String> {
        self.inner.title.clone()
    }

    /// Provider market description.
    #[getter(description)]
    fn py_description(&self) -> Option<String> {
        self.inner.description.clone()
    }

    /// Nested exchange metadata, when supplied.
    #[getter(exchange)]
    fn py_exchange(&self) -> Option<PyFinFeedExchange> {
        self.inner.exchange.clone().map(Into::into)
    }

    /// Venue-specific metadata retained as raw JSON values.
    #[getter(source_specific_data)]
    fn py_source_specific_data(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        let Some(source) = &self.inner.source_specific_data else {
            return Ok(None);
        };

        let loads = py.import("json")?.getattr("loads")?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("parse_float", py.import("decimal")?.getattr("Decimal")?)?;
        let values = PyDict::new(py);
        for (key, raw) in source {
            values.set_item(key, loads.call((raw.get(),), Some(&kwargs))?)?;
        }

        Ok(Some(values.unbind()))
    }

    /// Parses a checked provider-owned Polymarket Yes/No definition.
    ///
    /// Polymarket uses pUSD and six-decimal shares, matching the native adapter's conventions.
    /// Unavailable activation uses its zero timestamp convention. `EndDateIso` sets the simulated
    /// trading cutoff; it does not prove actual venue closure or authoritative settlement.
    /// Other venue families fail until their required field rules are verified.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The venue or contract is unsupported.
    /// - Required identity or metadata is missing, invalid, or conflicting.
    /// - The tick is outside `(0, 1)` or the provider price is outside `[0, 1]`.
    /// - A price requires rounding or expiry is outside the supported timestamp range.
    #[pyo3(signature = (ts_init=0))]
    fn to_instrument(&self, ts_init: u64) -> PyResult<BinaryOption> {
        let ts_init = UnixNanos::from(ts_init);
        parse_instrument(&self.inner, ts_init).map_err(to_pyvalue_err)
    }
}

impl From<Market> for PyFinFeedMarket {
    fn from(inner: Market) -> Self {
        Self { inner }
    }
}

/// A cursor listing envelope; a missing next cursor establishes termination.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedMarketsPage",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedMarketsPage {
    inner: MarketsPage,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedMarketsPage {
    /// Outcome definitions for this page; absent or null data remains `None`.
    #[getter(data)]
    fn py_data(&self) -> Option<Vec<PyFinFeedMarket>> {
        self.inner
            .data
            .clone()
            .map(|records| records.into_iter().map(Into::into).collect())
    }

    /// Continuation cursor; `None` ends traversal.
    #[getter(next_cursor)]
    fn py_next_cursor(&self) -> Option<String> {
        self.inner.next_cursor.clone()
    }
}

impl From<MarketsPage> for PyFinFeedMarketsPage {
    fn from(inner: MarketsPage) -> Self {
        Self { inner }
    }
}

/// A provider trade with exact price and quantity, without aggressor-side inference.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedTrade",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedTrade {
    inner: Trade,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedTrade {
    /// Provider outcome identifier.
    #[getter(market_id)]
    fn py_market_id(&self) -> Option<String> {
        self.inner.market_id.map(|value| value.to_string())
    }

    /// Opaque provider trade ID; native conversion requires a nonempty ID.
    #[getter(id)]
    fn py_id(&self) -> Option<String> {
        self.inner.id.clone()
    }

    /// Provider outcome label, when supplied.
    #[getter(outcome)]
    fn py_outcome(&self) -> Option<String> {
        self.inner.outcome.clone()
    }

    /// Provider side label; does not identify the aggressor.
    #[getter(side)]
    fn py_side(&self) -> FinFeedTradeSide {
        self.inner.side.into()
    }

    /// Trade price, decoded without floating-point conversion.
    #[getter(price)]
    fn py_price(&self) -> Decimal {
        self.inner.price
    }

    /// Trade quantity, decoded without floating-point conversion.
    #[getter(quantity)]
    fn py_quantity(&self) -> Decimal {
        self.inner.quantity
    }

    /// Sole provider timestamp, used for both native event and initialization times.
    #[getter(timestamp)]
    fn py_timestamp(&self) -> String {
        self.inner.timestamp.to_string()
    }
}

impl From<Trade> for PyFinFeedTrade {
    fn from(inner: Trade) -> Self {
        Self { inner }
    }
}

/// Exact quote prices and sizes, retaining both provider timestamps.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedQuote",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedQuote {
    inner: Quote,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedQuote {
    /// Bid price, decoded exactly.
    #[getter(bid)]
    fn py_bid(&self) -> Decimal {
        self.inner.bid
    }

    /// Ask price, decoded exactly.
    #[getter(ask)]
    fn py_ask(&self) -> Decimal {
        self.inner.ask
    }

    /// Size available at the bid, decoded exactly.
    #[getter(bid_volume)]
    fn py_bid_volume(&self) -> Decimal {
        self.inner.bid_volume
    }

    /// Size available at the ask, decoded exactly.
    #[getter(ask_volume)]
    fn py_ask_volume(&self) -> Decimal {
        self.inner.ask_volume
    }

    /// Provider entry timestamp, used as native `ts_event`.
    #[getter(entry_time)]
    fn py_entry_time(&self) -> String {
        self.inner.entry_time.to_string()
    }

    /// Provider receive timestamp, used as native `ts_init`.
    #[getter(recv_time)]
    fn py_recv_time(&self) -> String {
        self.inner.recv_time.to_string()
    }
}

impl From<Quote> for PyFinFeedQuote {
    fn from(inner: Quote) -> Self {
        Self { inner }
    }
}

/// The latest trade and quote, each independently optional.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedActivityCurrent",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedActivityCurrent {
    inner: ActivityCurrent,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedActivityCurrent {
    /// Latest quote; absent or null values remain `None`.
    #[getter(quote)]
    fn py_quote(&self) -> Option<PyFinFeedQuote> {
        self.inner.quote.clone().map(Into::into)
    }

    /// Latest trade; absent or null values remain `None`.
    #[getter(trade)]
    fn py_trade(&self) -> Option<PyFinFeedTrade> {
        self.inner.trade.clone().map(Into::into)
    }
}

impl From<ActivityCurrent> for PyFinFeedActivityCurrent {
    fn from(inner: ActivityCurrent) -> Self {
        Self { inner }
    }
}

/// Latest activity lists, retaining the distinction between absent and empty lists.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedActivityBatch",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedActivityBatch {
    inner: ActivityBatch,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedActivityBatch {
    /// Quote list, independently bounded by the activity query limit.
    #[getter(quotes)]
    fn py_quotes(&self) -> Option<Vec<PyFinFeedQuote>> {
        self.inner
            .quotes
            .clone()
            .map(|records| records.into_iter().map(Into::into).collect())
    }

    /// Trade list, independently bounded by the activity query limit.
    #[getter(trades)]
    fn py_trades(&self) -> Option<Vec<PyFinFeedTrade>> {
        self.inner
            .trades
            .clone()
            .map(|records| records.into_iter().map(Into::into).collect())
    }
}

impl From<ActivityBatch> for PyFinFeedActivityBatch {
    fn from(inner: ActivityBatch) -> Self {
        Self { inner }
    }
}

/// A snapshot price level with exact size.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedBookLevel",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedBookLevel {
    inner: BookLevel,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedBookLevel {
    /// Price of the aggregated level, decoded exactly.
    #[getter(price)]
    fn py_price(&self) -> Decimal {
        self.inner.price
    }

    /// Size of the aggregated level, decoded exactly.
    #[getter(size)]
    fn py_size(&self) -> Decimal {
        self.inner.size
    }
}

impl From<BookLevel> for PyFinFeedBookLevel {
    fn from(inner: BookLevel) -> Self {
        Self { inner }
    }
}

/// A raw current snapshot with nullable sides, without continuity or recovery inference.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedBook",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedBook {
    inner: Book,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedBook {
    /// Provider exchange identifier.
    #[getter(exchange_id)]
    fn py_exchange_id(&self) -> Option<String> {
        self.inner.exchange_id.map(|value| value.to_string())
    }

    /// Provider outcome identifier.
    #[getter(market_id)]
    fn py_market_id(&self) -> Option<String> {
        self.inner.market_id.map(|value| value.to_string())
    }

    /// Bid levels; `None` remains distinct from an empty side.
    #[getter(bids)]
    fn py_bids(&self) -> Option<Vec<PyFinFeedBookLevel>> {
        self.inner
            .bids
            .clone()
            .map(|records| records.into_iter().map(Into::into).collect())
    }

    /// Ask levels; `None` remains distinct from an empty side.
    #[getter(asks)]
    fn py_asks(&self) -> Option<Vec<PyFinFeedBookLevel>> {
        self.inner
            .asks
            .clone()
            .map(|records| records.into_iter().map(Into::into).collect())
    }

    /// Exchange timestamp, used as native `ts_event`.
    #[getter(time_exchange)]
    fn py_time_exchange(&self) -> String {
        self.inner.time_exchange.to_string()
    }

    /// Provider receive timestamp, used as native `ts_init`.
    #[getter(time_coinapi)]
    fn py_time_coinapi(&self) -> String {
        self.inner.time_coinapi.to_string()
    }
}

impl From<Book> for PyFinFeedBook {
    fn from(inner: Book) -> Self {
        Self { inner }
    }
}

/// A raw historical book event with uninterpreted numeric actions and provider ordering.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedBookEvent",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedBookEvent {
    inner: BookEvent,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedBookEvent {
    /// Provider symbol identifier, retained verbatim.
    #[getter(symbol_id)]
    fn py_symbol_id(&self) -> String {
        self.inner.symbol_id.to_string()
    }

    /// Provider event identifier, retained verbatim.
    #[getter(id)]
    fn py_id(&self) -> String {
        self.inner.id.clone()
    }

    /// Raw numeric action code, interpreted only during native L2 replay.
    #[getter(update_type)]
    fn py_update_type(&self) -> i64 {
        self.inner.update_type
    }

    /// Whether the event applies to the ask side.
    #[getter(is_sell_ask)]
    fn py_is_sell_ask(&self) -> bool {
        self.inner.is_sell_ask
    }

    /// Raw event price, decoded exactly.
    #[getter(price)]
    fn py_price(&self) -> Decimal {
        self.inner.price
    }

    /// Raw event size; absolute or relative meaning depends on the action.
    #[getter(size)]
    fn py_size(&self) -> Decimal {
        self.inner.size
    }

    /// Exchange event timestamp.
    #[getter(time_exchange)]
    fn py_time_exchange(&self) -> String {
        self.inner.time_exchange.to_string()
    }

    /// Optional provider processing timestamp; omissions remain `None`.
    #[getter(time_coinapi)]
    fn py_time_coinapi(&self) -> Option<String> {
        self.inner.time_coinapi.map(|time| time.to_string())
    }
}

impl From<BookEvent> for PyFinFeedBookEvent {
    fn from(inner: BookEvent) -> Self {
        Self { inner }
    }
}

/// A provider OHLCV period definition.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedOhlcvPeriod",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedOhlcvPeriod {
    inner: OhlcvPeriod,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedOhlcvPeriod {
    /// Provider identifier accepted by DB-backed OHLCV queries.
    #[getter(period_id)]
    fn py_period_id(&self) -> Option<String> {
        self.inner.period_id.map(|value| value.to_string())
    }

    /// Provider unit label.
    #[getter(unit_name)]
    fn py_unit_name(&self) -> Option<String> {
        self.inner.unit_name.map(|value| value.to_string())
    }

    /// Number of units in the provider period definition.
    #[getter(unit_count)]
    fn py_unit_count(&self) -> Option<i32> {
        self.inner.unit_count
    }

    /// Fixed period length in seconds.
    #[getter(length_seconds)]
    fn py_length_seconds(&self) -> i32 {
        self.inner.length_seconds
    }

    /// Calendar period length in months.
    #[getter(length_months)]
    fn py_length_months(&self) -> i32 {
        self.inner.length_months
    }

    /// Provider display label for the period.
    #[getter(display_name)]
    fn py_display_name(&self) -> Option<String> {
        self.inner.display_name.map(|value| value.to_string())
    }
}

impl From<OhlcvPeriod> for PyFinFeedOhlcvPeriod {
    fn from(inner: OhlcvPeriod) -> Self {
        Self { inner }
    }
}

/// A provider bar with nullable prices, exact aggregate volume, and an optional raw market identifier.
#[derive(Debug, Clone)]
#[pyclass(
    name = "FinFeedOhlcv",
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedOhlcv {
    inner: Ohlcv,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedOhlcv {
    /// Raw outcome identifier; exchange-wide history can include unknown or missing IDs.
    #[getter(market_id_exchange)]
    fn py_market_id_exchange(&self) -> Option<String> {
        self.inner.market_id_exchange.map(|value| value.to_string())
    }

    /// Opening price; absent or null values remain `None`.
    #[getter(price_open)]
    fn py_price_open(&self) -> Option<Decimal> {
        self.inner.price_open
    }

    /// Highest price; absent or null values remain `None`.
    #[getter(price_high)]
    fn py_price_high(&self) -> Option<Decimal> {
        self.inner.price_high
    }

    /// Lowest price; absent or null values remain `None`.
    #[getter(price_low)]
    fn py_price_low(&self) -> Option<Decimal> {
        self.inner.price_low
    }

    /// Closing price; absent or null values remain `None`.
    #[getter(price_close)]
    fn py_price_close(&self) -> Option<Decimal> {
        self.inner.price_close
    }

    /// Provider aggregate volume, decoded without floating-point conversion.
    #[getter(volume_traded)]
    fn py_volume_traded(&self) -> Decimal {
        self.inner.volume_traded
    }

    /// Provider-reported trade count.
    #[getter(trades_count)]
    fn py_trades_count(&self) -> i64 {
        self.inner.trades_count
    }

    /// Period start timestamp.
    #[getter(time_period_start)]
    fn py_time_period_start(&self) -> String {
        self.inner.time_period_start.to_string()
    }

    /// Period end timestamp, used for both native bar timestamps.
    #[getter(time_period_end)]
    fn py_time_period_end(&self) -> String {
        self.inner.time_period_end.to_string()
    }

    /// Provider timestamp of the opening observation, when supplied.
    #[getter(time_open)]
    fn py_time_open(&self) -> Option<String> {
        self.inner.time_open.map(|time| time.to_string())
    }

    /// Provider timestamp of the closing observation, when supplied.
    #[getter(time_close)]
    fn py_time_close(&self) -> Option<String> {
        self.inner.time_close.map(|time| time.to_string())
    }
}

impl From<Ohlcv> for PyFinFeedOhlcv {
    fn from(inner: Ohlcv) -> Self {
        Self { inner }
    }
}

/// Provider lifecycle labels, without settlement inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[pyclass(
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object,
    eq,
    rename_all = "SCREAMING_SNAKE_CASE"
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.finfeed")]
pub enum FinFeedMarketStatus {
    /// Provider labels the market open.
    Open,
    /// Provider labels the market closed; this is not a settlement event.
    Closed,
    /// Provider labels the market resolved; no payout is inferred.
    Resolved,
    /// Provider labels the market suspended.
    Suspended,
    /// Unrecognized provider lifecycle label.
    Unknown,
}

impl From<MarketStatus> for FinFeedMarketStatus {
    fn from(value: MarketStatus) -> Self {
        match value {
            MarketStatus::Open => Self::Open,
            MarketStatus::Closed => Self::Closed,
            MarketStatus::Resolved => Self::Resolved,
            MarketStatus::Suspended => Self::Suspended,
            MarketStatus::Unknown => Self::Unknown,
        }
    }
}

/// Provider outcome classifications, independent of Nautilus mapping support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[pyclass(
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object,
    eq,
    rename_all = "SCREAMING_SNAKE_CASE"
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.finfeed")]
pub enum FinFeedOutcomeType {
    /// Binary outcome contract.
    Binary,
    /// Multiple-choice outcome contract.
    MultipleChoice,
    /// Numeric outcome contract.
    Numeric,
    /// Unrecognized provider outcome classification.
    Other,
}

impl From<OutcomeType> for FinFeedOutcomeType {
    fn from(value: OutcomeType) -> Self {
        match value {
            OutcomeType::Binary => Self::Binary,
            OutcomeType::MultipleChoice => Self::MultipleChoice,
            OutcomeType::Numeric => Self::Numeric,
            OutcomeType::Other => Self::Other,
        }
    }
}

/// Provider trade-side labels, without aggressor-side inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[pyclass(
    module = "nautilus_trader.adapters.finfeed",
    frozen,
    from_py_object,
    eq,
    rename_all = "SCREAMING_SNAKE_CASE"
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.finfeed")]
pub enum FinFeedTradeSide {
    /// Provider buy-side label; does not identify the aggressor.
    Buy,
    /// Provider sell-side label; does not identify the aggressor.
    Sell,
    /// Unrecognized provider trade-side label.
    Other,
}

impl From<TradeSide> for FinFeedTradeSide {
    fn from(value: TradeSide) -> Self {
        match value {
            TradeSide::Buy => Self::Buy,
            TradeSide::Sell => Self::Sell,
            TradeSide::Other => Self::Other,
        }
    }
}

pub(super) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyFinFeedExchange>()?;
    m.add_class::<PyFinFeedMarket>()?;
    m.add_class::<PyFinFeedMarketsPage>()?;
    m.add_class::<PyFinFeedTrade>()?;
    m.add_class::<PyFinFeedQuote>()?;
    m.add_class::<PyFinFeedActivityCurrent>()?;
    m.add_class::<PyFinFeedActivityBatch>()?;
    m.add_class::<PyFinFeedBookLevel>()?;
    m.add_class::<PyFinFeedBook>()?;
    m.add_class::<PyFinFeedBookEvent>()?;
    m.add_class::<PyFinFeedOhlcvPeriod>()?;
    m.add_class::<PyFinFeedOhlcv>()?;
    m.add_class::<FinFeedMarketStatus>()?;
    m.add_class::<FinFeedOutcomeType>()?;
    m.add_class::<FinFeedTradeSide>()?;
    Ok(())
}
