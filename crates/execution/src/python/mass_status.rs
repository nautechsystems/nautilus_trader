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

use indexmap::IndexMap;
use nautilus_core::{
    UUID4,
    python::{IntoPyObjectNautilusExt, serialization::from_dict_pyo3},
};
use nautilus_model::identifiers::{AccountId, ClientId, InstrumentId, Venue, VenueOrderId};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};

use crate::reports::{
    fill::FillReport, mass_status::ExecutionMassStatus, order::OrderStatusReport,
    position::PositionStatusReport,
};

#[pymethods]
impl ExecutionMassStatus {
    #[new]
    #[pyo3(signature = (client_id, account_id, venue, ts_init, report_id=None))]
    fn py_new(
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        ts_init: u64,
        report_id: Option<UUID4>,
    ) -> PyResult<Self> {
        Ok(Self::new(
            client_id,
            account_id,
            venue,
            ts_init.into(),
            report_id,
        ))
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "client_id")]
    const fn py_client_id(&self) -> ClientId {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "account_id")]
    const fn py_account_id(&self) -> AccountId {
        self.account_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    const fn py_venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    #[pyo3(name = "report_id")]
    const fn py_report_id(&self) -> UUID4 {
        self.report_id
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    const fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "order_reports")]
    fn py_order_reports(&self) -> PyResult<IndexMap<VenueOrderId, OrderStatusReport>> {
        Ok(self.order_reports())
    }

    #[getter]
    #[pyo3(name = "fill_reports")]
    fn py_fill_reports(&self) -> PyResult<IndexMap<VenueOrderId, Vec<FillReport>>> {
        Ok(self.fill_reports())
    }

    #[getter]
    #[pyo3(name = "position_reports")]
    fn py_position_reports(&self) -> PyResult<IndexMap<InstrumentId, Vec<PositionStatusReport>>> {
        Ok(self.position_reports())
    }

    #[pyo3(name = "add_order_reports")]
    fn py_add_order_reports(&mut self, reports: Vec<OrderStatusReport>) -> PyResult<()> {
        self.add_order_reports(reports);
        Ok(())
    }

    #[pyo3(name = "add_fill_reports")]
    fn py_add_fill_reports(&mut self, reports: Vec<FillReport>) -> PyResult<()> {
        self.add_fill_reports(reports);
        Ok(())
    }

    #[pyo3(name = "add_position_reports")]
    fn py_add_position_reports(&mut self, reports: Vec<PositionStatusReport>) -> PyResult<()> {
        self.add_position_reports(reports);
        Ok(())
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(ExecutionMassStatus))?;
        dict.set_item("client_id", self.client_id.to_string())?;
        dict.set_item("account_id", self.account_id.to_string())?;
        dict.set_item("venue", self.venue.to_string())?;
        dict.set_item("report_id", self.report_id.to_string())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;

        let order_reports_dict = PyDict::new(py);
        for (key, value) in &self.order_reports() {
            order_reports_dict.set_item(key.to_string(), value.py_to_dict(py)?)?;
        }
        dict.set_item("order_reports", order_reports_dict)?;

        let fill_reports_dict = PyDict::new(py);
        for (key, value) in &self.fill_reports() {
            let reports: PyResult<Vec<_>> = value.iter().map(|r| r.py_to_dict(py)).collect();
            fill_reports_dict.set_item(key.to_string(), reports?)?;
        }
        dict.set_item("fill_reports", fill_reports_dict)?;

        let position_reports_dict = PyDict::new(py);
        for (key, value) in &self.position_reports() {
            let reports: PyResult<Vec<_>> = value.iter().map(|r| r.py_to_dict(py)).collect();
            position_reports_dict.set_item(key.to_string(), reports?)?;
        }
        dict.set_item("position_reports", position_reports_dict)?;

        Ok(dict.into())
    }
}
