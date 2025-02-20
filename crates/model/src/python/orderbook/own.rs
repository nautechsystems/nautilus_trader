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
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::python::IntoPyObjectNautilusExt;
use pyo3::{prelude::*, pyclass::CompareOp, Python};

use crate::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{ClientOrderId, InstrumentId},
    orderbook::{own::OwnOrderBook, OwnBookOrder},
    types::{Price, Quantity},
};

#[pymethods]
impl OwnBookOrder {
    #[new]
    fn py_new(
        client_order_id: ClientOrderId,
        side: OrderSide,
        price: Price,
        size: Quantity,
        order_type: OrderType,
        time_in_force: TimeInForce,
        ts_init: u64,
    ) -> PyResult<Self> {
        Ok(OwnBookOrder::new(
            client_order_id,
            side.as_specified(),
            price,
            size,
            order_type,
            time_in_force,
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
    #[pyo3(name = "count")]
    fn py_count(&self) -> u64 {
        self.count
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
    fn py_update(&mut self, order: OwnBookOrder) {
        self.update(order);
    }

    #[pyo3(name = "delete")]
    fn py_delete(&mut self, order: OwnBookOrder) {
        self.delete(order);
    }

    #[pyo3(name = "clear")]
    fn py_clear(&mut self) {
        self.clear();
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }
}
