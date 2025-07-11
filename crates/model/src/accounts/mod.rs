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

//! Account types such as `CashAccount` and `MarginAccount`.

pub mod any;
pub mod base;
pub mod cash;
pub mod margin;

#[cfg(any(test, feature = "stubs"))]
pub mod stubs;

use std::collections::HashMap;

use enum_dispatch::enum_dispatch;
use nautilus_core::UnixNanos;

// Re-exports
pub use crate::accounts::{
    any::AccountAny, base::BaseAccount, cash::CashAccount, margin::MarginAccount,
};
use crate::{
    enums::{AccountType, LiquiditySide, OrderSide},
    events::{AccountState, OrderFilled},
    identifiers::AccountId,
    instruments::InstrumentAny,
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

#[enum_dispatch]
pub trait Account: 'static + Send {
    fn id(&self) -> AccountId;
    fn account_type(&self) -> AccountType;
    fn base_currency(&self) -> Option<Currency>;
    fn is_cash_account(&self) -> bool;
    fn is_margin_account(&self) -> bool;
    fn calculated_account_state(&self) -> bool;
    fn balance_total(&self, currency: Option<Currency>) -> Option<Money>;
    fn balances_total(&self) -> HashMap<Currency, Money>;
    fn balance_free(&self, currency: Option<Currency>) -> Option<Money>;
    fn balances_free(&self) -> HashMap<Currency, Money>;
    fn balance_locked(&self, currency: Option<Currency>) -> Option<Money>;
    fn balances_locked(&self) -> HashMap<Currency, Money>;
    fn balance(&self, currency: Option<Currency>) -> Option<&AccountBalance>;
    fn last_event(&self) -> Option<AccountState>;
    fn events(&self) -> Vec<AccountState>;
    fn event_count(&self) -> usize;
    fn currencies(&self) -> Vec<Currency>;
    fn starting_balances(&self) -> HashMap<Currency, Money>;
    fn balances(&self) -> HashMap<Currency, AccountBalance>;
    fn apply(&mut self, event: AccountState);
    fn purge_account_events(&mut self, ts_now: UnixNanos, lookback_secs: u64);

    /// Calculates locked balance for the order parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if calculating locked balance fails.
    fn calculate_balance_locked(
        &mut self,
        instrument: InstrumentAny,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money>;

    /// Calculates PnLs for the fill and position.
    ///
    /// # Errors
    ///
    /// Returns an error if calculating PnLs fails.
    fn calculate_pnls(
        &self,
        instrument: InstrumentAny,
        fill: OrderFilled,
        position: Option<Position>,
    ) -> anyhow::Result<Vec<Money>>;

    /// Calculates commission for the order fill parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if calculating commission fails.
    fn calculate_commission(
        &self,
        instrument: InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money>;
}
