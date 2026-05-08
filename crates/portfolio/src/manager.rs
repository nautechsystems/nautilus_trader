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

//! Provides account management functionality.

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use ahash::AHashMap;
use nautilus_common::{cache::Cache, clock::Clock};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::{Account, AccountAny, BettingAccount, CashAccount, MarginAccount},
    enums::{AccountType, OrderSide, PriceType},
    events::{AccountState, OrderFilled},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    position::Position,
    types::{AccountBalance, Currency, Money},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

/// Manages account balance updates and calculations for portfolio management.
///
/// The accounts manager handles balance updates for different account types,
/// including cash and margin accounts, based on order fills and position changes.
pub struct AccountsManager {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
}

impl Debug for AccountsManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(AccountsManager)).finish()
    }
}

impl AccountsManager {
    /// Creates a new [`AccountsManager`] instance.
    pub fn new(clock: Rc<RefCell<dyn Clock>>, cache: Rc<RefCell<Cache>>) -> Self {
        Self { clock, cache }
    }

    /// Updates the given account state based on a filled order.
    ///
    /// Mutations are applied to `account` in place so the caller can persist
    /// the recalculated balances and commissions back to the cache.
    ///
    /// # Panics
    ///
    /// Panics if the position list for the filled instrument is empty.
    #[must_use]
    pub fn update_balances(
        &self,
        mut account: AccountAny,
        instrument: &InstrumentAny,
        fill: OrderFilled,
    ) -> (AccountAny, AccountState) {
        let position_id = if let Some(position_id) = fill.position_id {
            position_id
        } else {
            let cache = self.cache.borrow();
            let positions_open = cache.positions_open(
                None,
                Some(&fill.instrument_id),
                None,
                Some(&fill.account_id),
                None,
            );
            positions_open
                .first()
                .unwrap_or_else(|| panic!("List of Positions is empty"))
                .id
        };

        let position = self.cache.borrow().position_owned(&position_id);

        let pnls = account.calculate_pnls(instrument, &fill, position);

        // Calculate final PnL including commissions
        match account.base_currency() {
            Some(base_currency) => {
                let pnl = pnls.map_or_else(
                    |_| Money::new(0.0, base_currency),
                    |pnl_list| {
                        pnl_list
                            .first()
                            .copied()
                            .unwrap_or_else(|| Money::new(0.0, base_currency))
                    },
                );

                self.update_balance_single_currency(&mut account, &fill, pnl);
            }
            None => {
                if let Ok(mut pnl_list) = pnls {
                    self.update_balance_multi_currency(&mut account, fill, &mut pnl_list);
                }
            }
        }

        let state = self.generate_account_state(&account, fill.ts_event);
        (account, state)
    }

    /// Updates account balances based on open orders.
    ///
    /// For cash accounts, updates the balance locked by open orders.
    /// For margin accounts, updates the initial margin requirements.
    #[must_use]
    pub fn update_orders(
        &self,
        account: &AccountAny,
        instrument: &InstrumentAny,
        orders_open: &[&OrderAny],
        ts_event: UnixNanos,
    ) -> Option<(AccountAny, AccountState)> {
        let mut account = account.clone();
        self.update_orders_in_place(&mut account, instrument, orders_open, ts_event)
            .map(|state| (account, state))
    }

    /// Updates account balances based on open orders in place.
    ///
    /// For cash accounts, updates the balance locked by open orders.
    /// For margin accounts, updates the initial margin requirements.
    #[must_use]
    pub fn update_orders_in_place(
        &self,
        account: &mut AccountAny,
        instrument: &InstrumentAny,
        orders_open: &[&OrderAny],
        ts_event: UnixNanos,
    ) -> Option<AccountState> {
        match account {
            AccountAny::Margin(margin_account) => {
                self.update_margin_init(margin_account, instrument, orders_open, ts_event)
            }
            AccountAny::Cash(cash_account) => {
                self.update_balance_locked(cash_account, instrument, orders_open, ts_event)
            }
            AccountAny::Betting(betting_account) => self.update_balance_locked_betting(
                betting_account,
                instrument,
                orders_open,
                ts_event,
            ),
        }
    }

    /// Updates the account based on current open positions.
    ///
    /// # Panics
    ///
    /// Panics if any position's `instrument_id` does not match the provided `instrument`.
    #[must_use]
    pub fn update_positions(
        &self,
        account: &MarginAccount,
        instrument: &InstrumentAny,
        positions: Vec<&Position>,
        ts_event: UnixNanos,
    ) -> Option<(MarginAccount, AccountState)> {
        let mut account = account.clone();
        self.update_positions_in_place(&mut account, instrument, positions, ts_event)
            .map(|state| (account, state))
    }

    /// Updates the account based on current open positions in place.
    ///
    /// # Panics
    ///
    /// Panics if any position's `instrument_id` does not match the provided `instrument`.
    #[must_use]
    pub fn update_positions_in_place(
        &self,
        account: &mut MarginAccount,
        instrument: &InstrumentAny,
        positions: Vec<&Position>,
        ts_event: UnixNanos,
    ) -> Option<AccountState> {
        let mut total_margin_maint = 0.0;
        let mut base_xrate: Option<f64> = None;
        let mut currency = instrument.settlement_currency();

        for position in positions {
            assert_eq!(
                position.instrument_id,
                instrument.id(),
                "Position not for instrument {}",
                instrument.id()
            );

            if !position.is_open() {
                continue;
            }

            let margin_maint = match instrument {
                InstrumentAny::Betting(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::BinaryOption(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::Cfd(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::Commodity(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::CryptoFuture(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::CryptoOption(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::CryptoPerpetual(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::CurrencyPair(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::Equity(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::FuturesContract(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::FuturesSpread(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::IndexInstrument(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::OptionContract(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::OptionSpread(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::PerpetualContract(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
                InstrumentAny::TokenizedAsset(i) => account
                    .calculate_maintenance_margin(
                        i,
                        position.quantity,
                        instrument.make_price(position.avg_px_open),
                        None,
                    )
                    .ok()?,
            };

            let mut margin_maint = margin_maint.as_f64();

            if let Some(base_currency) = account.base_currency {
                if base_xrate.is_none() {
                    currency = base_currency;
                    base_xrate = self.calculate_xrate_to_base(account.base_currency, instrument);
                }

                if let Some(xrate) = base_xrate {
                    margin_maint *= xrate;
                } else {
                    log::debug!(
                        "Cannot calculate maintenance (position) margin: insufficient data for {}/{}",
                        instrument.settlement_currency(),
                        base_currency
                    );
                    return None;
                }
            }

            total_margin_maint += margin_maint;
        }

        let margin_maint = Money::new(total_margin_maint, currency);
        account.update_maintenance_margin(instrument.id(), margin_maint);

        log::info!("{} margin_maint={margin_maint}", instrument.id());

        // Generate and return account state
        Some(self.generate_margin_account_state(account, ts_event))
    }

    fn update_balance_locked(
        &self,
        account: &mut CashAccount,
        instrument: &InstrumentAny,
        orders_open: &[&OrderAny],
        ts_event: UnixNanos,
    ) -> Option<AccountState> {
        if orders_open.is_empty() {
            account.clear_balance_locked(instrument.id());
            return Some(self.generate_cash_account_state(account, ts_event));
        }

        let mut total_locked: AHashMap<Currency, Money> = AHashMap::new();
        let mut base_xrate: Option<f64> = None;

        let mut currency = instrument.settlement_currency();

        for order in orders_open {
            assert_eq!(
                order.instrument_id(),
                instrument.id(),
                "Order not for instrument {}",
                instrument.id()
            );
            assert!(order.is_open(), "Order is not open");

            if order.price().is_none() && order.trigger_price().is_none() {
                continue;
            }

            if order.is_reduce_only() {
                continue; // Does not contribute to locked balance
            }

            let price = if order.price().is_some() {
                order.price()
            } else {
                order.trigger_price()
            };

            let mut locked = account
                .calculate_balance_locked(
                    instrument,
                    order.order_side(),
                    order.quantity(),
                    price?,
                    None,
                )
                .unwrap();

            if let Some(base_curr) = account.base_currency() {
                if base_xrate.is_none() {
                    currency = base_curr;
                    base_xrate = self.calculate_xrate_to_base(account.base_currency(), instrument);
                }

                if let Some(xrate) = base_xrate {
                    locked = Money::new(locked.as_f64() * xrate, currency);
                } else {
                    log::error!(
                        "Cannot calculate balance locked: insufficient data for {}/{}",
                        instrument.settlement_currency(),
                        base_curr
                    );
                    return None;
                }
            }

            total_locked
                .entry(locked.currency)
                .and_modify(|total| *total = *total + locked)
                .or_insert(locked);
        }

        if total_locked.is_empty() {
            account.clear_balance_locked(instrument.id());
            return Some(self.generate_cash_account_state(account, ts_event));
        }

        // Clear existing locks before applying new ones to remove stale currency entries
        account.clear_balance_locked(instrument.id());

        for (_, balance_locked) in total_locked {
            account.update_balance_locked(instrument.id(), balance_locked);
            log::info!("{} balance_locked={balance_locked}", instrument.id());
        }

        Some(self.generate_cash_account_state(account, ts_event))
    }

    fn update_margin_init(
        &self,
        account: &mut MarginAccount,
        instrument: &InstrumentAny,
        orders_open: &[&OrderAny],
        ts_event: UnixNanos,
    ) -> Option<AccountState> {
        let mut total_margin_init = 0.0;
        let mut base_xrate: Option<f64> = None;
        let mut currency = instrument.settlement_currency();

        for order in orders_open {
            assert_eq!(
                order.instrument_id(),
                instrument.id(),
                "Order not for instrument {}",
                instrument.id()
            );

            if !order.is_open() || (order.price().is_none() && order.trigger_price().is_none()) {
                continue;
            }

            if order.is_reduce_only() {
                continue; // Does not contribute to margin
            }

            let price = if order.price().is_some() {
                order.price()
            } else {
                order.trigger_price()
            };

            let margin_init = match instrument {
                InstrumentAny::Betting(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::BinaryOption(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::Cfd(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::Commodity(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::CryptoFuture(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::CryptoOption(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::CryptoPerpetual(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::CurrencyPair(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::Equity(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::FuturesContract(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::FuturesSpread(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::IndexInstrument(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::OptionContract(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::OptionSpread(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::PerpetualContract(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::TokenizedAsset(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
            };

            let mut margin_init = margin_init.as_f64();

            if let Some(base_currency) = account.base_currency {
                if base_xrate.is_none() {
                    currency = base_currency;
                    base_xrate = self.calculate_xrate_to_base(account.base_currency, instrument);
                }

                if let Some(xrate) = base_xrate {
                    margin_init *= xrate;
                } else {
                    log::debug!(
                        "Cannot calculate initial margin: insufficient data for {}/{}",
                        instrument.settlement_currency(),
                        base_currency
                    );
                    continue;
                }
            }

            total_margin_init += margin_init;
        }

        let money = Money::new(total_margin_init, currency);
        let margin_init = {
            account.update_initial_margin(instrument.id(), money);
            money
        };

        log::info!("{} margin_init={margin_init}", instrument.id());

        Some(self.generate_margin_account_state(account, ts_event))
    }

    fn update_balance_locked_betting(
        &self,
        account: &mut BettingAccount,
        instrument: &InstrumentAny,
        orders_open: &[&OrderAny],
        ts_event: UnixNanos,
    ) -> Option<AccountState> {
        if orders_open.is_empty() {
            account.clear_balance_locked(instrument.id());
            return Some(self.generate_betting_account_state(account, ts_event));
        }

        let mut total_locked: AHashMap<Currency, Money> = AHashMap::new();
        let mut base_xrate: Option<f64> = None;
        let mut currency = instrument.settlement_currency();

        for order in orders_open {
            assert_eq!(
                order.instrument_id(),
                instrument.id(),
                "Order not for instrument {}",
                instrument.id()
            );
            assert!(order.is_open(), "Order is not open");

            if order.price().is_none() && order.trigger_price().is_none() {
                continue;
            }

            if order.is_reduce_only() {
                continue;
            }

            let price = if order.price().is_some() {
                order.price()
            } else {
                order.trigger_price()
            };

            let mut locked = account
                .calculate_balance_locked(
                    instrument,
                    order.order_side(),
                    order.quantity(),
                    price?,
                    None,
                )
                .unwrap();

            if let Some(base_curr) = account.base_currency() {
                if base_xrate.is_none() {
                    currency = base_curr;
                    base_xrate = self.calculate_xrate_to_base(account.base_currency(), instrument);
                }

                if let Some(xrate) = base_xrate {
                    locked = Money::new(locked.as_f64() * xrate, currency);
                } else {
                    log::error!(
                        "Cannot calculate balance locked: insufficient data for {}/{}",
                        instrument.settlement_currency(),
                        base_curr
                    );
                    return None;
                }
            }

            total_locked
                .entry(locked.currency)
                .and_modify(|total| *total = *total + locked)
                .or_insert(locked);
        }

        if total_locked.is_empty() {
            account.clear_balance_locked(instrument.id());
            return Some(self.generate_betting_account_state(account, ts_event));
        }

        account.clear_balance_locked(instrument.id());

        for (_, balance_locked) in total_locked {
            account.update_balance_locked(instrument.id(), balance_locked);
            log::info!("{} balance_locked={balance_locked}", instrument.id());
        }

        Some(self.generate_betting_account_state(account, ts_event))
    }

    fn update_balance_single_currency(
        &self,
        account: &mut AccountAny,
        fill: &OrderFilled,
        mut pnl: Money,
    ) {
        let base_currency = if let Some(currency) = account.base_currency() {
            currency
        } else {
            log::error!("Account has no base currency set");
            return;
        };

        let mut balances = Vec::new();
        let mut commission = fill.commission;

        if let Some(ref mut comm) = commission
            && comm.currency != base_currency
        {
            let xrate = self.cache.borrow().get_xrate(
                fill.instrument_id.venue,
                comm.currency,
                base_currency,
                if fill.order_side == OrderSide::Sell {
                    PriceType::Bid
                } else {
                    PriceType::Ask
                },
            );

            if let Some(xrate) = xrate {
                *comm = Money::new(comm.as_f64() * xrate, base_currency);
            } else {
                log::error!(
                    "Cannot calculate account state: insufficient data for {}/{}",
                    comm.currency,
                    base_currency
                );
                return;
            }
        }

        if pnl.currency != base_currency {
            let xrate = self.cache.borrow().get_xrate(
                fill.instrument_id.venue,
                pnl.currency,
                base_currency,
                if fill.order_side == OrderSide::Sell {
                    PriceType::Bid
                } else {
                    PriceType::Ask
                },
            );

            if let Some(xrate) = xrate {
                pnl = Money::new(pnl.as_f64() * xrate, base_currency);
            } else {
                log::error!(
                    "Cannot calculate account state: insufficient data for {}/{}",
                    pnl.currency,
                    base_currency
                );
                return;
            }
        }

        if let Some(comm) = commission {
            pnl = pnl - comm;
        }

        if pnl.is_zero() {
            return;
        }

        let existing_balances = account.balances();
        let balance = if let Some(b) = existing_balances.get(&pnl.currency) {
            b
        } else {
            log::error!(
                "Cannot complete transaction: no balance for {}",
                pnl.currency
            );
            return;
        };

        let new_balance =
            AccountBalance::new(balance.total + pnl, balance.locked, balance.free + pnl);
        balances.push(new_balance);

        match account {
            AccountAny::Margin(margin) => {
                margin.update_balances(&balances);

                if let Some(comm) = commission {
                    margin.update_commissions(comm);
                }
            }
            AccountAny::Cash(cash) => {
                if let Err(e) = cash.update_balances(&balances) {
                    log::error!("Cannot update cash account balance: {e}");
                    return;
                }

                if let Some(comm) = commission {
                    cash.update_commissions(comm);
                }
            }
            AccountAny::Betting(betting) => {
                if let Err(e) = betting.update_balances(&balances) {
                    log::error!("Cannot update betting account balance: {e}");
                    return;
                }

                if let Some(comm) = commission {
                    betting.update_commissions(comm);
                }
            }
        }
    }

    fn update_balance_multi_currency(
        &self,
        account: &mut AccountAny,
        fill: OrderFilled,
        pnls: &mut [Money],
    ) {
        let mut new_balances = Vec::new();
        let commission = fill.commission;
        let mut apply_commission = commission.is_some_and(|c| !c.is_zero());

        for pnl in pnls.iter_mut() {
            if apply_commission && pnl.currency == commission.unwrap().currency {
                *pnl = *pnl - commission.unwrap();
                apply_commission = false;
            }

            if pnl.is_zero() {
                continue; // No Adjustment
            }

            let currency = pnl.currency;
            let balances = account.balances();

            let new_balance = if let Some(balance) = balances.get(&currency) {
                let new_total = balance.total.as_f64() + pnl.as_f64();
                let new_free = balance.free.as_f64() + pnl.as_f64();
                let total = Money::new(new_total, currency);
                let free = Money::new(new_free, currency);

                AccountBalance::new(total, balance.locked, free)
            } else {
                // Mirrors Python `_update_balance_multi_currency`: a fill that
                // would open a new debit currency on a non-seeded account is
                // rejected even when `allow_cash_borrowing=true`. The
                // existing-currency branch above lets the per-account
                // `update_balances` enforce the borrowing policy, so the two
                // branches are intentionally asymmetric until cross-currency
                // equity tracking lands (see TODO in `manager.pyx`).
                if pnl.as_decimal() < Decimal::ZERO {
                    log::error!(
                        "Cannot complete transaction: no {currency} to deduct a {pnl} realized PnL from"
                    );
                    return;
                }
                AccountBalance::new(*pnl, Money::new(0.0, currency), *pnl)
            };

            new_balances.push(new_balance);
        }

        if apply_commission {
            let commission = commission.unwrap();
            let currency = commission.currency;
            let balances = account.balances();

            let commission_balance = if let Some(balance) = balances.get(&currency) {
                let new_total = balance.total.as_decimal() - commission.as_decimal();
                let new_free = balance.free.as_decimal() - commission.as_decimal();
                AccountBalance::new(
                    Money::new(new_total.to_f64().unwrap(), currency),
                    balance.locked,
                    Money::new(new_free.to_f64().unwrap(), currency),
                )
            } else {
                if commission.as_decimal() > Decimal::ZERO {
                    log::error!(
                        "Cannot complete transaction: no {currency} balance to deduct a {commission} commission from"
                    );
                    return;
                }
                AccountBalance::new(
                    Money::new(0.0, currency),
                    Money::new(0.0, currency),
                    Money::new(0.0, currency),
                )
            };
            new_balances.push(commission_balance);
        }

        if new_balances.is_empty() {
            return;
        }

        match account {
            AccountAny::Margin(margin) => {
                margin.update_balances(&new_balances);

                if let Some(commission) = commission {
                    margin.update_commissions(commission);
                }
            }
            AccountAny::Cash(cash) => {
                if let Err(e) = cash.update_balances(&new_balances) {
                    log::error!("Cannot update cash account balance: {e}");
                    return;
                }

                if let Some(commission) = commission {
                    cash.update_commissions(commission);
                }
            }
            AccountAny::Betting(betting) => {
                if let Err(e) = betting.update_balances(&new_balances) {
                    log::error!("Cannot update betting account balance: {e}");
                    return;
                }

                if let Some(commission) = commission {
                    betting.update_commissions(commission);
                }
            }
        }
    }

    fn generate_account_state(&self, account: &AccountAny, ts_event: UnixNanos) -> AccountState {
        match account {
            AccountAny::Margin(margin_account) => {
                self.generate_margin_account_state(margin_account, ts_event)
            }
            AccountAny::Cash(cash_account) => {
                self.generate_cash_account_state(cash_account, ts_event)
            }
            AccountAny::Betting(betting_account) => {
                self.generate_betting_account_state(betting_account, ts_event)
            }
        }
    }

    fn generate_margin_account_state(
        &self,
        margin_account: &MarginAccount,
        ts_event: UnixNanos,
    ) -> AccountState {
        // Include both per-instrument (`margins`) and account-wide
        // (`account_margins`, keyed by collateral currency) entries so
        // regenerated state events preserve the full margin picture.
        let mut margins: Vec<_> = margin_account.margins.values().copied().collect();
        margins.extend(margin_account.account_margins.values().copied());
        AccountState::new(
            margin_account.id,
            AccountType::Margin,
            margin_account.balances.clone().into_values().collect(),
            margins,
            false,
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            margin_account.base_currency(),
        )
    }

    fn generate_cash_account_state(
        &self,
        cash_account: &CashAccount,
        ts_event: UnixNanos,
    ) -> AccountState {
        AccountState::new(
            cash_account.id,
            AccountType::Cash,
            cash_account.balances.clone().into_values().collect(),
            vec![],
            false,
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            cash_account.base_currency(),
        )
    }

    fn generate_betting_account_state(
        &self,
        betting_account: &BettingAccount,
        ts_event: UnixNanos,
    ) -> AccountState {
        AccountState::new(
            betting_account.id,
            AccountType::Betting,
            betting_account.balances.clone().into_values().collect(),
            vec![],
            false,
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            betting_account.base_currency(),
        )
    }

    fn calculate_xrate_to_base(
        &self,
        base_currency: Option<Currency>,
        instrument: &InstrumentAny,
    ) -> Option<f64> {
        match base_currency {
            None => Some(1.0),
            Some(base_curr) => self.cache.borrow().get_xrate(
                instrument.id().venue,
                instrument.settlement_currency(),
                base_curr,
                PriceType::Mid,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_model::{
        accounts::{BettingAccount, CashAccount, MarginAccount},
        enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderType},
        events::{AccountState, OrderAccepted, OrderEventAny, OrderFilled, OrderSubmitted},
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
            VenueOrderId,
        },
        instruments::{
            Instrument, InstrumentAny,
            stubs::{audusd_sim, betting},
        },
        orders::{OrderAny, OrderTestBuilder},
        position::Position,
        stubs::TestDefault,
        types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_update_balance_locked_with_base_currency_multiple_orders() {
        let usd = Currency::USD();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::new(1_000_000.0, usd),
                Money::new(0.0, usd),
                Money::new(1_000_000.0, usd),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(usd),
        );

        let account = CashAccount::new(account_state, true, false);

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        cache
            .borrow_mut()
            .add_account(AccountAny::Cash(account.clone()))
            .unwrap();

        let manager = AccountsManager::new(clock, cache);

        let instrument = audusd_sim();

        let order1 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .price(Price::from("0.75000"))
            .build();

        let order2 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("50000"))
            .price(Price::from("0.74500"))
            .build();

        let order3 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("75000"))
            .price(Price::from("0.74000"))
            .build();

        let mut order1 = order1;
        let mut order2 = order2;
        let mut order3 = order3;

        let submitted1 = OrderSubmitted::new(
            order1.trader_id(),
            order1.strategy_id(),
            order1.instrument_id(),
            order1.client_order_id(),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let accepted1 = OrderAccepted::new(
            order1.trader_id(),
            order1.strategy_id(),
            order1.instrument_id(),
            order1.client_order_id(),
            order1.venue_order_id().unwrap_or(VenueOrderId::new("1")),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );

        order1.apply(OrderEventAny::Submitted(submitted1)).unwrap();
        order1.apply(OrderEventAny::Accepted(accepted1)).unwrap();

        let submitted2 = OrderSubmitted::new(
            order2.trader_id(),
            order2.strategy_id(),
            order2.instrument_id(),
            order2.client_order_id(),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let accepted2 = OrderAccepted::new(
            order2.trader_id(),
            order2.strategy_id(),
            order2.instrument_id(),
            order2.client_order_id(),
            order2.venue_order_id().unwrap_or(VenueOrderId::new("2")),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );

        order2.apply(OrderEventAny::Submitted(submitted2)).unwrap();
        order2.apply(OrderEventAny::Accepted(accepted2)).unwrap();

        let submitted3 = OrderSubmitted::new(
            order3.trader_id(),
            order3.strategy_id(),
            order3.instrument_id(),
            order3.client_order_id(),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let accepted3 = OrderAccepted::new(
            order3.trader_id(),
            order3.strategy_id(),
            order3.instrument_id(),
            order3.client_order_id(),
            order3.venue_order_id().unwrap_or(VenueOrderId::new("3")),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );

        order3.apply(OrderEventAny::Submitted(submitted3)).unwrap();
        order3.apply(OrderEventAny::Accepted(accepted3)).unwrap();

        let orders: Vec<&OrderAny> = vec![&order1, &order2, &order3];

        let result = manager.update_orders(
            &AccountAny::Cash(account),
            &InstrumentAny::CurrencyPair(instrument),
            &orders,
            UnixNanos::default(),
        );

        assert!(result.is_some());
        let (updated_account, _state) = result.unwrap();

        if let AccountAny::Cash(cash_account) = updated_account {
            let locked_balance = cash_account.balance_locked(Some(usd));

            // Order 1: 100k * 0.75 = 75k, Order 2: 50k * 0.745 = 37.25k, Order 3: 75k * 0.74 = 55.5k
            let expected_locked = Money::new(167_750.0, usd);

            assert_eq!(locked_balance, Some(expected_locked));
            let aud = Currency::AUD();
            assert_eq!(cash_account.balance_locked(Some(aud)), None);
        } else {
            panic!("Expected CashAccount");
        }
    }

    #[rstest]
    fn test_update_orders_betting_account_uses_liability_for_locked_balance() {
        let gbp = Currency::GBP();
        let account_state = AccountState::new(
            AccountId::new("BETTING-001"),
            AccountType::Betting,
            vec![AccountBalance::new(
                Money::new(1_000.0, gbp),
                Money::new(0.0, gbp),
                Money::new(1_000.0, gbp),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(gbp),
        );

        let account = BettingAccount::new(account_state, true);

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        cache
            .borrow_mut()
            .add_account(AccountAny::Betting(account.clone()))
            .unwrap();

        let manager = AccountsManager::new(clock, cache);
        let instrument = betting();

        let mut back_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10"))
            .price(Price::from("1.25"))
            .build();

        let mut lay_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("12"))
            .price(Price::from("3.00"))
            .build();

        let submitted_back = OrderSubmitted::new(
            back_order.trader_id(),
            back_order.strategy_id(),
            back_order.instrument_id(),
            back_order.client_order_id(),
            AccountId::new("BETTING-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let accepted_back = OrderAccepted::new(
            back_order.trader_id(),
            back_order.strategy_id(),
            back_order.instrument_id(),
            back_order.client_order_id(),
            VenueOrderId::new("B1"),
            AccountId::new("BETTING-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        back_order
            .apply(OrderEventAny::Submitted(submitted_back))
            .unwrap();
        back_order
            .apply(OrderEventAny::Accepted(accepted_back))
            .unwrap();

        let submitted_lay = OrderSubmitted::new(
            lay_order.trader_id(),
            lay_order.strategy_id(),
            lay_order.instrument_id(),
            lay_order.client_order_id(),
            AccountId::new("BETTING-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let accepted_lay = OrderAccepted::new(
            lay_order.trader_id(),
            lay_order.strategy_id(),
            lay_order.instrument_id(),
            lay_order.client_order_id(),
            VenueOrderId::new("L1"),
            AccountId::new("BETTING-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        lay_order
            .apply(OrderEventAny::Submitted(submitted_lay))
            .unwrap();
        lay_order
            .apply(OrderEventAny::Accepted(accepted_lay))
            .unwrap();

        let orders: Vec<&OrderAny> = vec![&back_order, &lay_order];
        let result = manager.update_orders(
            &AccountAny::Betting(account),
            &InstrumentAny::Betting(instrument),
            &orders,
            UnixNanos::default(),
        );

        assert!(result.is_some());
        let (updated_account, state) = result.unwrap();

        if let AccountAny::Betting(betting_account) = updated_account {
            assert_eq!(
                betting_account.balance_locked(Some(gbp)),
                Some(Money::new(14.5, gbp))
            );
            assert_eq!(
                betting_account.balance_free(Some(gbp)),
                Some(Money::new(985.5, gbp))
            );
            assert_eq!(state.account_type, AccountType::Betting);
        } else {
            panic!("Expected BettingAccount");
        }
    }

    #[rstest]
    fn test_betting_order_canceled_releases_locked_balance() {
        let gbp = Currency::GBP();
        let account_state = AccountState::new(
            AccountId::new("BETFAIR-001"),
            AccountType::Betting,
            vec![AccountBalance::new(
                Money::new(1_000.0, gbp),
                Money::new(0.0, gbp),
                Money::new(1_000.0, gbp),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(gbp),
        );

        let account = BettingAccount::new(account_state, true);

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        cache
            .borrow_mut()
            .add_account(AccountAny::Betting(account.clone()))
            .unwrap();

        let manager = AccountsManager::new(clock, cache);
        let instrument = betting();

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10"))
            .price(Price::from("5.0"))
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::new("BETFAIR-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            VenueOrderId::new("B2"),
            AccountId::new("BETFAIR-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );

        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        let result = manager.update_orders(
            &AccountAny::Betting(account),
            &InstrumentAny::Betting(instrument.clone()),
            &[&order],
            UnixNanos::default(),
        );

        assert!(result.is_some());
        let (updated_account, _) = result.unwrap();

        if let AccountAny::Betting(ref betting_account) = updated_account {
            assert_eq!(
                betting_account.balance_locked(Some(gbp)),
                Some(Money::new(40.0, gbp))
            );
            assert_eq!(
                betting_account.balance_free(Some(gbp)),
                Some(Money::new(960.0, gbp))
            );
        } else {
            panic!("Expected BettingAccount");
        }

        let result = manager.update_orders(
            &updated_account,
            &InstrumentAny::Betting(instrument),
            &[],
            UnixNanos::default(),
        );

        assert!(result.is_some());
        let (final_account, _) = result.unwrap();

        if let AccountAny::Betting(betting_account) = final_account {
            assert_eq!(
                betting_account.balance_locked(Some(gbp)),
                Some(Money::new(0.0, gbp))
            );
            assert_eq!(
                betting_account.balance_free(Some(gbp)),
                Some(Money::new(1_000.0, gbp))
            );
            assert_eq!(
                betting_account.balance_total(Some(gbp)),
                Some(Money::new(1_000.0, gbp))
            );
        } else {
            panic!("Expected BettingAccount");
        }
    }

    #[rstest]
    fn test_update_orders_clears_stale_currency_locks_when_order_sides_change() {
        let usd = Currency::USD();
        let aud = Currency::AUD();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![
                AccountBalance::new(
                    Money::new(1_000_000.0, usd),
                    Money::new(0.0, usd),
                    Money::new(1_000_000.0, usd),
                ),
                AccountBalance::new(
                    Money::new(1_000_000.0, aud),
                    Money::new(0.0, aud),
                    Money::new(1_000_000.0, aud),
                ),
            ],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );

        let account = CashAccount::new(account_state, true, false);

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        cache
            .borrow_mut()
            .add_account(AccountAny::Cash(account.clone()))
            .unwrap();

        let manager = AccountsManager::new(clock, cache);
        let instrument = audusd_sim();

        let mut buy_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .price(Price::from("0.80000"))
            .build();

        let mut sell_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("50000"))
            .price(Price::from("0.81000"))
            .build();

        // Submit and accept orders
        let submitted_buy = OrderSubmitted::new(
            buy_order.trader_id(),
            buy_order.strategy_id(),
            buy_order.instrument_id(),
            buy_order.client_order_id(),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let accepted_buy = OrderAccepted::new(
            buy_order.trader_id(),
            buy_order.strategy_id(),
            buy_order.instrument_id(),
            buy_order.client_order_id(),
            VenueOrderId::new("1"),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        buy_order
            .apply(OrderEventAny::Submitted(submitted_buy))
            .unwrap();
        buy_order
            .apply(OrderEventAny::Accepted(accepted_buy))
            .unwrap();

        let submitted_sell = OrderSubmitted::new(
            sell_order.trader_id(),
            sell_order.strategy_id(),
            sell_order.instrument_id(),
            sell_order.client_order_id(),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let accepted_sell = OrderAccepted::new(
            sell_order.trader_id(),
            sell_order.strategy_id(),
            sell_order.instrument_id(),
            sell_order.client_order_id(),
            VenueOrderId::new("2"),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        sell_order
            .apply(OrderEventAny::Submitted(submitted_sell))
            .unwrap();
        sell_order
            .apply(OrderEventAny::Accepted(accepted_sell))
            .unwrap();

        let orders_both: Vec<&OrderAny> = vec![&buy_order, &sell_order];
        let result = manager.update_orders(
            &AccountAny::Cash(account),
            &InstrumentAny::CurrencyPair(instrument.clone()),
            &orders_both,
            UnixNanos::default(),
        );

        assert!(result.is_some());
        let (updated_account, _) = result.unwrap();

        if let AccountAny::Cash(cash_account) = &updated_account {
            assert_eq!(
                cash_account.balance_locked(Some(usd)),
                Some(Money::new(80_000.0, usd))
            );
            assert_eq!(
                cash_account.balance_locked(Some(aud)),
                Some(Money::new(50_000.0, aud))
            );
        } else {
            panic!("Expected CashAccount");
        }

        // Cancel BUY order, only SELL remains - USD lock should be cleared
        let orders_sell_only: Vec<&OrderAny> = vec![&sell_order];
        let result = manager.update_orders(
            &updated_account,
            &InstrumentAny::CurrencyPair(instrument),
            &orders_sell_only,
            UnixNanos::default(),
        );

        assert!(result.is_some());
        let (final_account, _) = result.unwrap();

        if let AccountAny::Cash(cash_account) = final_account {
            assert_eq!(
                cash_account.balance_locked(Some(usd)),
                Some(Money::new(0.0, usd))
            );
            assert_eq!(
                cash_account.balance_locked(Some(aud)),
                Some(Money::new(50_000.0, aud))
            );
        } else {
            panic!("Expected CashAccount");
        }
    }

    #[rstest]
    fn test_cash_account_rejects_negative_balance_when_borrowing_disabled() {
        let usd = Currency::USD();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::new(1_000.0, usd),
                Money::new(0.0, usd),
                Money::new(1_000.0, usd),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(usd),
        );

        let mut account = CashAccount::new(account_state, true, false);

        let negative_balances = vec![AccountBalance::new(
            Money::new(-500.0, usd),
            Money::new(0.0, usd),
            Money::new(-500.0, usd),
        )];

        let result = account.update_balances(&negative_balances);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("negative"));
        assert!(err_msg.contains("borrowing not allowed"));
    }

    #[rstest]
    fn test_manager_update_balances_skips_update_on_negative_balance_error() {
        let usd = Currency::USD();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::new(100.0, usd),
                Money::new(0.0, usd),
                Money::new(100.0, usd),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(usd),
        );

        let account = CashAccount::new(account_state, true, false);
        let initial_balance = account.balance_total(Some(usd)).unwrap();

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        cache
            .borrow_mut()
            .add_account(AccountAny::Cash(account.clone()))
            .unwrap();

        let manager = AccountsManager::new(clock, cache.clone());
        let instrument = audusd_sim();

        let mut order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            VenueOrderId::new("1"),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        // Fill with large cost ($80k) that exceeds $100 balance
        let fill = OrderFilled::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            instrument.id(),
            order.client_order_id(),
            VenueOrderId::new("1"),
            AccountId::new("SIM-001"),
            TradeId::new("1"),
            OrderSide::Buy,
            order.order_type(),
            Quantity::from("100000"),
            Price::from("0.80000"),
            usd,
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(1),
            false,
            Some(PositionId::new("P-001")),
            Some(Money::new(20.0, usd)),
        );

        let position = Position::new(&InstrumentAny::CurrencyPair(instrument.clone()), fill);
        cache
            .borrow_mut()
            .add_position(&position, OmsType::Netting)
            .unwrap();

        let fill2 = OrderFilled::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            instrument.id(),
            order.client_order_id(),
            VenueOrderId::new("2"),
            AccountId::new("SIM-001"),
            TradeId::new("2"),
            OrderSide::Buy,
            order.order_type(),
            Quantity::from("100000"),
            Price::from("0.80000"),
            usd,
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::from(2),
            UnixNanos::from(2),
            false,
            Some(PositionId::new("P-001")),
            Some(Money::new(20.0, usd)),
        );
        let _state = manager.update_balances(
            AccountAny::Cash(account),
            &InstrumentAny::CurrencyPair(instrument),
            fill2,
        );

        let account_after = cache
            .borrow()
            .account(&AccountId::new("SIM-001"))
            .unwrap()
            .clone();

        if let AccountAny::Cash(cash) = account_after {
            assert_eq!(cash.balance_total(Some(usd)), Some(initial_balance));
        } else {
            panic!("Expected CashAccount");
        }
    }

    #[rstest]
    fn test_order_canceled_releases_locked_balance() {
        // Regression test for https://github.com/nautechsystems/nautilus_trader/issues/3525
        let usd = Currency::USD();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::new(100_000.0, usd),
                Money::new(0.0, usd),
                Money::new(100_000.0, usd),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(usd),
        );

        let account = CashAccount::new(account_state, true, false);

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        cache
            .borrow_mut()
            .add_account(AccountAny::Cash(account.clone()))
            .unwrap();

        let manager = AccountsManager::new(clock, cache);
        let instrument = audusd_sim();

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .price(Price::from("0.80000"))
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id().unwrap_or(VenueOrderId::new("1")),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );

        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        let result = manager.update_orders(
            &AccountAny::Cash(account),
            &InstrumentAny::CurrencyPair(instrument.clone()),
            &[&order],
            UnixNanos::default(),
        );

        assert!(result.is_some());
        let (updated_account, _) = result.unwrap();

        if let AccountAny::Cash(ref cash) = updated_account {
            // 100k * 0.80 = 80k USD locked
            assert_eq!(
                cash.balance_locked(Some(usd)),
                Some(Money::new(80_000.0, usd))
            );
            assert_eq!(
                cash.balance_free(Some(usd)),
                Some(Money::new(20_000.0, usd))
            );
        } else {
            panic!("Expected CashAccount");
        }

        let result = manager.update_orders(
            &updated_account,
            &InstrumentAny::CurrencyPair(instrument),
            &[],
            UnixNanos::default(),
        );

        assert!(result.is_some());
        let (final_account, _) = result.unwrap();

        if let AccountAny::Cash(cash) = final_account {
            assert_eq!(cash.balance_locked(Some(usd)), Some(Money::new(0.0, usd)));
            assert_eq!(
                cash.balance_free(Some(usd)),
                Some(Money::new(100_000.0, usd))
            );
            assert_eq!(
                cash.balance_total(Some(usd)),
                Some(Money::new(100_000.0, usd))
            );
        } else {
            panic!("Expected CashAccount");
        }
    }

    #[rstest]
    fn test_generate_account_state_preserves_per_instrument_and_account_wide_margins() {
        let usd = Currency::USD();
        let audusd = InstrumentId::from("AUD/USD.SIM");
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::new(1_000_000.0, usd),
                Money::new(0.0, usd),
                Money::new(1_000_000.0, usd),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(usd),
        );
        let mut account = MarginAccount::new(account_state, false);
        account.update_margin(MarginBalance::new(
            Money::new(150.0, usd),
            Money::new(75.0, usd),
            Some(audusd),
        ));
        account.update_margin(MarginBalance::new(
            Money::new(500.0, usd),
            Money::new(250.0, usd),
            None,
        ));

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache);

        let state =
            manager.generate_account_state(&AccountAny::Margin(account), UnixNanos::default());

        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.balances[0].currency, usd);
        assert_eq!(state.balances[0].total, Money::new(1_000_000.0, usd));
        assert_eq!(state.balances[0].locked, Money::new(975.0, usd));
        assert_eq!(state.balances[0].free, Money::new(999_025.0, usd));

        assert_eq!(state.margins.len(), 2);
        let per_instrument: Vec<_> = state
            .margins
            .iter()
            .filter(|m| m.instrument_id.is_some())
            .collect();
        let account_wide: Vec<_> = state
            .margins
            .iter()
            .filter(|m| m.instrument_id.is_none())
            .collect();
        assert_eq!(per_instrument.len(), 1);
        assert_eq!(per_instrument[0].instrument_id, Some(audusd));
        assert_eq!(per_instrument[0].initial, Money::new(150.0, usd));
        assert_eq!(per_instrument[0].maintenance, Money::new(75.0, usd));
        assert_eq!(account_wide.len(), 1);
        assert_eq!(account_wide[0].currency, usd);
        assert_eq!(account_wide[0].initial, Money::new(500.0, usd));
        assert_eq!(account_wide[0].maintenance, Money::new(250.0, usd));
    }

    #[rstest]
    fn test_update_balances_returns_recalculated_balance_for_cash_account() {
        let usd = Currency::USD();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::new(1_000_000.0, usd),
                Money::new(0.0, usd),
                Money::new(1_000_000.0, usd),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(usd),
        );

        let account = CashAccount::new(account_state, true, false);

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        cache
            .borrow_mut()
            .add_account(AccountAny::Cash(account.clone()))
            .unwrap();

        let manager = AccountsManager::new(clock, cache.clone());
        let instrument = audusd_sim();

        let mut order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .build();
        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            VenueOrderId::new("1"),
            AccountId::new("SIM-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        let fill = OrderFilled::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            instrument.id(),
            order.client_order_id(),
            VenueOrderId::new("1"),
            AccountId::new("SIM-001"),
            TradeId::new("1"),
            OrderSide::Buy,
            order.order_type(),
            Quantity::from("100000"),
            Price::from("0.80000"),
            usd,
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(1),
            false,
            Some(PositionId::new("P-001")),
            Some(Money::new(20.0, usd)),
        );
        let position = Position::new(&InstrumentAny::CurrencyPair(instrument.clone()), fill);
        cache
            .borrow_mut()
            .add_position(&position, OmsType::Netting)
            .unwrap();

        let (updated, state) = manager.update_balances(
            AccountAny::Cash(account),
            &InstrumentAny::CurrencyPair(instrument),
            fill,
        );

        // Buy 100k at 0.80 → 80,000 USD cost, 20 USD commission, expect 919,980 USD
        let expected = Money::new(919_980.0, usd);

        match updated {
            AccountAny::Cash(cash) => {
                assert_eq!(cash.balance_total(Some(usd)), Some(expected));
                assert_eq!(cash.balance_free(Some(usd)), Some(expected));
            }
            _ => panic!("Expected CashAccount"),
        }
        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.balances[0].currency, usd);
        assert_eq!(state.balances[0].total, expected);
        assert_eq!(state.balances[0].free, expected);
    }

    fn multi_currency_cash_account(allow_borrowing: bool) -> CashAccount {
        let aud = Currency::AUD();
        let usd = Currency::USD();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![
                AccountBalance::new(
                    Money::new(10_000.0, aud),
                    Money::new(0.0, aud),
                    Money::new(10_000.0, aud),
                ),
                AccountBalance::new(
                    Money::new(100.0, usd),
                    Money::new(0.0, usd),
                    Money::new(100.0, usd),
                ),
            ],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        CashAccount::new(account_state, true, allow_borrowing)
    }

    fn buy_audusd_fill(qty: &str, px: &str, commission: f64) -> OrderFilled {
        let instrument = audusd_sim();
        let usd = Currency::USD();
        OrderFilled::new(
            TraderId::test_default(),
            StrategyId::test_default(),
            instrument.id(),
            ClientOrderId::new("O-001"),
            VenueOrderId::new("1"),
            AccountId::new("SIM-001"),
            TradeId::new("1"),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from(qty),
            Price::from(px),
            usd,
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(1),
            false,
            Some(PositionId::new("P-001")),
            Some(Money::new(commission, usd)),
        )
    }

    #[rstest]
    fn test_update_balance_multi_currency_persists_negative_balance_with_allow_borrowing() {
        let aud = Currency::AUD();
        let usd = Currency::USD();
        let account = multi_currency_cash_account(true);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        cache
            .borrow_mut()
            .add_account(AccountAny::Cash(account.clone()))
            .unwrap();
        let manager = AccountsManager::new(clock, cache.clone());
        let instrument = audusd_sim();
        let fill = buy_audusd_fill("10000", "0.80000", 20.0);
        let position = Position::new(&InstrumentAny::CurrencyPair(instrument.clone()), fill);
        cache
            .borrow_mut()
            .add_position(&position, OmsType::Netting)
            .unwrap();

        let (updated, _state) = manager.update_balances(
            AccountAny::Cash(account),
            &InstrumentAny::CurrencyPair(instrument),
            fill,
        );

        match updated {
            AccountAny::Cash(cash) => {
                assert_eq!(
                    cash.balance_total(Some(aud)),
                    Some(Money::new(20_000.0, aud))
                );
                assert_eq!(
                    cash.balance_total(Some(usd)),
                    Some(Money::new(-7_920.0, usd))
                );
            }
            _ => panic!("Expected CashAccount"),
        }
    }

    #[rstest]
    fn test_update_balance_multi_currency_rejects_negative_balance_without_allow_borrowing() {
        let aud = Currency::AUD();
        let usd = Currency::USD();
        let account = multi_currency_cash_account(false);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        cache
            .borrow_mut()
            .add_account(AccountAny::Cash(account.clone()))
            .unwrap();
        let manager = AccountsManager::new(clock, cache.clone());
        let instrument = audusd_sim();
        let fill = buy_audusd_fill("10000", "0.80000", 20.0);
        let position = Position::new(&InstrumentAny::CurrencyPair(instrument.clone()), fill);
        cache
            .borrow_mut()
            .add_position(&position, OmsType::Netting)
            .unwrap();

        let (updated, _state) = manager.update_balances(
            AccountAny::Cash(account),
            &InstrumentAny::CurrencyPair(instrument),
            fill,
        );

        // Rejected by `cash.update_balances`: original balances preserved
        match updated {
            AccountAny::Cash(cash) => {
                assert_eq!(
                    cash.balance_total(Some(aud)),
                    Some(Money::new(10_000.0, aud))
                );
                assert_eq!(cash.balance_total(Some(usd)), Some(Money::new(100.0, usd)));
            }
            _ => panic!("Expected CashAccount"),
        }
    }

    #[rstest]
    fn test_update_balance_multi_currency_rejects_new_currency_negative_pnl() {
        let aud = Currency::AUD();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::new(10_000.0, aud),
                Money::new(0.0, aud),
                Money::new(10_000.0, aud),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        let account = CashAccount::new(account_state, true, true);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        cache
            .borrow_mut()
            .add_account(AccountAny::Cash(account.clone()))
            .unwrap();
        let manager = AccountsManager::new(clock, cache.clone());
        let instrument = audusd_sim();
        // Buy AUD/USD on an AUD-only account: produces negative USD pnl on a missing currency,
        // which the documented Python-parity branch rejects even with `allow_borrowing=true`.
        let fill = buy_audusd_fill("10000", "0.80000", 0.0);
        let position = Position::new(&InstrumentAny::CurrencyPair(instrument.clone()), fill);
        cache
            .borrow_mut()
            .add_position(&position, OmsType::Netting)
            .unwrap();

        let (updated, _state) = manager.update_balances(
            AccountAny::Cash(account),
            &InstrumentAny::CurrencyPair(instrument),
            fill,
        );

        // Rejected at the no-existing-balance + negative-pnl branch (Python parity)
        match updated {
            AccountAny::Cash(cash) => {
                assert_eq!(
                    cash.balance_total(Some(aud)),
                    Some(Money::new(10_000.0, aud))
                );
                assert_eq!(cash.balance_total(Some(Currency::USD())), None);
            }
            _ => panic!("Expected CashAccount"),
        }
    }
}
