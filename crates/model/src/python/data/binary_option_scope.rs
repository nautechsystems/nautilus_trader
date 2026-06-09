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

use crate::{
    data::binary_option_scope::{
        BinaryOptionScope, BinaryOptionScopeMember, BinaryOptionScopeSlice,
        BinaryOptionScopeStreams,
    },
    identifiers::{InstrumentId, Venue},
};

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinaryOptionScope {
    /// Creates a new binary option scope key.
    #[new]
    #[pyo3(signature = (scope_id, venue))]
    #[allow(clippy::needless_pass_by_value)]
    fn py_new(scope_id: String, venue: Venue) -> Self {
        Self::new(&scope_id, venue)
    }

    #[getter(scope_id)]
    fn py_scope_id(&self) -> String {
        self.scope_id.to_string()
    }

    #[getter(venue)]
    fn py_venue(&self) -> Venue {
        self.venue
    }

    fn __repr__(&self) -> String {
        format!(
            "BinaryOptionScope(scope_id='{}', venue={})",
            self.scope_id, self.venue
        )
    }
}

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinaryOptionScopeStreams {
    #[new]
    #[pyo3(signature = (quotes=false, trades=false, book_deltas=false))]
    fn py_new(quotes: bool, trades: bool, book_deltas: bool) -> Self {
        Self {
            quotes,
            trades,
            book_deltas,
        }
    }
}

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinaryOptionScopeMember {
    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    fn outcome(&self) -> Option<String> {
        self.outcome.map(|outcome| outcome.to_string())
    }

    #[getter]
    fn expiration_ns(&self) -> u64 {
        self.expiration_ns.as_u64()
    }

    fn __repr__(&self) -> String {
        match self.outcome() {
            Some(outcome) => format!(
                "BinaryOptionScopeMember(instrument_id={}, outcome='{}', expiration_ns={})",
                self.instrument_id,
                outcome,
                self.expiration_ns()
            ),
            None => format!(
                "BinaryOptionScopeMember(instrument_id={}, outcome=None, expiration_ns={})",
                self.instrument_id,
                self.expiration_ns()
            ),
        }
    }
}

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinaryOptionScopeSlice {
    #[getter]
    fn scope_id(&self) -> String {
        self.scope_id.to_string()
    }

    #[getter]
    fn venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    fn members(&self) -> Vec<BinaryOptionScopeMember> {
        self.members.clone()
    }

    #[getter]
    fn window_start_ns(&self) -> u64 {
        self.window_start_ns.as_u64()
    }

    #[getter]
    fn window_end_ns(&self) -> u64 {
        self.window_end_ns.as_u64()
    }

    #[getter]
    fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    fn __repr__(&self) -> String {
        format!("{self}")
    }
}
