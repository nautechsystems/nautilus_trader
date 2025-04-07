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

// TODO: Under development
#![allow(dead_code)] // For PortfolioConfig

//! Provides a generic `Portfolio` for all environments.
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use nautilus_analysis::analyzer::PortfolioAnalyzer;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    msgbus::{
        self,
        handler::{ShareableMessageHandler, TypedMessageHandler},
    },
};
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, QuoteTick},
    enums::{OrderSide, OrderType, PositionSide, PriceType},
    events::{AccountState, OrderEventAny, position::PositionEvent},
    identifiers::{InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    position::Position,
    types::{Currency, Money, Price},
};
use rust_decimal::{Decimal, prelude::FromPrimitive};
use ustr::Ustr;

use crate::{config::PortfolioConfig, manager::AccountsManager};

struct PortfolioState {
    accounts: AccountsManager,
    analyzer: PortfolioAnalyzer,
    unrealized_pnls: HashMap<InstrumentId, Money>,
    realized_pnls: HashMap<InstrumentId, Money>,
    net_positions: HashMap<InstrumentId, Decimal>,
    pending_calcs: HashSet<InstrumentId>,
    bar_close_prices: HashMap<InstrumentId, Price>,
    initialized: bool,
}

impl PortfolioState {
    fn new(clock: Rc<RefCell<dyn Clock>>, cache: Rc<RefCell<Cache>>) -> Self {
        Self {
            accounts: AccountsManager::new(clock, cache),
            analyzer: PortfolioAnalyzer::default(),
            unrealized_pnls: HashMap::new(),
            realized_pnls: HashMap::new(),
            net_positions: HashMap::new(),
            pending_calcs: HashSet::new(),
            bar_close_prices: HashMap::new(),
            initialized: false,
        }
    }

    fn reset(&mut self) {
        log::debug!("RESETTING");
        self.net_positions.clear();
        self.unrealized_pnls.clear();
        self.realized_pnls.clear();
        self.pending_calcs.clear();
        self.analyzer.reset();
        log::debug!("READY");
    }
}

pub struct Portfolio {
    pub(crate) clock: Rc<RefCell<dyn Clock>>,
    pub(crate) cache: Rc<RefCell<Cache>>,
    inner: Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
}

impl Portfolio {
    pub fn new(
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
        config: Option<PortfolioConfig>,
    ) -> Self {
        let inner = Rc::new(RefCell::new(PortfolioState::new(
            clock.clone(),
            cache.clone(),
        )));
        let config = config.unwrap_or_default();

        Self::register_message_handlers(
            cache.clone(),
            clock.clone(),
            inner.clone(),
            config.bar_updates,
        );

        Self {
            clock,
            cache,
            inner,
            config,
        }
    }

    fn register_message_handlers(
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
        inner: Rc<RefCell<PortfolioState>>,
        bar_updates: bool,
    ) {
        let update_account_handler = {
            let cache = cache.clone();
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |event: &AccountState| {
                    update_account(cache.clone(), event);
                },
            )))
        };

        let update_position_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner.clone();
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |event: &PositionEvent| {
                    update_position(cache.clone(), clock.clone(), inner.clone(), event);
                },
            )))
        };

        let update_quote_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner.clone();
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |quote: &QuoteTick| {
                    update_quote_tick(cache.clone(), clock.clone(), inner.clone(), quote);
                },
            )))
        };

        let update_bar_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner.clone();
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(move |bar: &Bar| {
                update_bar(cache.clone(), clock.clone(), inner.clone(), bar);
            })))
        };

        let update_order_handler = {
            let cache = cache;
            let clock = clock.clone();
            let inner = inner;
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |event: &OrderEventAny| {
                    update_order(cache.clone(), clock.clone(), inner.clone(), event);
                },
            )))
        };

        msgbus::register("Portfolio.update_account", update_account_handler.clone());

        msgbus::subscribe("data.quotes.*", update_quote_handler, Some(10));
        if bar_updates {
            msgbus::subscribe("data.quotes.*EXTERNAL", update_bar_handler, Some(10));
        }
        msgbus::subscribe("events.order.*", update_order_handler, Some(10));
        msgbus::subscribe("events.position.*", update_position_handler, Some(10));
        msgbus::subscribe("events.account.*", update_account_handler, Some(10));
    }

    pub fn reset(&mut self) {
        log::debug!("RESETTING");
        self.inner.borrow_mut().reset();
        log::debug!("READY");
    }

    // -- QUERIES ---------------------------------------------------------------------------------

    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.inner.borrow().initialized
    }

    #[must_use]
    pub fn balances_locked(&self, venue: &Venue) -> HashMap<Currency, Money> {
        self.cache.borrow().account_for_venue(venue).map_or_else(
            || {
                log::error!("Cannot get balances locked: no account generated for {venue}");
                HashMap::new()
            },
            AccountAny::balances_locked,
        )
    }

    #[must_use]
    pub fn margins_init(&self, venue: &Venue) -> HashMap<InstrumentId, Money> {
        self.cache.borrow().account_for_venue(venue).map_or_else(
            || {
                log::error!(
                    "Cannot get initial (order) margins: no account registered for {venue}"
                );
                HashMap::new()
            },
            |account| match account {
                AccountAny::Margin(margin_account) => margin_account.initial_margins(),
                AccountAny::Cash(_) => {
                    log::warn!("Initial margins not applicable for cash account");
                    HashMap::new()
                }
            },
        )
    }

    #[must_use]
    pub fn margins_maint(&self, venue: &Venue) -> HashMap<InstrumentId, Money> {
        self.cache.borrow().account_for_venue(venue).map_or_else(
            || {
                log::error!(
                    "Cannot get maintenance (position) margins: no account registered for {venue}"
                );
                HashMap::new()
            },
            |account| match account {
                AccountAny::Margin(margin_account) => margin_account.maintenance_margins(),
                AccountAny::Cash(_) => {
                    log::warn!("Maintenance margins not applicable for cash account");
                    HashMap::new()
                }
            },
        )
    }

    #[must_use]
    pub fn unrealized_pnls(&mut self, venue: &Venue) -> HashMap<Currency, Money> {
        let instrument_ids = {
            let cache = self.cache.borrow();
            let positions = cache.positions(Some(venue), None, None, None);

            if positions.is_empty() {
                return HashMap::new(); // Nothing to calculate
            }

            let instrument_ids: HashSet<InstrumentId> =
                positions.iter().map(|p| p.instrument_id).collect();

            instrument_ids
        };

        let mut unrealized_pnls: HashMap<Currency, f64> = HashMap::new();

        for instrument_id in instrument_ids {
            if let Some(&pnl) = self.inner.borrow_mut().unrealized_pnls.get(&instrument_id) {
                // PnL already calculated
                *unrealized_pnls.entry(pnl.currency).or_insert(0.0) += pnl.as_f64();
                continue;
            }

            // Calculate PnL
            match self.calculate_unrealized_pnl(&instrument_id) {
                Some(pnl) => *unrealized_pnls.entry(pnl.currency).or_insert(0.0) += pnl.as_f64(),
                None => continue,
            }
        }

        unrealized_pnls
            .into_iter()
            .map(|(currency, amount)| (currency, Money::new(amount, currency)))
            .collect()
    }

    #[must_use]
    pub fn realized_pnls(&mut self, venue: &Venue) -> HashMap<Currency, Money> {
        let instrument_ids = {
            let cache = self.cache.borrow();
            let positions = cache.positions(Some(venue), None, None, None);

            if positions.is_empty() {
                return HashMap::new(); // Nothing to calculate
            }

            let instrument_ids: HashSet<InstrumentId> =
                positions.iter().map(|p| p.instrument_id).collect();

            instrument_ids
        };

        let mut realized_pnls: HashMap<Currency, f64> = HashMap::new();

        for instrument_id in instrument_ids {
            if let Some(&pnl) = self.inner.borrow_mut().realized_pnls.get(&instrument_id) {
                // PnL already calculated
                *realized_pnls.entry(pnl.currency).or_insert(0.0) += pnl.as_f64();
                continue;
            }

            // Calculate PnL
            match self.calculate_realized_pnl(&instrument_id) {
                Some(pnl) => *realized_pnls.entry(pnl.currency).or_insert(0.0) += pnl.as_f64(),
                None => continue,
            }
        }

        realized_pnls
            .into_iter()
            .map(|(currency, amount)| (currency, Money::new(amount, currency)))
            .collect()
    }

    #[must_use]
    pub fn net_exposures(&self, venue: &Venue) -> Option<HashMap<Currency, Money>> {
        let cache = self.cache.borrow();
        let account = if let Some(account) = cache.account_for_venue(venue) {
            account
        } else {
            log::error!("Cannot calculate net exposures: no account registered for {venue}");
            return None; // Cannot calculate
        };

        let positions_open = cache.positions_open(Some(venue), None, None, None);
        if positions_open.is_empty() {
            return Some(HashMap::new()); // Nothing to calculate
        }

        let mut net_exposures: HashMap<Currency, f64> = HashMap::new();

        for position in positions_open {
            let instrument = if let Some(instrument) = cache.instrument(&position.instrument_id) {
                instrument
            } else {
                log::error!(
                    "Cannot calculate net exposures: no instrument for {}",
                    position.instrument_id
                );
                return None; // Cannot calculate
            };

            if position.side == PositionSide::Flat {
                log::error!(
                    "Cannot calculate net exposures: position is flat for {}",
                    position.instrument_id
                );
                continue; // Nothing to calculate
            }

            let price = self.get_price(position)?;
            let xrate = if let Some(xrate) =
                self.calculate_xrate_to_base(instrument, account, position.entry)
            {
                xrate
            } else {
                log::error!(
                    // TODO: Improve logging
                    "Cannot calculate net exposures: insufficient data for {}/{:?}",
                    instrument.settlement_currency(),
                    account.base_currency()
                );
                return None; // Cannot calculate
            };

            let settlement_currency = account
                .base_currency()
                .unwrap_or_else(|| instrument.settlement_currency());

            let net_exposure = instrument
                .calculate_notional_value(position.quantity, price, None)
                .as_f64()
                * xrate;

            let net_exposure = (net_exposure * 10f64.powi(settlement_currency.precision.into()))
                .round()
                / 10f64.powi(settlement_currency.precision.into());

            *net_exposures.entry(settlement_currency).or_insert(0.0) += net_exposure;
        }

        Some(
            net_exposures
                .into_iter()
                .map(|(currency, amount)| (currency, Money::new(amount, currency)))
                .collect(),
        )
    }

    #[must_use]
    pub fn unrealized_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        if let Some(pnl) = self
            .inner
            .borrow()
            .unrealized_pnls
            .get(instrument_id)
            .copied()
        {
            return Some(pnl);
        }

        let pnl = self.calculate_unrealized_pnl(instrument_id)?;
        self.inner
            .borrow_mut()
            .unrealized_pnls
            .insert(*instrument_id, pnl);
        Some(pnl)
    }

    #[must_use]
    pub fn realized_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        if let Some(pnl) = self
            .inner
            .borrow()
            .realized_pnls
            .get(instrument_id)
            .copied()
        {
            return Some(pnl);
        }

        let pnl = self.calculate_realized_pnl(instrument_id)?;
        self.inner
            .borrow_mut()
            .realized_pnls
            .insert(*instrument_id, pnl);
        Some(pnl)
    }

    #[must_use]
    pub fn net_exposure(&self, instrument_id: &InstrumentId) -> Option<Money> {
        let cache = self.cache.borrow();
        let account = if let Some(account) = cache.account_for_venue(&instrument_id.venue) {
            account
        } else {
            log::error!(
                "Cannot calculate net exposure: no account registered for {}",
                instrument_id.venue
            );
            return None;
        };

        let instrument = if let Some(instrument) = cache.instrument(instrument_id) {
            instrument
        } else {
            log::error!("Cannot calculate net exposure: no instrument for {instrument_id}");
            return None;
        };

        let positions_open = cache.positions_open(
            None, // Faster query filtering
            Some(instrument_id),
            None,
            None,
        );

        if positions_open.is_empty() {
            return Some(Money::new(0.0, instrument.settlement_currency()));
        }

        let mut net_exposure = 0.0;

        for position in positions_open {
            let price = self.get_price(position)?;
            let xrate = if let Some(xrate) =
                self.calculate_xrate_to_base(instrument, account, position.entry)
            {
                xrate
            } else {
                log::error!(
                    // TODO: Improve logging
                    "Cannot calculate net exposures: insufficient data for {}/{:?}",
                    instrument.settlement_currency(),
                    account.base_currency()
                );
                return None; // Cannot calculate
            };

            let notional_value =
                instrument.calculate_notional_value(position.quantity, price, None);
            net_exposure += notional_value.as_f64() * xrate;
        }

        let settlement_currency = account
            .base_currency()
            .unwrap_or_else(|| instrument.settlement_currency());

        Some(Money::new(net_exposure, settlement_currency))
    }

    #[must_use]
    pub fn net_position(&self, instrument_id: &InstrumentId) -> Decimal {
        self.inner
            .borrow()
            .net_positions
            .get(instrument_id)
            .copied()
            .unwrap_or(Decimal::ZERO)
    }

    #[must_use]
    pub fn is_net_long(&self, instrument_id: &InstrumentId) -> bool {
        self.inner
            .borrow()
            .net_positions
            .get(instrument_id)
            .copied()
            .map_or_else(|| false, |net_position| net_position > Decimal::ZERO)
    }

    #[must_use]
    pub fn is_net_short(&self, instrument_id: &InstrumentId) -> bool {
        self.inner
            .borrow()
            .net_positions
            .get(instrument_id)
            .copied()
            .map_or_else(|| false, |net_position| net_position < Decimal::ZERO)
    }

    #[must_use]
    pub fn is_flat(&self, instrument_id: &InstrumentId) -> bool {
        self.inner
            .borrow()
            .net_positions
            .get(instrument_id)
            .copied()
            .map_or_else(|| true, |net_position| net_position == Decimal::ZERO)
    }

    #[must_use]
    pub fn is_completely_flat(&self) -> bool {
        for net_position in self.inner.borrow().net_positions.values() {
            if *net_position != Decimal::ZERO {
                return false;
            }
        }
        true
    }

    // -- COMMANDS --------------------------------------------------------------------------------

    pub fn initialize_orders(&mut self) {
        let mut initialized = true;
        let orders_and_instruments = {
            let cache = self.cache.borrow();
            let all_orders_open = cache.orders_open(None, None, None, None);

            let mut instruments_with_orders = Vec::new();
            let mut instruments = HashSet::new();

            for order in &all_orders_open {
                instruments.insert(order.instrument_id());
            }

            for instrument_id in instruments {
                if let Some(instrument) = cache.instrument(&instrument_id) {
                    let orders = cache
                        .orders_open(None, Some(&instrument_id), None, None)
                        .into_iter()
                        .cloned()
                        .collect::<Vec<OrderAny>>();
                    instruments_with_orders.push((instrument.clone(), orders));
                } else {
                    log::error!(
                        "Cannot update initial (order) margin: no instrument found for {instrument_id}"
                    );
                    initialized = false;
                    break;
                }
            }
            instruments_with_orders
        };

        for (instrument, orders_open) in &orders_and_instruments {
            let mut cache = self.cache.borrow_mut();
            let account = if let Some(account) = cache.account_for_venue(&instrument.id().venue) {
                account
            } else {
                log::error!(
                    "Cannot update initial (order) margin: no account registered for {}",
                    instrument.id().venue
                );
                initialized = false;
                break;
            };

            let result = self.inner.borrow_mut().accounts.update_orders(
                account,
                instrument.clone(),
                orders_open.iter().collect(),
                self.clock.borrow().timestamp_ns(),
            );

            match result {
                Some((updated_account, _)) => {
                    cache.add_account(updated_account).unwrap(); // Temp Fix to update the mutated account
                }
                None => {
                    initialized = false;
                }
            }
        }

        let total_orders = orders_and_instruments
            .into_iter()
            .map(|(_, orders)| orders.len())
            .sum::<usize>();

        log::info!(
            "Initialized {} open order{}",
            total_orders,
            if total_orders == 1 { "" } else { "s" }
        );

        self.inner.borrow_mut().initialized = initialized;
    }

    pub fn initialize_positions(&mut self) {
        self.inner.borrow_mut().unrealized_pnls.clear();
        self.inner.borrow_mut().realized_pnls.clear();
        let all_positions_open: Vec<Position>;
        let mut instruments = HashSet::new();
        {
            let cache = self.cache.borrow();
            all_positions_open = cache
                .positions_open(None, None, None, None)
                .into_iter()
                .cloned()
                .collect();
            for position in &all_positions_open {
                instruments.insert(position.instrument_id);
            }
        }

        let mut initialized = true;

        for instrument_id in instruments {
            let positions_open: Vec<Position> = {
                let cache = self.cache.borrow();
                cache
                    .positions_open(None, Some(&instrument_id), None, None)
                    .into_iter()
                    .cloned()
                    .collect()
            };

            self.update_net_position(&instrument_id, positions_open);

            let calculated_unrealized_pnl = self
                .calculate_unrealized_pnl(&instrument_id)
                .expect("Failed to calculate unrealized PnL");
            let calculated_realized_pnl = self
                .calculate_realized_pnl(&instrument_id)
                .expect("Failed to calculate realized PnL");

            self.inner
                .borrow_mut()
                .unrealized_pnls
                .insert(instrument_id, calculated_unrealized_pnl);
            self.inner
                .borrow_mut()
                .realized_pnls
                .insert(instrument_id, calculated_realized_pnl);

            let cache = self.cache.borrow();
            let account = if let Some(account) = cache.account_for_venue(&instrument_id.venue) {
                account
            } else {
                log::error!(
                    "Cannot update maintenance (position) margin: no account registered for {}",
                    instrument_id.venue
                );
                initialized = false;
                break;
            };

            let account = match account {
                AccountAny::Cash(_) => continue,
                AccountAny::Margin(margin_account) => margin_account,
            };

            let mut cache = self.cache.borrow_mut();
            let instrument = if let Some(instrument) = cache.instrument(&instrument_id) {
                instrument
            } else {
                log::error!(
                    "Cannot update maintenance (position) margin: no instrument found for {instrument_id}"
                );
                initialized = false;
                break;
            };

            let result = self.inner.borrow_mut().accounts.update_positions(
                account,
                instrument.clone(),
                self.cache
                    .borrow()
                    .positions_open(None, Some(&instrument_id), None, None),
                self.clock.borrow().timestamp_ns(),
            );

            match result {
                Some((updated_account, _)) => {
                    cache
                        .add_account(AccountAny::Margin(updated_account)) // Temp Fix to update the mutated account
                        .unwrap();
                }
                None => {
                    initialized = false;
                }
            }
        }

        let open_count = all_positions_open.len();
        self.inner.borrow_mut().initialized = initialized;
        log::info!(
            "Initialized {} open position{}",
            open_count,
            if open_count == 1 { "" } else { "s" }
        );
    }

    pub fn update_quote_tick(&mut self, quote: &QuoteTick) {
        update_quote_tick(
            self.cache.clone(),
            self.clock.clone(),
            self.inner.clone(),
            quote,
        );
    }

    pub fn update_bar(&mut self, bar: &Bar) {
        update_bar(
            self.cache.clone(),
            self.clock.clone(),
            self.inner.clone(),
            bar,
        );
    }

    pub fn update_account(&mut self, event: &AccountState) {
        update_account(self.cache.clone(), event);
    }

    pub fn update_order(&mut self, event: &OrderEventAny) {
        update_order(
            self.cache.clone(),
            self.clock.clone(),
            self.inner.clone(),
            event,
        );
    }

    pub fn update_position(&mut self, event: &PositionEvent) {
        update_position(
            self.cache.clone(),
            self.clock.clone(),
            self.inner.clone(),
            event,
        );
    }

    // -- INTERNAL --------------------------------------------------------------------------------

    fn update_net_position(&mut self, instrument_id: &InstrumentId, positions_open: Vec<Position>) {
        let mut net_position = Decimal::ZERO;

        for open_position in positions_open {
            log::debug!("open_position: {open_position}");
            net_position += Decimal::from_f64(open_position.signed_qty).unwrap_or(Decimal::ZERO);
        }

        let existing_position = self.net_position(instrument_id);
        if existing_position != net_position {
            self.inner
                .borrow_mut()
                .net_positions
                .insert(*instrument_id, net_position);
            log::info!("{instrument_id} net_position={net_position}");
        }
    }

    fn calculate_unrealized_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        let cache = self.cache.borrow();
        let account = if let Some(account) = cache.account_for_venue(&instrument_id.venue) {
            account
        } else {
            log::error!(
                "Cannot calculate unrealized PnL: no account registered for {}",
                instrument_id.venue
            );
            return None;
        };

        let instrument = if let Some(instrument) = cache.instrument(instrument_id) {
            instrument
        } else {
            log::error!("Cannot calculate unrealized PnL: no instrument for {instrument_id}");
            return None;
        };

        let currency = account
            .base_currency()
            .unwrap_or_else(|| instrument.settlement_currency());

        let positions_open = cache.positions_open(
            None, // Faster query filtering
            Some(instrument_id),
            None,
            None,
        );

        if positions_open.is_empty() {
            return Some(Money::new(0.0, currency));
        }

        let mut total_pnl = 0.0;

        for position in positions_open {
            if position.instrument_id != *instrument_id {
                continue; // Nothing to calculate
            }

            if position.side == PositionSide::Flat {
                continue; // Nothing to calculate
            }

            let price = if let Some(price) = self.get_price(position) {
                price
            } else {
                log::debug!("Cannot calculate unrealized PnL: no prices for {instrument_id}");
                self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                return None; // Cannot calculate
            };

            let mut pnl = position.unrealized_pnl(price).as_f64();

            if let Some(base_currency) = account.base_currency() {
                let xrate = if let Some(xrate) =
                    self.calculate_xrate_to_base(instrument, account, position.entry)
                {
                    xrate
                } else {
                    log::error!(
                        // TODO: Improve logging
                        "Cannot calculate unrealized PnL: insufficient data for {}/{}",
                        instrument.settlement_currency(),
                        base_currency
                    );
                    self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                    return None; // Cannot calculate
                };

                let scale = 10f64.powi(currency.precision.into());
                pnl = ((pnl * xrate) * scale).round() / scale;
            }

            total_pnl += pnl;
        }

        Some(Money::new(total_pnl, currency))
    }

    fn calculate_realized_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        let cache = self.cache.borrow();
        let account = if let Some(account) = cache.account_for_venue(&instrument_id.venue) {
            account
        } else {
            log::error!(
                "Cannot calculate realized PnL: no account registered for {}",
                instrument_id.venue
            );
            return None;
        };

        let instrument = if let Some(instrument) = cache.instrument(instrument_id) {
            instrument
        } else {
            log::error!("Cannot calculate realized PnL: no instrument for {instrument_id}");
            return None;
        };

        let currency = account
            .base_currency()
            .unwrap_or_else(|| instrument.settlement_currency());

        let positions = cache.positions(
            None, // Faster query filtering
            Some(instrument_id),
            None,
            None,
        );

        if positions.is_empty() {
            return Some(Money::new(0.0, currency));
        }

        let mut total_pnl = 0.0;

        for position in positions {
            if position.instrument_id != *instrument_id {
                continue; // Nothing to calculate
            }

            if position.realized_pnl.is_none() {
                continue; // Nothing to calculate
            }

            let mut pnl = position.realized_pnl?.as_f64();

            if let Some(base_currency) = account.base_currency() {
                let xrate = if let Some(xrate) =
                    self.calculate_xrate_to_base(instrument, account, position.entry)
                {
                    xrate
                } else {
                    log::error!(
                        // TODO: Improve logging
                        "Cannot calculate realized PnL: insufficient data for {}/{}",
                        instrument.settlement_currency(),
                        base_currency
                    );
                    self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                    return None; // Cannot calculate
                };

                let scale = 10f64.powi(currency.precision.into());
                pnl = ((pnl * xrate) * scale).round() / scale;
            }

            total_pnl += pnl;
        }

        Some(Money::new(total_pnl, currency))
    }

    fn get_price(&self, position: &Position) -> Option<Price> {
        let price_type = match position.side {
            PositionSide::Long => PriceType::Bid,
            PositionSide::Short => PriceType::Ask,
            _ => panic!("invalid `PositionSide`, was {}", position.side),
        };

        let cache = self.cache.borrow();

        let instrument_id = &position.instrument_id;
        cache
            .price(instrument_id, price_type)
            .or_else(|| cache.price(instrument_id, PriceType::Last))
            .or_else(|| {
                self.inner
                    .borrow()
                    .bar_close_prices
                    .get(instrument_id)
                    .copied()
            })
    }

    fn calculate_xrate_to_base(
        &self,
        instrument: &InstrumentAny,
        account: &AccountAny,
        side: OrderSide,
    ) -> Option<f64> {
        if !self.config.convert_to_account_base_currency {
            return Some(1.0); // No conversion needed
        }

        match account.base_currency() {
            None => Some(1.0), // No conversion needed
            Some(base_currency) => {
                let cache = self.cache.borrow();

                if self.config.use_mark_xrates {
                    return cache.get_mark_xrate(instrument.settlement_currency(), base_currency);
                }

                let price_type = if side == OrderSide::Buy {
                    PriceType::Bid
                } else {
                    PriceType::Ask
                };

                cache.get_xrate(
                    instrument.id().venue,
                    instrument.settlement_currency(),
                    base_currency,
                    price_type,
                )
            }
        }
    }
}

// Helper functions
fn update_quote_tick(
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<dyn Clock>>,
    inner: Rc<RefCell<PortfolioState>>,
    quote: &QuoteTick,
) {
    update_instrument_id(cache, clock.clone(), inner, &quote.instrument_id);
}

fn update_bar(
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<dyn Clock>>,
    inner: Rc<RefCell<PortfolioState>>,
    bar: &Bar,
) {
    let instrument_id = bar.bar_type.instrument_id();
    inner
        .borrow_mut()
        .bar_close_prices
        .insert(instrument_id, bar.close);
    update_instrument_id(cache, clock.clone(), inner, &instrument_id);
}

fn update_instrument_id(
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<dyn Clock>>,
    inner: Rc<RefCell<PortfolioState>>,
    instrument_id: &InstrumentId,
) {
    inner.borrow_mut().unrealized_pnls.remove(instrument_id);

    if inner.borrow().initialized || !inner.borrow().pending_calcs.contains(instrument_id) {
        return;
    }

    let result_init;
    let mut result_maint = None;

    let account = {
        let borrowed_cache = cache.borrow();
        let account = if let Some(account) = borrowed_cache.account_for_venue(&instrument_id.venue)
        {
            account
        } else {
            log::error!(
                "Cannot update tick: no account registered for {}",
                instrument_id.venue
            );
            return;
        };

        let mut borrowed_cache = cache.borrow_mut();
        let instrument = if let Some(instrument) = borrowed_cache.instrument(instrument_id) {
            instrument.clone()
        } else {
            log::error!("Cannot update tick: no instrument found for {instrument_id}");
            return;
        };

        // Clone the orders and positions to own the data
        let orders_open: Vec<OrderAny> = borrowed_cache
            .orders_open(None, Some(instrument_id), None, None)
            .iter()
            .map(|o| (*o).clone())
            .collect();

        let positions_open: Vec<Position> = borrowed_cache
            .positions_open(None, Some(instrument_id), None, None)
            .iter()
            .map(|p| (*p).clone())
            .collect();

        result_init = inner.borrow().accounts.update_orders(
            account,
            instrument.clone(),
            orders_open.iter().collect(),
            clock.borrow().timestamp_ns(),
        );

        if let AccountAny::Margin(margin_account) = account {
            result_maint = inner.borrow().accounts.update_positions(
                margin_account,
                instrument,
                positions_open.iter().collect(),
                clock.borrow().timestamp_ns(),
            );
        }

        if let Some((ref updated_account, _)) = result_init {
            borrowed_cache.add_account(updated_account.clone()).unwrap(); // Temp Fix to update the mutated account
        }
        account.clone()
    };

    let mut portfolio_clone = Portfolio {
        clock: clock.clone(),
        cache,
        inner: inner.clone(),
        config: PortfolioConfig::default(), // TODO: TBD
    };

    let result_unrealized_pnl: Option<Money> =
        portfolio_clone.calculate_unrealized_pnl(instrument_id);

    if result_init.is_some()
        && (matches!(account, AccountAny::Cash(_))
            || (result_maint.is_some() && result_unrealized_pnl.is_some()))
    {
        inner.borrow_mut().pending_calcs.remove(instrument_id);
        if inner.borrow().pending_calcs.is_empty() {
            inner.borrow_mut().initialized = true;
        }
    }
}

fn update_order(
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<dyn Clock>>,
    inner: Rc<RefCell<PortfolioState>>,
    event: &OrderEventAny,
) {
    let borrowed_cache = cache.borrow();
    let account_id = match event.account_id() {
        Some(account_id) => account_id,
        None => {
            return; // No Account Assigned
        }
    };

    let account = if let Some(account) = borrowed_cache.account(&account_id) {
        account
    } else {
        log::error!("Cannot update order: no account registered for {account_id}");
        return;
    };

    match account {
        AccountAny::Cash(cash_account) => {
            if !cash_account.base.calculate_account_state {
                return;
            }
        }
        AccountAny::Margin(margin_account) => {
            if !margin_account.base.calculate_account_state {
                return;
            }
        }
    }

    match event {
        OrderEventAny::Accepted(_)
        | OrderEventAny::Canceled(_)
        | OrderEventAny::Rejected(_)
        | OrderEventAny::Updated(_)
        | OrderEventAny::Filled(_) => {}
        _ => {
            return;
        }
    }

    let borrowed_cache = cache.borrow();
    let order = if let Some(order) = borrowed_cache.order(&event.client_order_id()) {
        order
    } else {
        log::error!(
            "Cannot update order: {} not found in the cache",
            event.client_order_id()
        );
        return; // No Order Found
    };

    if matches!(event, OrderEventAny::Rejected(_)) && order.order_type() != OrderType::StopLimit {
        return; // No change to account state
    }

    let instrument = if let Some(instrument_id) = borrowed_cache.instrument(&event.instrument_id())
    {
        instrument_id
    } else {
        log::error!(
            "Cannot update order: no instrument found for {}",
            event.instrument_id()
        );
        return;
    };

    if let OrderEventAny::Filled(order_filled) = event {
        let _ = inner.borrow().accounts.update_balances(
            account.clone(),
            instrument.clone(),
            *order_filled,
        );

        let mut portfolio_clone = Portfolio {
            clock: clock.clone(),
            cache: cache.clone(),
            inner: inner.clone(),
            config: PortfolioConfig::default(), // TODO: TBD
        };

        match portfolio_clone.calculate_unrealized_pnl(&order_filled.instrument_id) {
            Some(unrealized_pnl) => {
                inner
                    .borrow_mut()
                    .unrealized_pnls
                    .insert(event.instrument_id(), unrealized_pnl);
            }
            None => {
                log::error!(
                    "Failed to calculate unrealized PnL for instrument {}",
                    event.instrument_id()
                );
            }
        }
    }

    let orders_open = borrowed_cache.orders_open(None, Some(&event.instrument_id()), None, None);

    let account_state = inner.borrow_mut().accounts.update_orders(
        account,
        instrument.clone(),
        orders_open,
        clock.borrow().timestamp_ns(),
    );

    let mut borrowed_cache = cache.borrow_mut();
    borrowed_cache.update_account(account.clone()).unwrap();

    if let Some(account_state) = account_state {
        msgbus::publish(
            &Ustr::from(&format!("events.account.{}", account.id())),
            &account_state,
        );
    } else {
        log::debug!("Added pending calculation for {}", instrument.id());
        inner.borrow_mut().pending_calcs.insert(instrument.id());
    }

    log::debug!("Updated {event}");
}

fn update_position(
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<dyn Clock>>,
    inner: Rc<RefCell<PortfolioState>>,
    event: &PositionEvent,
) {
    let instrument_id = event.instrument_id();

    let positions_open: Vec<Position> = {
        let borrowed_cache = cache.borrow();

        borrowed_cache
            .positions_open(None, Some(&instrument_id), None, None)
            .iter()
            .map(|o| (*o).clone())
            .collect()
    };

    log::debug!("postion fresh from cache -> {positions_open:?}");

    let mut portfolio_clone = Portfolio {
        clock: clock.clone(),
        cache: cache.clone(),
        inner: inner.clone(),
        config: PortfolioConfig::default(), // TODO: TBD
    };

    portfolio_clone.update_net_position(&instrument_id, positions_open.clone());

    let calculated_unrealized_pnl = portfolio_clone
        .calculate_unrealized_pnl(&instrument_id)
        .expect("Failed to calculate unrealized PnL");
    let calculated_realized_pnl = portfolio_clone
        .calculate_realized_pnl(&instrument_id)
        .expect("Failed to calculate realized PnL");

    inner
        .borrow_mut()
        .unrealized_pnls
        .insert(event.instrument_id(), calculated_unrealized_pnl);
    inner
        .borrow_mut()
        .realized_pnls
        .insert(event.instrument_id(), calculated_realized_pnl);

    let borrowed_cache = cache.borrow();
    let account = borrowed_cache.account(&event.account_id());

    if let Some(AccountAny::Margin(margin_account)) = account {
        if !margin_account.calculate_account_state {
            return; // Nothing to calculate
        }

        let borrowed_cache = cache.borrow();
        let instrument = if let Some(instrument) = borrowed_cache.instrument(&instrument_id) {
            instrument
        } else {
            log::error!("Cannot update position: no instrument found for {instrument_id}");
            return;
        };

        let result = inner.borrow_mut().accounts.update_positions(
            margin_account,
            instrument.clone(),
            positions_open.iter().collect(),
            clock.borrow().timestamp_ns(),
        );
        let mut borrowed_cache = cache.borrow_mut();
        if let Some((margin_account, _)) = result {
            borrowed_cache
                .add_account(AccountAny::Margin(margin_account)) // Temp Fix to update the mutated account
                .unwrap();
        }
    } else if account.is_none() {
        log::error!(
            "Cannot update position: no account registered for {}",
            event.account_id()
        );
    }
}

pub fn update_account(cache: Rc<RefCell<Cache>>, event: &AccountState) {
    let mut borrowed_cache = cache.borrow_mut();

    if let Some(existing) = borrowed_cache.account(&event.account_id) {
        let mut account = existing.clone();
        account.apply(event.clone());

        if let Err(e) = borrowed_cache.update_account(account.clone()) {
            log::error!("Failed to update account: {e}");
            return;
        }
    } else {
        let account = match AccountAny::from_events(vec![event.clone()]) {
            Ok(account) => account,
            Err(e) => {
                log::error!("Failed to create account: {e}");
                return;
            }
        };

        if let Err(e) = borrowed_cache.add_account(account) {
            log::error!("Failed to add account: {e}");
            return;
        }
    }

    log::info!("Updated {event}");
}
