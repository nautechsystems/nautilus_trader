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

use indexmap::{IndexMap, IndexSet};
use nautilus_core::python::{to_pynotimplemented_err, to_pyvalue_err};
use nautilus_model::{
    accounts::AccountAny,
    identifiers::{AccountId, InstrumentId, Venue},
    python::account::account_any_to_pyobject,
    types::{Currency, Money, Price},
};
use pyo3::{prelude::*, types::PyDict};
use rust_decimal::Decimal;

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
#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.portfolio",
    name = "Portfolio",
    unsendable,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.portfolio")]
#[derive(Debug, Clone)]
pub struct PyPortfolio(Rc<RefCell<Portfolio>>);

impl PyPortfolio {
    /// Creates a [`PyPortfolio`] from a shared [`Portfolio`].
    #[must_use]
    pub fn from_rc(rc: Rc<RefCell<Portfolio>>) -> Self {
        Self(rc)
    }

    /// Returns the inner shared [`Portfolio`].
    #[must_use]
    pub fn portfolio_rc(&self) -> Rc<RefCell<Portfolio>> {
        self.0.clone()
    }
}

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyPortfolio {
    #[pyo3(name = "is_initialized")]
    fn py_is_initialized(&self) -> bool {
        self.0.borrow().is_initialized()
    }

    #[pyo3(name = "account", signature = (venue=None, account_id=None))]
    fn py_account(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
    ) -> PyResult<Option<Py<PyAny>>> {
        match self.account_for_required_query(venue.as_ref(), account_id.as_ref())? {
            Some(account) => Ok(Some(account_any_to_pyobject(py, account)?)),
            None => Ok(None),
        }
    }

    #[pyo3(name = "balances_locked", signature = (venue=None, account_id=None))]
    fn py_balances_locked(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
    ) -> PyResult<Option<Py<PyDict>>> {
        match self.account_for_required_query(venue.as_ref(), account_id.as_ref())? {
            Some(account) => Ok(Some(currency_money_map_to_pydict(
                py,
                account.balances_locked(),
            )?)),
            None => Ok(None),
        }
    }

    #[pyo3(name = "margins_init", signature = (venue=None, account_id=None))]
    fn py_margins_init(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
    ) -> PyResult<Option<Py<PyDict>>> {
        match self.account_for_required_query(venue.as_ref(), account_id.as_ref())? {
            Some(AccountAny::Margin(account)) => Ok(Some(instrument_money_map_to_pydict(
                py,
                account.initial_margins(),
            )?)),
            Some(AccountAny::Cash(_) | AccountAny::Betting(_)) | None => Ok(None),
        }
    }

    #[pyo3(name = "margins_maint", signature = (venue=None, account_id=None))]
    fn py_margins_maint(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
    ) -> PyResult<Option<Py<PyDict>>> {
        match self.account_for_required_query(venue.as_ref(), account_id.as_ref())? {
            Some(AccountAny::Margin(account)) => Ok(Some(instrument_money_map_to_pydict(
                py,
                account.maintenance_margins(),
            )?)),
            Some(AccountAny::Cash(_) | AccountAny::Betting(_)) | None => Ok(None),
        }
    }

    #[pyo3(name = "realized_pnls", signature = (venue=None, account_id=None, target_currency=None))]
    fn py_realized_pnls(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
        target_currency: Option<Currency>,
    ) -> PyResult<Py<PyDict>> {
        unsupported_target_currency(target_currency.as_ref())?;
        let Some(venue) = venue else {
            let venues = self.position_venues(false, account_id.as_ref());
            return self.aggregate_currency_maps(py, venues, |portfolio, venue| {
                portfolio.realized_pnls(venue, account_id.as_ref())
            });
        };

        let map = self
            .0
            .borrow_mut()
            .realized_pnls(&venue, account_id.as_ref());
        currency_money_map_to_pydict(py, map)
    }

    #[pyo3(name = "unrealized_pnls", signature = (venue=None, account_id=None, target_currency=None))]
    fn py_unrealized_pnls(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
        target_currency: Option<Currency>,
    ) -> PyResult<Py<PyDict>> {
        unsupported_target_currency(target_currency.as_ref())?;
        let Some(venue) = venue else {
            let venues = self.position_venues(true, account_id.as_ref());
            return self.aggregate_currency_maps(py, venues, |portfolio, venue| {
                portfolio.unrealized_pnls(venue, account_id.as_ref())
            });
        };

        let map = self
            .0
            .borrow_mut()
            .unrealized_pnls(&venue, account_id.as_ref());
        currency_money_map_to_pydict(py, map)
    }

    #[pyo3(name = "total_pnls", signature = (venue=None, account_id=None, target_currency=None))]
    fn py_total_pnls(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
        target_currency: Option<Currency>,
    ) -> PyResult<Py<PyDict>> {
        unsupported_target_currency(target_currency.as_ref())?;
        let Some(venue) = venue else {
            // Closed-only venues still contribute realized PnL.
            let venues = self.position_venues(false, account_id.as_ref());
            return self.aggregate_currency_maps(py, venues, |portfolio, venue| {
                portfolio.total_pnls(venue, account_id.as_ref())
            });
        };

        let map = self.0.borrow_mut().total_pnls(&venue, account_id.as_ref());
        currency_money_map_to_pydict(py, map)
    }

    #[pyo3(name = "net_exposures", signature = (venue=None, account_id=None, target_currency=None))]
    fn py_net_exposures(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
        target_currency: Option<Currency>,
    ) -> PyResult<Option<Py<PyDict>>> {
        unsupported_target_currency(target_currency.as_ref())?;
        let Some(venue) = venue else {
            return self.aggregate_net_exposures(py, account_id.as_ref());
        };

        match self.0.borrow().net_exposures(&venue, account_id.as_ref()) {
            Some(map) => Ok(Some(currency_money_map_to_pydict(py, map)?)),
            None => Ok(None),
        }
    }

    #[pyo3(name = "mark_values", signature = (venue=None, account_id=None))]
    fn py_mark_values(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
    ) -> PyResult<Py<PyDict>> {
        let Some(venue) = venue else {
            let venues = self.position_venues(true, account_id.as_ref());
            return self.aggregate_currency_maps(py, venues, |portfolio, venue| {
                portfolio.mark_values(venue, account_id.as_ref())
            });
        };

        let map = self.0.borrow_mut().mark_values(&venue, account_id.as_ref());
        currency_money_map_to_pydict(py, map)
    }

    #[pyo3(name = "equity", signature = (venue=None, account_id=None))]
    fn py_equity(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
    ) -> PyResult<Py<PyDict>> {
        if venue.is_none() && account_id.is_none() {
            return Err(to_pyvalue_err("venue or account_id must be provided"));
        }

        let Some(venue) = venue else {
            return self.account_equity(py, account_id.as_ref());
        };

        let map = self.0.borrow_mut().equity(&venue, account_id.as_ref());
        currency_money_map_to_pydict(py, map)
    }

    #[pyo3(name = "missing_price_instruments")]
    fn py_missing_price_instruments(&self, venue: Venue) -> Vec<InstrumentId> {
        self.0.borrow().missing_price_instruments(&venue)
    }

    #[pyo3(name = "realized_pnl", signature = (instrument_id, account_id=None, target_currency=None))]
    fn py_realized_pnl(
        &self,
        instrument_id: InstrumentId,
        account_id: Option<AccountId>,
        target_currency: Option<Currency>,
    ) -> PyResult<Option<Money>> {
        unsupported_target_currency(target_currency.as_ref())?;
        Ok(self
            .0
            .borrow_mut()
            .realized_pnl_for_account(&instrument_id, account_id.as_ref()))
    }

    #[pyo3(
        name = "unrealized_pnl",
        signature = (instrument_id, price=None, account_id=None, target_currency=None)
    )]
    fn py_unrealized_pnl(
        &self,
        instrument_id: InstrumentId,
        price: Option<Price>,
        account_id: Option<AccountId>,
        target_currency: Option<Currency>,
    ) -> PyResult<Option<Money>> {
        unsupported_price(price.as_ref())?;
        unsupported_target_currency(target_currency.as_ref())?;
        Ok(self
            .0
            .borrow_mut()
            .unrealized_pnl_for_account(&instrument_id, account_id.as_ref()))
    }

    #[pyo3(
        name = "total_pnl",
        signature = (instrument_id, price=None, account_id=None, target_currency=None)
    )]
    fn py_total_pnl(
        &self,
        instrument_id: InstrumentId,
        price: Option<Price>,
        account_id: Option<AccountId>,
        target_currency: Option<Currency>,
    ) -> PyResult<Option<Money>> {
        unsupported_price(price.as_ref())?;
        unsupported_target_currency(target_currency.as_ref())?;
        Ok(self
            .0
            .borrow_mut()
            .total_pnl_for_account(&instrument_id, account_id.as_ref()))
    }

    #[pyo3(
        name = "net_exposure",
        signature = (instrument_id, price=None, account_id=None, target_currency=None)
    )]
    fn py_net_exposure(
        &self,
        instrument_id: InstrumentId,
        price: Option<Price>,
        account_id: Option<AccountId>,
        target_currency: Option<Currency>,
    ) -> PyResult<Option<Money>> {
        unsupported_price(price.as_ref())?;
        unsupported_target_currency(target_currency.as_ref())?;
        Ok(self
            .0
            .borrow()
            .net_exposure(&instrument_id, account_id.as_ref()))
    }

    #[pyo3(name = "net_position", signature = (instrument_id, account_id=None))]
    fn py_net_position(
        &self,
        instrument_id: InstrumentId,
        account_id: Option<AccountId>,
    ) -> Decimal {
        self.net_position_for_account(&instrument_id, account_id.as_ref())
    }

    #[pyo3(name = "is_net_long", signature = (instrument_id, account_id=None))]
    fn py_is_net_long(&self, instrument_id: InstrumentId, account_id: Option<AccountId>) -> bool {
        self.net_position_for_account(&instrument_id, account_id.as_ref()) > Decimal::ZERO
    }

    #[pyo3(name = "is_net_short", signature = (instrument_id, account_id=None))]
    fn py_is_net_short(&self, instrument_id: InstrumentId, account_id: Option<AccountId>) -> bool {
        self.net_position_for_account(&instrument_id, account_id.as_ref()) < Decimal::ZERO
    }

    #[pyo3(name = "is_flat", signature = (instrument_id, account_id=None))]
    fn py_is_flat(&self, instrument_id: InstrumentId, account_id: Option<AccountId>) -> bool {
        self.net_position_for_account(&instrument_id, account_id.as_ref()) == Decimal::ZERO
    }

    #[pyo3(name = "is_completely_flat", signature = (account_id=None))]
    fn py_is_completely_flat(&self, account_id: Option<AccountId>) -> bool {
        self.is_completely_flat_for_account(account_id.as_ref())
    }
}

/// Loaded as `nautilus_pyo3.portfolio`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn portfolio(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PortfolioConfig>()?;
    m.add_class::<PyPortfolio>()?;
    Ok(())
}

impl PyPortfolio {
    fn account_for_query(
        &self,
        venue: Option<&Venue>,
        account_id: Option<&AccountId>,
    ) -> Option<AccountAny> {
        let portfolio = self.0.borrow();
        let cache = portfolio.cache().borrow();
        if let Some(account_id) = account_id {
            cache.account(account_id).map(|account| account.clone())
        } else if let Some(venue) = venue {
            cache
                .account_for_venue(venue)
                .map(|account| account.clone())
        } else {
            None
        }
    }

    fn account_for_required_query(
        &self,
        venue: Option<&Venue>,
        account_id: Option<&AccountId>,
    ) -> PyResult<Option<AccountAny>> {
        if venue.is_none() && account_id.is_none() {
            return Err(to_pyvalue_err("venue or account_id must be provided"));
        }

        Ok(self.account_for_query(venue, account_id))
    }

    fn position_venues(&self, open_only: bool, account_id: Option<&AccountId>) -> Vec<Venue> {
        let portfolio = self.0.borrow();
        let cache = portfolio.cache().borrow();
        let venues: IndexSet<Venue> = if open_only {
            cache
                .positions_open(None, None, None, account_id, None)
                .iter()
                .map(|position| position.instrument_id.venue)
                .collect()
        } else {
            cache
                .positions(None, None, None, account_id, None)
                .iter()
                .map(|position| position.instrument_id.venue)
                .collect()
        };
        venues.into_iter().collect()
    }

    fn aggregate_currency_maps<F>(
        &self,
        py: Python<'_>,
        venues: Vec<Venue>,
        mut query: F,
    ) -> PyResult<Py<PyDict>>
    where
        F: FnMut(&mut Portfolio, &Venue) -> IndexMap<Currency, Money>,
    {
        let mut totals: IndexMap<Currency, f64> = IndexMap::new();
        let mut portfolio = self.0.borrow_mut();

        for venue in venues {
            add_money_map(&mut totals, query(&mut portfolio, &venue));
        }

        currency_totals_to_pydict(py, totals)
    }

    fn aggregate_net_exposures(
        &self,
        py: Python<'_>,
        account_id: Option<&AccountId>,
    ) -> PyResult<Option<Py<PyDict>>> {
        let venues = self.position_venues(true, account_id);
        if venues.is_empty() {
            return match account_id {
                Some(account_id) if self.account_for_query(None, Some(account_id)).is_some() => {
                    Ok(Some(currency_money_map_to_pydict(py, IndexMap::new())?))
                }
                _ => Ok(None),
            };
        }

        let mut totals: IndexMap<Currency, f64> = IndexMap::new();
        let portfolio = self.0.borrow();
        for venue in venues {
            let Some(exposures) = portfolio.net_exposures(&venue, account_id) else {
                return Ok(None);
            };
            add_money_map(&mut totals, exposures);
        }

        Ok(Some(currency_totals_to_pydict(py, totals)?))
    }

    fn account_equity(
        &self,
        py: Python<'_>,
        account_id: Option<&AccountId>,
    ) -> PyResult<Py<PyDict>> {
        let Some(account_id) = account_id else {
            return Err(to_pyvalue_err("account_id must be provided"));
        };

        let Some(snapshot) = self.0.borrow_mut().build_snapshot(account_id) else {
            return currency_money_map_to_pydict(py, IndexMap::new());
        };

        let map = snapshot
            .total_equity
            .into_iter()
            .map(|money| (money.currency, money))
            .collect();
        currency_money_map_to_pydict(py, map)
    }

    fn net_position_for_account(
        &self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
    ) -> Decimal {
        self.0
            .borrow()
            .cache()
            .borrow()
            .positions_open(None, Some(instrument_id), None, account_id, None)
            .iter()
            .map(|position| position.signed_decimal_qty())
            .sum()
    }

    fn is_completely_flat_for_account(&self, account_id: Option<&AccountId>) -> bool {
        let portfolio = self.0.borrow();
        let cache = portfolio.cache().borrow();
        let mut net_positions: IndexMap<InstrumentId, Decimal> = IndexMap::new();

        for position in cache.positions_open(None, None, None, account_id, None) {
            *net_positions
                .entry(position.instrument_id)
                .or_insert(Decimal::ZERO) += position.signed_decimal_qty();
        }

        net_positions
            .values()
            .all(|quantity| *quantity == Decimal::ZERO)
    }
}

fn currency_money_map_to_pydict(
    py: Python<'_>,
    map: IndexMap<Currency, Money>,
) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    for (currency, money) in map {
        dict.set_item(currency, money)?;
    }
    Ok(dict.unbind())
}

fn currency_totals_to_pydict(
    py: Python<'_>,
    totals: IndexMap<Currency, f64>,
) -> PyResult<Py<PyDict>> {
    currency_money_map_to_pydict(
        py,
        totals
            .into_iter()
            .map(|(currency, amount)| (currency, Money::new(amount, currency)))
            .collect(),
    )
}

fn instrument_money_map_to_pydict(
    py: Python<'_>,
    map: IndexMap<InstrumentId, Money>,
) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    for (instrument_id, money) in map {
        dict.set_item(instrument_id, money)?;
    }
    Ok(dict.unbind())
}

fn add_money_map(totals: &mut IndexMap<Currency, f64>, map: IndexMap<Currency, Money>) {
    for (currency, money) in map {
        *totals.entry(currency).or_insert(0.0) += money.as_f64();
    }
}

fn unsupported_price(price: Option<&Price>) -> PyResult<()> {
    match price {
        Some(_) => Err(to_pynotimplemented_err(
            "price override is not yet supported by the Rust Portfolio",
        )),
        None => Ok(()),
    }
}

fn unsupported_target_currency(target_currency: Option<&Currency>) -> PyResult<()> {
    match target_currency {
        Some(_) => Err(to_pynotimplemented_err(
            "target_currency conversion is not yet supported by the Rust Portfolio",
        )),
        None => Ok(()),
    }
}
