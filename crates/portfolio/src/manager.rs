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
    enums::{AccountType, OrderSide, OrderType, PriceType},
    events::{AccountState, OrderFilled},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    position::{Position, fold_net_position},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

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
    /// Maintenance margin is computed on the net per-instrument exposure: open
    /// positions are folded into a NETTING-equivalent state and the margin model
    /// runs once on the result.
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
        let mut ordered: Vec<&Position> = positions;
        ordered.sort_by_key(|p| (p.ts_opened, p.id));

        let legs: Vec<(Decimal, Decimal, u64)> = ordered
            .iter()
            .map(|p| {
                assert_eq!(
                    p.instrument_id,
                    instrument.id(),
                    "Position not for instrument {}",
                    instrument.id()
                );
                (
                    p.signed_decimal_qty(),
                    Decimal::try_from(p.avg_px_open).unwrap_or(Decimal::ZERO),
                    p.ts_opened.as_u64(),
                )
            })
            .collect();

        let (net_signed_qty, net_avg_px) = fold_net_position(&legs);

        let currency = account
            .base_currency
            .unwrap_or_else(|| instrument.settlement_currency());

        let mut total_margin_maint = Decimal::ZERO;

        let net_qty =
            match Quantity::from_decimal_dp(net_signed_qty.abs(), instrument.size_precision()) {
                Ok(q) if q.is_zero() => None,
                Ok(q) => Some(q),
                Err(e) => {
                    log::error!(
                        "Cannot calculate maintenance (position) margin: net quantity \
                     conversion failed for {}: {e}",
                        instrument.id()
                    );
                    return None;
                }
            };

        if let Some(quantity) = net_qty {
            let price = Price::from_decimal_dp(net_avg_px, instrument.price_precision()).ok()?;

            let margin_maint = match instrument {
                InstrumentAny::Betting(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::BinaryOption(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::Cfd(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::Commodity(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::CryptoFuture(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::CryptoFuturesSpread(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::CryptoOption(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::CryptoOptionSpread(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::CryptoPerpetual(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::CurrencyPair(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::Equity(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::FuturesContract(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::FuturesSpread(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::IndexInstrument(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::OptionContract(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::OptionSpread(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::PerpetualContract(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
                InstrumentAny::TokenizedAsset(i) => account
                    .calculate_maintenance_margin(i, quantity, price, None)
                    .ok()?,
            };

            total_margin_maint = margin_maint.as_decimal();

            if let Some(base_currency) = account.base_currency {
                if let Some(xrate) = self.calculate_xrate_to_base(account.base_currency, instrument)
                {
                    total_margin_maint *= xrate;
                } else {
                    log::debug!(
                        "Cannot calculate maintenance (position) margin: insufficient data for {}/{}",
                        instrument.settlement_currency(),
                        base_currency
                    );
                    return None;
                }
            }
        }

        let margin_maint = Money::from_decimal(total_margin_maint, currency).ok()?;
        account.update_maintenance_margin(instrument.id(), margin_maint);

        log::info!("{} margin_maint={margin_maint}", instrument.id());

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
        let mut base_xrate: Option<Decimal> = None;

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
                    locked = match Money::from_decimal(locked.as_decimal() * xrate, currency) {
                        Ok(money) => money,
                        Err(e) => {
                            log::error!("Cannot calculate balance locked: {e}");
                            return None;
                        }
                    };
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
        let mut total_margin_init = Decimal::ZERO;
        let mut base_xrate: Option<Decimal> = None;
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
                InstrumentAny::CryptoFuturesSpread(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::CryptoOption(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::CryptoOptionSpread(i) => account
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

            let mut margin_init = margin_init.as_decimal();

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

        let money = match Money::from_decimal(total_margin_init, currency) {
            Ok(money) => money,
            Err(e) => {
                log::error!("Cannot calculate initial margin: {e}");
                return None;
            }
        };
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
        let mut base_xrate: Option<Decimal> = None;
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
                    locked = match Money::from_decimal(locked.as_decimal() * xrate, currency) {
                        Ok(money) => money,
                        Err(e) => {
                            log::error!("Cannot calculate balance locked: {e}");
                            return None;
                        }
                    };
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
                *comm = match Money::from_decimal(comm.as_decimal() * xrate, base_currency) {
                    Ok(money) => money,
                    Err(e) => {
                        log::error!("Cannot calculate account state: {e}");
                        return;
                    }
                };
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
                pnl = match Money::from_decimal(pnl.as_decimal() * xrate, base_currency) {
                    Ok(money) => money,
                    Err(e) => {
                        log::error!("Cannot calculate account state: {e}");
                        return;
                    }
                };
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

        let new_total = balance.total.as_decimal() + pnl.as_decimal();

        let new_balance = match AccountBalance::from_total_and_locked(
            new_total,
            balance.locked.as_decimal(),
            pnl.currency,
        ) {
            Ok(new_balance) => new_balance,
            Err(e) => {
                log::error!("Cannot update {} balance: {e}", pnl.currency);
                return;
            }
        };

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
                let new_total = balance.total.as_decimal() + pnl.as_decimal();
                let mut new_locked = balance.locked.as_decimal();

                if pnl.as_decimal() < Decimal::ZERO
                    && fill.order_type != OrderType::Market
                    && !self.is_sports_betting_fill(fill.instrument_id)
                {
                    new_locked += pnl.as_decimal();

                    if new_locked < Decimal::ZERO {
                        new_locked = Decimal::ZERO;
                    }
                }

                match AccountBalance::from_total_and_locked(new_total, new_locked, currency) {
                    Ok(new_balance) => new_balance,
                    Err(e) => {
                        log::error!("Cannot update {currency} balance: {e}");
                        return;
                    }
                }
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

                match AccountBalance::from_total_and_locked(
                    new_total,
                    balance.locked.as_decimal(),
                    currency,
                ) {
                    Ok(commission_balance) => commission_balance,
                    Err(e) => {
                        log::error!("Cannot deduct {currency} commission: {e}");
                        return;
                    }
                }
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

    fn is_sports_betting_fill(&self, instrument_id: InstrumentId) -> bool {
        self.cache
            .borrow()
            .instrument(&instrument_id)
            .is_some_and(|instrument| matches!(instrument, InstrumentAny::Betting(_)))
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
    ) -> Option<Decimal> {
        match base_currency {
            None => Some(Decimal::ONE),
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
        enums::{AccountType, OmsType, OrderSide, OrderType},
        events::{
            AccountState, OrderAccepted, OrderEventAny, OrderFilled, OrderSubmitted,
            order::spec::{OrderAcceptedSpec, OrderFilledSpec, OrderSubmittedSpec},
        },
        identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId},
        instruments::{
            Instrument, InstrumentAny,
            stubs::{audusd_sim, betting, currency_pair_btcusdt},
        },
        orders::{OrderAny, OrderTestBuilder},
        position::Position,
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

        let submitted1 = order_submitted_for(&order1);
        let accepted1 = order_accepted_for(&order1, VenueOrderId::new("1"));

        order1.apply(OrderEventAny::Submitted(submitted1)).unwrap();
        order1.apply(OrderEventAny::Accepted(accepted1)).unwrap();

        let submitted2 = order_submitted_for(&order2);
        let accepted2 = order_accepted_for(&order2, VenueOrderId::new("2"));

        order2.apply(OrderEventAny::Submitted(submitted2)).unwrap();
        order2.apply(OrderEventAny::Accepted(accepted2)).unwrap();

        let submitted3 = order_submitted_for(&order3);
        let accepted3 = order_accepted_for(&order3, VenueOrderId::new("3"));

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

        let submitted_back =
            order_submitted_for_account(&back_order, AccountId::new("BETTING-001"));
        let accepted_back = order_accepted_for_account(
            &back_order,
            VenueOrderId::new("B1"),
            AccountId::new("BETTING-001"),
        );
        back_order
            .apply(OrderEventAny::Submitted(submitted_back))
            .unwrap();
        back_order
            .apply(OrderEventAny::Accepted(accepted_back))
            .unwrap();

        let submitted_lay = order_submitted_for_account(&lay_order, AccountId::new("BETTING-001"));
        let accepted_lay = order_accepted_for_account(
            &lay_order,
            VenueOrderId::new("L1"),
            AccountId::new("BETTING-001"),
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

        let submitted = order_submitted_for_account(&order, AccountId::new("BETFAIR-001"));
        let accepted = order_accepted_for_account(
            &order,
            VenueOrderId::new("B2"),
            AccountId::new("BETFAIR-001"),
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
        let submitted_buy = order_submitted_for(&buy_order);
        let accepted_buy = order_accepted_for(&buy_order, VenueOrderId::new("1"));
        buy_order
            .apply(OrderEventAny::Submitted(submitted_buy))
            .unwrap();
        buy_order
            .apply(OrderEventAny::Accepted(accepted_buy))
            .unwrap();

        let submitted_sell = order_submitted_for(&sell_order);
        let accepted_sell = order_accepted_for(&sell_order, VenueOrderId::new("2"));
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

        let submitted = order_submitted_for(&order);
        let accepted = order_accepted_for(&order, VenueOrderId::new("1"));
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();

        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        // Fill with large cost ($80k) that exceeds $100 balance
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(order.client_order_id())
            .venue_order_id(VenueOrderId::new("1"))
            .last_qty(Quantity::from("100000"))
            .last_px(Price::from("0.80000"))
            .ts_event(UnixNanos::from(1))
            .ts_init(UnixNanos::from(1))
            .position_id(PositionId::new("P-001"))
            .commission(Money::new(20.0, usd))
            .build();

        let position = Position::new(&InstrumentAny::CurrencyPair(instrument.clone()), fill);
        cache
            .borrow_mut()
            .add_position(&position, OmsType::Netting)
            .unwrap();

        let fill2 = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(order.client_order_id())
            .venue_order_id(VenueOrderId::new("2"))
            .trade_id(TradeId::new("2"))
            .last_qty(Quantity::from("100000"))
            .last_px(Price::from("0.80000"))
            .ts_event(UnixNanos::from(2))
            .ts_init(UnixNanos::from(2))
            .position_id(PositionId::new("P-001"))
            .commission(Money::new(20.0, usd))
            .build();
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

        let submitted = order_submitted_for(&order);
        let accepted = order_accepted_for(&order, VenueOrderId::new("1"));

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
        let submitted = order_submitted_for(&order);
        let accepted = order_accepted_for(&order, VenueOrderId::new("1"));
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(order.client_order_id())
            .venue_order_id(VenueOrderId::new("1"))
            .last_qty(Quantity::from("100000"))
            .last_px(Price::from("0.80000"))
            .ts_event(UnixNanos::from(1))
            .ts_init(UnixNanos::from(1))
            .position_id(PositionId::new("P-001"))
            .commission(Money::new(20.0, usd))
            .build();
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
        OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .last_qty(Quantity::from(qty))
            .last_px(Price::from(px))
            .ts_event(UnixNanos::from(1))
            .ts_init(UnixNanos::from(1))
            .position_id(PositionId::new("P-001"))
            .commission(Money::new(commission, usd))
            .build()
    }

    fn multi_currency_cash_account_with_usd_locked(total: f64, locked: f64) -> CashAccount {
        multi_currency_cash_account_with_usd_locked_and_borrowing(total, locked, false)
    }

    fn multi_currency_cash_account_with_usd_locked_and_borrowing(
        total: f64,
        locked: f64,
        allow_borrowing: bool,
    ) -> CashAccount {
        let usd = Currency::USD();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::new(total, usd),
                Money::new(locked, usd),
                Money::new(total - locked, usd),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        CashAccount::new(account_state, true, allow_borrowing)
    }

    fn multi_currency_betting_account_with_gbp_locked(total: f64, locked: f64) -> BettingAccount {
        let gbp = Currency::GBP();
        let account_state = AccountState::new(
            AccountId::new("BETFAIR-001"),
            AccountType::Betting,
            vec![AccountBalance::new(
                Money::new(total, gbp),
                Money::new(locked, gbp),
                Money::new(total - locked, gbp),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        BettingAccount::new(account_state, true)
    }

    fn order_submitted_for(order: &OrderAny) -> OrderSubmitted {
        OrderSubmittedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .build()
    }

    fn order_submitted_for_account(order: &OrderAny, account_id: AccountId) -> OrderSubmitted {
        OrderSubmittedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(account_id)
            .build()
    }

    fn order_accepted_for(order: &OrderAny, venue_order_id: VenueOrderId) -> OrderAccepted {
        OrderAcceptedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(venue_order_id)
            .build()
    }

    fn order_accepted_for_account(
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
    ) -> OrderAccepted {
        OrderAcceptedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(venue_order_id)
            .account_id(account_id)
            .build()
    }

    #[rstest]
    fn test_update_balance_multi_currency_market_debit_keeps_locked_balance() {
        let usd = Currency::USD();
        let account = multi_currency_cash_account_with_usd_locked(1_000.0, 200.0);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache.clone());
        let instrument = audusd_sim();
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CurrencyPair(instrument.clone()))
            .unwrap();

        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .order_type(OrderType::Market)
            .commission(Money::new(20.0, usd))
            .build();
        let mut account = AccountAny::Cash(account);
        let mut pnls = vec![Money::new(-100.0, usd)];

        manager.update_balance_multi_currency(&mut account, fill, &mut pnls);

        match account {
            AccountAny::Cash(cash) => {
                assert_eq!(cash.balance_total(Some(usd)), Some(Money::new(880.0, usd)));
                assert_eq!(cash.balance_locked(Some(usd)), Some(Money::new(200.0, usd)));
                assert_eq!(cash.balance_free(Some(usd)), Some(Money::new(680.0, usd)));
                assert_eq!(cash.commission(&usd), Some(Money::new(20.0, usd)));
            }
            _ => panic!("Expected CashAccount"),
        }
    }

    #[rstest]
    fn test_update_balance_multi_currency_limit_debit_reduces_locked_balance() {
        let usd = Currency::USD();
        let account = multi_currency_cash_account_with_usd_locked(1_000.0, 200.0);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache.clone());
        let instrument = audusd_sim();
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CurrencyPair(instrument.clone()))
            .unwrap();

        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .order_type(OrderType::Limit)
            .commission(Money::new(20.0, usd))
            .build();
        let mut account = AccountAny::Cash(account);
        let mut pnls = vec![Money::new(-100.0, usd)];

        manager.update_balance_multi_currency(&mut account, fill, &mut pnls);

        match account {
            AccountAny::Cash(cash) => {
                assert_eq!(cash.balance_total(Some(usd)), Some(Money::new(880.0, usd)));
                assert_eq!(cash.balance_locked(Some(usd)), Some(Money::new(80.0, usd)));
                assert_eq!(cash.balance_free(Some(usd)), Some(Money::new(800.0, usd)));
                assert_eq!(cash.commission(&usd), Some(Money::new(20.0, usd)));
            }
            _ => panic!("Expected CashAccount"),
        }
    }

    #[rstest]
    fn test_update_balance_multi_currency_limit_debit_spills_from_locked_to_free() {
        let usd = Currency::USD();
        let account = multi_currency_cash_account_with_usd_locked(1_000.0, 50.0);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache.clone());
        let instrument = audusd_sim();
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CurrencyPair(instrument.clone()))
            .unwrap();

        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .order_type(OrderType::Limit)
            .commission(Money::new(20.0, usd))
            .build();
        let mut account = AccountAny::Cash(account);
        let mut pnls = vec![Money::new(-100.0, usd)];

        manager.update_balance_multi_currency(&mut account, fill, &mut pnls);

        match account {
            AccountAny::Cash(cash) => {
                assert_eq!(cash.balance_total(Some(usd)), Some(Money::new(880.0, usd)));
                assert_eq!(cash.balance_locked(Some(usd)), Some(Money::new(0.0, usd)));
                assert_eq!(cash.balance_free(Some(usd)), Some(Money::new(880.0, usd)));
                assert_eq!(cash.commission(&usd), Some(Money::new(20.0, usd)));
            }
            _ => panic!("Expected CashAccount"),
        }
    }

    #[rstest]
    fn test_update_balance_multi_currency_limit_debit_floors_locked_on_negative_total() {
        let usd = Currency::USD();
        let account = multi_currency_cash_account_with_usd_locked_and_borrowing(100.0, 50.0, true);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache.clone());
        let instrument = audusd_sim();
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CurrencyPair(instrument.clone()))
            .unwrap();

        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .order_type(OrderType::Limit)
            .commission(Money::new(20.0, usd))
            .build();
        let mut account = AccountAny::Cash(account);
        let mut pnls = vec![Money::new(-200.0, usd)];

        manager.update_balance_multi_currency(&mut account, fill, &mut pnls);

        match account {
            AccountAny::Cash(cash) => {
                assert_eq!(cash.balance_total(Some(usd)), Some(Money::new(-120.0, usd)));
                assert_eq!(cash.balance_locked(Some(usd)), Some(Money::new(0.0, usd)));
                assert_eq!(cash.balance_free(Some(usd)), Some(Money::new(-120.0, usd)));
                assert_eq!(cash.commission(&usd), Some(Money::new(20.0, usd)));
            }
            _ => panic!("Expected CashAccount"),
        }
    }

    #[rstest]
    fn test_update_balance_multi_currency_betting_limit_debit_keeps_locked_balance() {
        let gbp = Currency::GBP();
        let account = multi_currency_betting_account_with_gbp_locked(1_000.0, 200.0);
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache.clone());
        let instrument = betting();
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::Betting(instrument.clone()))
            .unwrap();

        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .order_type(OrderType::Limit)
            .commission(Money::new(20.0, gbp))
            .build();
        let mut account = AccountAny::Betting(account);
        let mut pnls = vec![Money::new(-100.0, gbp)];

        manager.update_balance_multi_currency(&mut account, fill, &mut pnls);

        match account {
            AccountAny::Betting(betting_account) => {
                assert_eq!(
                    betting_account.balance_total(Some(gbp)),
                    Some(Money::new(880.0, gbp))
                );
                assert_eq!(
                    betting_account.balance_locked(Some(gbp)),
                    Some(Money::new(200.0, gbp))
                );
                assert_eq!(
                    betting_account.balance_free(Some(gbp)),
                    Some(Money::new(680.0, gbp))
                );
                assert_eq!(
                    betting_account.commission(&gbp),
                    Some(Money::new(20.0, gbp))
                );
            }
            _ => panic!("Expected BettingAccount"),
        }
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

    // ~100M USDT total with non-zero locked margin: the raw fixed-point value exceeds f64's
    // exact-integer range (2^53), which is the condition that triggers issue #4165.
    fn large_locked_usdt_margin_account() -> (AccountAny, Money, Money) {
        let usdt = Currency::USDT();
        let total =
            Money::from_decimal(Decimal::from_str_exact("99999997.91829666").unwrap(), usdt)
                .unwrap();
        let locked =
            Money::from_decimal(Decimal::from_str_exact("32.85965").unwrap(), usdt).unwrap();
        let free = Money::from_raw(total.raw - locked.raw, usdt);
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Margin,
            vec![AccountBalance::new(total, locked, free)],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None, // No base currency routes PnL through `update_balance_multi_currency`
        );
        (
            AccountAny::Margin(MarginAccount::new(account_state, false)),
            total,
            locked,
        )
    }

    #[rstest]
    fn test_update_balance_multi_currency_preserves_invariant_with_large_locked() {
        // Regression for issue #4165: applying realized PnL to a large multi-currency margin
        // balance via independent f64 round-trips drifts `total` and `free` relative to each
        // other, breaking `total == locked + free` and panicking `AccountBalance::new`.
        let usdt = Currency::USDT();
        let (mut account, total, locked) = large_locked_usdt_margin_account();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache);

        // No commission on the fill: only the realized-PnL branch runs. This PnL lands on an
        // 8dp tick where the old independent f64 round-trips drifted by 2e-8.
        let fill = OrderFilledSpec::builder().build();
        let pnl =
            Money::from_decimal(Decimal::from_str_exact("0.00000064").unwrap(), usdt).unwrap();
        let mut pnls = [pnl];
        manager.update_balance_multi_currency(&mut account, fill, &mut pnls);

        let balances = account.balances();
        let balance = balances.get(&usdt).expect("USDT balance");
        assert_eq!(balance.locked, locked, "locked margin preserved");
        assert_eq!(balance.total, total + pnl, "total moved by realized PnL");
        assert_eq!(
            balance.total.raw,
            balance.locked.raw + balance.free.raw,
            "invariant total == locked + free must hold"
        );
    }

    #[rstest]
    fn test_update_balance_multi_currency_commission_preserves_invariant_with_large_locked() {
        // Regression for issue #4165: the commission branch had the same f64 round-trip drift.
        let usdt = Currency::USDT();
        let (mut account, total, locked) = large_locked_usdt_margin_account();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache);

        // This commission reproduces the exact panic values from issue #4165: the old
        // Decimal-then-f64 round-trips yielded total=99999997.91829666, free=99999965.05864664.
        let commission =
            Money::from_decimal(Decimal::from_str_exact("0.00000001").unwrap(), usdt).unwrap();
        let fill = OrderFilledSpec::builder().commission(commission).build();

        // No PnL entries: only the commission branch runs.
        let mut pnls: [Money; 0] = [];
        manager.update_balance_multi_currency(&mut account, fill, &mut pnls);

        let balances = account.balances();
        let balance = balances.get(&usdt).expect("USDT balance");
        assert_eq!(balance.locked, locked, "locked margin preserved");
        assert_eq!(
            balance.total,
            total - commission,
            "total reduced by commission"
        );
        assert_eq!(
            balance.total.raw,
            balance.locked.raw + balance.free.raw,
            "invariant total == locked + free must hold"
        );
    }

    fn build_margin_account_usd(balance: f64) -> MarginAccount {
        let usd = Currency::USD();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::new(balance, usd),
                Money::new(0.0, usd),
                Money::new(balance, usd),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        MarginAccount::new(account_state, false)
    }

    fn build_margin_account_usdt(balance: f64) -> MarginAccount {
        let usdt = Currency::USDT();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::new(balance, usdt),
                Money::new(0.0, usdt),
                Money::new(balance, usdt),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        MarginAccount::new(account_state, false)
    }

    fn build_hedging_position(
        instrument: &InstrumentAny,
        side: OrderSide,
        qty: &str,
        price: &str,
        id: &str,
    ) -> Position {
        build_hedging_position_at(instrument, side, qty, price, id, UnixNanos::default())
    }

    fn build_hedging_position_at(
        instrument: &InstrumentAny,
        side: OrderSide,
        qty: &str,
        price: &str,
        id: &str,
        ts_event: UnixNanos,
    ) -> Position {
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::new(id))
            .venue_order_id(VenueOrderId::new(id))
            .trade_id(TradeId::new(id))
            .order_side(side)
            .last_qty(Quantity::from(qty))
            .last_px(Price::from(price))
            .currency(instrument.settlement_currency())
            .ts_event(ts_event)
            .ts_init(ts_event)
            .position_id(PositionId::new(id))
            .build();
        Position::new(instrument, fill)
    }

    #[rstest]
    fn test_update_positions_in_place_nets_hedging_subpositions() {
        let usd = Currency::USD();
        let mut account = build_margin_account_usd(1_000_000.0);
        let instrument = audusd_sim();
        account.set_leverage(instrument.id(), Decimal::ONE);
        let instrument_any = InstrumentAny::CurrencyPair(instrument.clone());

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache);

        // 5 long + 2 short, each 50 @ 1.0: net long 150 -> 150 * 1.0 * 0.03 = 4.50 USD
        let mut positions: Vec<Position> = Vec::new();
        for i in 0..5 {
            positions.push(build_hedging_position(
                &instrument_any,
                OrderSide::Buy,
                "50",
                "1.00000",
                &format!("L{i}"),
            ));
        }

        for i in 0..2 {
            positions.push(build_hedging_position(
                &instrument_any,
                OrderSide::Sell,
                "50",
                "1.00000",
                &format!("S{i}"),
            ));
        }

        let position_refs: Vec<&Position> = positions.iter().collect();
        let result = manager.update_positions_in_place(
            &mut account,
            &instrument_any,
            position_refs,
            UnixNanos::default(),
        );
        assert!(result.is_some(), "update_positions_in_place returned None");

        let margin_maint = account.maintenance_margin(instrument.id());
        assert_eq!(
            margin_maint,
            Money::new(4.50, usd),
            "Maintenance margin must reflect net exposure (150 @ 1.00), not per-position sum",
        );
    }

    #[rstest]
    fn test_update_positions_in_place_net_zero_hedge_has_no_margin() {
        let usd = Currency::USD();
        let mut account = build_margin_account_usd(1_000_000.0);
        let instrument = audusd_sim();
        account.set_leverage(instrument.id(), Decimal::ONE);
        let instrument_any = InstrumentAny::CurrencyPair(instrument.clone());

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache);

        // Long 100 plus short 100 at the same price: net zero, margin zero
        let long = build_hedging_position(&instrument_any, OrderSide::Buy, "100", "1.00000", "L");
        let short = build_hedging_position(&instrument_any, OrderSide::Sell, "100", "1.00000", "S");

        let result = manager.update_positions_in_place(
            &mut account,
            &instrument_any,
            vec![&long, &short],
            UnixNanos::default(),
        );
        assert!(result.is_some(), "update_positions_in_place returned None");

        let margin_maint = account.maintenance_margin(instrument.id());
        assert_eq!(margin_maint, Money::new(0.0, usd));
    }

    #[rstest]
    fn test_update_positions_in_place_uses_net_side_avg_open_price() {
        let usd = Currency::USD();
        let mut account = build_margin_account_usd(1_000_000.0);
        let instrument = audusd_sim();
        account.set_leverage(instrument.id(), Decimal::ONE);
        let instrument_any = InstrumentAny::CurrencyPair(instrument.clone());

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache);

        // Long 300 @ 0.80, short 100 @ 1.00: short closes part of long, residual long 200
        // @ 0.80, margin = 200 * 0.80 * 0.03 = 4.80 USD
        let long = build_hedging_position(&instrument_any, OrderSide::Buy, "300", "0.80000", "L1");
        let short =
            build_hedging_position(&instrument_any, OrderSide::Sell, "100", "1.00000", "S1");

        let result = manager.update_positions_in_place(
            &mut account,
            &instrument_any,
            vec![&long, &short],
            UnixNanos::default(),
        );
        assert!(result.is_some(), "update_positions_in_place returned None");

        let margin_maint = account.maintenance_margin(instrument.id());
        assert_eq!(margin_maint, Money::new(4.80, usd));
    }

    #[rstest]
    fn test_update_positions_in_place_floating_dust_clears_margin() {
        // Sub-precision dust on a flat hedge (e.g. 0.3 - 0.2 - 0.1) must clear the
        // margin instead of feeding a sub-tick quantity into `make_qty`.
        let usdt = Currency::USDT();
        let mut account = build_margin_account_usdt(1_000_000.0);
        let instrument = currency_pair_btcusdt();
        account.set_leverage(instrument.id(), Decimal::ONE);
        let instrument_any = InstrumentAny::CurrencyPair(instrument.clone());

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache);

        // 0.3 - 0.2 - 0.1 as f64 leaves ~5.55e-17, well below size_precision 6
        let long =
            build_hedging_position(&instrument_any, OrderSide::Buy, "0.300000", "50000.00", "L");
        let short_a = build_hedging_position(
            &instrument_any,
            OrderSide::Sell,
            "0.200000",
            "50000.00",
            "S1",
        );
        let short_b = build_hedging_position(
            &instrument_any,
            OrderSide::Sell,
            "0.100000",
            "50000.00",
            "S2",
        );

        let result = manager.update_positions_in_place(
            &mut account,
            &instrument_any,
            vec![&long, &short_a, &short_b],
            UnixNanos::default(),
        );
        assert!(result.is_some(), "update_positions_in_place returned None");

        let margin_maint = account.maintenance_margin(instrument.id());
        assert_eq!(margin_maint, Money::new(0.0, usdt));
    }

    #[rstest]
    fn test_update_positions_in_place_net_flat_clears_prior_base_currency_margin() {
        // A net-flat snapshot must clear margin in the same currency the prior update
        // used, not strand a base-currency lock under a settlement-currency zero.
        let usdt = Currency::USDT();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::new(1_000_000.0, usdt),
                Money::new(0.0, usdt),
                Money::new(1_000_000.0, usdt),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(usdt),
        );
        let mut account = MarginAccount::new(account_state, false);
        let instrument = currency_pair_btcusdt();
        account.set_leverage(instrument.id(), Decimal::ONE);
        let instrument_any = InstrumentAny::CurrencyPair(instrument.clone());

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache);

        // First snapshot: net long 0.5 BTC @ 50_000 -> non-zero base-currency margin.
        let long =
            build_hedging_position(&instrument_any, OrderSide::Buy, "0.500000", "50000.00", "L");
        let first = manager.update_positions_in_place(
            &mut account,
            &instrument_any,
            vec![&long],
            UnixNanos::default(),
        );
        assert!(first.is_some());
        let prior_margin = account.maintenance_margin(instrument.id());
        assert!(prior_margin.as_f64() > 0.0);
        assert_eq!(prior_margin.currency, usdt);
        let prior_locked = account.balance_locked(Some(usdt)).unwrap();
        assert!(prior_locked.as_f64() > 0.0);

        // Second snapshot: offsetting short closes the net exposure.
        let short = build_hedging_position(
            &instrument_any,
            OrderSide::Sell,
            "0.500000",
            "50000.00",
            "S",
        );
        let second = manager.update_positions_in_place(
            &mut account,
            &instrument_any,
            vec![&long, &short],
            UnixNanos::default(),
        );
        assert!(second.is_some());

        // Net-flat: maintenance margin and the resulting base-currency locked balance must clear.
        assert_eq!(
            account.maintenance_margin(instrument.id()),
            Money::new(0.0, usdt)
        );
        assert_eq!(
            account.balance_locked(Some(usdt)).unwrap(),
            Money::new(0.0, usdt)
        );
    }

    #[rstest]
    fn test_update_positions_in_place_flip_uses_flipping_fill_price() {
        // NETTING leaves the residual at the flipping fill's price; the replay must too,
        // or a gross net-side average will under-margin reversal cases.
        let usd = Currency::USD();
        let mut account = build_margin_account_usd(1_000_000.0);
        let instrument = audusd_sim();
        account.set_leverage(instrument.id(), Decimal::ONE);
        let instrument_any = InstrumentAny::CurrencyPair(instrument.clone());

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache);

        // L100@1, S50@2, S100@3: NETTING residual short 50 @ 3 -> 4.50 USD,
        // a gross net-side average would give 4.00 USD (under-margin).
        let long = build_hedging_position_at(
            &instrument_any,
            OrderSide::Buy,
            "100",
            "1.00000",
            "L",
            UnixNanos::from(1),
        );
        let short_partial = build_hedging_position_at(
            &instrument_any,
            OrderSide::Sell,
            "50",
            "2.00000",
            "S1",
            UnixNanos::from(2),
        );
        let short_flip = build_hedging_position_at(
            &instrument_any,
            OrderSide::Sell,
            "100",
            "3.00000",
            "S2",
            UnixNanos::from(3),
        );

        let result = manager.update_positions_in_place(
            &mut account,
            &instrument_any,
            vec![&long, &short_partial, &short_flip],
            UnixNanos::default(),
        );
        assert!(result.is_some(), "update_positions_in_place returned None");

        let margin_maint = account.maintenance_margin(instrument.id());
        assert_eq!(margin_maint, Money::new(4.50, usd));
    }

    #[rstest]
    fn test_update_positions_in_place_same_ts_legs_ordering_is_deterministic() {
        // positions_open iterates an AHashSet; without a tie-breaker the fold of
        // same-ts reversal legs would vary across runs.
        let usd = Currency::USD();
        let instrument = audusd_sim();
        let instrument_any = InstrumentAny::CurrencyPair(instrument.clone());

        // Same-ts reversal: long 100 @ 1.0 (A), short 50 @ 2.0 (B), short 100 @ 3.0 (C).
        // Ordered by position_id, the fold yields short 50 @ 3.0 -> 4.50 USD.
        let same_ts = UnixNanos::from(42);
        let l_a = build_hedging_position_at(
            &instrument_any,
            OrderSide::Buy,
            "100",
            "1.00000",
            "A",
            same_ts,
        );
        let s_b = build_hedging_position_at(
            &instrument_any,
            OrderSide::Sell,
            "50",
            "2.00000",
            "B",
            same_ts,
        );
        let s_c = build_hedging_position_at(
            &instrument_any,
            OrderSide::Sell,
            "100",
            "3.00000",
            "C",
            same_ts,
        );

        let permutations: Vec<Vec<&Position>> = vec![
            vec![&l_a, &s_b, &s_c],
            vec![&s_c, &s_b, &l_a],
            vec![&s_b, &l_a, &s_c],
            vec![&s_c, &l_a, &s_b],
        ];

        let mut results: Vec<Money> = Vec::new();

        for perm in permutations {
            let mut account = build_margin_account_usd(1_000_000.0);
            account.set_leverage(instrument.id(), Decimal::ONE);

            let clock = Rc::new(RefCell::new(TestClock::new()));
            let cache = Rc::new(RefCell::new(Cache::new(None, None)));
            let manager = AccountsManager::new(clock, cache);

            let result = manager.update_positions_in_place(
                &mut account,
                &instrument_any,
                perm,
                UnixNanos::default(),
            );
            assert!(result.is_some());
            results.push(account.maintenance_margin(instrument.id()));
        }

        let first = results[0];
        for r in &results[1..] {
            assert_eq!(
                *r, first,
                "maintenance margin must be deterministic across permutations"
            );
        }
        // Canonical sorted order yields the NETTING residual short 50 @ 3.0
        assert_eq!(first, Money::new(4.50, usd));
    }

    #[rstest]
    fn test_update_positions_in_place_xrate_unavailable_returns_none() {
        // EUR base account on a USD-settled instrument with no xrate must bail out
        // rather than write a stale or zero margin.
        let eur = Currency::EUR();
        let account_state = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::new(1_000_000.0, eur),
                Money::new(0.0, eur),
                Money::new(1_000_000.0, eur),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(eur),
        );
        let mut account = MarginAccount::new(account_state, false);
        let instrument = audusd_sim();
        account.set_leverage(instrument.id(), Decimal::ONE);
        let instrument_any = InstrumentAny::CurrencyPair(instrument);

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache);

        let pos = build_hedging_position(&instrument_any, OrderSide::Buy, "100", "1.00000", "L");
        let result = manager.update_positions_in_place(
            &mut account,
            &instrument_any,
            vec![&pos],
            UnixNanos::default(),
        );
        assert!(result.is_none(), "xrate-unavailable must return None");
    }

    #[rstest]
    fn test_update_positions_in_place_closed_positions_filtered() {
        // Closed positions in the input must not contribute to net exposure.
        let usd = Currency::USD();
        let mut account = build_margin_account_usd(1_000_000.0);
        let instrument = audusd_sim();
        account.set_leverage(instrument.id(), Decimal::ONE);
        let instrument_any = InstrumentAny::CurrencyPair(instrument.clone());

        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let manager = AccountsManager::new(clock, cache);

        // Close a position by applying an offsetting fill
        let open_long = build_hedging_position_at(
            &instrument_any,
            OrderSide::Buy,
            "100",
            "1.00000",
            "C",
            UnixNanos::from(1),
        );
        let close_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::new("Cclose"))
            .venue_order_id(VenueOrderId::new("Cclose"))
            .trade_id(TradeId::new("Cclose"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from("100"))
            .last_px(Price::from("1.00000"))
            .currency(instrument.settlement_currency())
            .ts_event(UnixNanos::from(2))
            .ts_init(UnixNanos::from(2))
            .position_id(PositionId::new("C"))
            .build();
        let mut closed = open_long;
        closed.apply(&close_fill);
        assert!(closed.is_closed());

        // Active open position alongside the closed one
        let live = build_hedging_position_at(
            &instrument_any,
            OrderSide::Buy,
            "50",
            "1.00000",
            "L",
            UnixNanos::from(3),
        );

        let result = manager.update_positions_in_place(
            &mut account,
            &instrument_any,
            vec![&closed, &live],
            UnixNanos::default(),
        );
        assert!(result.is_some());

        // Only the live 50 long contributes: margin = 50 * 1.0 * 0.03 = 1.50 USD
        assert_eq!(
            account.maintenance_margin(instrument.id()),
            Money::new(1.50, usd)
        );
    }
}
