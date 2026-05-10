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

use indexmap::IndexMap;
use nautilus_core::{
    UnixNanos,
    python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err},
};
use pyo3::{IntoPyObjectExt, basic::CompareOp, prelude::*, types::PyDict};
use rust_decimal::Decimal;

use crate::{
    accounts::{Account, MarginAccount},
    enums::{AccountType, LiquiditySide, OrderSide},
    events::{AccountState, OrderFilled},
    identifiers::{AccountId, InstrumentId},
    instruments::InstrumentAny,
    position::Position,
    python::instruments::pyobject_to_instrument_any,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl MarginAccount {
    /// Creates a new `MarginAccount` instance.
    #[new]
    fn py_new(event: AccountState, calculate_account_state: bool) -> Self {
        Self::new(event, calculate_account_state)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    #[getter]
    fn id(&self) -> AccountId {
        self.id
    }

    #[getter]
    #[pyo3(name = "account_type")]
    fn py_account_type(&self) -> AccountType {
        self.account_type
    }

    #[getter]
    #[pyo3(name = "base_currency")]
    fn py_base_currency(&self) -> Option<Currency> {
        self.base_currency
    }

    #[getter]
    fn default_leverage(&self) -> Decimal {
        self.default_leverage
    }

    #[getter]
    #[pyo3(name = "calculate_account_state")]
    fn py_calculate_account_state(&self) -> bool {
        self.calculate_account_state
    }

    #[getter]
    #[pyo3(name = "last_event")]
    fn py_last_event(&self) -> Option<AccountState> {
        Account::last_event(self)
    }

    #[getter]
    #[pyo3(name = "event_count")]
    fn py_event_count(&self) -> usize {
        Account::event_count(self)
    }

    #[getter]
    #[pyo3(name = "events")]
    fn py_events(&self) -> Vec<AccountState> {
        Account::events(self)
    }

    #[pyo3(name = "balance_total")]
    #[pyo3(signature = (currency=None))]
    fn py_balance_total(&self, currency: Option<Currency>) -> Option<Money> {
        Account::balance_total(self, currency)
    }

    #[pyo3(name = "balances_total")]
    fn py_balances_total(&self) -> IndexMap<Currency, Money> {
        Account::balances_total(self)
    }

    #[pyo3(name = "balance_free")]
    #[pyo3(signature = (currency=None))]
    fn py_balance_free(&self, currency: Option<Currency>) -> Option<Money> {
        Account::balance_free(self, currency)
    }

    #[pyo3(name = "balances_free")]
    fn py_balances_free(&self) -> IndexMap<Currency, Money> {
        Account::balances_free(self)
    }

    #[pyo3(name = "balance_locked")]
    #[pyo3(signature = (currency=None))]
    fn py_balance_locked(&self, currency: Option<Currency>) -> Option<Money> {
        Account::balance_locked(self, currency)
    }

    #[pyo3(name = "balances_locked")]
    fn py_balances_locked(&self) -> IndexMap<Currency, Money> {
        Account::balances_locked(self)
    }

    #[pyo3(name = "balance")]
    #[pyo3(signature = (currency=None))]
    fn py_balance(&self, currency: Option<Currency>) -> Option<AccountBalance> {
        Account::balance(self, currency).copied()
    }

    #[pyo3(name = "balances")]
    fn py_balances(&self) -> IndexMap<Currency, AccountBalance> {
        Account::balances(self)
    }

    #[pyo3(name = "starting_balances")]
    fn py_starting_balances(&self) -> IndexMap<Currency, Money> {
        Account::starting_balances(self)
    }

    #[pyo3(name = "currencies")]
    fn py_currencies(&self) -> Vec<Currency> {
        Account::currencies(self)
    }

    #[pyo3(name = "is_cash_account")]
    fn py_is_cash_account(&self) -> bool {
        Account::is_cash_account(self)
    }

    #[pyo3(name = "is_margin_account")]
    fn py_is_margin_account(&self) -> bool {
        Account::is_margin_account(self)
    }

    #[pyo3(name = "apply")]
    fn py_apply(&mut self, event: AccountState) -> PyResult<()> {
        Account::apply(self, event).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "purge_account_events")]
    fn py_purge_account_events(&mut self, ts_now: u64, lookback_secs: u64) {
        Account::purge_account_events(self, UnixNanos::from(ts_now), lookback_secs);
    }

    #[pyo3(name = "calculate_balance_locked")]
    #[pyo3(signature = (instrument, side, quantity, price, use_quote_for_inverse=None))]
    fn py_calculate_balance_locked(
        &mut self,
        instrument: Py<PyAny>,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
        py: Python,
    ) -> PyResult<Money> {
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        Account::calculate_balance_locked(
            self,
            &instrument,
            side,
            quantity,
            price,
            use_quote_for_inverse,
        )
        .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "calculate_commission")]
    #[pyo3(signature = (instrument, last_qty, last_px, liquidity_side, use_quote_for_inverse=None))]
    fn py_calculate_commission(
        &self,
        instrument: Py<PyAny>,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: Option<bool>,
        py: Python,
    ) -> PyResult<Money> {
        if liquidity_side == LiquiditySide::NoLiquiditySide {
            return Err(to_pyvalue_err("Invalid liquidity side"));
        }
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        Account::calculate_commission(
            self,
            &instrument,
            last_qty,
            last_px,
            liquidity_side,
            use_quote_for_inverse,
        )
        .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "calculate_pnls")]
    #[pyo3(signature = (instrument, fill, position=None))]
    fn py_calculate_pnls(
        &self,
        instrument: Py<PyAny>,
        fill: OrderFilled,
        position: Option<Position>,
        py: Python,
    ) -> PyResult<Vec<Money>> {
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        Account::calculate_pnls(self, &instrument, &fill, position).map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!(
            "{}(id={}, type={}, base={})",
            stringify!(MarginAccount),
            self.id,
            self.account_type,
            self.base_currency.map_or_else(
                || "None".to_string(),
                |base_currency| format!("{}", base_currency.code)
            ),
        )
    }

    /// Sets the default leverage for the account.
    #[pyo3(name = "set_default_leverage")]
    fn py_set_default_leverage(&mut self, default_leverage: Decimal) {
        self.set_default_leverage(default_leverage);
    }

    #[pyo3(name = "leverages")]
    fn py_leverages(&self, py: Python) -> PyResult<Py<PyAny>> {
        let leverages = PyDict::new(py);
        for (key, &value) in &self.leverages {
            leverages
                .set_item(key.into_py_any_unwrap(py), value)
                .unwrap();
        }
        leverages.into_py_any(py)
    }

    #[pyo3(name = "leverage")]
    fn py_leverage(&self, instrument_id: &InstrumentId) -> Decimal {
        self.get_leverage(instrument_id)
    }

    /// Sets the leverage for a specific instrument.
    #[pyo3(name = "set_leverage")]
    fn py_set_leverage(&mut self, instrument_id: InstrumentId, leverage: Decimal) {
        self.set_leverage(instrument_id, leverage);
    }

    #[pyo3(name = "is_unleveraged")]
    fn py_is_unleveraged(&self, instrument_id: InstrumentId) -> bool {
        self.is_unleveraged(instrument_id)
    }

    #[pyo3(name = "initial_margins")]
    fn py_initial_margins(&self, py: Python) -> PyResult<Py<PyAny>> {
        let initial_margins = PyDict::new(py);
        for (key, &value) in &self.initial_margins() {
            initial_margins
                .set_item(key.into_py_any_unwrap(py), value.into_py_any_unwrap(py))
                .unwrap();
        }
        initial_margins.into_py_any(py)
    }

    #[pyo3(name = "maintenance_margins")]
    fn py_maintenance_margins(&self, py: Python) -> PyResult<Py<PyAny>> {
        let maintenance_margins = PyDict::new(py);
        for (key, &value) in &self.maintenance_margins() {
            maintenance_margins
                .set_item(key.into_py_any_unwrap(py), value.into_py_any_unwrap(py))
                .unwrap();
        }
        maintenance_margins.into_py_any(py)
    }

    /// Updates the initial margin for the specified instrument.
    #[pyo3(name = "update_initial_margin")]
    fn py_update_initial_margin(&mut self, instrument_id: InstrumentId, initial_margin: Money) {
        self.update_initial_margin(instrument_id, initial_margin);
    }

    /// Returns the initial margin amount for the specified instrument.
    #[pyo3(name = "initial_margin")]
    fn py_initial_margin(&self, instrument_id: InstrumentId) -> Money {
        self.initial_margin(instrument_id)
    }

    /// Updates the maintenance margin for the specified instrument.
    #[pyo3(name = "update_maintenance_margin")]
    fn py_update_maintenance_margin(
        &mut self,
        instrument_id: InstrumentId,
        maintenance_margin: Money,
    ) {
        self.update_maintenance_margin(instrument_id, maintenance_margin);
    }

    /// Returns the maintenance margin amount for the specified instrument.
    #[pyo3(name = "maintenance_margin")]
    fn py_maintenance_margin(&self, instrument_id: InstrumentId) -> Money {
        self.maintenance_margin(instrument_id)
    }

    #[pyo3(name = "calculate_initial_margin")]
    #[pyo3(signature = (instrument, quantity, price, use_quote_for_inverse=None))]
    /// Calculates the initial margin amount for the specified instrument and quantity.
    ///
    /// Delegates to the configured `MarginModel`.
    pub fn py_calculate_initial_margin(
        &mut self,
        instrument: Py<PyAny>,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
        py: Python,
    ) -> PyResult<Money> {
        let instrument_type = pyobject_to_instrument_any(py, instrument)?;
        match instrument_type {
            InstrumentAny::Betting(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::BinaryOption(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::Cfd(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::Commodity(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::CryptoFuture(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::CryptoOption(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::CryptoPerpetual(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::CurrencyPair(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::Equity(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::FuturesContract(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::FuturesSpread(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::IndexInstrument(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::OptionContract(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::OptionSpread(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::PerpetualContract(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::TokenizedAsset(inst) => self
                .calculate_initial_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
        }
    }

    /// Calculates the maintenance margin amount for the specified instrument and quantity.
    ///
    /// Delegates to the configured `MarginModel`.
    #[pyo3(name = "calculate_maintenance_margin")]
    #[pyo3(signature = (instrument, quantity, price, use_quote_for_inverse=None))]
    pub fn py_calculate_maintenance_margin(
        &mut self,
        instrument: Py<PyAny>,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
        py: Python,
    ) -> PyResult<Money> {
        let instrument_type = pyobject_to_instrument_any(py, instrument)?;
        match instrument_type {
            InstrumentAny::Betting(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::BinaryOption(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::Cfd(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::Commodity(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::CryptoFuture(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::CryptoOption(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::CryptoPerpetual(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::CurrencyPair(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::Equity(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::FuturesContract(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::FuturesSpread(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::IndexInstrument(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::OptionContract(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::OptionSpread(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::PerpetualContract(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
            InstrumentAny::TokenizedAsset(inst) => self
                .calculate_maintenance_margin(&inst, quantity, price, use_quote_for_inverse)
                .map_err(to_pyvalue_err),
        }
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("calculate_account_state", self.calculate_account_state)?;
        let events_list: PyResult<Vec<Py<PyAny>>> =
            self.events.iter().map(|item| item.py_to_dict(py)).collect();
        dict.set_item("events", events_list.unwrap())?;
        Ok(dict.into())
    }
}
