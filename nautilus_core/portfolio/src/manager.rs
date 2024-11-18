// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

//! Provides account management functionality.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, clock::Clock};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    accounts::{any::AccountAny, cash::CashAccount, margin::MarginAccount},
    enums::OrderSideSpecified,
    events::{account::state::AccountState, order::OrderFilled},
    instruments::any::InstrumentAny,
    orders::any::OrderAny,
    position::Position,
    types::money::Money,
};
use rust_decimal::Decimal;

pub struct AccountsManager {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
}

impl AccountsManager {
    #[must_use]
    pub fn update_balances(
        &self,
        account: AccountAny,
        instrument: InstrumentAny,
        fill: OrderFilled,
    ) -> AccountState {
        todo!()
    }

    #[must_use]
    pub fn update_orders(
        &self,
        account: AccountAny,
        instrument: InstrumentAny,
        orders_open: &[OrderAny],
        ts_event: UnixNanos,
    ) -> AccountState {
        todo!()
    }

    #[must_use]
    pub fn update_positions(
        &self,
        account: MarginAccount,
        instrument: InstrumentAny,
        positions: &[Position],
        ts_event: UnixNanos,
    ) -> AccountState {
        todo!()
    }

    fn update_balance_locked(
        &self,
        account: CashAccount,
        instrument: InstrumentAny,
        fill: OrderFilled,
    ) -> AccountState {
        todo!()
    }

    fn update_margin_init(
        &self,
        account: MarginAccount,
        instrument: InstrumentAny,
        orders_open: &[OrderAny],
        ts_event: UnixNanos,
    ) -> AccountState {
        todo!()
    }

    fn update_balance_single_currency(&self, account: AccountAny, fill: OrderFilled, pnl: Money) {
        todo!()
    }

    fn update_balance_multi_currency(
        &self,
        account: AccountAny,
        fill: OrderFilled,
        pnls: &[Money],
    ) {
        todo!()
    }

    fn generate_account_state(&self, account: AccountAny, ts_event: UnixNanos) -> AccountState {
        todo!()
    }

    fn calculate_xrate_to_base(
        &self,
        account: AccountAny,
        instrument: InstrumentAny,
        side: OrderSideSpecified,
    ) -> Decimal {
        todo!()
    }
}
