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

use nautilus_core::python::IntoPyObjectNautilusExt;
use pyo3::{basic::CompareOp, prelude::*};

use crate::{
    identifiers::{ClientOrderId, InstrumentId, OrderListId, StrategyId},
    orders::OrderList,
};

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl OrderList {
    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        self.id.inner().precomputed_hash() as isize
    }

    fn __len__(&self) -> usize {
        self.len()
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "id")]
    fn py_id(&self) -> OrderListId {
        self.id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    /// Returns the client order IDs contained in the order list.
    #[pyo3(name = "client_order_ids")]
    fn py_client_order_ids(&self) -> Vec<ClientOrderId> {
        self.client_order_ids.clone()
    }

    #[getter]
    #[pyo3(name = "first_client_order_id")]
    fn py_first_client_order_id(&self) -> Option<ClientOrderId> {
        self.first().copied()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use pyo3::{
        Py, Python,
        types::{PyAnyMethods, PyStringMethods},
    };
    use rstest::rstest;

    use crate::{
        identifiers::{ClientOrderId, InstrumentId, OrderListId, StrategyId},
        orders::OrderList,
    };

    fn create_order_list(order_list_id: &str) -> OrderList {
        OrderList::new(
            OrderListId::from(order_list_id),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-001"),
            vec![ClientOrderId::from("O-001"), ClientOrderId::from("O-002")],
            UnixNanos::from(42_u64),
        )
    }

    #[rstest]
    fn test_python_order_list_exposes_readonly_api() {
        Python::initialize();
        Python::attach(|py| {
            let order_list = create_order_list("OL-001");
            let py_order_list = Py::new(py, order_list.clone()).unwrap();
            let bound = py_order_list.bind(py);

            assert_eq!(
                bound
                    .getattr("id")
                    .unwrap()
                    .extract::<OrderListId>()
                    .unwrap(),
                order_list.id,
            );
            assert_eq!(
                bound
                    .getattr("instrument_id")
                    .unwrap()
                    .extract::<InstrumentId>()
                    .unwrap(),
                order_list.instrument_id,
            );
            assert_eq!(
                bound
                    .getattr("strategy_id")
                    .unwrap()
                    .extract::<StrategyId>()
                    .unwrap(),
                order_list.strategy_id,
            );
            assert_eq!(
                bound.getattr("ts_init").unwrap().extract::<u64>().unwrap(),
                order_list.ts_init.as_u64(),
            );
            assert_eq!(
                bound
                    .call_method0("client_order_ids")
                    .unwrap()
                    .extract::<Vec<ClientOrderId>>()
                    .unwrap(),
                order_list.client_order_ids,
            );
            assert_eq!(
                bound
                    .getattr("first_client_order_id")
                    .unwrap()
                    .extract::<ClientOrderId>()
                    .unwrap(),
                order_list.client_order_ids[0],
            );
            assert_eq!(bound.len().unwrap(), order_list.len());
            assert_eq!(
                bound.str().unwrap().to_str().unwrap(),
                order_list.to_string(),
            );
            assert_eq!(
                bound.repr().unwrap().to_str().unwrap(),
                order_list.to_string(),
            );
            assert_eq!(
                bound.hash().unwrap(),
                bound.getattr("id").unwrap().hash().unwrap(),
            );

            let same = Py::new(py, order_list).unwrap();
            assert!(
                bound
                    .call_method1("__eq__", (same,))
                    .unwrap()
                    .extract::<bool>()
                    .unwrap(),
            );

            let different = Py::new(py, create_order_list("OL-002")).unwrap();
            assert!(
                !bound
                    .call_method1("__eq__", (different,))
                    .unwrap()
                    .extract::<bool>()
                    .unwrap(),
            );
            assert!(bound.setattr("id", OrderListId::from("OL-003")).is_err());
        });
    }
}
