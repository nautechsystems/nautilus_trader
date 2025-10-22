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

//! Provides account management functionality.

use std::{cell::RefCell, collections::HashMap, fmt::Debug, rc::Rc};

use nautilus_common::{cache::Cache, clock::Clock};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::{Account, AccountAny, CashAccount, MarginAccount},
    enums::{AccountType, OrderSide, OrderSideSpecified, PriceType},
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
    /// # Panics
    ///
    /// Panics if the position list for the filled instrument is empty.
    #[must_use]
    pub fn update_balances(
        &self,
        account: AccountAny,
        instrument: InstrumentAny,
        fill: OrderFilled,
    ) -> AccountState {
        let cache = self.cache.borrow();
        let position_id = if let Some(position_id) = fill.position_id {
            position_id
        } else {
            let positions_open = cache.positions_open(None, Some(&fill.instrument_id), None, None);
            positions_open
                .first()
                .unwrap_or_else(|| panic!("List of Positions is empty"))
                .id
        };

        let position = cache.position(&position_id);

        let pnls = account.calculate_pnls(instrument, fill, position.cloned());

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

                self.update_balance_single_currency(account.clone(), &fill, pnl);
            }
            None => {
                if let Ok(mut pnl_list) = pnls {
                    self.update_balance_multi_currency(account.clone(), fill, &mut pnl_list);
                }
            }
        }

        // Generate and return account state
        self.generate_account_state(account, fill.ts_event)
    }

    /// Updates account balances based on open orders.
    ///
    /// For cash accounts, updates the balance locked by open orders.
    /// For margin accounts, updates the initial margin requirements.
    #[must_use]
    pub fn update_orders(
        &self,
        account: &AccountAny,
        instrument: InstrumentAny,
        orders_open: Vec<&OrderAny>,
        ts_event: UnixNanos,
    ) -> Option<(AccountAny, AccountState)> {
        match account.clone() {
            AccountAny::Cash(cash_account) => self
                .update_balance_locked(&cash_account, instrument, orders_open, ts_event)
                .map(|(updated_cash_account, state)| {
                    (AccountAny::Cash(updated_cash_account), state)
                }),
            AccountAny::Margin(margin_account) => self
                .update_margin_init(&margin_account, instrument, orders_open, ts_event)
                .map(|(updated_margin_account, state)| {
                    (AccountAny::Margin(updated_margin_account), state)
                }),
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
        instrument: InstrumentAny,
        positions: Vec<&Position>,
        ts_event: UnixNanos,
    ) -> Option<(MarginAccount, AccountState)> {
        let mut total_margin_maint = 0.0;
        let mut base_xrate: Option<f64> = None;
        let mut currency = instrument.settlement_currency();
        let mut account = account.clone();

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
            };

            let mut margin_maint = margin_maint.as_f64();

            if let Some(base_currency) = account.base_currency {
                if base_xrate.is_none() {
                    currency = base_currency;
                    base_xrate = self.calculate_xrate_to_base(
                        AccountAny::Margin(account.clone()),
                        instrument.clone(),
                        position.entry.as_specified(),
                    );
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
        Some((
            account.clone(),
            self.generate_account_state(AccountAny::Margin(account), ts_event),
        ))
    }

    fn update_balance_locked(
        &self,
        account: &CashAccount,
        instrument: InstrumentAny,
        orders_open: Vec<&OrderAny>,
        ts_event: UnixNanos,
    ) -> Option<(CashAccount, AccountState)> {
        let mut account = account.clone();
        if orders_open.is_empty() {
            let balance = account.balances.remove(&instrument.quote_currency());
            if let Some(balance) = balance {
                account.recalculate_balance(balance.currency);
            }
            return Some((
                account.clone(),
                self.generate_account_state(AccountAny::Cash(account), ts_event),
            ));
        }

        let mut total_locked: HashMap<Currency, Money> = HashMap::new();
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
                    instrument.clone(),
                    order.order_side(),
                    order.quantity(),
                    price?,
                    None,
                )
                .unwrap();

            if let Some(base_curr) = account.base_currency() {
                if base_xrate.is_none() {
                    currency = base_curr;
                    base_xrate = self.calculate_xrate_to_base(
                        AccountAny::Cash(account.clone()),
                        instrument.clone(),
                        order.order_side_specified(),
                    );
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
                .and_modify(|total| *total += locked)
                .or_insert(locked);
        }

        for (_, balance_locked) in total_locked {
            if let Some(balance) = account.balances.get_mut(&balance_locked.currency) {
                balance.locked = balance_locked;
                let currency = balance.currency;
                account.recalculate_balance(currency);
            }

            log::info!("{} balance_locked={balance_locked}", instrument.id());
        }

        Some((
            account.clone(),
            self.generate_account_state(AccountAny::Cash(account), ts_event),
        ))
    }

    fn update_margin_init(
        &self,
        account: &MarginAccount,
        instrument: InstrumentAny,
        orders_open: Vec<&OrderAny>,
        ts_event: UnixNanos,
    ) -> Option<(MarginAccount, AccountState)> {
        let mut total_margin_init = 0.0;
        let mut base_xrate: Option<f64> = None;
        let mut currency = instrument.settlement_currency();
        let mut account = account.clone();

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
                InstrumentAny::OptionContract(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
                InstrumentAny::OptionSpread(i) => account
                    .calculate_initial_margin(i, order.quantity(), price?, None)
                    .ok()?,
            };

            let mut margin_init = margin_init.as_f64();

            if let Some(base_currency) = account.base_currency {
                if base_xrate.is_none() {
                    currency = base_currency;
                    base_xrate = self.calculate_xrate_to_base(
                        AccountAny::Margin(account.clone()),
                        instrument.clone(),
                        order.order_side_specified(),
                    );
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

        Some((
            account.clone(),
            self.generate_account_state(AccountAny::Margin(account), ts_event),
        ))
    }

    fn update_balance_single_currency(
        &self,
        account: AccountAny,
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
            pnl -= comm;
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
            AccountAny::Cash(mut cash) => {
                cash.update_balances(balances);
                if let Some(comm) = commission {
                    cash.update_commissions(comm);
                }
            }
            AccountAny::Margin(mut margin) => {
                margin.update_balances(balances);
                if let Some(comm) = commission {
                    margin.update_commissions(comm);
                }
            }
        }
    }

    fn update_balance_multi_currency(
        &self,
        account: AccountAny,
        fill: OrderFilled,
        pnls: &mut [Money],
    ) {
        let mut new_balances = Vec::new();
        let commission = fill.commission;
        let mut apply_commission = commission.is_some_and(|c| !c.is_zero());

        for pnl in pnls.iter_mut() {
            if apply_commission && pnl.currency == commission.unwrap().currency {
                *pnl -= commission.unwrap();
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

                if new_total < 0.0 {
                    log::error!(
                        "AccountBalanceNegative: balance = {}, currency = {}",
                        total.as_decimal(),
                        currency
                    );
                    return;
                }
                if new_free < 0.0 {
                    log::error!(
                        "AccountMarginExceeded: balance = {}, margin = {}, currency = {}",
                        total.as_decimal(),
                        balance.locked.as_decimal(),
                        currency
                    );
                    return;
                }

                AccountBalance::new(total, balance.locked, free)
            } else {
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
            AccountAny::Cash(mut cash) => {
                cash.update_balances(new_balances);
                if let Some(commission) = commission {
                    cash.update_commissions(commission);
                }
            }
            AccountAny::Margin(mut margin) => {
                margin.update_balances(new_balances);
                if let Some(commission) = commission {
                    margin.update_commissions(commission);
                }
            }
        }
    }

    fn generate_account_state(&self, account: AccountAny, ts_event: UnixNanos) -> AccountState {
        match account {
            AccountAny::Cash(cash_account) => AccountState::new(
                cash_account.id,
                AccountType::Cash,
                cash_account.balances.clone().into_values().collect(),
                vec![],
                false,
                UUID4::new(),
                ts_event,
                self.clock.borrow().timestamp_ns(),
                cash_account.base_currency(),
            ),
            AccountAny::Margin(margin_account) => AccountState::new(
                margin_account.id,
                AccountType::Margin,
                vec![],
                margin_account.margins.clone().into_values().collect(),
                false,
                UUID4::new(),
                ts_event,
                self.clock.borrow().timestamp_ns(),
                margin_account.base_currency(),
            ),
        }
    }

    fn calculate_xrate_to_base(
        &self,
        account: AccountAny,
        instrument: InstrumentAny,
        side: OrderSideSpecified,
    ) -> Option<f64> {
        match account.base_currency() {
            None => Some(1.0),
            Some(base_curr) => self.cache.borrow().get_xrate(
                instrument.id().venue,
                instrument.settlement_currency(),
                base_curr,
                match side {
                    OrderSideSpecified::Sell => PriceType::Bid,
                    OrderSideSpecified::Buy => PriceType::Ask,
                },
            ),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_model::{
        accounts::CashAccount,
        enums::{AccountType, OrderSide, OrderType},
        events::{AccountState, OrderAccepted, OrderEventAny, OrderSubmitted},
        identifiers::{AccountId, VenueOrderId},
        instruments::{InstrumentAny, stubs::audusd_sim},
        orders::{OrderAny, OrderTestBuilder},
        types::{AccountBalance, Currency, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_update_balance_locked_with_base_currency_multiple_orders() {
        // Arrange - Create account with USD base currency
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
            Some(usd), // Base currency set to USD
        );

        let account = CashAccount::new(account_state, true, false);

        // Create cache and manager
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        cache
            .borrow_mut()
            .add_account(AccountAny::Cash(account.clone()))
            .unwrap();

        let manager = AccountsManager::new(clock, cache);

        // Create instrument
        let instrument = audusd_sim();

        // Create multiple orders for the same instrument
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

        // Submit and accept orders to mark them as open
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

        // Act
        let result = manager.update_orders(
            &AccountAny::Cash(account),
            InstrumentAny::CurrencyPair(instrument),
            orders,
            UnixNanos::default(),
        );

        // Assert
        assert!(result.is_some());
        let (updated_account, _state) = result.unwrap();

        if let AccountAny::Cash(cash_account) = updated_account {
            let locked_balance = cash_account.balance_locked(Some(usd));

            // Calculate expected locked balance in USD
            // Order 1: 100,000 * 0.75000 = 75,000 USD
            // Order 2: 50,000 * 0.74500 = 37,250 USD
            // Order 3: 75,000 * 0.74000 = 55,500 USD
            // Total: 167,750 USD
            let expected_locked = Money::new(167_750.0, usd);

            assert_eq!(locked_balance, Some(expected_locked));

            // Verify no locked balance in AUD (should all be converted to base USD)
            let aud = Currency::AUD();
            assert_eq!(cash_account.balance_locked(Some(aud)), None);
        } else {
            panic!("Expected CashAccount");
        }
    }
}
