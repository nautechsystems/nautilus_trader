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

use std::{
    collections::{HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use indexmap::IndexMap;
use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyruntime_err};
use pyo3::{Python, prelude::*, pyclass::CompareOp};
use rust_decimal::Decimal;

use crate::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{ClientOrderId, InstrumentId, TraderId, VenueOrderId},
    orderbook::{OwnBookOrder, own::OwnOrderBook},
    types::{Price, Quantity},
};

#[pymethods]
impl OwnBookOrder {
    #[pyo3(signature = (trader_id, client_order_id, side, price, size, order_type, time_in_force, status, ts_last, ts_accepted, ts_submitted, ts_init, venue_order_id=None))]
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        side: OrderSide,
        price: Price,
        size: Quantity,
        order_type: OrderType,
        time_in_force: TimeInForce,
        status: OrderStatus,
        ts_last: u64,
        ts_accepted: u64,
        ts_submitted: u64,
        ts_init: u64,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Self> {
        Ok(Self::new(
            trader_id,
            client_order_id,
            venue_order_id,
            side.as_specified(),
            price,
            size,
            order_type,
            time_in_force,
            status,
            ts_last.into(),
            ts_accepted.into(),
            ts_submitted.into(),
            ts_init.into(),
        ))
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish() as isize
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }

    #[getter]
    #[pyo3(name = "side")]
    fn py_side(&self) -> OrderSide {
        self.side.as_order_side()
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Price {
        self.price
    }

    #[getter]
    #[pyo3(name = "size")]
    fn py_size(&self) -> Quantity {
        self.size
    }

    #[getter]
    #[pyo3(name = "order_type")]
    fn py_order_type(&self) -> OrderType {
        self.order_type
    }

    #[getter]
    #[pyo3(name = "time_in_force")]
    fn py_time_in_force(&self) -> TimeInForce {
        self.time_in_force
    }

    #[getter]
    #[pyo3(name = "status")]
    fn py_status(&self) -> OrderStatus {
        self.status
    }

    #[getter]
    #[pyo3(name = "ts_last")]
    fn py_ts_last(&self) -> u64 {
        self.ts_last.into()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.into()
    }

    #[pyo3(name = "exposure")]
    fn py_exposure(&self) -> f64 {
        self.exposure()
    }

    #[pyo3(name = "signed_size")]
    fn py_signed_size(&self) -> f64 {
        self.signed_size()
    }
}

#[pymethods]
impl OwnOrderBook {
    #[new]
    fn py_new(instrument_id: InstrumentId) -> Self {
        Self::new(instrument_id)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "ts_last")]
    fn py_ts_last(&self) -> u64 {
        self.ts_last.as_u64()
    }

    #[getter]
    #[pyo3(name = "update_count")]
    fn py_update_count(&self) -> u64 {
        self.update_count
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    #[pyo3(name = "add")]
    fn py_add(&mut self, order: OwnBookOrder) {
        self.add(order);
    }

    #[pyo3(name = "update")]
    fn py_update(&mut self, order: OwnBookOrder) -> PyResult<()> {
        self.update(order).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "delete")]
    fn py_delete(&mut self, order: OwnBookOrder) -> PyResult<()> {
        self.delete(order).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "clear")]
    fn py_clear(&mut self) {
        self.clear();
    }

    #[pyo3(name = "bid_client_order_ids")]
    pub fn py_bid_client_order_ids(&self) -> Vec<ClientOrderId> {
        self.bid_client_order_ids()
    }

    #[pyo3(name = "ask_client_order_ids")]
    pub fn py_ask_client_order_ids(&self) -> Vec<ClientOrderId> {
        self.ask_client_order_ids()
    }

    #[pyo3(name = "is_order_in_book")]
    pub fn py_is_order_in_book(&self, client_order_id: &ClientOrderId) -> bool {
        self.is_order_in_book(client_order_id)
    }

    #[pyo3(name = "orders_to_list")]
    fn py_orders_to_list(&self) -> Vec<OwnBookOrder> {
        let total_orders = self.bids.cache.len() + self.asks.cache.len();
        let mut all_orders = Vec::with_capacity(total_orders);

        all_orders.extend(
            self.bids()
                .flat_map(|level| level.orders.values().copied())
                .chain(self.asks().flat_map(|level| level.orders.values().copied())),
        );

        all_orders
    }

    #[pyo3(name = "bids_to_list")]
    fn py_bids_to_list(&self) -> Vec<OwnBookOrder> {
        self.bids()
            .flat_map(|level| level.orders.values().copied())
            .collect()
    }

    #[pyo3(name = "asks_to_list")]
    fn py_asks_to_list(&self) -> Vec<OwnBookOrder> {
        self.asks()
            .flat_map(|level| level.orders.values().copied())
            .collect()
    }

    #[pyo3(name = "bids_to_dict")]
    #[pyo3(signature = (status=None, accepted_buffer_ns=None, ts_now=None))]
    fn py_bids_to_dict(
        &self,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Vec<OwnBookOrder>> {
        self.bids_as_map(status, accepted_buffer_ns, ts_now)
    }

    #[pyo3(name = "asks_to_dict")]
    #[pyo3(signature = (status=None, accepted_buffer_ns=None, ts_now=None))]
    fn py_asks_to_dict(
        &self,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Vec<OwnBookOrder>> {
        self.asks_as_map(status, accepted_buffer_ns, ts_now)
    }

    #[pyo3(name = "bid_quantity")]
    #[pyo3(signature = (status=None, depth=None, group_size=None, accepted_buffer_ns=None, ts_now=None))]
    fn py_bid_quantity(
        &self,
        status: Option<HashSet<OrderStatus>>,
        depth: Option<usize>,
        group_size: Option<Decimal>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        self.bid_quantity(status, depth, group_size, accepted_buffer_ns, ts_now)
    }

    #[pyo3(name = "ask_quantity")]
    #[pyo3(signature = (status=None, depth=None, group_size=None, accepted_buffer_ns=None, ts_now=None))]
    fn py_ask_quantity(
        &self,
        status: Option<HashSet<OrderStatus>>,
        depth: Option<usize>,
        group_size: Option<Decimal>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        self.ask_quantity(status, depth, group_size, accepted_buffer_ns, ts_now)
    }

    #[pyo3(name = "audit_open_orders")]
    fn py_audit_open_orders(&mut self, open_order_ids: HashSet<ClientOrderId>) {
        self.audit_open_orders(&open_order_ids);
    }

    #[pyo3(name = "pprint")]
    #[pyo3(signature = (num_levels=3, group_size=None))]
    fn py_pprint(&self, num_levels: usize, group_size: Option<Decimal>) -> String {
        self.pprint(num_levels, group_size)
    }
}
