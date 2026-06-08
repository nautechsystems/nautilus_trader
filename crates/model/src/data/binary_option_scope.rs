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

use std::{any::Any, fmt::Display, sync::Arc};

use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    data::{HasTsInit, custom::CustomDataTrait},
    identifiers::{InstrumentId, Venue},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct BinaryOptionScope {
    pub scope_id: Ustr,
    pub venue: Venue,
}

impl BinaryOptionScope {
    /// Creates a new binary option scope key.
    ///
    /// # Panics
    ///
    /// Panics if `scope_id` is empty after trimming.
    #[must_use]
    pub fn new(scope_id: &str, venue: Venue) -> Self {
        let scope_id = scope_id.trim();
        assert!(!scope_id.is_empty(), "scope_id must be non-empty");

        Self {
            scope_id: Ustr::from(scope_id),
            venue,
        }
    }

    #[must_use]
    pub fn venue(&self) -> Venue {
        self.venue
    }

    #[must_use]
    pub fn scope_id(&self) -> Ustr {
        self.scope_id
    }
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BinaryOptionScope {
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

impl Display for BinaryOptionScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BinaryOptionScope({}, venue={})",
            self.scope_id, self.venue
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct BinaryOptionScopeStreams {
    pub quotes: bool,
    pub trades: bool,
    pub book_deltas: bool,
}

#[cfg(feature = "python")]
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

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct BinaryOptionScopeMember {
    pub instrument_id: InstrumentId,
    pub outcome: Option<Ustr>,
    pub expiration_ns: UnixNanos,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct BinaryOptionScopeSlice {
    pub scope_id: Ustr,
    pub venue: Venue,
    pub members: Vec<BinaryOptionScopeMember>,
    pub window_start_ns: UnixNanos,
    pub window_end_ns: UnixNanos,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl BinaryOptionScopeSlice {
    #[must_use]
    pub fn empty(
        scope_id: Ustr,
        venue: Venue,
        window_start_ns: UnixNanos,
        window_end_ns: UnixNanos,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            scope_id,
            venue,
            members: Vec::new(),
            window_start_ns,
            window_end_ns,
            ts_event,
            ts_init,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

impl HasTsInit for BinaryOptionScopeSlice {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl Display for BinaryOptionScopeSlice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BinaryOptionScopeSlice({}, members={}, window=[{}, {}])",
            self.scope_id,
            self.members.len(),
            self.window_start_ns,
            self.window_end_ns
        )
    }
}

impl CustomDataTrait for BinaryOptionScopeSlice {
    fn type_name(&self) -> &'static str {
        Self::type_name_static()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|value| value == self)
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        crate::data::custom::clone_pyclass_to_pyobject(self, py)
    }

    fn type_name_static() -> &'static str
    where
        Self: Sized,
    {
        "BinaryOptionScopeSlice"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>>
    where
        Self: Sized,
    {
        let json = serde_json::to_string(&value)?;
        let parsed: Self = serde_json::from_str(&json)?;
        Ok(Arc::new(parsed))
    }
}

#[cfg(feature = "python")]
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

#[cfg(feature = "python")]
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

#[cfg(test)]
mod tests {
    use crate::identifiers::Venue;
    use rstest::rstest;

    #[rstest]
    fn binary_option_scope_trims_scope_id() {
        let scope = super::BinaryOptionScope::new(" btc-5m ", Venue::from("POLYMARKET"));
        assert_eq!(scope.scope_id.as_str(), "btc-5m");
        assert_eq!(scope.venue, Venue::from("POLYMARKET"));
    }

    #[rstest]
    fn binary_option_scope_display_is_stable() {
        let scope = super::BinaryOptionScope::new("btc-5m", Venue::from("POLYMARKET"));
        assert_eq!(
            scope.to_string(),
            "BinaryOptionScope(btc-5m, venue=POLYMARKET)"
        );
    }

    #[rstest]
    #[should_panic(expected = "scope_id must be non-empty")]
    fn binary_option_scope_rejects_empty_scope_id() {
        let _ = super::BinaryOptionScope::new("   ", Venue::from("POLYMARKET"));
    }
}
