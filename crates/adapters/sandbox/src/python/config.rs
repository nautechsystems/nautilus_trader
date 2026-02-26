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

use nautilus_model::{
    enums::{AccountType, BookType, OmsType},
    identifiers::{AccountId, TraderId, Venue},
    types::{Currency, Money},
};
use pyo3::prelude::*;
use rust_decimal::Decimal;

use crate::config::SandboxExecutionClientConfig;

#[pymethods]
impl SandboxExecutionClientConfig {
    #[new]
    #[pyo3(signature = (venue, starting_balances, trader_id=None, account_id=None, base_currency=None, oms_type=None, account_type=None, default_leverage=None, book_type=None, frozen_account=false, bar_execution=true, trade_execution=true, reject_stop_orders=true, support_gtd_orders=true, support_contingent_orders=true, use_position_ids=true, use_random_ids=false, use_reduce_only=true))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        venue: Venue,
        starting_balances: Vec<Money>,
        trader_id: Option<TraderId>,
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
    ) -> Self {
        // Generate default IDs from venue if not provided
        let trader_id =
            trader_id.unwrap_or_else(|| TraderId::from(format!("{venue}-001").as_str()));
        let account_id =
            account_id.unwrap_or_else(|| AccountId::from(format!("{venue}-SANDBOX-001").as_str()));

        Self {
            trader_id,
            account_id,
            venue,
            starting_balances,
            base_currency,
            oms_type: oms_type.unwrap_or(OmsType::Netting),
            account_type: account_type.unwrap_or(AccountType::Margin),
            default_leverage: default_leverage.unwrap_or(Decimal::ONE),
            leverages: ahash::AHashMap::new(),
            book_type: book_type.unwrap_or(BookType::L1_MBP),
            frozen_account,
            bar_execution,
            trade_execution,
            reject_stop_orders,
            support_gtd_orders,
            support_contingent_orders,
            use_position_ids,
            use_random_ids,
            use_reduce_only,
        }
    }

    #[getter]
    fn trader_id(&self) -> TraderId {
        self.trader_id
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
}
