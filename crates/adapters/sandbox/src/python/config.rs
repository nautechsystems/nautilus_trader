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

//! Python bindings for sandbox configuration.

use nautilus_execution::{
    models::{fee::FeeModelAny, fill::FillModelAny},
    python::{
        fee::{fee_model_any_to_pyobject, pyobject_to_fee_model_any},
        fill::{fill_model_any_to_pyobject, pyobject_to_fill_model_any},
    },
};
use nautilus_model::{
    enums::{AccountType, BookType, OmsType},
    identifiers::{AccountId, Venue},
    types::{Currency, Money},
};
use pyo3::{Py, PyAny, Python, prelude::*};
use rust_decimal::Decimal;

use crate::config::SandboxExecutionClientConfig;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SandboxExecutionClientConfig {
    /// Configuration for `SandboxExecutionClient` instances.
    #[new]
    #[pyo3(signature = (venue, starting_balances, account_id=None, base_currency=None, oms_type=None, account_type=None, default_leverage=None, book_type=None, frozen_account=false, bar_execution=true, trade_execution=true, reject_stop_orders=true, support_gtd_orders=true, support_contingent_orders=true, use_position_ids=true, use_random_ids=false, use_reduce_only=true, fee_model=None, fill_model=None, queue_position=false, liquidity_consumption=false, bar_adaptive_high_low_ordering=false, use_market_order_acks=false, oto_full_trigger=false, price_protection_points=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        venue: Venue,
        starting_balances: Vec<Money>,
        account_id: Option<AccountId>,
        base_currency: Option<Currency>,
        oms_type: Option<OmsType>,
        account_type: Option<AccountType>,
        default_leverage: Option<Decimal>,
        book_type: Option<BookType>,
        frozen_account: bool,
        bar_execution: bool,
        trade_execution: bool,
        reject_stop_orders: bool,
        support_gtd_orders: bool,
        support_contingent_orders: bool,
        use_position_ids: bool,
        use_random_ids: bool,
        use_reduce_only: bool,
        fee_model: Option<Py<PyAny>>,
        fill_model: Option<Py<PyAny>>,
        queue_position: bool,
        liquidity_consumption: bool,
        bar_adaptive_high_low_ordering: bool,
        use_market_order_acks: bool,
        oto_full_trigger: bool,
        price_protection_points: Option<u32>,
    ) -> PyResult<Self> {
        // Generate the default account ID from the venue
        let account_id =
            account_id.unwrap_or_else(|| AccountId::from(format!("{venue}-SANDBOX-001").as_str()));
        let fee_model: Option<FeeModelAny> = fee_model
            .map(|obj| Python::attach(|py| pyobject_to_fee_model_any(obj.bind(py))))
            .transpose()?;
        let fill_model: Option<FillModelAny> = fill_model
            .map(|obj| Python::attach(|py| pyobject_to_fill_model_any(obj.bind(py))))
            .transpose()?;

        Ok(Self {
            account_id,
            venue,
            starting_balances,
            base_currency,
            oms_type: oms_type.unwrap_or(OmsType::Netting),
            account_type: account_type.unwrap_or(AccountType::Margin),
            default_leverage: default_leverage.unwrap_or(Decimal::ONE),
            leverages: ahash::AHashMap::new(),
            book_type: book_type.unwrap_or(BookType::L1_MBP),
            fee_model,
            fill_model,
            frozen_account,
            bar_execution,
            trade_execution,
            reject_stop_orders,
            support_gtd_orders,
            support_contingent_orders,
            use_position_ids,
            use_random_ids,
            use_reduce_only,
            queue_position,
            liquidity_consumption,
            bar_adaptive_high_low_ordering,
            use_market_order_acks,
            oto_full_trigger,
            price_protection_points: price_protection_points.unwrap_or(0),
        })
    }

    #[getter]
    fn account_id(&self) -> AccountId {
        self.account_id
    }

    #[getter]
    fn venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    fn starting_balances(&self) -> Vec<Money> {
        self.starting_balances.clone()
    }

    #[getter]
    fn base_currency(&self) -> Option<Currency> {
        self.base_currency
    }

    #[getter]
    fn oms_type(&self) -> OmsType {
        self.oms_type
    }

    #[getter]
    fn account_type(&self) -> AccountType {
        self.account_type
    }

    #[getter]
    fn default_leverage(&self) -> Decimal {
        self.default_leverage
    }

    #[getter]
    fn book_type(&self) -> BookType {
        self.book_type
    }

    #[getter]
    fn fee_model(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.fee_model
            .as_ref()
            .map(|model| fee_model_any_to_pyobject(py, model))
            .transpose()
    }

    #[getter]
    fn fill_model(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.fill_model
            .as_ref()
            .map(|model| fill_model_any_to_pyobject(py, model))
            .transpose()
    }

    #[getter]
    fn frozen_account(&self) -> bool {
        self.frozen_account
    }

    #[getter]
    fn bar_execution(&self) -> bool {
        self.bar_execution
    }

    #[getter]
    fn trade_execution(&self) -> bool {
        self.trade_execution
    }

    #[getter]
    fn reject_stop_orders(&self) -> bool {
        self.reject_stop_orders
    }

    #[getter]
    fn support_gtd_orders(&self) -> bool {
        self.support_gtd_orders
    }

    #[getter]
    fn support_contingent_orders(&self) -> bool {
        self.support_contingent_orders
    }

    #[getter]
    fn use_position_ids(&self) -> bool {
        self.use_position_ids
    }

    #[getter]
    fn use_random_ids(&self) -> bool {
        self.use_random_ids
    }

    #[getter]
    fn use_reduce_only(&self) -> bool {
        self.use_reduce_only
    }

    #[getter]
    fn queue_position(&self) -> bool {
        self.queue_position
    }

    #[getter]
    fn liquidity_consumption(&self) -> bool {
        self.liquidity_consumption
    }

    #[getter]
    fn bar_adaptive_high_low_ordering(&self) -> bool {
        self.bar_adaptive_high_low_ordering
    }

    #[getter]
    fn use_market_order_acks(&self) -> bool {
        self.use_market_order_acks
    }

    #[getter]
    fn oto_full_trigger(&self) -> bool {
        self.oto_full_trigger
    }

    #[getter]
    fn price_protection_points(&self) -> u32 {
        self.price_protection_points
    }
}
