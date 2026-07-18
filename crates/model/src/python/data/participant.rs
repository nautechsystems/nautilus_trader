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

//! Python bindings for participant data types.

use std::str::FromStr;

use nautilus_core::{
    UnixNanos,
    python::{IntoPyObjectNautilusExt, to_pyvalue_err},
};
use pyo3::{prelude::*, pyclass::CompareOp};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    data::{
        Participant, ParticipantKind, ParticipantProfile, ParticipantTransaction, ProfileState,
        TransactionMethod,
    },
    identifiers::{InstrumentId, ParticipantId, Venue},
    reports::{OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money, Price},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ParticipantKind {
    #[new]
    fn py_new(value: &str) -> PyResult<Self> {
        Self::from_str(value).map_err(to_pyvalue_err)
    }

    const fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    const fn value(&self) -> u8 {
        *self as u8
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ProfileState {
    #[new]
    fn py_new(value: &str) -> PyResult<Self> {
        Self::from_str(value).map_err(to_pyvalue_err)
    }

    const fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    const fn value(&self) -> u8 {
        *self as u8
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl Participant {
    #[new]
    #[pyo3(signature = (id, venue, kind, first_seen_at, last_seen_at, ts_init, metadata=None))]
    fn py_new(
        id: ParticipantId,
        venue: Venue,
        kind: ParticipantKind,
        first_seen_at: u64,
        last_seen_at: u64,
        ts_init: u64,
        metadata: Option<pyo3::Py<pyo3::types::PyAny>>,
    ) -> PyResult<Self> {
        let mut participant = Self::new_checked(
            id,
            venue,
            kind,
            UnixNanos::from(first_seen_at),
            UnixNanos::from(last_seen_at),
            UnixNanos::from(ts_init),
        )
        .map_err(to_pyvalue_err)?;

        if let Some(meta) = metadata {
            pyo3::Python::with_gil(|py| -> PyResult<()> {
                let json_str = py
                    .import("json")?
                    .call_method1("dumps", (meta.bind(py),))?
                    .extract::<String>()?;
                participant.metadata =
                    Some(serde_json::from_str(&json_str).map_err(to_pyvalue_err)?);
                Ok(())
            })?;
        }

        Ok(participant)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    const fn id(&self) -> ParticipantId {
        self.id
    }

    #[getter]
    const fn venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    const fn kind(&self) -> ParticipantKind {
        self.kind
    }

    #[getter]
    const fn first_seen_at(&self) -> u64 {
        self.first_seen_at.as_u64()
    }

    #[getter]
    const fn last_seen_at(&self) -> u64 {
        self.last_seen_at.as_u64()
    }

    #[getter]
    const fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ParticipantTransaction {
    #[new]
    fn py_new(
        hash: &str,
        method: TransactionMethod,
        ts_event: u64,
        amount: Decimal,
        instrument_id: InstrumentId,
        price: Price,
        value: Money,
    ) -> Self {
        Self::new(
            Ustr::from(hash),
            method,
            UnixNanos::from(ts_event),
            amount,
            instrument_id,
            price,
            value,
        )
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn hash(&self) -> String {
        self.hash.to_string()
    }

    #[getter]
    const fn method(&self) -> TransactionMethod {
        self.method
    }

    #[getter]
    const fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    const fn amount(&self) -> Decimal {
        self.amount
    }

    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    const fn price(&self) -> Price {
        self.price
    }

    #[getter]
    const fn value(&self) -> Money {
        self.value
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ParticipantProfile {
    #[new]
    #[pyo3(signature = (
        participant_id,
        ts_init,
        balances=None,
        margins=None,
        positions=None,
        open_orders=None,
        transactions=None,
    ))]
    fn py_new(
        participant_id: ParticipantId,
        ts_init: u64,
        balances: Option<Vec<AccountBalance>>,
        margins: Option<Vec<MarginBalance>>,
        positions: Option<Vec<PositionStatusReport>>,
        open_orders: Option<Vec<OrderStatusReport>>,
        transactions: Option<Vec<ParticipantTransaction>>,
    ) -> Self {
        Self::new(
            participant_id,
            balances,
            margins,
            positions,
            open_orders,
            transactions,
            UnixNanos::from(ts_init),
        )
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    const fn participant_id(&self) -> ParticipantId {
        self.participant_id
    }

    #[getter]
    fn balances(&self) -> Option<Vec<AccountBalance>> {
        self.balances.clone()
    }

    #[getter]
    fn margins(&self) -> Option<Vec<MarginBalance>> {
        self.margins.clone()
    }

    #[getter]
    fn positions(&self) -> Option<Vec<PositionStatusReport>> {
        self.positions.clone()
    }

    #[getter]
    fn open_orders(&self) -> Option<Vec<OrderStatusReport>> {
        self.open_orders.clone()
    }

    #[getter]
    fn transactions(&self) -> Option<Vec<ParticipantTransaction>> {
        self.transactions.clone()
    }

    #[getter]
    const fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }
}
