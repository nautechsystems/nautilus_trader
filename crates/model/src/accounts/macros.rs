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

//! Provides macros for generating shared account functionality.

// Emits the `Account` members every account answers from its `BaseAccount`, for a type that
// derefs to one. Members carrying account-specific semantics stay written out per account:
// `is_cash_account`, `is_margin_account`, `apply`, `calculate_balance_locked`, and
// `calculate_pnls`.
macro_rules! impl_account_base_members {
    () => {
        fn id(&self) -> $crate::identifiers::AccountId {
            self.id
        }

        fn account_type(&self) -> $crate::enums::AccountType {
            self.account_type
        }

        fn base_currency(&self) -> Option<$crate::types::Currency> {
            self.base_currency
        }

        fn calculated_account_state(&self) -> bool {
            self.calculate_account_state
        }

        fn balance_total(
            &self,
            currency: Option<$crate::types::Currency>,
        ) -> Option<$crate::types::Money> {
            self.base_balance_total(currency)
        }

        fn balances_total(
            &self,
        ) -> ::indexmap::IndexMap<$crate::types::Currency, $crate::types::Money> {
            self.base_balances_total()
        }

        fn balance_free(
            &self,
            currency: Option<$crate::types::Currency>,
        ) -> Option<$crate::types::Money> {
            self.base_balance_free(currency)
        }

        fn balances_free(
            &self,
        ) -> ::indexmap::IndexMap<$crate::types::Currency, $crate::types::Money> {
            self.base_balances_free()
        }

        fn balance_locked(
            &self,
            currency: Option<$crate::types::Currency>,
        ) -> Option<$crate::types::Money> {
            self.base_balance_locked(currency)
        }

        fn balances_locked(
            &self,
        ) -> ::indexmap::IndexMap<$crate::types::Currency, $crate::types::Money> {
            self.base_balances_locked()
        }

        fn balance(
            &self,
            currency: Option<$crate::types::Currency>,
        ) -> Option<&$crate::types::AccountBalance> {
            self.base_balance(currency)
        }

        fn last_event(&self) -> Option<$crate::events::AccountState> {
            self.base_last_event()
        }

        fn events(&self) -> Vec<$crate::events::AccountState> {
            self.events.clone()
        }

        fn event_count(&self) -> usize {
            self.events.len()
        }

        fn currencies(&self) -> Vec<$crate::types::Currency> {
            self.balances.keys().copied().collect()
        }

        fn starting_balances(
            &self,
        ) -> ::indexmap::IndexMap<$crate::types::Currency, $crate::types::Money> {
            self.balances_starting.clone()
        }

        fn balances(
            &self,
        ) -> ::indexmap::IndexMap<$crate::types::Currency, $crate::types::AccountBalance> {
            self.balances.clone()
        }

        fn purge_account_events(&mut self, ts_now: ::nautilus_core::UnixNanos, lookback_secs: u64) {
            self.base.base_purge_account_events(ts_now, lookback_secs);
        }

        fn calculate_commission(
            &self,
            instrument: &$crate::instruments::InstrumentAny,
            last_qty: $crate::types::Quantity,
            last_px: $crate::types::Price,
            liquidity_side: $crate::enums::LiquiditySide,
            use_quote_for_inverse: Option<bool>,
        ) -> anyhow::Result<$crate::types::Money> {
            self.base_calculate_commission(
                instrument,
                last_qty,
                last_px,
                liquidity_side,
                use_quote_for_inverse,
            )
        }
    };
}
