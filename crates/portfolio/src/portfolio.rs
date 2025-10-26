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

//! Provides a generic `Portfolio` for all environments.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt::Debug,
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
use nautilus_core::{WeakCell, datetime::NANOSECONDS_IN_MILLISECOND};
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, MarkPriceUpdate, QuoteTick},
    enums::{OmsType, OrderSide, OrderType, PositionSide, PriceType},
    events::{AccountState, OrderEventAny, position::PositionEvent},
    identifiers::{AccountId, InstrumentId, PositionId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    position::Position,
    types::{Currency, Money, Price},
};
use rust_decimal::{Decimal, prelude::FromPrimitive};

use crate::{config::PortfolioConfig, manager::AccountsManager};

struct PortfolioState {
    accounts: AccountsManager,
    analyzer: PortfolioAnalyzer,
    unrealized_pnls: HashMap<InstrumentId, Money>,
    realized_pnls: HashMap<InstrumentId, Money>,
    snapshot_sum_per_position: HashMap<PositionId, Money>,
    snapshot_last_per_position: HashMap<PositionId, Money>,
    snapshot_processed_counts: HashMap<PositionId, usize>,
    net_positions: HashMap<InstrumentId, Decimal>,
    pending_calcs: HashSet<InstrumentId>,
    bar_close_prices: HashMap<InstrumentId, Price>,
    initialized: bool,
    last_account_state_log_ts: HashMap<AccountId, u64>,
    min_account_state_logging_interval_ns: u64,
}

impl PortfolioState {
    fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        config: &PortfolioConfig,
    ) -> Self {
        let min_account_state_logging_interval_ns = config
            .min_account_state_logging_interval_ms
            .map_or(0, |ms| ms * NANOSECONDS_IN_MILLISECOND);

        Self {
            accounts: AccountsManager::new(clock, cache),
            analyzer: PortfolioAnalyzer::default(),
            unrealized_pnls: HashMap::new(),
            realized_pnls: HashMap::new(),
            snapshot_sum_per_position: HashMap::new(),
            snapshot_last_per_position: HashMap::new(),
            snapshot_processed_counts: HashMap::new(),
            net_positions: HashMap::new(),
            pending_calcs: HashSet::new(),
            bar_close_prices: HashMap::new(),
            initialized: false,
            last_account_state_log_ts: HashMap::new(),
            min_account_state_logging_interval_ns,
        }
    }

    fn reset(&mut self) {
        log::debug!("RESETTING");
        self.net_positions.clear();
        self.unrealized_pnls.clear();
        self.realized_pnls.clear();
        self.snapshot_sum_per_position.clear();
        self.snapshot_last_per_position.clear();
        self.snapshot_processed_counts.clear();
        self.pending_calcs.clear();
        self.last_account_state_log_ts.clear();
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

impl Debug for Portfolio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Portfolio)).finish()
    }
}

impl Portfolio {
    pub fn new(
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
        config: Option<PortfolioConfig>,
    ) -> Self {
        let config = config.unwrap_or_default();
        let inner = Rc::new(RefCell::new(PortfolioState::new(
            clock.clone(),
            cache.clone(),
            &config,
        )));

        Self::register_message_handlers(
            cache.clone(),
            clock.clone(),
            inner.clone(),
            config.clone(),
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
        config: PortfolioConfig,
    ) {
        let inner_weak = WeakCell::from(Rc::downgrade(&inner));

        let update_account_handler = {
            let cache = cache.clone();
            let inner = inner_weak.clone();
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |event: &AccountState| {
                    if let Some(inner_rc) = inner.upgrade() {
                        update_account(cache.clone(), inner_rc.into(), event);
                    }
                },
            )))
        };

        let update_position_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner_weak.clone();
            let config = config.clone();
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |event: &PositionEvent| {
                    if let Some(inner_rc) = inner.upgrade() {
                        update_position(
                            cache.clone(),
                            clock.clone(),
                            inner_rc.into(),
                            config.clone(),
                            event,
                        );
                    }
                },
            )))
        };

        let update_quote_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner_weak.clone();
            let config = config.clone();
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |quote: &QuoteTick| {
                    if let Some(inner_rc) = inner.upgrade() {
                        update_quote_tick(
                            cache.clone(),
                            clock.clone(),
                            inner_rc.into(),
                            config.clone(),
                            quote,
                        );
                    }
                },
            )))
        };

        let update_bar_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner_weak.clone();
            let config = config.clone();
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(move |bar: &Bar| {
                if let Some(inner_rc) = inner.upgrade() {
                    update_bar(
                        cache.clone(),
                        clock.clone(),
                        inner_rc.into(),
                        config.clone(),
                        bar,
                    );
                }
            })))
        };

        let update_mark_price_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner_weak.clone();
            let config = config.clone();
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |mark_price: &MarkPriceUpdate| {
                    if let Some(inner_rc) = inner.upgrade() {
                        update_instrument_id(
                            cache.clone(),
                            clock.clone(),
                            inner_rc.into(),
                            config.clone(),
                            &mark_price.instrument_id,
                        );
                    }
                },
            )))
        };

        let update_order_handler = {
            let cache = cache;
            let clock = clock.clone();
            let inner = inner_weak;
            let config = config.clone();
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |event: &OrderEventAny| {
                    if let Some(inner_rc) = inner.upgrade() {
                        update_order(
                            cache.clone(),
                            clock.clone(),
                            inner_rc.into(),
                            config.clone(),
                            event,
                        );
                    }
                },
            )))
        };

        msgbus::register(
            "Portfolio.update_account".into(),
            update_account_handler.clone(),
        );

        msgbus::subscribe("data.quotes.*".into(), update_quote_handler, Some(10));
        if config.bar_updates {
            msgbus::subscribe("data.bars.*EXTERNAL".into(), update_bar_handler, Some(10));
        }
        if config.use_mark_prices {
            msgbus::subscribe(
                "data.mark_prices.*".into(),
                update_mark_price_handler,
                Some(10),
            );
        }
        msgbus::subscribe("events.order.*".into(), update_order_handler, Some(10));
        msgbus::subscribe(
            "events.position.*".into(),
            update_position_handler,
            Some(10),
        );
        msgbus::subscribe("events.account.*".into(), update_account_handler, Some(10));
    }

    pub fn reset(&mut self) {
        log::debug!("RESETTING");
        self.inner.borrow_mut().reset();
        log::debug!("READY");
    }

    // -- QUERIES ---------------------------------------------------------------------------------

    /// Returns `true` if the portfolio has been initialized.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.inner.borrow().initialized
    }

    /// Returns the locked balances for the given venue.
    ///
    /// Locked balances represent funds reserved for open orders.
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

    /// Returns the initial margin requirements for the given venue.
    ///
    /// Only applicable for margin accounts. Returns empty map for cash accounts.
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

    /// Returns the maintenance margin requirements for the given venue.
    ///
    /// Only applicable for margin accounts. Returns empty map for cash accounts.
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

    /// Returns the unrealized PnLs for all positions at the given venue.
    ///
    /// Calculates mark-to-market PnL based on current market prices.
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

    /// Returns the realized PnLs for all positions at the given venue.
    ///
    /// Calculates total realized profit and loss from closed positions.
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

    /// Returns the total PnL for the given instrument ID.
    ///
    /// Total PnL = Realized PnL + Unrealized PnL
    #[must_use]
    pub fn total_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        let realized = self.realized_pnl(instrument_id)?;
        let unrealized = self.unrealized_pnl(instrument_id)?;

        if realized.currency != unrealized.currency {
            log::error!(
                "Cannot calculate total PnL: currency mismatch {} vs {}",
                realized.currency,
                unrealized.currency
            );
            return None;
        }

        Some(Money::new(
            realized.as_f64() + unrealized.as_f64(),
            realized.currency,
        ))
    }

    /// Returns the total PnLs for the given venue.
    ///
    /// Total PnL = Realized PnL + Unrealized PnL for each currency
    #[must_use]
    pub fn total_pnls(&mut self, venue: &Venue) -> HashMap<Currency, Money> {
        let realized_pnls = self.realized_pnls(venue);
        let unrealized_pnls = self.unrealized_pnls(venue);

        let mut total_pnls: HashMap<Currency, Money> = HashMap::new();

        // Add realized PnLs
        for (currency, realized) in realized_pnls {
            total_pnls.insert(currency, realized);
        }

        // Add unrealized PnLs
        for (currency, unrealized) in unrealized_pnls {
            match total_pnls.get_mut(&currency) {
                Some(total) => {
                    *total = Money::new(total.as_f64() + unrealized.as_f64(), currency);
                }
                None => {
                    total_pnls.insert(currency, unrealized);
                }
            }
        }

        total_pnls
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

    /// Initializes account margin based on existing open orders.
    ///
    /// # Panics
    ///
    /// Panics if updating the cache with a mutated account fails.
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
                    cache.update_account(updated_account).unwrap();
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

    /// Initializes account margin based on existing open positions.
    ///
    /// # Panics
    ///
    /// Panics if calculation of PnL or updating the cache with a mutated account fails.
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

            if let Some(calculated_unrealized_pnl) = self.calculate_unrealized_pnl(&instrument_id) {
                self.inner
                    .borrow_mut()
                    .unrealized_pnls
                    .insert(instrument_id, calculated_unrealized_pnl);
            } else {
                log::warn!(
                    "Failed to calculate unrealized PnL for {instrument_id}, marking as pending"
                );
                self.inner.borrow_mut().pending_calcs.insert(instrument_id);
            }

            if let Some(calculated_realized_pnl) = self.calculate_realized_pnl(&instrument_id) {
                self.inner
                    .borrow_mut()
                    .realized_pnls
                    .insert(instrument_id, calculated_realized_pnl);
            } else {
                log::warn!(
                    "Failed to calculate realized PnL for {instrument_id}, marking as pending"
                );
                self.inner.borrow_mut().pending_calcs.insert(instrument_id);
            }

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
                        .update_account(AccountAny::Margin(updated_account))
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

    /// Updates portfolio calculations based on a new quote tick.
    ///
    /// Recalculates unrealized PnL for positions affected by the quote update.
    pub fn update_quote_tick(&mut self, quote: &QuoteTick) {
        update_quote_tick(
            self.cache.clone(),
            self.clock.clone(),
            self.inner.clone(),
            self.config.clone(),
            quote,
        );
    }

    /// Updates portfolio calculations based on a new bar.
    ///
    /// Updates cached bar close prices and recalculates unrealized PnL.
    pub fn update_bar(&mut self, bar: &Bar) {
        update_bar(
            self.cache.clone(),
            self.clock.clone(),
            self.inner.clone(),
            self.config.clone(),
            bar,
        );
    }

    /// Updates portfolio with a new account state event.
    pub fn update_account(&mut self, event: &AccountState) {
        update_account(self.cache.clone(), self.inner.clone(), event);
    }

    /// Updates portfolio calculations based on an order event.
    ///
    /// Handles balance updates for order fills and margin calculations for order changes.
    pub fn update_order(&mut self, event: &OrderEventAny) {
        update_order(
            self.cache.clone(),
            self.clock.clone(),
            self.inner.clone(),
            self.config.clone(),
            event,
        );
    }

    /// Updates portfolio calculations based on a position event.
    ///
    /// Recalculates net positions, unrealized PnL, and margin requirements.
    pub fn update_position(&mut self, event: &PositionEvent) {
        update_position(
            self.cache.clone(),
            self.clock.clone(),
            self.inner.clone(),
            self.config.clone(),
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

    fn ensure_snapshot_pnls_cached_for(&mut self, instrument_id: &InstrumentId) {
        // Performance: This method maintains an incremental cache of snapshot PnLs
        // It only deserializes new snapshots that haven't been processed yet
        // Tracks sum and last PnL per position for efficient NETTING OMS support

        // Get all position IDs that have snapshots for this instrument
        let snapshot_position_ids = self.cache.borrow().position_snapshot_ids(instrument_id);

        if snapshot_position_ids.is_empty() {
            return; // Nothing to process
        }

        let mut rebuild = false;

        // Detect purge/reset (count regression) to trigger full rebuild
        for position_id in &snapshot_position_ids {
            let position_snapshots = self.cache.borrow().position_snapshot_bytes(position_id);
            let curr_count = position_snapshots.map_or(0, |s| {
                // Count the number of snapshots (they're serialized as JSON objects)
                s.split(|&b| b == b'{').count() - 1
            });
            let prev_count = self
                .inner
                .borrow()
                .snapshot_processed_counts
                .get(position_id)
                .copied()
                .unwrap_or(0);

            if prev_count > curr_count {
                rebuild = true;
                break;
            }
        }

        if rebuild {
            // Full rebuild: process all snapshots from scratch
            for position_id in &snapshot_position_ids {
                if let Some(position_snapshots) =
                    self.cache.borrow().position_snapshot_bytes(position_id)
                {
                    let mut sum_pnl: Option<Money> = None;
                    let mut last_pnl: Option<Money> = None;

                    // Snapshots are concatenated JSON objects
                    let mut start = 0;
                    let mut depth = 0;
                    let mut in_string = false;
                    let mut escape_next = false;

                    for (i, &byte) in position_snapshots.iter().enumerate() {
                        if escape_next {
                            escape_next = false;
                            continue;
                        }

                        if byte == b'\\' && in_string {
                            escape_next = true;
                            continue;
                        }

                        if byte == b'"' && !escape_next {
                            in_string = !in_string;
                        }

                        if !in_string {
                            if byte == b'{' {
                                if depth == 0 {
                                    start = i;
                                }
                                depth += 1;
                            } else if byte == b'}' {
                                depth -= 1;
                                if depth == 0
                                    && let Ok(snapshot) = serde_json::from_slice::<Position>(
                                        &position_snapshots[start..=i],
                                    )
                                    && let Some(realized_pnl) = snapshot.realized_pnl
                                {
                                    if let Some(ref mut sum) = sum_pnl {
                                        if sum.currency == realized_pnl.currency {
                                            *sum = Money::new(
                                                sum.as_f64() + realized_pnl.as_f64(),
                                                sum.currency,
                                            );
                                        }
                                    } else {
                                        sum_pnl = Some(realized_pnl);
                                    }
                                    last_pnl = Some(realized_pnl);
                                }
                            }
                        }
                    }

                    let mut inner = self.inner.borrow_mut();
                    if let Some(sum) = sum_pnl {
                        inner.snapshot_sum_per_position.insert(*position_id, sum);
                        if let Some(last) = last_pnl {
                            inner.snapshot_last_per_position.insert(*position_id, last);
                        }
                    } else {
                        inner.snapshot_sum_per_position.remove(position_id);
                        inner.snapshot_last_per_position.remove(position_id);
                    }

                    let snapshot_count = position_snapshots.split(|&b| b == b'{').count() - 1;
                    inner
                        .snapshot_processed_counts
                        .insert(*position_id, snapshot_count);
                }
            }
        } else {
            // Incremental path: only process new snapshots
            for position_id in &snapshot_position_ids {
                if let Some(position_snapshots) =
                    self.cache.borrow().position_snapshot_bytes(position_id)
                {
                    let curr_count = position_snapshots.split(|&b| b == b'{').count() - 1;
                    let prev_count = self
                        .inner
                        .borrow()
                        .snapshot_processed_counts
                        .get(position_id)
                        .copied()
                        .unwrap_or(0);

                    if prev_count >= curr_count {
                        continue;
                    }

                    let mut sum_pnl = self
                        .inner
                        .borrow()
                        .snapshot_sum_per_position
                        .get(position_id)
                        .copied();
                    let mut last_pnl = self
                        .inner
                        .borrow()
                        .snapshot_last_per_position
                        .get(position_id)
                        .copied();

                    // Process only new snapshots
                    let mut start = 0;
                    let mut depth = 0;
                    let mut in_string = false;
                    let mut escape_next = false;
                    let mut snapshot_index = 0;

                    for (i, &byte) in position_snapshots.iter().enumerate() {
                        if escape_next {
                            escape_next = false;
                            continue;
                        }

                        if byte == b'\\' && in_string {
                            escape_next = true;
                            continue;
                        }

                        if byte == b'"' && !escape_next {
                            in_string = !in_string;
                        }

                        if !in_string {
                            if byte == b'{' {
                                if depth == 0 {
                                    start = i;
                                }
                                depth += 1;
                            } else if byte == b'}' {
                                depth -= 1;
                                if depth == 0 {
                                    snapshot_index += 1;
                                    // Only process new snapshots
                                    if snapshot_index > prev_count
                                        && let Ok(snapshot) = serde_json::from_slice::<Position>(
                                            &position_snapshots[start..=i],
                                        )
                                        && let Some(realized_pnl) = snapshot.realized_pnl
                                    {
                                        if let Some(ref mut sum) = sum_pnl {
                                            if sum.currency == realized_pnl.currency {
                                                *sum = Money::new(
                                                    sum.as_f64() + realized_pnl.as_f64(),
                                                    sum.currency,
                                                );
                                            }
                                        } else {
                                            sum_pnl = Some(realized_pnl);
                                        }
                                        last_pnl = Some(realized_pnl);
                                    }
                                }
                            }
                        }
                    }

                    let mut inner = self.inner.borrow_mut();
                    if let Some(sum) = sum_pnl {
                        inner.snapshot_sum_per_position.insert(*position_id, sum);
                        if let Some(last) = last_pnl {
                            inner.snapshot_last_per_position.insert(*position_id, last);
                        }
                    }
                    inner
                        .snapshot_processed_counts
                        .insert(*position_id, curr_count);
                }
            }
        }
    }

    fn calculate_realized_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        // Ensure snapshot PnLs are cached for this instrument
        self.ensure_snapshot_pnls_cached_for(instrument_id);

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

        let snapshot_position_ids = cache.position_snapshot_ids(instrument_id);

        // Check if we need to use NETTING OMS logic
        let is_netting = positions
            .iter()
            .any(|p| cache.oms_type(&p.id) == Some(OmsType::Netting));

        let mut total_pnl = 0.0;

        if is_netting && !snapshot_position_ids.is_empty() {
            // NETTING OMS: Apply 3-case rule for position cycles

            for position_id in &snapshot_position_ids {
                let is_active = positions.iter().any(|p| p.id == *position_id);

                if is_active {
                    // Case 1 & 2: Active position - use only the last snapshot PnL
                    let last_pnl = self
                        .inner
                        .borrow()
                        .snapshot_last_per_position
                        .get(position_id)
                        .copied();
                    if let Some(last_pnl) = last_pnl {
                        let mut pnl = last_pnl.as_f64();

                        if let Some(base_currency) = account.base_currency()
                            && let Some(position) = positions.iter().find(|p| p.id == *position_id)
                        {
                            let xrate = if let Some(xrate) =
                                self.calculate_xrate_to_base(instrument, account, position.entry)
                            {
                                xrate
                            } else {
                                log::error!(
                                    "Cannot calculate realized PnL: insufficient exchange rate data for {}/{}, marking as pending calculation",
                                    instrument.settlement_currency(),
                                    base_currency
                                );
                                self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                                return Some(Money::new(0.0, currency));
                            };

                            let scale = 10f64.powi(currency.precision.into());
                            pnl = ((pnl * xrate) * scale).round() / scale;
                        }

                        total_pnl += pnl;
                    }
                } else {
                    // Case 3: Closed position - use sum of all snapshot PnLs
                    let sum_pnl = self
                        .inner
                        .borrow()
                        .snapshot_sum_per_position
                        .get(position_id)
                        .copied();
                    if let Some(sum_pnl) = sum_pnl {
                        let mut pnl = sum_pnl.as_f64();

                        if let Some(base_currency) = account.base_currency() {
                            // For closed positions, we don't have entry price, use current rates
                            let xrate = cache.get_xrate(
                                instrument_id.venue,
                                instrument.settlement_currency(),
                                base_currency,
                                PriceType::Mid,
                            );

                            if let Some(xrate) = xrate {
                                let scale = 10f64.powi(currency.precision.into());
                                pnl = ((pnl * xrate) * scale).round() / scale;
                            } else {
                                log::error!(
                                    "Cannot calculate realized PnL: insufficient exchange rate data for {}/{}, marking as pending calculation",
                                    instrument.settlement_currency(),
                                    base_currency
                                );
                                self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                                return Some(Money::new(0.0, currency));
                            }
                        }

                        total_pnl += pnl;
                    }
                }
            }

            // Add realized PnL from current active positions
            for position in positions {
                if position.instrument_id != *instrument_id {
                    continue;
                }

                if let Some(realized_pnl) = position.realized_pnl {
                    let mut pnl = realized_pnl.as_f64();

                    if let Some(base_currency) = account.base_currency() {
                        let xrate = if let Some(xrate) =
                            self.calculate_xrate_to_base(instrument, account, position.entry)
                        {
                            xrate
                        } else {
                            log::error!(
                                "Cannot calculate realized PnL: insufficient exchange rate data for {}/{}, marking as pending calculation",
                                instrument.settlement_currency(),
                                base_currency
                            );
                            self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                            return Some(Money::new(0.0, currency));
                        };

                        let scale = 10f64.powi(currency.precision.into());
                        pnl = ((pnl * xrate) * scale).round() / scale;
                    }

                    total_pnl += pnl;
                }
            }
        } else {
            // HEDGING OMS or no snapshots: Simple aggregation
            // Add snapshot PnLs (sum all)
            for position_id in &snapshot_position_ids {
                let sum_pnl = self
                    .inner
                    .borrow()
                    .snapshot_sum_per_position
                    .get(position_id)
                    .copied();
                if let Some(sum_pnl) = sum_pnl {
                    let mut pnl = sum_pnl.as_f64();

                    if let Some(base_currency) = account.base_currency() {
                        let xrate = cache.get_xrate(
                            instrument_id.venue,
                            instrument.settlement_currency(),
                            base_currency,
                            PriceType::Mid,
                        );

                        if let Some(xrate) = xrate {
                            let scale = 10f64.powi(currency.precision.into());
                            pnl = ((pnl * xrate) * scale).round() / scale;
                        } else {
                            log::error!(
                                "Cannot calculate realized PnL: insufficient exchange rate data for {}/{}, marking as pending calculation",
                                instrument.settlement_currency(),
                                base_currency
                            );
                            self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                            return Some(Money::new(0.0, currency));
                        }
                    }

                    total_pnl += pnl;
                }
            }

            // Add realized PnL from current positions
            for position in positions {
                if position.instrument_id != *instrument_id {
                    continue;
                }

                if let Some(realized_pnl) = position.realized_pnl {
                    let mut pnl = realized_pnl.as_f64();

                    if let Some(base_currency) = account.base_currency() {
                        let xrate = if let Some(xrate) =
                            self.calculate_xrate_to_base(instrument, account, position.entry)
                        {
                            xrate
                        } else {
                            log::error!(
                                "Cannot calculate realized PnL: insufficient exchange rate data for {}/{}, marking as pending calculation",
                                instrument.settlement_currency(),
                                base_currency
                            );
                            self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                            return Some(Money::new(0.0, currency));
                        };

                        let scale = 10f64.powi(currency.precision.into());
                        pnl = ((pnl * xrate) * scale).round() / scale;
                    }

                    total_pnl += pnl;
                }
            }
        }

        Some(Money::new(total_pnl, currency))
    }

    fn get_price(&self, position: &Position) -> Option<Price> {
        let cache = self.cache.borrow();
        let instrument_id = &position.instrument_id;

        // Check for mark price first if configured
        if self.config.use_mark_prices
            && let Some(mark_price) = cache.mark_price(instrument_id)
        {
            return Some(mark_price.value);
        }

        // Fall back to bid/ask based on position side
        let price_type = match position.side {
            PositionSide::Long => PriceType::Bid,
            PositionSide::Short => PriceType::Ask,
            _ => panic!("invalid `PositionSide`, was {}", position.side),
        };

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
    config: PortfolioConfig,
    quote: &QuoteTick,
) {
    update_instrument_id(cache, clock.clone(), inner, config, &quote.instrument_id);
}

fn update_bar(
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<dyn Clock>>,
    inner: Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    bar: &Bar,
) {
    let instrument_id = bar.bar_type.instrument_id();
    inner
        .borrow_mut()
        .bar_close_prices
        .insert(instrument_id, bar.close);
    update_instrument_id(cache, clock.clone(), inner, config, &instrument_id);
}

fn update_instrument_id(
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<dyn Clock>>,
    inner: Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    instrument_id: &InstrumentId,
) {
    inner.borrow_mut().unrealized_pnls.remove(instrument_id);

    if inner.borrow().initialized || !inner.borrow().pending_calcs.contains(instrument_id) {
        return;
    }

    let result_init;
    let mut result_maint = None;

    let account = {
        let cache_ref = cache.borrow();
        let account = if let Some(account) = cache_ref.account_for_venue(&instrument_id.venue) {
            account
        } else {
            log::error!(
                "Cannot update tick: no account registered for {}",
                instrument_id.venue
            );
            return;
        };

        let mut cache_ref = cache.borrow_mut();
        let instrument = if let Some(instrument) = cache_ref.instrument(instrument_id) {
            instrument.clone()
        } else {
            log::error!("Cannot update tick: no instrument found for {instrument_id}");
            return;
        };

        // Clone the orders and positions to own the data
        let orders_open: Vec<OrderAny> = cache_ref
            .orders_open(None, Some(instrument_id), None, None)
            .iter()
            .map(|o| (*o).clone())
            .collect();

        let positions_open: Vec<Position> = cache_ref
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
            cache_ref.update_account(updated_account.clone()).unwrap();
        }
        account.clone()
    };

    let mut portfolio_clone = Portfolio {
        clock: clock.clone(),
        cache,
        inner: inner.clone(),
        config,
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
    _config: PortfolioConfig,
    event: &OrderEventAny,
) {
    let cache_ref = cache.borrow();
    let account_id = match event.account_id() {
        Some(account_id) => account_id,
        None => {
            return; // No Account Assigned
        }
    };

    let account = if let Some(account) = cache_ref.account(&account_id) {
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

    let cache_ref = cache.borrow();
    let order = if let Some(order) = cache_ref.order(&event.client_order_id()) {
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

    let instrument = if let Some(instrument_id) = cache_ref.instrument(&event.instrument_id()) {
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

    let orders_open = cache_ref.orders_open(None, Some(&event.instrument_id()), None, None);

    let account_state = inner.borrow_mut().accounts.update_orders(
        account,
        instrument.clone(),
        orders_open,
        clock.borrow().timestamp_ns(),
    );

    let mut cache_ref = cache.borrow_mut();
    cache_ref.update_account(account.clone()).unwrap();

    if let Some(account_state) = account_state {
        msgbus::publish(
            format!("events.account.{}", account.id()).into(),
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
    _config: PortfolioConfig,
    event: &PositionEvent,
) {
    let instrument_id = event.instrument_id();

    let positions_open: Vec<Position> = {
        let cache_ref = cache.borrow();

        cache_ref
            .positions_open(None, Some(&instrument_id), None, None)
            .iter()
            .map(|o| (*o).clone())
            .collect()
    };

    log::debug!("position fresh from cache -> {positions_open:?}");

    let mut portfolio_clone = Portfolio {
        clock: clock.clone(),
        cache: cache.clone(),
        inner: inner.clone(),
        config: PortfolioConfig::default(), // TODO: TBD
    };

    portfolio_clone.update_net_position(&instrument_id, positions_open.clone());

    if let Some(calculated_unrealized_pnl) =
        portfolio_clone.calculate_unrealized_pnl(&instrument_id)
    {
        inner
            .borrow_mut()
            .unrealized_pnls
            .insert(event.instrument_id(), calculated_unrealized_pnl);
    } else {
        log::warn!(
            "Failed to calculate unrealized PnL for {}, marking as pending",
            event.instrument_id()
        );
        inner
            .borrow_mut()
            .pending_calcs
            .insert(event.instrument_id());
    }

    if let Some(calculated_realized_pnl) = portfolio_clone.calculate_realized_pnl(&instrument_id) {
        inner
            .borrow_mut()
            .realized_pnls
            .insert(event.instrument_id(), calculated_realized_pnl);
    } else {
        log::warn!(
            "Failed to calculate realized PnL for {}, marking as pending",
            event.instrument_id()
        );
        inner
            .borrow_mut()
            .pending_calcs
            .insert(event.instrument_id());
    }

    let cache_ref = cache.borrow();
    let account = cache_ref.account(&event.account_id());

    if let Some(AccountAny::Margin(margin_account)) = account {
        if !margin_account.calculate_account_state {
            return; // Nothing to calculate
        }

        let cache_ref = cache.borrow();
        let instrument = if let Some(instrument) = cache_ref.instrument(&instrument_id) {
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
        let mut cache_ref = cache.borrow_mut();
        if let Some((margin_account, _)) = result {
            cache_ref
                .update_account(AccountAny::Margin(margin_account))
                .unwrap();
        }
    } else if account.is_none() {
        log::error!(
            "Cannot update position: no account registered for {}",
            event.account_id()
        );
    }
}

fn update_account(
    cache: Rc<RefCell<Cache>>,
    inner: Rc<RefCell<PortfolioState>>,
    event: &AccountState,
) {
    let mut cache_ref = cache.borrow_mut();

    if let Some(existing) = cache_ref.account(&event.account_id) {
        let mut account = existing.clone();
        account.apply(event.clone());

        if let Err(e) = cache_ref.update_account(account.clone()) {
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

        if let Err(e) = cache_ref.add_account(account) {
            log::error!("Failed to add account: {e}");
            return;
        }
    }

    // Throttled logging logic
    let mut inner_ref = inner.borrow_mut();
    let should_log = if inner_ref.min_account_state_logging_interval_ns > 0 {
        let current_ts = event.ts_init.as_u64();
        let last_ts = inner_ref
            .last_account_state_log_ts
            .get(&event.account_id)
            .copied()
            .unwrap_or(0);

        if last_ts == 0 || (current_ts - last_ts) >= inner_ref.min_account_state_logging_interval_ns
        {
            inner_ref
                .last_account_state_log_ts
                .insert(event.account_id, current_ts);
            true
        } else {
            false
        }
    } else {
        true // Throttling disabled, always log
    };

    if should_log {
        log::info!("Updated {event}");
    }
}
