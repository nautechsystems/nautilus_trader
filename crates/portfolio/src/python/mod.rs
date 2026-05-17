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

//! Python bindings from [PyO3](https://pyo3.rs).

use std::{cell::RefCell, rc::Rc};

use nautilus_model::{
    identifiers::{AccountId, InstrumentId, Venue},
    types::{Currency, Money},
};
use pyo3::prelude::*;
use pyo3::{pymethods, types::PyDict};

use crate::{config::PortfolioConfig, portfolio::Portfolio};

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PortfolioConfig {
    /// Configuration for `Portfolio` instances.
    #[new]
    #[pyo3(signature = (use_mark_prices=None, use_mark_xrates=None, bar_updates=None, convert_to_account_base_currency=None, min_account_state_logging_interval_ms=None, debug=None, snapshot_interval_ms=None))]
    fn py_new(
        use_mark_prices: Option<bool>,
        use_mark_xrates: Option<bool>,
        bar_updates: Option<bool>,
        convert_to_account_base_currency: Option<bool>,
        min_account_state_logging_interval_ms: Option<u64>,
        debug: Option<bool>,
        snapshot_interval_ms: Option<u64>,
    ) -> Self {
        let default = Self::default();
        Self {
            use_mark_prices: use_mark_prices.unwrap_or(default.use_mark_prices),
            use_mark_xrates: use_mark_xrates.unwrap_or(default.use_mark_xrates),
            bar_updates: bar_updates.unwrap_or(default.bar_updates),
            convert_to_account_base_currency: convert_to_account_base_currency
                .unwrap_or(default.convert_to_account_base_currency),
            min_account_state_logging_interval_ms,
            snapshot_interval_ms,
            debug: debug.unwrap_or(default.debug),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn use_mark_prices(&self) -> bool {
        self.use_mark_prices
    }

    #[getter]
    fn use_mark_xrates(&self) -> bool {
        self.use_mark_xrates
    }

    #[getter]
    fn bar_updates(&self) -> bool {
        self.bar_updates
    }

    #[getter]
    fn convert_to_account_base_currency(&self) -> bool {
        self.convert_to_account_base_currency
    }

    #[getter]
    fn min_account_state_logging_interval_ms(&self) -> Option<u64> {
        self.min_account_state_logging_interval_ms
    }

    #[getter]
    fn snapshot_interval_ms(&self) -> Option<u64> {
        self.snapshot_interval_ms
    }

    #[getter]
    fn debug(&self) -> bool {
        self.debug
    }
}

/// Wrapper providing shared access to [`Portfolio`] from Python.
///
/// This wrapper holds an `Rc<RefCell<Portfolio>>` allowing strategies to share
/// the same portfolio instance. All methods delegate to the underlying portfolio.
#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.portfolio",
    name = "Portfolio",
    unsendable,
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.portfolio")]
#[derive(Debug, Clone)]
pub struct PyPortfolio(Rc<RefCell<Portfolio>>);

impl PyPortfolio {
    /// Creates a `PyPortfolio` from an `Rc<RefCell<Portfolio>>`.
    #[must_use]
    pub fn from_rc(rc: Rc<RefCell<Portfolio>>) -> Self {
        Self(rc)
    }

    /// Gets the inner `Rc<RefCell<Portfolio>>` for use in Rust code.
    #[must_use]
    pub fn portfolio_rc(&self) -> Rc<RefCell<Portfolio>> {
        self.0.clone()
    }
}

fn currency_money_map_to_pydict(
    py: Python<'_>,
    map: indexmap::IndexMap<Currency, Money>,
) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    for (currency, money) in map {
        dict.set_item(currency, money)?;
    }
    Ok(dict.unbind())
}

fn instrument_money_map_to_pydict(
    py: Python<'_>,
    map: indexmap::IndexMap<InstrumentId, Money>,
) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    for (instrument_id, money) in map {
        dict.set_item(instrument_id, money)?;
    }
    Ok(dict.unbind())
}

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyPortfolio {
    /// Returns whether the portfolio has been initialized.
    #[pyo3(name = "is_initialized")]
    fn py_is_initialized(&self) -> bool {
        self.0.borrow().is_initialized()
    }

    /// Returns the locked balances for the given venue.
    #[pyo3(name = "balances_locked")]
    fn py_balances_locked(&self, py: Python<'_>, venue: Venue) -> PyResult<Py<PyDict>> {
        let map = self.0.borrow().balances_locked(&venue);
        currency_money_map_to_pydict(py, map)
    }

    /// Returns the initial margin requirements for the given venue.
    #[pyo3(name = "margins_init")]
    fn py_margins_init(&self, py: Python<'_>, venue: Venue) -> PyResult<Py<PyDict>> {
        let map = self.0.borrow().margins_init(&venue);
        instrument_money_map_to_pydict(py, map)
    }

    /// Returns the maintenance margin requirements for the given venue.
    #[pyo3(name = "margins_maint")]
    fn py_margins_maint(&self, py: Python<'_>, venue: Venue) -> PyResult<Py<PyDict>> {
        let map = self.0.borrow().margins_maint(&venue);
        instrument_money_map_to_pydict(py, map)
    }

    /// Returns the unrealized PnLs for all positions at the given venue.
    #[pyo3(name = "unrealized_pnls")]
    #[pyo3(signature = (venue, account_id=None))]
    fn py_unrealized_pnls(
        &self,
        py: Python<'_>,
        venue: Venue,
        account_id: Option<AccountId>,
    ) -> PyResult<Py<PyDict>> {
        let map = self
            .0
            .borrow_mut()
            .unrealized_pnls(&venue, account_id.as_ref());
        currency_money_map_to_pydict(py, map)
    }

    /// Returns the realized PnLs for all positions at the given venue.
    #[pyo3(name = "realized_pnls")]
    #[pyo3(signature = (venue, account_id=None))]
    fn py_realized_pnls(
        &self,
        py: Python<'_>,
        venue: Venue,
        account_id: Option<AccountId>,
    ) -> PyResult<Py<PyDict>> {
        let map = self
            .0
            .borrow_mut()
            .realized_pnls(&venue, account_id.as_ref());
        currency_money_map_to_pydict(py, map)
    }

    /// Returns the total PnLs for all positions at the given venue.
    #[pyo3(name = "total_pnls")]
    #[pyo3(signature = (venue, account_id=None))]
    fn py_total_pnls(
        &self,
        py: Python<'_>,
        venue: Venue,
        account_id: Option<AccountId>,
    ) -> PyResult<Py<PyDict>> {
        let map = self.0.borrow_mut().total_pnls(&venue, account_id.as_ref());
        currency_money_map_to_pydict(py, map)
    }

    /// Returns the net exposures for the given venue.
    #[pyo3(name = "net_exposures")]
    #[pyo3(signature = (venue, account_id=None))]
    fn py_net_exposures(
        &self,
        py: Python<'_>,
        venue: Venue,
        account_id: Option<AccountId>,
    ) -> PyResult<Option<Py<PyDict>>> {
        match self.0.borrow().net_exposures(&venue, account_id.as_ref()) {
            Some(map) => Ok(Some(currency_money_map_to_pydict(py, map)?)),
            None => Ok(None),
        }
    }

    /// Returns the mark-to-market values for open positions at the given venue.
    #[pyo3(name = "mark_values")]
    #[pyo3(signature = (venue, account_id=None))]
    fn py_mark_values(
        &self,
        py: Python<'_>,
        venue: Venue,
        account_id: Option<AccountId>,
    ) -> PyResult<Py<PyDict>> {
        let map = self.0.borrow_mut().mark_values(&venue, account_id.as_ref());
        currency_money_map_to_pydict(py, map)
    }

    /// Returns the total equity for the given venue.
    #[pyo3(name = "equity")]
    #[pyo3(signature = (venue, account_id=None))]
    fn py_equity(
        &self,
        py: Python<'_>,
        venue: Venue,
        account_id: Option<AccountId>,
    ) -> PyResult<Py<PyDict>> {
        let map = self.0.borrow_mut().equity(&venue, account_id.as_ref());
        currency_money_map_to_pydict(py, map)
    }

    /// Returns the unrealized PnL for the given instrument.
    #[pyo3(name = "unrealized_pnl")]
    fn py_unrealized_pnl(&self, instrument_id: InstrumentId) -> Option<Money> {
        self.0.borrow_mut().unrealized_pnl(&instrument_id)
    }

    /// Returns the realized PnL for the given instrument.
    #[pyo3(name = "realized_pnl")]
    fn py_realized_pnl(&self, instrument_id: InstrumentId) -> Option<Money> {
        self.0.borrow_mut().realized_pnl(&instrument_id)
    }

    /// Returns the total PnL for the given instrument.
    #[pyo3(name = "total_pnl")]
    fn py_total_pnl(&self, instrument_id: InstrumentId) -> Option<Money> {
        self.0.borrow_mut().total_pnl(&instrument_id)
    }

    /// Returns the net exposure for the given instrument.
    #[pyo3(name = "net_exposure")]
    #[pyo3(signature = (instrument_id, account_id=None))]
    fn py_net_exposure(
        &self,
        instrument_id: InstrumentId,
        account_id: Option<AccountId>,
    ) -> Option<Money> {
        self.0
            .borrow()
            .net_exposure(&instrument_id, account_id.as_ref())
    }

    /// Returns the net position for the given instrument as a float.
    #[pyo3(name = "net_position")]
    fn py_net_position(&self, instrument_id: InstrumentId) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        self.0
            .borrow()
            .net_position(&instrument_id)
            .to_f64()
            .unwrap_or(0.0)
    }

    /// Returns whether the portfolio is net long for the given instrument.
    #[pyo3(name = "is_net_long")]
    fn py_is_net_long(&self, instrument_id: InstrumentId) -> bool {
        self.0.borrow().is_net_long(&instrument_id)
    }

    /// Returns whether the portfolio is net short for the given instrument.
    #[pyo3(name = "is_net_short")]
    fn py_is_net_short(&self, instrument_id: InstrumentId) -> bool {
        self.0.borrow().is_net_short(&instrument_id)
    }

    /// Returns whether the portfolio is flat for the given instrument.
    #[pyo3(name = "is_flat")]
    fn py_is_flat(&self, instrument_id: InstrumentId) -> bool {
        self.0.borrow().is_flat(&instrument_id)
    }

    /// Returns whether the portfolio is completely flat (no open positions).
    #[pyo3(name = "is_completely_flat")]
    fn py_is_completely_flat(&self) -> bool {
        self.0.borrow().is_completely_flat()
    }
}
