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

//! Python bindings for the [`Cache`] component.

use std::{cell::RefCell, rc::Rc};

use bytes::Bytes;
use nautilus_core::python::to_pyvalue_err;
#[cfg(feature = "defi")]
use nautilus_model::defi::{Pool, PoolProfiler};
use nautilus_model::{
    data::{
        Bar, BarType, FundingRateUpdate, InstrumentStatus, QuoteTick, TradeTick,
        prices::{IndexPriceUpdate, MarkPriceUpdate},
    },
    enums::{AggregationSource, OmsType, OrderSide, PositionSide, PriceType},
    identifiers::{
        AccountId, ClientId, ClientOrderId, ComponentId, ExecAlgorithmId, InstrumentId,
        OrderListId, PositionId, StrategyId, Venue, VenueOrderId,
    },
    instruments::SyntheticInstrument,
    orderbook::{OrderBook, own::OwnOrderBook},
    orders::OrderList,
    position::Position,
    python::{
        account::account_any_to_pyobject,
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
        orders::{order_any_to_pyobject, pyobject_to_order_any},
    },
    types::{Currency, Money, Price, Quantity},
};
use pyo3::prelude::*;

use crate::{
    cache::{Cache, CacheConfig},
    enums::SerializationEncoding,
};

/// Wrapper providing shared access to [`Cache`] from Python.
///
/// This wrapper holds an `Rc<RefCell<Cache>>` allowing actors to share
/// the same cache instance. All methods delegate to the underlying cache.
#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.common",
    name = "Cache",
    unsendable,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")]
#[derive(Debug, Clone)]
pub struct PyCache(Rc<RefCell<Cache>>);

impl PyCache {
    /// Creates a `PyCache` from an `Rc<RefCell<Cache>>`.
    #[must_use]
    pub fn from_rc(rc: Rc<RefCell<Cache>>) -> Self {
        Self(rc)
    }

    /// Gets the inner `Rc<RefCell<Cache>>` for use in Rust code.
    #[must_use]
    pub fn cache_rc(&self) -> Rc<RefCell<Cache>> {
        self.0.clone()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyCache {
    #[new]
    #[pyo3(signature = (config=None))]
    fn py_new(config: Option<CacheConfig>) -> Self {
        Self(Rc::new(RefCell::new(Cache::new(config, None))))
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.0.borrow_mut().reset();
    }

    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self) {
        self.0.borrow_mut().dispose();
    }

    #[pyo3(name = "get")]
    fn py_get(&self, key: &str) -> PyResult<Option<Vec<u8>>> {
        match self.0.borrow().get(key).map_err(to_pyvalue_err)? {
            Some(bytes) => Ok(Some(bytes.to_vec())),
            None => Ok(None),
        }
    }

    #[pyo3(name = "add")]
    fn py_add_general(&mut self, key: &str, value: Vec<u8>) -> PyResult<()> {
        self.0
            .borrow_mut()
            .add(key, Bytes::from(value))
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "quote", signature = (instrument_id, index=0))]
    fn py_quote(&self, instrument_id: InstrumentId, index: usize) -> Option<QuoteTick> {
        self.0
            .borrow()
            .quote_at_index(&instrument_id, index)
            .copied()
    }

    #[pyo3(name = "trade", signature = (instrument_id, index=0))]
    fn py_trade(&self, instrument_id: InstrumentId, index: usize) -> Option<TradeTick> {
        self.0
            .borrow()
            .trade_at_index(&instrument_id, index)
            .copied()
    }

    #[pyo3(name = "bar", signature = (bar_type, index=0))]
    fn py_bar(&self, bar_type: BarType, index: usize) -> Option<Bar> {
        self.0.borrow().bar_at_index(&bar_type, index).copied()
    }

    #[pyo3(name = "quotes")]
    fn py_quotes(&self, instrument_id: InstrumentId) -> Option<Vec<QuoteTick>> {
        self.0.borrow().quotes(&instrument_id)
    }

    #[pyo3(name = "trades")]
    fn py_trades(&self, instrument_id: InstrumentId) -> Option<Vec<TradeTick>> {
        self.0.borrow().trades(&instrument_id)
    }

    #[pyo3(name = "bars")]
    fn py_bars(&self, bar_type: BarType) -> Option<Vec<Bar>> {
        self.0.borrow().bars(&bar_type)
    }

    #[pyo3(name = "bar_types", signature = (aggregation_source, instrument_id=None, price_type=None))]
    fn py_bar_types(
        &self,
        aggregation_source: AggregationSource,
        instrument_id: Option<InstrumentId>,
        price_type: Option<PriceType>,
    ) -> Vec<BarType> {
        self.0
            .borrow()
            .bar_types(
                instrument_id.as_ref(),
                price_type.as_ref(),
                aggregation_source,
            )
            .into_iter()
            .copied()
            .collect()
    }

    #[pyo3(name = "mark_price")]
    fn py_mark_price(&self, instrument_id: InstrumentId) -> Option<MarkPriceUpdate> {
        self.0.borrow().mark_price(&instrument_id).copied()
    }

    #[pyo3(name = "mark_prices")]
    fn py_mark_prices(&self, instrument_id: InstrumentId) -> Option<Vec<MarkPriceUpdate>> {
        self.0.borrow().mark_prices(&instrument_id)
    }

    #[pyo3(name = "index_price")]
    fn py_index_price(&self, instrument_id: InstrumentId) -> Option<IndexPriceUpdate> {
        self.0.borrow().index_price(&instrument_id).copied()
    }

    #[pyo3(name = "index_prices")]
    fn py_index_prices(&self, instrument_id: InstrumentId) -> Option<Vec<IndexPriceUpdate>> {
        self.0.borrow().index_prices(&instrument_id)
    }

    #[pyo3(name = "funding_rate")]
    fn py_funding_rate(&self, instrument_id: InstrumentId) -> Option<FundingRateUpdate> {
        self.0.borrow().funding_rate(&instrument_id).copied()
    }

    #[pyo3(name = "instrument_status")]
    fn py_instrument_status(&self, instrument_id: InstrumentId) -> Option<InstrumentStatus> {
        self.0.borrow().instrument_status(&instrument_id).copied()
    }

    #[pyo3(name = "instrument_statuses")]
    fn py_instrument_statuses(&self, instrument_id: InstrumentId) -> Option<Vec<InstrumentStatus>> {
        self.0.borrow().instrument_statuses(&instrument_id)
    }

    #[pyo3(name = "price")]
    fn py_price(&self, instrument_id: InstrumentId, price_type: PriceType) -> Option<Price> {
        self.0.borrow().price(&instrument_id, price_type)
    }

    #[pyo3(name = "order_book")]
    fn py_order_book(&self, instrument_id: InstrumentId) -> Option<OrderBook> {
        self.0.borrow().order_book(&instrument_id).cloned()
    }

    #[pyo3(name = "has_order_book")]
    fn py_has_order_book(&self, instrument_id: InstrumentId) -> bool {
        self.0.borrow().has_order_book(&instrument_id)
    }

    #[pyo3(name = "book_update_count")]
    fn py_book_update_count(&self, instrument_id: InstrumentId) -> usize {
        self.0.borrow().book_update_count(&instrument_id)
    }

    #[pyo3(name = "has_quote_ticks")]
    fn py_has_quote_ticks(&self, instrument_id: InstrumentId) -> bool {
        self.0.borrow().has_quote_ticks(&instrument_id)
    }

    #[pyo3(name = "has_trade_ticks")]
    fn py_has_trade_ticks(&self, instrument_id: InstrumentId) -> bool {
        self.0.borrow().has_trade_ticks(&instrument_id)
    }

    #[pyo3(name = "has_bars")]
    fn py_has_bars(&self, bar_type: BarType) -> bool {
        self.0.borrow().has_bars(&bar_type)
    }

    #[pyo3(name = "quote_count")]
    fn py_quote_count(&self, instrument_id: InstrumentId) -> usize {
        self.0.borrow().quote_count(&instrument_id)
    }

    #[pyo3(name = "trade_count")]
    fn py_trade_count(&self, instrument_id: InstrumentId) -> usize {
        self.0.borrow().trade_count(&instrument_id)
    }

    #[pyo3(name = "bar_count")]
    fn py_bar_count(&self, bar_type: BarType) -> usize {
        self.0.borrow().bar_count(&bar_type)
    }

    #[pyo3(name = "get_xrate")]
    fn py_get_xrate(
        &self,
        venue: Venue,
        from_currency: Currency,
        to_currency: Currency,
        price_type: PriceType,
    ) -> Option<f64> {
        self.0
            .borrow()
            .get_xrate(venue, from_currency, to_currency, price_type)
    }

    #[pyo3(name = "get_mark_xrate")]
    fn py_get_mark_xrate(&self, from_currency: Currency, to_currency: Currency) -> Option<f64> {
        self.0.borrow().get_mark_xrate(from_currency, to_currency)
    }

    #[pyo3(name = "own_order_book")]
    fn py_own_order_book(&self, instrument_id: InstrumentId) -> Option<OwnOrderBook> {
        self.0.borrow().own_order_book(&instrument_id).cloned()
    }

    #[pyo3(name = "instrument")]
    fn py_instrument(
        &self,
        py: Python,
        instrument_id: InstrumentId,
    ) -> PyResult<Option<Py<PyAny>>> {
        let cache = self.0.borrow();
        match cache.instrument(&instrument_id) {
            Some(instrument) => Ok(Some(instrument_any_to_pyobject(py, instrument.clone())?)),
            None => Ok(None),
        }
    }

    #[pyo3(name = "instrument_ids", signature = (venue=None))]
    fn py_instrument_ids(&self, venue: Option<Venue>) -> Vec<InstrumentId> {
        self.0
            .borrow()
            .instrument_ids(venue.as_ref())
            .into_iter()
            .copied()
            .collect()
    }

    #[pyo3(name = "instruments", signature = (venue=None))]
    fn py_instruments(&self, py: Python, venue: Option<Venue>) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        let mut py_instruments = Vec::new();

        match venue {
            Some(venue) => {
                for instrument in cache.instruments(&venue, None) {
                    py_instruments.push(instrument_any_to_pyobject(py, (*instrument).clone())?);
                }
            }
            None => {
                for instrument_id in cache.instrument_ids(None) {
                    if let Some(instrument) = cache.instrument(instrument_id) {
                        py_instruments.push(instrument_any_to_pyobject(py, instrument.clone())?);
                    }
                }
            }
        }
        Ok(py_instruments)
    }

    #[pyo3(name = "synthetic")]
    fn py_synthetic(&self, instrument_id: InstrumentId) -> Option<SyntheticInstrument> {
        self.0.borrow().synthetic(&instrument_id).cloned()
    }

    #[pyo3(name = "synthetic_ids")]
    fn py_synthetic_ids(&self) -> Vec<InstrumentId> {
        self.0
            .borrow()
            .synthetic_ids()
            .into_iter()
            .copied()
            .collect()
    }

    #[pyo3(name = "account")]
    fn py_account(&self, py: Python, account_id: AccountId) -> PyResult<Option<Py<PyAny>>> {
        let cache = self.0.borrow();
        match cache.account(&account_id) {
            Some(account) => Ok(Some(account_any_to_pyobject(py, account.clone())?)),
            None => Ok(None),
        }
    }

    #[pyo3(name = "account_for_venue")]
    fn py_account_for_venue(&self, py: Python, venue: Venue) -> PyResult<Option<Py<PyAny>>> {
        let cache = self.0.borrow();
        match cache.account_for_venue(&venue) {
            Some(account) => Ok(Some(account_any_to_pyobject(py, account.clone())?)),
            None => Ok(None),
        }
    }

    #[pyo3(name = "account_id")]
    fn py_account_id(&self, venue: Venue) -> Option<AccountId> {
        self.0.borrow().account_id(&venue).copied()
    }

    #[pyo3(name = "client_order_ids", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None))]
    fn py_client_order_ids(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<ClientOrderId> {
        self.0
            .borrow()
            .client_order_ids(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
            )
            .into_iter()
            .collect()
    }

    #[pyo3(name = "client_order_ids_open", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None))]
    fn py_client_order_ids_open(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<ClientOrderId> {
        self.0
            .borrow()
            .client_order_ids_open(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
            )
            .into_iter()
            .collect()
    }

    #[pyo3(name = "client_order_ids_closed", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None))]
    fn py_client_order_ids_closed(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<ClientOrderId> {
        self.0
            .borrow()
            .client_order_ids_closed(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
            )
            .into_iter()
            .collect()
    }

    #[pyo3(name = "client_order_ids_emulated", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None))]
    fn py_client_order_ids_emulated(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<ClientOrderId> {
        self.0
            .borrow()
            .client_order_ids_emulated(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
            )
            .into_iter()
            .collect()
    }

    #[pyo3(name = "client_order_ids_inflight", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None))]
    fn py_client_order_ids_inflight(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<ClientOrderId> {
        self.0
            .borrow()
            .client_order_ids_inflight(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
            )
            .into_iter()
            .collect()
    }

    #[pyo3(name = "position_ids", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None))]
    fn py_position_ids(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<PositionId> {
        self.0
            .borrow()
            .position_ids(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
            )
            .into_iter()
            .collect()
    }

    #[pyo3(name = "position_open_ids", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None))]
    fn py_position_open_ids(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<PositionId> {
        self.0
            .borrow()
            .position_open_ids(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
            )
            .into_iter()
            .collect()
    }

    #[pyo3(name = "position_closed_ids", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None))]
    fn py_position_closed_ids(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<PositionId> {
        self.0
            .borrow()
            .position_closed_ids(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
            )
            .into_iter()
            .collect()
    }

    #[pyo3(name = "actor_ids")]
    fn py_actor_ids(&self) -> Vec<ComponentId> {
        self.0.borrow().actor_ids().into_iter().collect()
    }

    #[pyo3(name = "strategy_ids")]
    fn py_strategy_ids(&self) -> Vec<StrategyId> {
        self.0.borrow().strategy_ids().into_iter().collect()
    }

    #[pyo3(name = "exec_algorithm_ids")]
    fn py_exec_algorithm_ids(&self) -> Vec<ExecAlgorithmId> {
        self.0.borrow().exec_algorithm_ids().into_iter().collect()
    }

    #[pyo3(name = "order")]
    fn py_order(&self, py: Python, client_order_id: ClientOrderId) -> PyResult<Option<Py<PyAny>>> {
        let cache = self.0.borrow();
        match cache.order(&client_order_id) {
            Some(order) => Ok(Some(order_any_to_pyobject(py, order.clone())?)),
            None => Ok(None),
        }
    }

    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self, venue_order_id: VenueOrderId) -> Option<ClientOrderId> {
        self.0.borrow().client_order_id(&venue_order_id).copied()
    }

    #[pyo3(name = "venue_order_id")]
    fn py_venue_order_id(&self, client_order_id: ClientOrderId) -> Option<VenueOrderId> {
        self.0.borrow().venue_order_id(&client_order_id).copied()
    }

    #[pyo3(name = "client_id")]
    fn py_client_id(&self, client_order_id: ClientOrderId) -> Option<ClientId> {
        self.0.borrow().client_id(&client_order_id).copied()
    }

    #[pyo3(name = "orders", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .orders(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
                side,
            )
            .into_iter()
            .map(|o| order_any_to_pyobject(py, o.clone()))
            .collect()
    }

    #[pyo3(name = "orders_open", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_open(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .orders_open(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
                side,
            )
            .into_iter()
            .map(|o| order_any_to_pyobject(py, o.clone()))
            .collect()
    }

    #[pyo3(name = "orders_closed", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_closed(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .orders_closed(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
                side,
            )
            .into_iter()
            .map(|o| order_any_to_pyobject(py, o.clone()))
            .collect()
    }

    #[pyo3(name = "orders_emulated", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_emulated(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .orders_emulated(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
                side,
            )
            .into_iter()
            .map(|o| order_any_to_pyobject(py, o.clone()))
            .collect()
    }

    #[pyo3(name = "orders_inflight", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_inflight(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .orders_inflight(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
                side,
            )
            .into_iter()
            .map(|o| order_any_to_pyobject(py, o.clone()))
            .collect()
    }

    #[pyo3(name = "orders_for_position")]
    fn py_orders_for_position(
        &self,
        py: Python,
        position_id: PositionId,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .orders_for_position(&position_id)
            .into_iter()
            .map(|o| order_any_to_pyobject(py, o.clone()))
            .collect()
    }

    #[pyo3(name = "order_exists")]
    fn py_order_exists(&self, client_order_id: ClientOrderId) -> bool {
        self.0.borrow().order_exists(&client_order_id)
    }

    #[pyo3(name = "is_order_open")]
    fn py_is_order_open(&self, client_order_id: ClientOrderId) -> bool {
        self.0.borrow().is_order_open(&client_order_id)
    }

    #[pyo3(name = "is_order_closed")]
    fn py_is_order_closed(&self, client_order_id: ClientOrderId) -> bool {
        self.0.borrow().is_order_closed(&client_order_id)
    }

    #[pyo3(name = "is_order_emulated")]
    fn py_is_order_emulated(&self, client_order_id: ClientOrderId) -> bool {
        self.0.borrow().is_order_emulated(&client_order_id)
    }

    #[pyo3(name = "is_order_inflight")]
    fn py_is_order_inflight(&self, client_order_id: ClientOrderId) -> bool {
        self.0.borrow().is_order_inflight(&client_order_id)
    }

    #[pyo3(name = "is_order_pending_cancel_local")]
    fn py_is_order_pending_cancel_local(&self, client_order_id: ClientOrderId) -> bool {
        self.0
            .borrow()
            .is_order_pending_cancel_local(&client_order_id)
    }

    #[pyo3(name = "orders_open_count", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_open_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.0.borrow().orders_open_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "orders_closed_count", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_closed_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.0.borrow().orders_closed_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "orders_emulated_count", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_emulated_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.0.borrow().orders_emulated_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "orders_inflight_count", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_inflight_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.0.borrow().orders_inflight_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "orders_total_count", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_total_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.0.borrow().orders_total_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "order_list")]
    fn py_order_list(&self, py: Python, order_list_id: OrderListId) -> PyResult<Option<Py<PyAny>>> {
        let cache = self.0.borrow();
        match cache.order_list(&order_list_id) {
            Some(order_list) => Ok(Some(order_list.clone().into_pyobject(py)?.into())),
            None => Ok(None),
        }
    }

    #[pyo3(name = "order_lists", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None))]
    fn py_order_lists(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .order_lists(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
            )
            .into_iter()
            .map(|ol| Ok(ol.clone().into_pyobject(py)?.into()))
            .collect()
    }

    #[pyo3(name = "order_list_exists")]
    fn py_order_list_exists(&self, order_list_id: OrderListId) -> bool {
        self.0.borrow().order_list_exists(&order_list_id)
    }

    #[pyo3(name = "orders_for_exec_algorithm", signature = (exec_algorithm_id, venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_orders_for_exec_algorithm(
        &self,
        py: Python,
        exec_algorithm_id: ExecAlgorithmId,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .orders_for_exec_algorithm(
                &exec_algorithm_id,
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
                side,
            )
            .into_iter()
            .map(|o| order_any_to_pyobject(py, o.clone()))
            .collect()
    }

    #[pyo3(name = "orders_for_exec_spawn")]
    fn py_orders_for_exec_spawn(
        &self,
        py: Python,
        exec_spawn_id: ClientOrderId,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .orders_for_exec_spawn(&exec_spawn_id)
            .into_iter()
            .map(|o| order_any_to_pyobject(py, o.clone()))
            .collect()
    }

    #[pyo3(name = "exec_spawn_total_quantity")]
    fn py_exec_spawn_total_quantity(
        &self,
        exec_spawn_id: ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        self.0
            .borrow()
            .exec_spawn_total_quantity(&exec_spawn_id, active_only)
    }

    #[pyo3(name = "exec_spawn_total_filled_qty")]
    fn py_exec_spawn_total_filled_qty(
        &self,
        exec_spawn_id: ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        self.0
            .borrow()
            .exec_spawn_total_filled_qty(&exec_spawn_id, active_only)
    }

    #[pyo3(name = "exec_spawn_total_leaves_qty")]
    fn py_exec_spawn_total_leaves_qty(
        &self,
        exec_spawn_id: ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        self.0
            .borrow()
            .exec_spawn_total_leaves_qty(&exec_spawn_id, active_only)
    }

    #[pyo3(name = "position")]
    fn py_position(&self, py: Python, position_id: PositionId) -> PyResult<Option<Py<PyAny>>> {
        let cache = self.0.borrow();
        match cache.position(&position_id) {
            Some(position) => Ok(Some(position.clone().into_pyobject(py)?.into())),
            None => Ok(None),
        }
    }

    #[pyo3(name = "position_for_order")]
    fn py_position_for_order(
        &self,
        py: Python,
        client_order_id: ClientOrderId,
    ) -> PyResult<Option<Py<PyAny>>> {
        let cache = self.0.borrow();
        match cache.position_for_order(&client_order_id) {
            Some(position) => Ok(Some(position.clone().into_pyobject(py)?.into())),
            None => Ok(None),
        }
    }

    #[pyo3(name = "position_id")]
    fn py_position_id(&self, client_order_id: ClientOrderId) -> Option<PositionId> {
        self.0.borrow().position_id(&client_order_id).copied()
    }

    #[pyo3(name = "positions", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_positions(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .positions(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
                side,
            )
            .into_iter()
            .map(|p| Ok(p.clone().into_pyobject(py)?.into()))
            .collect()
    }

    #[pyo3(name = "positions_open", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_positions_open(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .positions_open(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
                side,
            )
            .into_iter()
            .map(|p| Ok(p.clone().into_pyobject(py)?.into()))
            .collect()
    }

    #[pyo3(name = "positions_closed", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_positions_closed(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .positions_closed(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
                side,
            )
            .into_iter()
            .map(|p| Ok(p.clone().into_pyobject(py)?.into()))
            .collect()
    }

    #[pyo3(name = "position_exists")]
    fn py_position_exists(&self, position_id: PositionId) -> bool {
        self.0.borrow().position_exists(&position_id)
    }

    #[pyo3(name = "is_position_open")]
    fn py_is_position_open(&self, position_id: PositionId) -> bool {
        self.0.borrow().is_position_open(&position_id)
    }

    #[pyo3(name = "is_position_closed")]
    fn py_is_position_closed(&self, position_id: PositionId) -> bool {
        self.0.borrow().is_position_closed(&position_id)
    }

    #[pyo3(name = "positions_open_count", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_positions_open_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.0.borrow().positions_open_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "positions_closed_count", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_positions_closed_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.0.borrow().positions_closed_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "positions_total_count", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_positions_total_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.0.borrow().positions_total_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "strategy_id_for_order")]
    fn py_strategy_id_for_order(&self, client_order_id: ClientOrderId) -> Option<StrategyId> {
        self.0
            .borrow()
            .strategy_id_for_order(&client_order_id)
            .copied()
    }

    #[pyo3(name = "strategy_id_for_position")]
    fn py_strategy_id_for_position(&self, position_id: PositionId) -> Option<StrategyId> {
        self.0
            .borrow()
            .strategy_id_for_position(&position_id)
            .copied()
    }

    #[pyo3(name = "position_snapshot_bytes")]
    fn py_position_snapshot_bytes(&self, position_id: PositionId) -> Option<Vec<Vec<u8>>> {
        self.0.borrow().position_snapshot_bytes(&position_id)
    }

    #[pyo3(name = "snapshot_position")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_snapshot_position(&self, py: Python, position: Py<PyAny>) -> PyResult<()> {
        let position_obj = position.extract::<Position>(py)?;
        self.0
            .borrow_mut()
            .snapshot_position(&position_obj)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "position_snapshots", signature = (position_id=None, account_id=None))]
    fn py_position_snapshots(
        &self,
        py: Python,
        position_id: Option<PositionId>,
        account_id: Option<AccountId>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let cache = self.0.borrow();
        cache
            .position_snapshots(position_id.as_ref(), account_id.as_ref())
            .into_iter()
            .map(|p| Ok(p.into_pyobject(py)?.into()))
            .collect()
    }
}

#[cfg(feature = "defi")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyCache {
    #[pyo3(name = "pool")]
    fn py_pool(&self, instrument_id: InstrumentId) -> Option<Pool> {
        self.0
            .try_borrow()
            .ok()
            .and_then(|cache| cache.pool(&instrument_id).cloned())
    }

    #[pyo3(name = "pool_profiler")]
    fn py_pool_profiler(&self, instrument_id: InstrumentId) -> Option<PoolProfiler> {
        self.0
            .try_borrow()
            .ok()
            .and_then(|cache| cache.pool_profiler(&instrument_id).cloned())
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CacheConfig {
    /// Configuration for `Cache` instances.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (
        encoding=None,
        timestamps_as_iso8601=None,
        buffer_interval_ms=None,
        bulk_read_batch_size=None,
        use_trader_prefix=None,
        use_instance_id=None,
        flush_on_start=None,
        drop_instruments_on_reset=None,
        tick_capacity=None,
        bar_capacity=None,
        save_market_data=None,
        persist_account_events=None,
    ))]
    fn py_new(
        encoding: Option<SerializationEncoding>,
        timestamps_as_iso8601: Option<bool>,
        buffer_interval_ms: Option<usize>,
        bulk_read_batch_size: Option<usize>,
        use_trader_prefix: Option<bool>,
        use_instance_id: Option<bool>,
        flush_on_start: Option<bool>,
        drop_instruments_on_reset: Option<bool>,
        tick_capacity: Option<usize>,
        bar_capacity: Option<usize>,
        save_market_data: Option<bool>,
        persist_account_events: Option<bool>,
    ) -> Self {
        Self::new(
            None, // database is None since we can't expose it to Python yet
            encoding.unwrap_or(SerializationEncoding::MsgPack),
            timestamps_as_iso8601.unwrap_or(false),
            buffer_interval_ms,
            bulk_read_batch_size,
            use_trader_prefix.unwrap_or(true),
            use_instance_id.unwrap_or(false),
            flush_on_start.unwrap_or(false),
            drop_instruments_on_reset.unwrap_or(true),
            tick_capacity.unwrap_or(10_000),
            bar_capacity.unwrap_or(10_000),
            persist_account_events.unwrap_or(true),
            save_market_data.unwrap_or(false),
        )
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn encoding(&self) -> SerializationEncoding {
        self.encoding
    }

    #[getter]
    fn timestamps_as_iso8601(&self) -> bool {
        self.timestamps_as_iso8601
    }

    #[getter]
    fn buffer_interval_ms(&self) -> Option<usize> {
        self.buffer_interval_ms
    }

    #[getter]
    fn bulk_read_batch_size(&self) -> Option<usize> {
        self.bulk_read_batch_size
    }

    #[getter]
    fn use_trader_prefix(&self) -> bool {
        self.use_trader_prefix
    }

    #[getter]
    fn use_instance_id(&self) -> bool {
        self.use_instance_id
    }

    #[getter]
    fn flush_on_start(&self) -> bool {
        self.flush_on_start
    }

    #[getter]
    fn drop_instruments_on_reset(&self) -> bool {
        self.drop_instruments_on_reset
    }

    #[getter]
    fn tick_capacity(&self) -> usize {
        self.tick_capacity
    }

    #[getter]
    fn bar_capacity(&self) -> usize {
        self.bar_capacity
    }

    #[getter]
    fn persist_account_events(&self) -> bool {
        self.persist_account_events
    }

    #[getter]
    fn save_market_data(&self) -> bool {
        self.save_market_data
    }
}

#[pymethods]
impl Cache {
    /// A common in-memory `Cache` for market and execution related data.
    #[new]
    fn py_new(config: Option<CacheConfig>) -> Self {
        Self::new(config, None)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    /// Resets the cache.
    ///
    /// All stateful fields are reset to their initial value. Instruments,
    /// currencies and synthetics are retained when `drop_instruments_on_reset`
    /// is `false` so that repeated backtest runs can reuse the same dataset.
    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    /// Dispose of the cache which will close any underlying database adapter.
    ///
    /// If closing the database connection fails, an error is logged.
    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self) {
        self.dispose();
    }

    /// Adds the `currency` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the currency to the backing database fails.
    #[pyo3(name = "add_currency")]
    fn py_add_currency(&mut self, currency: Currency) -> PyResult<()> {
        self.add_currency(currency).map_err(to_pyvalue_err)
    }

    /// Adds the `instrument` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the instrument to the backing database fails.
    #[pyo3(name = "add_instrument")]
    fn py_add_instrument(&mut self, py: Python, instrument: Py<PyAny>) -> PyResult<()> {
        let instrument_any = pyobject_to_instrument_any(py, instrument)?;
        self.add_instrument(instrument_any).map_err(to_pyvalue_err)
    }

    /// Returns a reference to the instrument for the `instrument_id` (if found).
    #[pyo3(name = "instrument")]
    fn py_instrument(
        &self,
        py: Python,
        instrument_id: InstrumentId,
    ) -> PyResult<Option<Py<PyAny>>> {
        match self.instrument(&instrument_id) {
            Some(instrument) => Ok(Some(instrument_any_to_pyobject(py, instrument.clone())?)),
            None => Ok(None),
        }
    }

    /// Returns references to all instrument IDs for the `venue`.
    #[pyo3(name = "instrument_ids")]
    fn py_instrument_ids(&self, venue: Option<Venue>) -> Vec<InstrumentId> {
        self.instrument_ids(venue.as_ref())
            .into_iter()
            .copied()
            .collect()
    }

    /// Returns references to all instruments for the `venue`.
    #[pyo3(name = "instruments")]
    fn py_instruments(&self, py: Python, venue: Option<Venue>) -> PyResult<Vec<Py<PyAny>>> {
        let mut py_instruments = Vec::new();

        match venue {
            Some(venue) => {
                let instruments = self.instruments(&venue, None);
                for instrument in instruments {
                    py_instruments.push(instrument_any_to_pyobject(py, (*instrument).clone())?);
                }
            }
            None => {
                let instrument_ids = self.instrument_ids(None);
                for instrument_id in instrument_ids {
                    if let Some(instrument) = self.instrument(instrument_id) {
                        py_instruments.push(instrument_any_to_pyobject(py, instrument.clone())?);
                    }
                }
            }
        }

        Ok(py_instruments)
    }

    /// Adds the `order` to the cache indexed with any given identifiers.
    ///
    /// # Parameters
    ///
    /// `override_existing`: If the added order should 'override' any existing order and replace
    /// it in the cache. This is currently used for emulated orders which are
    /// being released and transformed into another type.
    #[pyo3(name = "add_order")]
    fn py_add_order(
        &mut self,
        py: Python,
        order: Py<PyAny>,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        replace_existing: Option<bool>,
    ) -> PyResult<()> {
        let order_any = pyobject_to_order_any(py, order)?;
        self.add_order(
            order_any,
            position_id,
            client_id,
            replace_existing.unwrap_or(false),
        )
        .map_err(to_pyvalue_err)
    }

    /// Gets a reference to the order with the `client_order_id` (if found).
    #[pyo3(name = "order")]
    fn py_order(&self, py: Python, client_order_id: ClientOrderId) -> PyResult<Option<Py<PyAny>>> {
        match self.order(&client_order_id) {
            Some(order) => Ok(Some(order_any_to_pyobject(py, order.clone())?)),
            None => Ok(None),
        }
    }

    /// Returns whether an order with the `client_order_id` exists.
    #[pyo3(name = "order_exists")]
    fn py_order_exists(&self, client_order_id: ClientOrderId) -> bool {
        self.order_exists(&client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is open.
    #[pyo3(name = "is_order_open")]
    fn py_is_order_open(&self, client_order_id: ClientOrderId) -> bool {
        self.is_order_open(&client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is closed.
    #[pyo3(name = "is_order_closed")]
    fn py_is_order_closed(&self, client_order_id: ClientOrderId) -> bool {
        self.is_order_closed(&client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is locally active.
    ///
    /// Locally active orders are in the `INITIALIZED`, `EMULATED`, or `RELEASED` state
    /// (a superset of emulated orders).
    #[pyo3(name = "is_order_active_local")]
    fn py_is_order_active_local(&self, client_order_id: ClientOrderId) -> bool {
        self.is_order_active_local(&client_order_id)
    }

    /// Returns references to all locally active orders matching the optional filter parameters.
    ///
    /// Locally active orders are in the `INITIALIZED`, `EMULATED`, or `RELEASED` state
    /// (a superset of emulated orders).
    #[pyo3(name = "orders_active_local")]
    fn py_orders_active_local(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.orders_active_local(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
        .into_iter()
        .map(|order| order_any_to_pyobject(py, order.clone()))
        .collect()
    }

    /// Returns the count of all locally active orders.
    ///
    /// Locally active orders are in the `INITIALIZED`, `EMULATED`, or `RELEASED` state
    /// (a superset of emulated orders).
    #[pyo3(name = "orders_active_local_count")]
    fn py_orders_active_local_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_active_local_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    /// Returns the count of all open orders.
    #[pyo3(name = "orders_open_count")]
    fn py_orders_open_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_open_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    /// Returns the count of all closed orders.
    #[pyo3(name = "orders_closed_count")]
    fn py_orders_closed_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_closed_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    /// Returns the count of all orders.
    #[pyo3(name = "orders_total_count")]
    fn py_orders_total_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_total_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    /// Adds the `position` to the cache.
    #[pyo3(name = "add_position")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_position(
        &mut self,
        py: Python,
        position: Py<PyAny>,
        oms_type: OmsType,
    ) -> PyResult<()> {
        let position_obj = position.extract::<Position>(py)?;
        self.add_position(&position_obj, oms_type)
            .map_err(to_pyvalue_err)
    }

    /// Creates a snapshot of the `position` by cloning it, assigning a new ID,
    /// serializing it, and storing it in the position snapshots.
    ///
    /// # Errors
    ///
    /// Returns an error if serializing or storing the position snapshot fails.
    #[pyo3(name = "snapshot_position")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_snapshot_position(&mut self, py: Python, position: Py<PyAny>) -> PyResult<()> {
        let position_obj = position.extract::<Position>(py)?;
        self.snapshot_position(&position_obj)
            .map_err(to_pyvalue_err)
    }

    /// Returns a reference to the position with the `position_id` (if found).
    #[pyo3(name = "position")]
    fn py_position(&self, py: Python, position_id: PositionId) -> PyResult<Option<Py<PyAny>>> {
        match self.position(&position_id) {
            Some(position) => Ok(Some(position.clone().into_pyobject(py)?.into())),
            None => Ok(None),
        }
    }

    /// Returns whether a position with the `position_id` exists.
    #[pyo3(name = "position_exists")]
    fn py_position_exists(&self, position_id: PositionId) -> bool {
        self.position_exists(&position_id)
    }

    /// Returns whether a position with the `position_id` is open.
    #[pyo3(name = "is_position_open")]
    fn py_is_position_open(&self, position_id: PositionId) -> bool {
        self.is_position_open(&position_id)
    }

    /// Returns whether a position with the `position_id` is closed.
    #[pyo3(name = "is_position_closed")]
    fn py_is_position_closed(&self, position_id: PositionId) -> bool {
        self.is_position_closed(&position_id)
    }

    /// Returns the count of all open positions.
    #[pyo3(name = "positions_open_count")]
    fn py_positions_open_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.positions_open_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    /// Returns the count of all closed positions.
    #[pyo3(name = "positions_closed_count")]
    fn py_positions_closed_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.positions_closed_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    /// Returns the count of all positions.
    #[pyo3(name = "positions_total_count")]
    fn py_positions_total_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.positions_total_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    /// Adds the `quote` tick to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the quote tick to the backing database fails.
    #[pyo3(name = "add_quote")]
    fn py_add_quote(&mut self, quote: QuoteTick) -> PyResult<()> {
        self.add_quote(quote).map_err(to_pyvalue_err)
    }

    /// Adds the `trade` tick to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the trade tick to the backing database fails.
    #[pyo3(name = "add_trade")]
    fn py_add_trade(&mut self, trade: TradeTick) -> PyResult<()> {
        self.add_trade(trade).map_err(to_pyvalue_err)
    }

    /// Adds the `bar` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the bar to the backing database fails.
    #[pyo3(name = "add_bar")]
    fn py_add_bar(&mut self, bar: Bar) -> PyResult<()> {
        self.add_bar(bar).map_err(to_pyvalue_err)
    }

    /// Gets a reference to the latest quote for the `instrument_id`.
    #[pyo3(name = "quote")]
    fn py_quote(&self, instrument_id: InstrumentId) -> Option<QuoteTick> {
        self.quote(&instrument_id).copied()
    }

    /// Gets a reference to the latest trade for the `instrument_id`.
    #[pyo3(name = "trade")]
    fn py_trade(&self, instrument_id: InstrumentId) -> Option<TradeTick> {
        self.trade(&instrument_id).copied()
    }

    /// Gets a reference to the latest bar for the `bar_type`.
    #[pyo3(name = "bar")]
    fn py_bar(&self, bar_type: BarType) -> Option<Bar> {
        self.bar(&bar_type).copied()
    }

    /// Gets all quotes for the `instrument_id`.
    #[pyo3(name = "quotes")]
    fn py_quotes(&self, instrument_id: InstrumentId) -> Option<Vec<QuoteTick>> {
        self.quotes(&instrument_id)
    }

    /// Gets all trades for the `instrument_id`.
    #[pyo3(name = "trades")]
    fn py_trades(&self, instrument_id: InstrumentId) -> Option<Vec<TradeTick>> {
        self.trades(&instrument_id)
    }

    /// Gets all bars for the `bar_type`.
    #[pyo3(name = "bars")]
    fn py_bars(&self, bar_type: BarType) -> Option<Vec<Bar>> {
        self.bars(&bar_type)
    }

    /// Returns whether the cache contains quotes for the `instrument_id`.
    #[pyo3(name = "has_quote_ticks")]
    fn py_has_quote_ticks(&self, instrument_id: InstrumentId) -> bool {
        self.has_quote_ticks(&instrument_id)
    }

    /// Returns whether the cache contains trades for the `instrument_id`.
    #[pyo3(name = "has_trade_ticks")]
    fn py_has_trade_ticks(&self, instrument_id: InstrumentId) -> bool {
        self.has_trade_ticks(&instrument_id)
    }

    /// Returns whether the cache contains bars for the `bar_type`.
    #[pyo3(name = "has_bars")]
    fn py_has_bars(&self, bar_type: BarType) -> bool {
        self.has_bars(&bar_type)
    }

    /// Gets the quote tick count for the `instrument_id`.
    #[pyo3(name = "quote_count")]
    fn py_quote_count(&self, instrument_id: InstrumentId) -> usize {
        self.quote_count(&instrument_id)
    }

    /// Gets the trade tick count for the `instrument_id`.
    #[pyo3(name = "trade_count")]
    fn py_trade_count(&self, instrument_id: InstrumentId) -> usize {
        self.trade_count(&instrument_id)
    }

    /// Gets the bar count for the `instrument_id`.
    #[pyo3(name = "bar_count")]
    fn py_bar_count(&self, bar_type: BarType) -> usize {
        self.bar_count(&bar_type)
    }

    /// Gets a reference to the latest mark price update for the `instrument_id`.
    #[pyo3(name = "mark_price")]
    fn py_mark_price(&self, instrument_id: InstrumentId) -> Option<MarkPriceUpdate> {
        self.mark_price(&instrument_id).copied()
    }

    /// Gets all mark price updates for the `instrument_id`.
    #[pyo3(name = "mark_prices")]
    fn py_mark_prices(&self, instrument_id: InstrumentId) -> Option<Vec<MarkPriceUpdate>> {
        self.mark_prices(&instrument_id)
    }

    /// Gets a reference to the latest index price update for the `instrument_id`.
    #[pyo3(name = "index_price")]
    fn py_index_price(&self, instrument_id: InstrumentId) -> Option<IndexPriceUpdate> {
        self.index_price(&instrument_id).copied()
    }

    /// Gets all index price updates for the `instrument_id`.
    #[pyo3(name = "index_prices")]
    fn py_index_prices(&self, instrument_id: InstrumentId) -> Option<Vec<IndexPriceUpdate>> {
        self.index_prices(&instrument_id)
    }

    /// Gets a reference to the latest funding rate update for the `instrument_id`.
    #[pyo3(name = "funding_rate")]
    fn py_funding_rate(&self, instrument_id: InstrumentId) -> Option<FundingRateUpdate> {
        self.funding_rate(&instrument_id).copied()
    }

    /// Gets a reference to the latest instrument status update for the `instrument_id`.
    #[pyo3(name = "instrument_status")]
    fn py_instrument_status(&self, instrument_id: InstrumentId) -> Option<InstrumentStatus> {
        self.instrument_status(&instrument_id).copied()
    }

    /// Gets all instrument status updates for the `instrument_id`.
    #[pyo3(name = "instrument_statuses")]
    fn py_instrument_statuses(&self, instrument_id: InstrumentId) -> Option<Vec<InstrumentStatus>> {
        self.instrument_statuses(&instrument_id)
    }

    /// Gets a reference to the order book for the `instrument_id`.
    #[pyo3(name = "order_book")]
    fn py_order_book(&self, instrument_id: InstrumentId) -> Option<OrderBook> {
        self.order_book(&instrument_id).cloned()
    }

    /// Returns whether the cache contains an order book for the `instrument_id`.
    #[pyo3(name = "has_order_book")]
    fn py_has_order_book(&self, instrument_id: InstrumentId) -> bool {
        self.has_order_book(&instrument_id)
    }

    /// Gets the order book update count for the `instrument_id`.
    #[pyo3(name = "book_update_count")]
    fn py_book_update_count(&self, instrument_id: InstrumentId) -> usize {
        self.book_update_count(&instrument_id)
    }

    /// Returns a reference to the synthetic instrument for the `instrument_id` (if found).
    #[pyo3(name = "synthetic")]
    fn py_synthetic(&self, instrument_id: InstrumentId) -> Option<SyntheticInstrument> {
        self.synthetic(&instrument_id).cloned()
    }

    /// Returns references to instrument IDs for all synthetic instruments contained in the cache.
    #[pyo3(name = "synthetic_ids")]
    fn py_synthetic_ids(&self) -> Vec<InstrumentId> {
        self.synthetic_ids().into_iter().copied().collect()
    }

    /// Returns the `ClientOrderId`s of all orders.
    #[pyo3(name = "client_order_ids")]
    fn py_client_order_ids(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<ClientOrderId> {
        self.client_order_ids(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
        )
        .into_iter()
        .collect()
    }

    /// Returns the `ClientOrderId`s of all open orders.
    #[pyo3(name = "client_order_ids_open")]
    fn py_client_order_ids_open(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<ClientOrderId> {
        self.client_order_ids_open(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
        )
        .into_iter()
        .collect()
    }

    /// Returns the `ClientOrderId`s of all closed orders.
    #[pyo3(name = "client_order_ids_closed")]
    fn py_client_order_ids_closed(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<ClientOrderId> {
        self.client_order_ids_closed(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
        )
        .into_iter()
        .collect()
    }

    /// Returns the `ClientOrderId`s of all emulated orders.
    #[pyo3(name = "client_order_ids_emulated")]
    fn py_client_order_ids_emulated(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<ClientOrderId> {
        self.client_order_ids_emulated(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
        )
        .into_iter()
        .collect()
    }

    /// Returns the `ClientOrderId`s of all in-flight orders.
    #[pyo3(name = "client_order_ids_inflight")]
    fn py_client_order_ids_inflight(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<ClientOrderId> {
        self.client_order_ids_inflight(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
        )
        .into_iter()
        .collect()
    }

    /// Returns `PositionId`s of all positions.
    #[pyo3(name = "position_ids")]
    fn py_position_ids(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<PositionId> {
        self.position_ids(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
        )
        .into_iter()
        .collect()
    }

    /// Returns the `PositionId`s of all open positions.
    #[pyo3(name = "position_open_ids")]
    fn py_position_open_ids(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<PositionId> {
        self.position_open_ids(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
        )
        .into_iter()
        .collect()
    }

    /// Returns the `PositionId`s of all closed positions.
    #[pyo3(name = "position_closed_ids")]
    fn py_position_closed_ids(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<PositionId> {
        self.position_closed_ids(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
        )
        .into_iter()
        .collect()
    }

    /// Returns the `ComponentId`s of all actors.
    #[pyo3(name = "actor_ids")]
    fn py_actor_ids(&self) -> Vec<ComponentId> {
        self.actor_ids().into_iter().collect()
    }

    /// Returns the `StrategyId`s of all strategies.
    #[pyo3(name = "strategy_ids")]
    fn py_strategy_ids(&self) -> Vec<StrategyId> {
        self.strategy_ids().into_iter().collect()
    }

    /// Returns the `ExecAlgorithmId`s of all execution algorithms.
    #[pyo3(name = "exec_algorithm_ids")]
    fn py_exec_algorithm_ids(&self) -> Vec<ExecAlgorithmId> {
        self.exec_algorithm_ids().into_iter().collect()
    }

    /// Gets a reference to the client order ID for the `venue_order_id` (if found).
    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self, venue_order_id: VenueOrderId) -> Option<ClientOrderId> {
        self.client_order_id(&venue_order_id).copied()
    }

    /// Gets a reference to the venue order ID for the `client_order_id` (if found).
    #[pyo3(name = "venue_order_id")]
    fn py_venue_order_id(&self, client_order_id: ClientOrderId) -> Option<VenueOrderId> {
        self.venue_order_id(&client_order_id).copied()
    }

    /// Gets a reference to the client ID indexed for then `client_order_id` (if found).
    #[pyo3(name = "client_id")]
    fn py_client_id(&self, client_order_id: ClientOrderId) -> Option<ClientId> {
        self.client_id(&client_order_id).copied()
    }

    /// Returns references to all orders matching the optional filter parameters.
    #[pyo3(name = "orders")]
    fn py_orders(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.orders(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
        .into_iter()
        .map(|o| order_any_to_pyobject(py, o.clone()))
        .collect()
    }

    /// Returns references to all open orders matching the optional filter parameters.
    #[pyo3(name = "orders_open")]
    fn py_orders_open(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.orders_open(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
        .into_iter()
        .map(|o| order_any_to_pyobject(py, o.clone()))
        .collect()
    }

    /// Returns references to all closed orders matching the optional filter parameters.
    #[pyo3(name = "orders_closed")]
    fn py_orders_closed(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.orders_closed(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
        .into_iter()
        .map(|o| order_any_to_pyobject(py, o.clone()))
        .collect()
    }

    /// Returns references to all emulated orders matching the optional filter parameters.
    #[pyo3(name = "orders_emulated")]
    fn py_orders_emulated(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.orders_emulated(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
        .into_iter()
        .map(|o| order_any_to_pyobject(py, o.clone()))
        .collect()
    }

    /// Returns references to all in-flight orders matching the optional filter parameters.
    #[pyo3(name = "orders_inflight")]
    fn py_orders_inflight(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.orders_inflight(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
        .into_iter()
        .map(|o| order_any_to_pyobject(py, o.clone()))
        .collect()
    }

    /// Returns references to all orders for the `position_id`.
    #[pyo3(name = "orders_for_position")]
    fn py_orders_for_position(
        &self,
        py: Python,
        position_id: PositionId,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.orders_for_position(&position_id)
            .into_iter()
            .map(|o| order_any_to_pyobject(py, o.clone()))
            .collect()
    }

    /// Returns whether an order with the `client_order_id` is emulated.
    #[pyo3(name = "is_order_emulated")]
    fn py_is_order_emulated(&self, client_order_id: ClientOrderId) -> bool {
        self.is_order_emulated(&client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is in-flight.
    #[pyo3(name = "is_order_inflight")]
    fn py_is_order_inflight(&self, client_order_id: ClientOrderId) -> bool {
        self.is_order_inflight(&client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is `PENDING_CANCEL` locally.
    #[pyo3(name = "is_order_pending_cancel_local")]
    fn py_is_order_pending_cancel_local(&self, client_order_id: ClientOrderId) -> bool {
        self.is_order_pending_cancel_local(&client_order_id)
    }

    /// Returns the count of all emulated orders.
    #[pyo3(name = "orders_emulated_count")]
    fn py_orders_emulated_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_emulated_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    /// Returns the count of all in-flight orders.
    #[pyo3(name = "orders_inflight_count")]
    fn py_orders_inflight_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_inflight_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
    }

    /// Returns the order list for the `order_list_id`.
    #[pyo3(name = "order_list")]
    fn py_order_list(&self, order_list_id: OrderListId) -> Option<OrderList> {
        self.order_list(&order_list_id).cloned()
    }

    /// Returns all order lists matching the optional filter parameters.
    #[pyo3(name = "order_lists")]
    fn py_order_lists(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> Vec<OrderList> {
        self.order_lists(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
        )
        .into_iter()
        .cloned()
        .collect()
    }

    /// Returns whether an order list with the `order_list_id` exists.
    #[pyo3(name = "order_list_exists")]
    fn py_order_list_exists(&self, order_list_id: OrderListId) -> bool {
        self.order_list_exists(&order_list_id)
    }

    /// Returns references to all orders associated with the `exec_algorithm_id` matching the
    /// optional filter parameters.
    #[pyo3(name = "orders_for_exec_algorithm")]
    #[expect(clippy::too_many_arguments)]
    fn py_orders_for_exec_algorithm(
        &self,
        py: Python,
        exec_algorithm_id: ExecAlgorithmId,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.orders_for_exec_algorithm(
            &exec_algorithm_id,
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
        .into_iter()
        .map(|o| order_any_to_pyobject(py, o.clone()))
        .collect()
    }

    /// Returns references to all orders with the `exec_spawn_id`.
    #[pyo3(name = "orders_for_exec_spawn")]
    fn py_orders_for_exec_spawn(
        &self,
        py: Python,
        exec_spawn_id: ClientOrderId,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.orders_for_exec_spawn(&exec_spawn_id)
            .into_iter()
            .map(|o| order_any_to_pyobject(py, o.clone()))
            .collect()
    }

    /// Returns the total order quantity for the `exec_spawn_id`.
    #[pyo3(name = "exec_spawn_total_quantity")]
    fn py_exec_spawn_total_quantity(
        &self,
        exec_spawn_id: ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        self.exec_spawn_total_quantity(&exec_spawn_id, active_only)
    }

    /// Returns the total filled quantity for all orders with the `exec_spawn_id`.
    #[pyo3(name = "exec_spawn_total_filled_qty")]
    fn py_exec_spawn_total_filled_qty(
        &self,
        exec_spawn_id: ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        self.exec_spawn_total_filled_qty(&exec_spawn_id, active_only)
    }

    /// Returns the total leaves quantity for all orders with the `exec_spawn_id`.
    #[pyo3(name = "exec_spawn_total_leaves_qty")]
    fn py_exec_spawn_total_leaves_qty(
        &self,
        exec_spawn_id: ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        self.exec_spawn_total_leaves_qty(&exec_spawn_id, active_only)
    }

    /// Returns a reference to the position for the `client_order_id` (if found).
    #[pyo3(name = "position_for_order")]
    fn py_position_for_order(
        &self,
        py: Python,
        client_order_id: ClientOrderId,
    ) -> PyResult<Option<Py<PyAny>>> {
        match self.position_for_order(&client_order_id) {
            Some(position) => Ok(Some(position.clone().into_pyobject(py)?.into())),
            None => Ok(None),
        }
    }

    /// Returns a reference to the position ID for the `client_order_id` (if found).
    #[pyo3(name = "position_id")]
    fn py_position_id(&self, client_order_id: ClientOrderId) -> Option<PositionId> {
        self.position_id(&client_order_id).copied()
    }

    /// Returns a reference to all positions matching the optional filter parameters.
    #[pyo3(name = "positions")]
    fn py_positions(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.positions(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
        .into_iter()
        .map(|p| Ok(p.clone().into_pyobject(py)?.into()))
        .collect()
    }

    /// Returns a reference to all open positions matching the optional filter parameters.
    #[pyo3(name = "positions_open")]
    fn py_positions_open(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.positions_open(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
        .into_iter()
        .map(|p| Ok(p.clone().into_pyobject(py)?.into()))
        .collect()
    }

    /// Returns a reference to all closed positions matching the optional filter parameters.
    #[pyo3(name = "positions_closed")]
    fn py_positions_closed(
        &self,
        py: Python,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.positions_closed(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            account_id.as_ref(),
            side,
        )
        .into_iter()
        .map(|p| Ok(p.clone().into_pyobject(py)?.into()))
        .collect()
    }

    /// Gets a reference to the strategy ID for the `client_order_id` (if found).
    #[pyo3(name = "strategy_id_for_order")]
    fn py_strategy_id_for_order(&self, client_order_id: ClientOrderId) -> Option<StrategyId> {
        self.strategy_id_for_order(&client_order_id).copied()
    }

    /// Gets a reference to the strategy ID for the `position_id` (if found).
    #[pyo3(name = "strategy_id_for_position")]
    fn py_strategy_id_for_position(&self, position_id: PositionId) -> Option<StrategyId> {
        self.strategy_id_for_position(&position_id).copied()
    }

    /// Gets the serialized position snapshot frames for the `position_id`.
    ///
    /// Each element in the returned vector is one JSON-encoded `Position` snapshot,
    /// in the order they were taken.
    #[pyo3(name = "position_snapshot_bytes")]
    fn py_position_snapshot_bytes(&self, position_id: PositionId) -> Option<Vec<Vec<u8>>> {
        self.position_snapshot_bytes(&position_id)
    }

    /// Returns all position snapshots with the given optional filters.
    ///
    /// When `position_id` is `Some`, only snapshots for that position are returned.
    /// When `account_id` is `Some`, snapshots are filtered to that account.
    /// Frames that fail to deserialize are skipped with a warning.
    #[pyo3(name = "position_snapshots", signature = (position_id=None, account_id=None))]
    fn py_position_snapshots(
        &self,
        py: Python,
        position_id: Option<PositionId>,
        account_id: Option<AccountId>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.position_snapshots(position_id.as_ref(), account_id.as_ref())
            .into_iter()
            .map(|p| Ok(p.into_pyobject(py)?.into()))
            .collect()
    }

    /// Returns a reference to the account for the `account_id` (if found).
    #[pyo3(name = "account")]
    fn py_account(&self, py: Python, account_id: AccountId) -> PyResult<Option<Py<PyAny>>> {
        match self.account(&account_id) {
            Some(account) => Ok(Some(account_any_to_pyobject(py, account.clone())?)),
            None => Ok(None),
        }
    }

    /// Returns a reference to the account for the `venue` (if found).
    #[pyo3(name = "account_for_venue")]
    fn py_account_for_venue(&self, py: Python, venue: Venue) -> PyResult<Option<Py<PyAny>>> {
        match self.account_for_venue(&venue) {
            Some(account) => Ok(Some(account_any_to_pyobject(py, account.clone())?)),
            None => Ok(None),
        }
    }

    /// Returns a reference to the account ID for the `venue` (if found).
    #[pyo3(name = "account_id")]
    fn py_account_id(&self, venue: Venue) -> Option<AccountId> {
        self.account_id(&venue).copied()
    }

    /// Gets a reference to the general value for the `key` (if found).
    ///
    /// # Errors
    ///
    /// Returns an error if the `key` is invalid.
    #[pyo3(name = "get")]
    fn py_get(&self, key: &str) -> PyResult<Option<Vec<u8>>> {
        match self.get(key).map_err(to_pyvalue_err)? {
            Some(bytes) => Ok(Some(bytes.to_vec())),
            None => Ok(None),
        }
    }

    /// Adds a general `value` to the cache for the given `key`.
    #[pyo3(name = "add")]
    fn py_add_general(&mut self, key: &str, value: Vec<u8>) -> PyResult<()> {
        self.add(key, Bytes::from(value)).map_err(to_pyvalue_err)
    }

    /// Returns the price for the `instrument_id` and `price_type` (if found).
    #[pyo3(name = "price")]
    fn py_price(&self, instrument_id: InstrumentId, price_type: PriceType) -> Option<Price> {
        self.price(&instrument_id, price_type)
    }

    /// Returns the exchange rate for the given parameters.
    #[pyo3(name = "get_xrate")]
    fn py_get_xrate(
        &self,
        venue: Venue,
        from_currency: Currency,
        to_currency: Currency,
        price_type: PriceType,
    ) -> Option<f64> {
        self.get_xrate(venue, from_currency, to_currency, price_type)
    }

    /// Returns the mark exchange rate for the given currency pair, or `None` if not set.
    #[pyo3(name = "get_mark_xrate")]
    fn py_get_mark_xrate(&self, from_currency: Currency, to_currency: Currency) -> Option<f64> {
        self.get_mark_xrate(from_currency, to_currency)
    }

    /// Sets the mark exchange rate for the given currency pair and automatically sets the inverse rate.
    #[pyo3(name = "set_mark_xrate")]
    fn py_set_mark_xrate(&mut self, from_currency: Currency, to_currency: Currency, xrate: f64) {
        self.set_mark_xrate(from_currency, to_currency, xrate);
    }

    /// Clears the mark exchange rate for the given currency pair.
    #[pyo3(name = "clear_mark_xrate")]
    fn py_clear_mark_xrate(&mut self, from_currency: Currency, to_currency: Currency) {
        self.clear_mark_xrate(from_currency, to_currency);
    }

    /// Clears all mark exchange rates.
    #[pyo3(name = "clear_mark_xrates")]
    fn py_clear_mark_xrates(&mut self) {
        self.clear_mark_xrates();
    }

    /// Calculates the unrealized PnL for the given position.
    #[pyo3(name = "calculate_unrealized_pnl")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_calculate_unrealized_pnl(
        &self,
        py: Python,
        position: Py<PyAny>,
    ) -> PyResult<Option<Money>> {
        let position = position.extract::<Position>(py)?;
        Ok(self.calculate_unrealized_pnl(&position))
    }

    /// Gets a reference to the own order book for the `instrument_id`.
    #[pyo3(name = "own_order_book")]
    fn py_own_order_book(&self, instrument_id: InstrumentId) -> Option<OwnOrderBook> {
        self.own_order_book(&instrument_id).cloned()
    }

    /// Updates the own order book with an order.
    ///
    /// This method adds, updates, or removes an order from the own order book
    /// based on the order's current state.
    ///
    /// Orders without prices (MARKET, etc.) are skipped as they cannot be
    /// represented in own books.
    #[pyo3(name = "update_own_order_book")]
    fn py_update_own_order_book(&mut self, py: Python, order: Py<PyAny>) -> PyResult<()> {
        let order_any = pyobject_to_order_any(py, order)?;
        self.update_own_order_book(&order_any);
        Ok(())
    }

    /// Force removal of an order from own order books and clean up all indexes.
    ///
    /// This method is used when order event application fails and we need to ensure
    /// terminal orders are properly cleaned up from own books and all relevant indexes.
    /// Replicates the index cleanup that update_order performs for closed orders.
    #[pyo3(name = "force_remove_from_own_order_book")]
    fn py_force_remove_from_own_order_book(&mut self, client_order_id: ClientOrderId) {
        self.force_remove_from_own_order_book(&client_order_id);
    }

    /// Audit all own order books against open and inflight order indexes.
    ///
    /// Ensures closed orders are removed from own order books. This includes both
    /// orders tracked in `orders_open` (ACCEPTED, TRIGGERED, PENDING_*, PARTIALLY_FILLED)
    /// and `orders_inflight` (INITIALIZED, SUBMITTED) to prevent false positives
    /// during venue latency windows.
    #[pyo3(name = "audit_own_order_books")]
    fn py_audit_own_order_books(&mut self) {
        self.audit_own_order_books();
    }
}

#[cfg(feature = "defi")]
#[pymethods]
impl Cache {
    /// Adds a `Pool` to the cache.
    ///
    /// # Errors
    ///
    /// This function currently does not return errors but follows the same pattern as other add methods for consistency.
    #[pyo3(name = "add_pool")]
    fn py_add_pool(&mut self, pool: Pool) -> PyResult<()> {
        self.add_pool(pool).map_err(to_pyvalue_err)
    }

    /// Gets a reference to the pool for the `instrument_id`.
    #[pyo3(name = "pool")]
    fn py_pool(&self, instrument_id: InstrumentId) -> Option<Pool> {
        self.pool(&instrument_id).cloned()
    }

    /// Returns the instrument IDs of all pools in the cache, optionally filtered by `venue`.
    #[pyo3(name = "pool_ids")]
    fn py_pool_ids(&self, venue: Option<Venue>) -> Vec<InstrumentId> {
        self.pool_ids(venue.as_ref())
    }

    /// Returns references to all pools in the cache, optionally filtered by `venue`.
    #[pyo3(name = "pools")]
    fn py_pools(&self, venue: Option<Venue>) -> Vec<Pool> {
        self.pools(venue.as_ref()).into_iter().cloned().collect()
    }

    /// Adds a `PoolProfiler` to the cache.
    ///
    /// # Errors
    ///
    /// This function currently does not return errors but follows the same pattern as other add methods for consistency.
    #[pyo3(name = "add_pool_profiler")]
    fn py_add_pool_profiler(&mut self, pool_profiler: PoolProfiler) -> PyResult<()> {
        self.add_pool_profiler(pool_profiler)
            .map_err(to_pyvalue_err)
    }

    /// Gets a reference to the pool profiler for the `instrument_id`.
    #[pyo3(name = "pool_profiler")]
    fn py_pool_profiler(&self, instrument_id: InstrumentId) -> Option<PoolProfiler> {
        self.pool_profiler(&instrument_id).cloned()
    }

    /// Returns the instrument IDs of all pool profilers in the cache, optionally filtered by `venue`.
    #[pyo3(name = "pool_profiler_ids")]
    fn py_pool_profiler_ids(&self, venue: Option<Venue>) -> Vec<InstrumentId> {
        self.pool_profiler_ids(venue.as_ref())
    }

    /// Returns references to all pool profilers in the cache, optionally filtered by `venue`.
    #[pyo3(name = "pool_profilers")]
    fn py_pool_profilers(&self, venue: Option<Venue>) -> Vec<PoolProfiler> {
        self.pool_profilers(venue.as_ref())
            .into_iter()
            .cloned()
            .collect()
    }
}
