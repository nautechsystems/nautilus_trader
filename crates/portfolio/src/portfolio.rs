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

//! Provides a generic `Portfolio` for all environments.

use std::{cell::RefCell, collections::VecDeque, fmt::Debug, rc::Rc};

use ahash::{AHashMap, AHashSet};
use indexmap::{IndexMap, IndexSet};
use nautilus_analysis::analyzer::PortfolioAnalyzer;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    enums::LogColor,
    msgbus::{self, MessagingSwitchboard, TypedHandler, TypedIntoHandler},
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{UUID4, WeakCell, datetime::NANOSECONDS_IN_MILLISECOND};
use nautilus_model::{
    accounts::{Account, AccountAny},
    data::{Bar, MarkPriceUpdate, QuoteTick},
    enums::{OmsType, OrderType, PositionSide, PriceType},
    events::{AccountState, OrderEventAny, PortfolioSnapshot, position::PositionEvent},
    identifiers::{AccountId, InstrumentId, PositionId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    position::Position,
    types::{AccountBalance, Currency, MarginBalance, Money, Price},
};
use rust_decimal::Decimal;

use crate::{config::PortfolioConfig, manager::AccountsManager};

struct PortfolioState {
    accounts: AccountsManager,
    analyzer: PortfolioAnalyzer,
    unrealized_pnls: IndexMap<InstrumentId, Money>,
    realized_pnls: IndexMap<InstrumentId, Money>,
    snapshot_sum_per_position: AHashMap<PositionId, Money>,
    snapshot_last_per_position: AHashMap<PositionId, Money>,
    snapshot_processed_counts: AHashMap<PositionId, usize>,
    snapshot_account_ids: AHashMap<PositionId, AccountId>,
    net_positions: IndexMap<InstrumentId, Decimal>,
    pending_calcs: AHashSet<InstrumentId>,
    bar_close_prices: AHashMap<InstrumentId, Price>,
    initialized: bool,
    last_account_state_log_ts: AHashMap<AccountId, u64>,
    min_account_state_logging_interval_ns: u64,
    venues_missing_price: AHashMap<Venue, AHashSet<InstrumentId>>,
    account_open_positions: AHashMap<AccountId, usize>,
    portfolio_snapshots: AHashMap<AccountId, VecDeque<PortfolioSnapshot>>,
    pre_position_fill_events: AHashSet<UUID4>,
}

// Sized for post-run backtest analysis (e.g. ~11 days at 1s cadence, or years
// at per-minute cadence), long-lived live deployments should consume snapshots
// via the message bus instead of relying on this buffer.
const SNAPSHOT_BUFFER_CAP: usize = 1_000_000;

#[derive(Clone, Copy)]
enum OrderUpdateSource {
    Endpoint,
    Topic,
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
            unrealized_pnls: IndexMap::new(),
            realized_pnls: IndexMap::new(),
            snapshot_sum_per_position: AHashMap::new(),
            snapshot_last_per_position: AHashMap::new(),
            snapshot_processed_counts: AHashMap::new(),
            snapshot_account_ids: AHashMap::new(),
            net_positions: IndexMap::new(),
            pending_calcs: AHashSet::new(),
            bar_close_prices: AHashMap::new(),
            initialized: false,
            last_account_state_log_ts: AHashMap::new(),
            min_account_state_logging_interval_ns,
            venues_missing_price: AHashMap::new(),
            account_open_positions: AHashMap::new(),
            portfolio_snapshots: AHashMap::new(),
            pre_position_fill_events: AHashSet::new(),
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
        self.snapshot_account_ids.clear();
        self.pending_calcs.clear();
        self.bar_close_prices.clear();
        self.last_account_state_log_ts.clear();
        self.venues_missing_price.clear();
        self.account_open_positions.clear();
        self.portfolio_snapshots.clear();
        self.pre_position_fill_events.clear();
        self.analyzer.reset();
        self.initialized = false;
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

        Self::register_message_handlers(&cache, &clock, &inner, config);

        Self {
            clock,
            cache,
            inner,
            config,
        }
    }

    /// Creates a shallow clone of the Portfolio that shares the same internal state.
    ///
    /// This is useful when multiple components need to reference the same Portfolio
    /// without creating duplicate msgbus handler registrations.
    #[must_use]
    pub fn clone_shallow(&self) -> Self {
        Self {
            clock: self.clock.clone(),
            cache: self.cache.clone(),
            inner: self.inner.clone(),
            config: self.config,
        }
    }

    fn register_message_handlers(
        cache: &Rc<RefCell<Cache>>,
        clock: &Rc<RefCell<dyn Clock>>,
        inner: &Rc<RefCell<PortfolioState>>,
        config: PortfolioConfig,
    ) {
        let inner_weak = WeakCell::from(Rc::downgrade(inner));

        // Typed handlers for subscriptions
        let update_account_handler = {
            let cache = cache.clone();
            let inner = inner_weak.clone();
            TypedHandler::from(move |event: &AccountState| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    update_account(&cache, &inner_rc, event);
                }
            })
        };

        let update_position_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner_weak.clone();
            TypedHandler::from(move |event: &PositionEvent| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    update_position(&cache, &clock, &inner_rc, config, event);
                }
            })
        };

        let update_quote_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner_weak.clone();
            TypedHandler::from(move |quote: &QuoteTick| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    update_quote_tick(&cache, &clock, &inner_rc, config, quote);
                }
            })
        };

        let update_bar_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner_weak.clone();
            TypedHandler::from(move |bar: &Bar| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    update_bar(&cache, &clock, &inner_rc, config, bar);
                }
            })
        };

        let update_mark_price_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner_weak.clone();
            TypedHandler::from(move |mark_price: &MarkPriceUpdate| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    update_instrument_id(
                        &cache,
                        &clock,
                        &inner_rc,
                        config,
                        &mark_price.instrument_id,
                    );
                }
            })
        };

        let update_order_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner_weak.clone();
            TypedHandler::from(move |event: &OrderEventAny| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    update_order(
                        &cache,
                        &clock,
                        &inner_rc,
                        config,
                        event,
                        OrderUpdateSource::Topic,
                    );
                }
            })
        };

        let endpoint = MessagingSwitchboard::portfolio_update_account();
        msgbus::register_account_state_endpoint(endpoint, update_account_handler.clone());

        let update_order_endpoint_handler = {
            let cache = cache.clone();
            let clock = clock.clone();
            let inner = inner_weak;
            TypedIntoHandler::from(move |event: OrderEventAny| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    update_order(
                        &cache,
                        &clock,
                        &inner_rc,
                        config,
                        &event,
                        OrderUpdateSource::Endpoint,
                    );
                }
            })
        };
        msgbus::register_order_event_endpoint(
            MessagingSwitchboard::portfolio_update_order(),
            update_order_endpoint_handler,
        );

        msgbus::subscribe_quotes("data.quotes.*".into(), update_quote_handler, Some(10));

        if config.bar_updates {
            msgbus::subscribe_bars("data.bars.*EXTERNAL".into(), update_bar_handler, Some(10));
        }

        if config.use_mark_prices {
            msgbus::subscribe_mark_prices(
                "data.mark_prices.*".into(),
                update_mark_price_handler,
                Some(10),
            );
        }
        msgbus::subscribe_order_events("events.order.*".into(), update_order_handler, Some(10));
        msgbus::subscribe_position_events(
            "events.position.*".into(),
            update_position_handler,
            Some(10),
        );
        msgbus::subscribe_account_state(
            "events.account.*".into(),
            update_account_handler,
            Some(10),
        );
    }

    pub fn reset(&mut self) {
        log::debug!("RESETTING");
        let account_ids: Vec<AccountId> = self
            .inner
            .borrow()
            .account_open_positions
            .keys()
            .copied()
            .collect();

        for account_id in account_ids {
            self.clock
                .borrow_mut()
                .cancel_timer(&snapshot_timer_name(&account_id));
        }
        self.inner.borrow_mut().reset();
        log::debug!("READY");
    }

    /// Returns a reference to the cache.
    #[must_use]
    pub fn cache(&self) -> &Rc<RefCell<Cache>> {
        &self.cache
    }

    /// Returns a reference to the clock.
    #[must_use]
    pub fn clock(&self) -> &Rc<RefCell<dyn Clock>> {
        &self.clock
    }

    /// Returns `true` if the portfolio has been initialized.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.inner.borrow().initialized
    }

    /// Returns the locked balances for the given venue.
    ///
    /// Locked balances represent funds reserved for open orders.
    #[must_use]
    pub fn balances_locked(&self, venue: &Venue) -> IndexMap<Currency, Money> {
        self.cache.borrow().account_for_venue(venue).map_or_else(
            || {
                log::error!("Cannot get balances locked: no account generated for {venue}");
                IndexMap::new()
            },
            |account| account.balances_locked(),
        )
    }

    /// Returns the initial margin requirements for the given venue.
    ///
    /// Only applicable for margin accounts. Returns empty map for cash accounts.
    #[must_use]
    pub fn margins_init(&self, venue: &Venue) -> IndexMap<InstrumentId, Money> {
        self.cache.borrow().account_for_venue(venue).map_or_else(
            || {
                log::error!(
                    "Cannot get initial (order) margins: no account registered for {venue}"
                );
                IndexMap::new()
            },
            |account| match &*account {
                AccountAny::Margin(margin_account) => margin_account.initial_margins(),
                AccountAny::Cash(_) | AccountAny::Betting(_) => {
                    log::warn!("Initial margins not applicable for cash account");
                    IndexMap::new()
                }
            },
        )
    }

    /// Returns the maintenance margin requirements for the given venue.
    ///
    /// Only applicable for margin accounts. Returns empty map for cash accounts.
    #[must_use]
    pub fn margins_maint(&self, venue: &Venue) -> IndexMap<InstrumentId, Money> {
        self.cache.borrow().account_for_venue(venue).map_or_else(
            || {
                log::error!(
                    "Cannot get maintenance (position) margins: no account registered for {venue}"
                );
                IndexMap::new()
            },
            |account| match &*account {
                AccountAny::Margin(margin_account) => margin_account.maintenance_margins(),
                AccountAny::Cash(_) | AccountAny::Betting(_) => {
                    log::warn!("Maintenance margins not applicable for cash account");
                    IndexMap::new()
                }
            },
        )
    }

    /// Returns the unrealized PnLs for all positions at the given venue.
    ///
    /// Calculates mark-to-market PnL based on current market prices.
    #[must_use]
    pub fn unrealized_pnls(
        &mut self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> IndexMap<Currency, Money> {
        let instrument_ids = {
            let cache = self.cache.borrow();
            let positions = cache.positions(Some(venue), None, None, account_id, None);

            if positions.is_empty() {
                return IndexMap::new(); // Nothing to calculate
            }

            // IndexSet preserves the deterministic order of cache.positions
            // through the dedup so the returned currency map iterates in a
            // stable order across runs.
            let instrument_ids: IndexSet<InstrumentId> =
                positions.iter().map(|p| p.instrument_id).collect();

            instrument_ids
        };

        let mut unrealized_pnls: IndexMap<Currency, f64> = IndexMap::new();

        for instrument_id in instrument_ids {
            // The instrument-keyed cache aggregates across all accounts on the
            // same venue, so bypass it when the caller filters by account_id.
            if account_id.is_none()
                && let Some(&pnl) = self.inner.borrow_mut().unrealized_pnls.get(&instrument_id)
            {
                *unrealized_pnls.entry(pnl.currency).or_insert(0.0) += pnl.as_f64();
                continue;
            }

            if let Some(pnl) = self.calculate_unrealized_pnl(&instrument_id, account_id) {
                *unrealized_pnls.entry(pnl.currency).or_insert(0.0) += pnl.as_f64();
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
    pub fn realized_pnls(
        &mut self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> IndexMap<Currency, Money> {
        let instrument_ids = {
            let cache = self.cache.borrow();
            let positions = cache.positions(Some(venue), None, None, account_id, None);

            if positions.is_empty() {
                return IndexMap::new(); // Nothing to calculate
            }

            let instrument_ids: IndexSet<InstrumentId> =
                positions.iter().map(|p| p.instrument_id).collect();

            instrument_ids
        };

        let mut realized_pnls: IndexMap<Currency, f64> = IndexMap::new();

        for instrument_id in instrument_ids {
            // The instrument-keyed cache aggregates across all accounts on the
            // same venue, so bypass it when the caller filters by account_id.
            if account_id.is_none()
                && let Some(&pnl) = self.inner.borrow_mut().realized_pnls.get(&instrument_id)
            {
                *realized_pnls.entry(pnl.currency).or_insert(0.0) += pnl.as_f64();
                continue;
            }

            if let Some(pnl) = self.calculate_realized_pnl(&instrument_id, account_id) {
                *realized_pnls.entry(pnl.currency).or_insert(0.0) += pnl.as_f64();
            }
        }

        realized_pnls
            .into_iter()
            .map(|(currency, amount)| (currency, Money::new(amount, currency)))
            .collect()
    }

    #[must_use]
    pub fn net_exposures(
        &self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> Option<IndexMap<Currency, Money>> {
        let cache = self.cache.borrow();
        let account = if let Some(id) = account_id {
            if let Some(account) = cache.account(id) {
                account
            } else {
                log::error!("Cannot calculate net exposures: no account for {id}");
                return None;
            }
        } else if let Some(account) = cache.account_for_venue(venue) {
            account
        } else {
            log::error!("Cannot calculate net exposures: no account registered for {venue}");
            return None;
        };

        let positions_open = cache.positions_open(Some(venue), None, None, account_id, None);
        if positions_open.is_empty() {
            return Some(IndexMap::new()); // Nothing to calculate
        }

        let mut net_exposures: IndexMap<Currency, f64> = IndexMap::new();

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

            let price = self.get_price(&position)?;
            let xrate = if let Some(xrate) = self.calculate_xrate_to_base(instrument, &account) {
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

        let pnl = self.calculate_unrealized_pnl(instrument_id, None)?;
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

        let pnl = self.calculate_realized_pnl(instrument_id, None)?;
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
    /// Total PnL = Realized PnL + Unrealized PnL for each currency. Pass `account_id`
    /// to scope the aggregation to a single account when multiple accounts share the venue.
    #[must_use]
    pub fn total_pnls(
        &mut self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> IndexMap<Currency, Money> {
        let realized_pnls = self.realized_pnls(venue, account_id);
        let unrealized_pnls = self.unrealized_pnls(venue, account_id);

        let mut total_pnls: IndexMap<Currency, Money> = IndexMap::new();

        // Add realized PnLs
        for (currency, realized) in realized_pnls {
            total_pnls.insert(currency, realized);
        }

        // Add unrealized PnLs
        for (currency, unrealized) in unrealized_pnls {
            match total_pnls.get_mut(&currency) {
                Some(total) => {
                    *total = *total + unrealized;
                }
                None => {
                    total_pnls.insert(currency, unrealized);
                }
            }
        }

        total_pnls
    }

    /// Returns the per-currency mark-to-market value of open positions at the given venue.
    ///
    /// For each open position the valuation uses the portfolio's internal price
    /// resolution, which prefers mark prices (when configured), falls back to
    /// side-appropriate bid/ask, then last trade, then the most recent bar close.
    /// Instruments without any available price are skipped and the venue is flagged
    /// for a no-price warning. Pass `account_id` to scope the aggregation to a
    /// single account when multiple accounts share the venue.
    #[must_use]
    pub fn mark_values(
        &mut self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> IndexMap<Currency, Money> {
        let mut values: IndexMap<Currency, f64> = IndexMap::new();
        let mut unpriced: AHashSet<InstrumentId> = AHashSet::new();

        if self.accumulate_mark_values(venue, account_id, &mut values, &mut unpriced) {
            self.update_missing_price_state(venue, &unpriced);
        } else if account_id.is_none() {
            // Only clear the tracker on an unfiltered sweep; otherwise we could
            // wipe another account's flags on the same venue.
            self.inner.borrow_mut().venues_missing_price.remove(venue);
        }

        values
            .into_iter()
            .map(|(c, v)| (c, Money::new(v, c)))
            .collect()
    }

    /// Returns the per-currency total equity for the given venue.
    ///
    /// For cash accounts: `balance.total + Σ mark_value(open positions)` per currency.
    /// For margin accounts: `balance.total + Σ unrealized_pnl(open positions)` per currency.
    ///
    /// Open-position instruments that cannot be priced are tracked via
    /// [`Portfolio::missing_price_instruments`] (and warned once) for both branches,
    /// so equity understatement does not go unnoticed. Pass `account_id` to scope
    /// the aggregation to a single account when multiple accounts share the venue.
    #[must_use]
    pub fn equity(
        &mut self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> IndexMap<Currency, Money> {
        let (mut equity, is_margin) = {
            let cache = self.cache.borrow();
            let account = match account_id {
                Some(id) => cache.account(id),
                None => cache.account_for_venue(venue),
            };

            match account {
                Some(account) => {
                    let equity: IndexMap<Currency, f64> = account
                        .balances_total()
                        .into_iter()
                        .map(|(c, m)| (c, m.as_f64()))
                        .collect();
                    (equity, matches!(&*account, AccountAny::Margin(_)))
                }
                None => return IndexMap::new(),
            }
        };

        let mut unpriced: AHashSet<InstrumentId> = AHashSet::new();

        if is_margin {
            // Sum cached unrealized PnLs; fall through to recalculation on cache miss.
            let instrument_ids: IndexSet<InstrumentId> = {
                let cache = self.cache.borrow();
                cache
                    .positions_open(Some(venue), None, None, account_id, None)
                    .iter()
                    .map(|p| p.instrument_id)
                    .collect()
            };

            if instrument_ids.is_empty() {
                if account_id.is_none() {
                    self.inner.borrow_mut().venues_missing_price.remove(venue);
                }
            } else {
                for instrument_id in instrument_ids {
                    // The instrument-keyed cache aggregates across all accounts on
                    // the same venue, so bypass it when the caller filters by
                    // account_id.
                    let cached = if account_id.is_none() {
                        self.inner
                            .borrow()
                            .unrealized_pnls
                            .get(&instrument_id)
                            .copied()
                    } else {
                        None
                    };
                    let pnl = match cached {
                        Some(pnl) => Some(pnl),
                        None => self.calculate_unrealized_pnl(&instrument_id, account_id),
                    };

                    match pnl {
                        Some(pnl) => {
                            *equity.entry(pnl.currency).or_insert(0.0) += pnl.as_f64();
                        }
                        None => {
                            unpriced.insert(instrument_id);
                        }
                    }
                }
                self.update_missing_price_state(venue, &unpriced);
            }
        } else if self.accumulate_mark_values(venue, account_id, &mut equity, &mut unpriced) {
            self.update_missing_price_state(venue, &unpriced);
        } else if account_id.is_none() {
            self.inner.borrow_mut().venues_missing_price.remove(venue);
        }

        equity
            .into_iter()
            .map(|(c, v)| (c, Money::new(v, c)))
            .collect()
    }

    /// Builds a [`PortfolioSnapshot`] for the given account at the current clock time.
    ///
    /// Unrealized PnL and mark values span the venues the account currently
    /// holds open positions on; realized PnL spans every venue the account has
    /// touched (open or closed) so a multi-venue account where one venue is
    /// now flat still reports its accumulated realized PnL. Returns `None` if
    /// no account is registered.
    #[must_use]
    pub fn build_snapshot(&mut self, account_id: &AccountId) -> Option<PortfolioSnapshot> {
        let account = self.cache.borrow().account_owned(account_id)?;

        let balances: Vec<AccountBalance> = account.balances().into_values().collect();
        let margins: Vec<MarginBalance> = match &account {
            AccountAny::Margin(m) => m
                .margins
                .values()
                .copied()
                .chain(m.account_margins.values().copied())
                .collect(),
            AccountAny::Cash(_) | AccountAny::Betting(_) => Vec::new(),
        };

        // Collect venues the account has touched. `open_venues` drives the
        // unrealized PnL and mark-value sums; `all_venues` extends to closed
        // positions so realized PnL on a venue with no open exposure (a
        // multi-venue account where one venue is now flat) still rolls up.
        let open_venues: AHashSet<Venue> = self
            .cache
            .borrow()
            .positions_open(None, None, None, Some(account_id), None)
            .iter()
            .map(|p| p.instrument_id.venue)
            .collect();
        let all_venues: AHashSet<Venue> = self
            .cache
            .borrow()
            .positions(None, None, None, Some(account_id), None)
            .iter()
            .map(|p| p.instrument_id.venue)
            .collect();

        let mut unrealized: IndexMap<Currency, f64> = IndexMap::new();
        let mut realized: IndexMap<Currency, f64> = IndexMap::new();
        let mut equity: IndexMap<Currency, f64> = account
            .balances_total()
            .into_iter()
            .map(|(c, m)| (c, m.as_f64()))
            .collect();

        for venue in &open_venues {
            for (currency, money) in self.unrealized_pnls(venue, Some(account_id)) {
                *unrealized.entry(currency).or_insert(0.0) += money.as_f64();
            }
        }

        for venue in &all_venues {
            for (currency, money) in self.realized_pnls(venue, Some(account_id)) {
                *realized.entry(currency).or_insert(0.0) += money.as_f64();
            }
        }

        match &account {
            AccountAny::Margin(_) => {
                for (currency, value) in &unrealized {
                    *equity.entry(*currency).or_insert(0.0) += *value;
                }
            }
            AccountAny::Cash(_) | AccountAny::Betting(_) => {
                for venue in &open_venues {
                    for (currency, money) in self.mark_values(venue, Some(account_id)) {
                        *equity.entry(currency).or_insert(0.0) += money.as_f64();
                    }
                }
            }
        }

        let unrealized_pnls: Vec<Money> = unrealized
            .into_iter()
            .map(|(c, v)| Money::new(v, c))
            .collect();
        let realized_pnls: Vec<Money> = realized
            .into_iter()
            .map(|(c, v)| Money::new(v, c))
            .collect();
        let total_equity: Vec<Money> = equity.into_iter().map(|(c, v)| Money::new(v, c)).collect();

        let ts_now = self.clock.borrow().timestamp_ns();

        Some(PortfolioSnapshot::new(
            account.id(),
            account.account_type(),
            account.base_currency(),
            balances,
            margins,
            unrealized_pnls,
            realized_pnls,
            total_equity,
            UUID4::new(),
            ts_now,
            ts_now,
        ))
    }

    /// Returns the recorded portfolio snapshots for the given account, in order of emission.
    ///
    /// Snapshots accumulate whenever `snapshot_interval_ms` is set and the account
    /// holds at least one open position. The ring is bounded; long-lived live
    /// deployments should consume snapshots via the message bus instead of relying
    /// on this buffer. Cleared on [`Portfolio::reset`].
    #[must_use]
    pub fn snapshots(&self, account_id: &AccountId) -> Vec<PortfolioSnapshot> {
        self.inner
            .borrow()
            .portfolio_snapshots
            .get(account_id)
            .map(|ring| ring.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Returns the instruments currently flagged as unpriced for the given venue.
    ///
    /// An entry is added the first time [`Portfolio::mark_values`] cannot source a
    /// price for an open position (after also emitting a warn log), and removed
    /// once the instrument is priced again so a subsequent drop re-warns.
    #[must_use]
    pub fn missing_price_instruments(&self, venue: &Venue) -> Vec<InstrumentId> {
        let mut ids: Vec<InstrumentId> = self
            .inner
            .borrow()
            .venues_missing_price
            .get(venue)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default();
        // Sort so the public Vec is deterministic even though the underlying
        // tracking set is AHash-backed.
        ids.sort();
        ids
    }

    fn update_missing_price_state(&self, venue: &Venue, unpriced: &AHashSet<InstrumentId>) {
        let mut inner = self.inner.borrow_mut();
        let tracked = inner.venues_missing_price.entry(*venue).or_default();

        // Sort first so the warn-log sequence is deterministic across runs.
        let mut ids: Vec<InstrumentId> = unpriced.iter().copied().collect();
        ids.sort();
        for instrument_id in ids {
            if tracked.insert(instrument_id) {
                log::warn!(
                    "No price available for open position {instrument_id}; \
                    subscribe to quotes, trades or bars for continuous mark-to-market equity"
                );
            }
        }

        // Instruments that are now priced should be removed so a future price drop re-warns
        tracked.retain(|id| unpriced.contains(id));
    }

    // Returns `true` if at least one open position was seen (priced or not),
    // `false` if the venue is flat. Unpriced instruments are written to
    // `unpriced` for the caller to flow into `update_missing_price_state`.
    fn accumulate_mark_values(
        &self,
        venue: &Venue,
        account_id: Option<&AccountId>,
        values: &mut IndexMap<Currency, f64>,
        unpriced: &mut AHashSet<InstrumentId>,
    ) -> bool {
        let cache = self.cache.borrow();
        let positions = cache.positions_open(Some(venue), None, None, account_id, None);

        if positions.is_empty() {
            return false;
        }

        let account = match account_id {
            Some(id) => cache.account(id),
            None => cache.account_for_venue(venue),
        };
        let mut xrate_cache: AHashMap<Currency, Option<f64>> = AHashMap::new();

        for position in positions {
            let sign = match position.side {
                PositionSide::Long => 1.0,
                PositionSide::Short => -1.0,
                PositionSide::Flat | PositionSide::NoPositionSide => continue,
            };

            let instrument = match cache.instrument(&position.instrument_id) {
                Some(i) => i,
                None => {
                    unpriced.insert(position.instrument_id);
                    continue;
                }
            };

            let price = match self.get_price(&position) {
                Some(p) => p,
                None => {
                    unpriced.insert(position.instrument_id);
                    continue;
                }
            };

            let settlement = instrument.settlement_currency();
            let (xrate, currency) = if self.config.convert_to_account_base_currency
                && let Some(account) = account.as_ref()
                && let Some(base_currency) = account.base_currency()
            {
                let xrate_opt = *xrate_cache
                    .entry(settlement)
                    .or_insert_with(|| self.calculate_xrate_to_base(instrument, account));
                let xrate = match xrate_opt {
                    Some(x) => x,
                    None => {
                        unpriced.insert(position.instrument_id);
                        continue;
                    }
                };
                (xrate, base_currency)
            } else {
                (1.0, settlement)
            };

            let notional = position.notional_value(price).as_f64() * xrate;
            *values.entry(currency).or_insert(0.0) += sign * notional;
        }

        true
    }

    #[must_use]
    pub fn net_exposure(
        &self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
    ) -> Option<Money> {
        let cache = self.cache.borrow();

        let instrument = if let Some(instrument) = cache.instrument(instrument_id) {
            instrument
        } else {
            log::error!("Cannot calculate net exposure: no instrument for {instrument_id}");
            return None;
        };

        let positions_open =
            cache.positions_open(None, Some(instrument_id), None, account_id, None);

        if positions_open.is_empty() {
            return Some(Money::new(0.0, instrument.settlement_currency()));
        }

        let mut net_exposure = 0.0;
        let mut first_base_currency: Option<Currency> = None;

        for position in &positions_open {
            // Get account for THIS position
            let account = if let Some(account) = cache.account(&position.account_id) {
                account
            } else {
                log::error!(
                    "Cannot calculate net exposure: no account for {}",
                    position.account_id
                );
                return None;
            };

            // Validate consistent base currency across accounts
            if let Some(base) = account.base_currency() {
                match first_base_currency {
                    None => {
                        first_base_currency = Some(base);
                    }
                    Some(first) if first != base => {
                        log::error!(
                            "Cannot calculate net exposure: accounts have different base \
                            currencies ({first} vs {base}); multi-account aggregation requires \
                            consistent base currencies"
                        );
                        return None;
                    }
                    _ => {}
                }
            }

            let price = self.get_price(position)?;
            let xrate = if let Some(xrate) = self.calculate_xrate_to_base(instrument, &account) {
                xrate
            } else {
                log::error!(
                    "Cannot calculate net exposures: insufficient data for {}/{:?}",
                    instrument.settlement_currency(),
                    account.base_currency()
                );
                return None;
            };

            let notional_value =
                instrument.calculate_notional_value(position.quantity, price, None);
            net_exposure += notional_value.as_f64() * xrate;
        }

        let settlement_currency =
            first_base_currency.unwrap_or_else(|| instrument.settlement_currency());

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

    /// Initializes account margin based on existing open orders.
    ///
    /// # Panics
    ///
    /// Panics if updating the cache with a mutated account fails.
    pub fn initialize_orders(&mut self) {
        let mut initialized = true;
        let orders_and_instruments = {
            let cache = self.cache.borrow();

            let mut instruments_with_orders = Vec::new();
            let mut instruments = AHashSet::new();

            for client_order_id in cache.iter_client_order_ids_open(None, None, None, None) {
                if let Some(order) = cache.order(&client_order_id) {
                    instruments.insert(order.instrument_id());
                }
            }

            for instrument_id in instruments {
                if let Some(instrument) = cache.instrument(&instrument_id) {
                    let orders = cache
                        .orders_open(None, Some(&instrument_id), None, None, None)
                        .into_iter()
                        .map(|order| order.clone())
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
            let account = {
                let cache = self.cache.borrow();
                if let Some(account) = cache.account_for_venue(&instrument.id().venue) {
                    account.clone()
                } else {
                    log::error!(
                        "Cannot update initial (order) margin: no account registered for {}",
                        instrument.id().venue
                    );
                    initialized = false;
                    break;
                }
            };

            let orders_open_refs: Vec<&OrderAny> = orders_open.iter().collect();
            let result = self.inner.borrow_mut().accounts.update_orders(
                &account,
                instrument,
                &orders_open_refs,
                self.clock.borrow().timestamp_ns(),
            );

            match result {
                Some((updated_account, _)) => {
                    self.cache
                        .borrow_mut()
                        .update_account(&updated_account)
                        .unwrap();
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
            color = if total_orders > 0 { LogColor::Blue as u8 } else { LogColor::Normal as u8 };
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
        let mut instruments = AHashSet::new();
        {
            let cache = self.cache.borrow();
            all_positions_open = cache
                .positions_open(None, None, None, None, None)
                .into_iter()
                .map(|p| p.cloned())
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
                    .positions_open(None, Some(&instrument_id), None, None, None)
                    .into_iter()
                    .map(|p| p.cloned())
                    .collect()
            };

            self.update_net_position(&instrument_id, &positions_open);

            if let Some(calculated_unrealized_pnl) =
                self.calculate_unrealized_pnl(&instrument_id, None)
            {
                self.inner
                    .borrow_mut()
                    .unrealized_pnls
                    .insert(instrument_id, calculated_unrealized_pnl);
            } else {
                log::debug!(
                    "Failed to calculate unrealized PnL for {instrument_id}, marking as pending"
                );
                self.inner.borrow_mut().pending_calcs.insert(instrument_id);
            }

            if let Some(calculated_realized_pnl) = self.calculate_realized_pnl(&instrument_id, None)
            {
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
            let Some(account) = cache.account_for_venue_owned(&instrument_id.venue) else {
                log::error!(
                    "Cannot update maintenance (position) margin: no account registered for {}",
                    instrument_id.venue
                );
                initialized = false;
                break;
            };

            let account = match account {
                AccountAny::Cash(_) | AccountAny::Betting(_) => continue,
                AccountAny::Margin(margin_account) => margin_account,
            };

            let Some(instrument) = cache.instrument(&instrument_id).cloned() else {
                log::error!(
                    "Cannot update maintenance (position) margin: no instrument found for {instrument_id}"
                );
                initialized = false;
                break;
            };
            let positions: Vec<Position> = cache
                .positions_open(None, Some(&instrument_id), None, None, None)
                .into_iter()
                .map(|p| p.cloned())
                .collect();
            drop(cache);

            let result = self.inner.borrow_mut().accounts.update_positions(
                &account,
                &instrument,
                positions.iter().collect(),
                self.clock.borrow().timestamp_ns(),
            );

            match result {
                Some((updated_account, _)) => {
                    self.cache
                        .borrow_mut()
                        .update_account(&AccountAny::Margin(updated_account))
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
            color = if open_count > 0 { LogColor::Blue as u8 } else { LogColor::Normal as u8 };
            "Initialized {} open position{}",
            open_count,
            if open_count == 1 { "" } else { "s" }
        );

        if self.config.snapshot_interval_ms.is_some() {
            let account_ids: AHashSet<AccountId> =
                all_positions_open.iter().map(|p| p.account_id).collect();

            for account_id in account_ids {
                update_snapshot_timer_state(
                    &self.cache,
                    &self.clock,
                    &self.inner,
                    self.config,
                    &account_id,
                );
            }
        }
    }

    /// Updates portfolio calculations based on a new quote tick.
    ///
    /// Recalculates unrealized PnL for positions affected by the quote update.
    pub fn update_quote_tick(&mut self, quote: &QuoteTick) {
        update_quote_tick(&self.cache, &self.clock, &self.inner, self.config, quote);
    }

    /// Updates portfolio calculations based on a new bar.
    ///
    /// Updates cached bar close prices and recalculates unrealized PnL.
    pub fn update_bar(&mut self, bar: &Bar) {
        update_bar(&self.cache, &self.clock, &self.inner, self.config, bar);
    }

    /// Updates portfolio with a new account state event.
    pub fn update_account(&mut self, event: &AccountState) {
        update_account(&self.cache, &self.inner, event);
    }

    /// Updates portfolio calculations based on an order event.
    ///
    /// Handles balance updates for order fills and margin calculations for order changes.
    pub fn update_order(&mut self, event: &OrderEventAny) {
        update_order(
            &self.cache,
            &self.clock,
            &self.inner,
            self.config,
            event,
            OrderUpdateSource::Topic,
        );
    }

    /// Updates portfolio calculations based on a position event.
    ///
    /// Recalculates net positions, unrealized PnL, and margin requirements.
    pub fn update_position(&mut self, event: &PositionEvent) {
        update_position(&self.cache, &self.clock, &self.inner, self.config, event);
    }

    fn update_net_position(&self, instrument_id: &InstrumentId, positions_open: &[Position]) {
        let mut net_position = Decimal::ZERO;

        for open_position in positions_open {
            log::debug!("open_position: {open_position}");
            net_position += open_position.signed_decimal_qty();
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

    fn calculate_unrealized_pnl(
        &self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
    ) -> Option<Money> {
        let cache = self.cache.borrow();
        let account = match account_id {
            Some(id) => cache.account(id),
            None => cache.account_for_venue(&instrument_id.venue),
        };
        let account = if let Some(account) = account {
            account
        } else {
            log::error!(
                "Cannot calculate unrealized PnL: no account for {} / {account_id:?}",
                instrument_id.venue,
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

        let positions_open =
            cache.positions_open(None, Some(instrument_id), None, account_id, None);

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

            let price = if let Some(price) = self.get_price(&position) {
                price
            } else {
                log::debug!("Cannot calculate unrealized PnL: no prices for {instrument_id}");
                self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                return None; // Cannot calculate
            };

            let mut pnl = position.unrealized_pnl(price).as_f64();

            if let Some(base_currency) = account.base_currency() {
                let xrate = if let Some(xrate) = self.calculate_xrate_to_base(instrument, &account)
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

    fn ensure_snapshot_pnls_cached_for(&self, instrument_id: &InstrumentId) {
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
            let curr_count = self.cache.borrow().position_snapshot_count(position_id);
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
                // Track the raw frame count, not the decoded count: snapshots that fail
                // to deserialize are skipped and would otherwise make the incremental
                // path reprocess trailing valid frames next time.
                let snapshot_count = self.cache.borrow().position_snapshot_count(position_id);
                let snapshots = self
                    .cache
                    .borrow()
                    .position_snapshots(Some(position_id), None);

                let mut sum_pnl: Option<Money> = None;
                let mut last_pnl: Option<Money> = None;
                let mut snapshot_account_id: Option<AccountId> = None;

                for snapshot in snapshots {
                    snapshot_account_id.get_or_insert(snapshot.account_id);
                    if let Some(realized_pnl) = snapshot.realized_pnl {
                        if let Some(sum) = sum_pnl {
                            if sum.currency == realized_pnl.currency {
                                sum_pnl = Some(sum + realized_pnl);
                            }
                        } else {
                            sum_pnl = Some(realized_pnl);
                        }
                        last_pnl = Some(realized_pnl);
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

                if let Some(account_id) = snapshot_account_id {
                    inner.snapshot_account_ids.insert(*position_id, account_id);
                } else {
                    inner.snapshot_account_ids.remove(position_id);
                }

                inner
                    .snapshot_processed_counts
                    .insert(*position_id, snapshot_count);
            }
        } else {
            // Incremental path: only process new snapshots
            for position_id in &snapshot_position_ids {
                // Compare raw frame counts first so untouched positions skip any
                // allocation/serde cost on repeated PnL refreshes.
                let curr_count = self.cache.borrow().position_snapshot_count(position_id);
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
                let mut snapshot_account_id: Option<AccountId> = None;

                let new_snapshots = self
                    .cache
                    .borrow()
                    .position_snapshots_from(position_id, prev_count);

                for snapshot in new_snapshots {
                    snapshot_account_id.get_or_insert(snapshot.account_id);
                    if let Some(realized_pnl) = snapshot.realized_pnl {
                        if let Some(sum) = sum_pnl {
                            if sum.currency == realized_pnl.currency {
                                sum_pnl = Some(sum + realized_pnl);
                            }
                        } else {
                            sum_pnl = Some(realized_pnl);
                        }
                        last_pnl = Some(realized_pnl);
                    }
                }

                let mut inner = self.inner.borrow_mut();

                if let Some(sum) = sum_pnl {
                    inner.snapshot_sum_per_position.insert(*position_id, sum);

                    if let Some(last) = last_pnl {
                        inner.snapshot_last_per_position.insert(*position_id, last);
                    }
                }

                if let Some(account_id) = snapshot_account_id
                    && !inner.snapshot_account_ids.contains_key(position_id)
                {
                    inner.snapshot_account_ids.insert(*position_id, account_id);
                }

                inner
                    .snapshot_processed_counts
                    .insert(*position_id, curr_count);
            }
        }
    }

    fn calculate_realized_pnl(
        &self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
    ) -> Option<Money> {
        // Ensure snapshot PnLs are cached for this instrument
        self.ensure_snapshot_pnls_cached_for(instrument_id);

        let cache = self.cache.borrow();
        let account = match account_id {
            Some(id) => cache.account(id),
            None => cache.account_for_venue(&instrument_id.venue),
        };
        let account = if let Some(account) = account {
            account
        } else {
            log::error!(
                "Cannot calculate realized PnL: no account for {} / {account_id:?}",
                instrument_id.venue,
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

        let positions = cache.positions(None, Some(instrument_id), None, account_id, None);

        // Filter snapshots by account when requested so closed-position PnL
        // from other accounts on the same venue does not leak in. Sort the
        // collected IDs so the per-snapshot pending-calcs/early-return path
        // and the value accumulation iterate in a deterministic sequence.
        let mut snapshot_position_ids: Vec<PositionId> = if let Some(filter_id) = account_id {
            let inner = self.inner.borrow();
            cache
                .position_snapshot_ids(instrument_id)
                .into_iter()
                .filter(|pid| {
                    inner
                        .snapshot_account_ids
                        .get(pid)
                        .is_some_and(|id| id == filter_id)
                })
                .collect()
        } else {
            cache
                .position_snapshot_ids(instrument_id)
                .into_iter()
                .collect()
        };
        snapshot_position_ids.sort();

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
                            && positions.iter().any(|p| p.id == *position_id)
                        {
                            let xrate = if let Some(xrate) =
                                self.calculate_xrate_to_base(instrument, &account)
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
                            self.calculate_xrate_to_base(instrument, &account)
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
                            self.calculate_xrate_to_base(instrument, &account)
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
    ) -> Option<f64> {
        if !self.config.convert_to_account_base_currency {
            return Some(1.0); // No conversion needed
        }

        let base_currency = match account.base_currency() {
            Some(base_currency) => base_currency,
            None => return Some(1.0),
        };

        let settlement = instrument.settlement_currency();
        let cache = self.cache.borrow();

        if self.config.use_mark_xrates
            && let Some(xrate) = cache.get_mark_xrate(settlement, base_currency)
        {
            return Some(xrate);
        }

        cache.get_xrate(
            instrument.id().venue,
            settlement,
            base_currency,
            PriceType::Mid,
        )
    }
}

// Helper functions
fn update_quote_tick(
    cache: &Rc<RefCell<Cache>>,
    clock: &Rc<RefCell<dyn Clock>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    quote: &QuoteTick,
) {
    update_instrument_id(cache, clock, inner, config, &quote.instrument_id);
}

fn update_bar(
    cache: &Rc<RefCell<Cache>>,
    clock: &Rc<RefCell<dyn Clock>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    bar: &Bar,
) {
    let instrument_id = bar.bar_type.instrument_id();
    inner
        .borrow_mut()
        .bar_close_prices
        .insert(instrument_id, bar.close);
    update_instrument_id(cache, clock, inner, config, &instrument_id);
}

fn update_instrument_id(
    cache: &Rc<RefCell<Cache>>,
    clock: &Rc<RefCell<dyn Clock>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    instrument_id: &InstrumentId,
) {
    inner
        .borrow_mut()
        .unrealized_pnls
        .shift_remove(instrument_id);

    if inner.borrow().initialized || !inner.borrow().pending_calcs.contains(instrument_id) {
        return;
    }

    let mut result_maint = None;

    // Scoped borrow: must drop before calling AccountsManager (which borrows cache internally)
    let (account, instrument, orders_open, positions_open) = {
        let cache_ref = cache.borrow();
        let account = if let Some(account) = cache_ref.account_for_venue(&instrument_id.venue) {
            account.clone()
        } else {
            log::error!(
                "Cannot update tick: no account registered for {}",
                instrument_id.venue
            );
            return;
        };
        let instrument = if let Some(instrument) = cache_ref.instrument(instrument_id) {
            instrument.clone()
        } else {
            log::error!("Cannot update tick: no instrument found for {instrument_id}");
            return;
        };
        let orders_open: Vec<OrderAny> = cache_ref
            .orders_open(None, Some(instrument_id), None, None, None)
            .iter()
            .map(|o| (*o).clone())
            .collect();
        let positions_open: Vec<Position> = cache_ref
            .positions_open(None, Some(instrument_id), None, None, None)
            .iter()
            .map(|p| (*p).clone())
            .collect();
        (account, instrument, orders_open, positions_open)
    };

    // No cache borrow held: AccountsManager borrows cache internally for xrate lookups
    let orders_open_refs: Vec<&OrderAny> = orders_open.iter().collect();
    let result_init = inner.borrow().accounts.update_orders(
        &account,
        &instrument,
        &orders_open_refs,
        clock.borrow().timestamp_ns(),
    );

    if let AccountAny::Margin(ref margin_account) = account {
        result_maint = inner.borrow().accounts.update_positions(
            margin_account,
            &instrument,
            positions_open.iter().collect(),
            clock.borrow().timestamp_ns(),
        );
    }

    if let Some((ref updated_account, _)) = result_init {
        cache.borrow_mut().update_account(updated_account).unwrap();
    }

    let portfolio_clone = Portfolio {
        clock: clock.clone(),
        cache: cache.clone(),
        inner: inner.clone(),
        config,
    };

    let result_unrealized_pnl: Option<Money> =
        portfolio_clone.calculate_unrealized_pnl(instrument_id, None);

    if result_init.is_some()
        && (matches!(account, AccountAny::Cash(_) | AccountAny::Betting(_))
            || (result_maint.is_some() && result_unrealized_pnl.is_some()))
    {
        inner.borrow_mut().pending_calcs.remove(instrument_id);
        if inner.borrow().pending_calcs.is_empty() {
            inner.borrow_mut().initialized = true;
        }
    }
}

fn update_order(
    cache: &Rc<RefCell<Cache>>,
    clock: &Rc<RefCell<dyn Clock>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    event: &OrderEventAny,
    source: OrderUpdateSource,
) {
    let mut mark_pre_position_fill_event = None;

    if let OrderEventAny::Filled(order_filled) = event {
        match source {
            OrderUpdateSource::Endpoint => {
                mark_pre_position_fill_event = Some(order_filled.event_id);
            }
            OrderUpdateSource::Topic => {
                if inner
                    .borrow_mut()
                    .pre_position_fill_events
                    .remove(&order_filled.event_id)
                {
                    return;
                }
            }
        }
    }

    let account_id = match event.account_id() {
        Some(account_id) => account_id,
        None => {
            return; // No Account Assigned
        }
    };

    // Scoped borrow: must drop before calling AccountsManager (which borrows cache internally)
    let (instrument, orders_open) = {
        let cache_ref = cache.borrow();

        let account = if let Some(account) = cache_ref.account(&account_id) {
            account
        } else {
            log::error!("Cannot update order: no account registered for {account_id}");
            return;
        };

        match &*account {
            AccountAny::Margin(margin_account) => {
                if !margin_account.base.calculate_account_state {
                    return;
                }
            }
            AccountAny::Cash(cash_account) => {
                if !cash_account.base.calculate_account_state {
                    return;
                }
            }
            AccountAny::Betting(betting_account) => {
                if !betting_account.base.calculate_account_state {
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

        let order = if let Some(order) = cache_ref.order(&event.client_order_id()) {
            order
        } else {
            log::error!(
                "Cannot update order: {} not found in the cache",
                event.client_order_id()
            );
            return; // No Order Found
        };

        if matches!(event, OrderEventAny::Rejected(_)) && order.order_type() != OrderType::StopLimit
        {
            return; // No change to account state
        }

        let instrument = if let Some(instrument) = cache_ref.instrument(&event.instrument_id()) {
            instrument.clone()
        } else {
            log::error!(
                "Cannot update order: no instrument found for {}",
                event.instrument_id()
            );
            return;
        };

        let orders_open: Vec<OrderAny> = cache_ref
            .orders_open(None, Some(&event.instrument_id()), None, None, None)
            .iter()
            .map(|o| (*o).clone())
            .collect();

        (instrument, orders_open)
    };

    // No cache borrow held: AccountsManager borrows cache internally for xrate lookups.
    let mut working_account = if let Some(account) = cache.borrow_mut().take_account(&account_id) {
        account
    } else {
        log::error!("Cannot update order: no account registered for {account_id}");
        return;
    };

    if let OrderEventAny::Filled(order_filled) = event {
        let (post_balance, _state) =
            inner
                .borrow()
                .accounts
                .update_balances(working_account, &instrument, *order_filled);
        working_account = post_balance;

        cache.borrow_mut().cache_account_owned(working_account);

        let portfolio_clone = Portfolio {
            clock: clock.clone(),
            cache: cache.clone(),
            inner: inner.clone(),
            config,
        };

        match portfolio_clone.calculate_unrealized_pnl(&order_filled.instrument_id, None) {
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

        working_account = cache
            .borrow_mut()
            .take_account(&account_id)
            .expect("account restored before unrealized PnL calculation");
    }

    let orders_open_refs: Vec<&OrderAny> = orders_open.iter().collect();
    let account_state = inner.borrow().accounts.update_orders_in_place(
        &mut working_account,
        &instrument,
        &orders_open_refs,
        clock.borrow().timestamp_ns(),
    );

    let updated_account_id = working_account.id();

    if account_state.is_some() || matches!(event, OrderEventAny::Filled(_)) {
        cache
            .borrow_mut()
            .update_account_owned(working_account)
            .unwrap();
    } else {
        cache.borrow_mut().cache_account_owned(working_account);
    }

    // Consumed by the matching `events.order.*` topic handler; engine publishes after every endpoint send
    if let Some(event_id) = mark_pre_position_fill_event {
        inner.borrow_mut().pre_position_fill_events.insert(event_id);
    }

    if let Some(account_state) = account_state {
        msgbus::publish_account_state(
            format!("events.account.{updated_account_id}").into(),
            &account_state,
        );
    } else {
        log::debug!("Added pending calculation for {}", instrument.id());
        inner.borrow_mut().pending_calcs.insert(instrument.id());
    }

    log::debug!("Updated {event}");
}

fn update_position(
    cache: &Rc<RefCell<Cache>>,
    clock: &Rc<RefCell<dyn Clock>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    event: &PositionEvent,
) {
    let instrument_id = event.instrument_id();
    let account_id = event.account_id();

    let positions_open: Vec<Position> = {
        let cache_ref = cache.borrow();

        cache_ref
            .positions_open(None, Some(&instrument_id), None, None, None)
            .iter()
            .map(|o| (*o).clone())
            .collect()
    };

    log::debug!("position fresh from cache -> {positions_open:?}");

    update_snapshot_timer_state(cache, clock, inner, config, &account_id);

    let portfolio_clone = Portfolio {
        clock: clock.clone(),
        cache: cache.clone(),
        inner: inner.clone(),
        config,
    };

    portfolio_clone.update_net_position(&instrument_id, &positions_open);

    if let Some(calculated_unrealized_pnl) =
        portfolio_clone.calculate_unrealized_pnl(&instrument_id, None)
    {
        inner
            .borrow_mut()
            .unrealized_pnls
            .insert(event.instrument_id(), calculated_unrealized_pnl);
    } else {
        log::debug!(
            "Failed to calculate unrealized PnL for {}, marking as pending",
            event.instrument_id()
        );
        inner
            .borrow_mut()
            .pending_calcs
            .insert(event.instrument_id());
    }

    if let Some(calculated_realized_pnl) =
        portfolio_clone.calculate_realized_pnl(&instrument_id, None)
    {
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

    let account = cache.borrow().account_owned(&event.account_id());

    match account {
        Some(AccountAny::Margin(margin_account)) => {
            if !margin_account.calculate_account_state {
                return; // Nothing to calculate
            }

            let instrument = match cache.borrow().instrument(&instrument_id).cloned() {
                Some(instrument) => instrument,
                None => {
                    log::error!("Cannot update position: no instrument found for {instrument_id}");
                    return;
                }
            };

            let result = inner.borrow_mut().accounts.update_positions(
                &margin_account,
                &instrument,
                positions_open.iter().collect(),
                clock.borrow().timestamp_ns(),
            );

            if let Some((margin_account, _)) = result {
                cache
                    .borrow_mut()
                    .update_account(&AccountAny::Margin(margin_account))
                    .unwrap();
            }
        }
        Some(_) => {}
        None => {
            log::error!(
                "Cannot update position: no account registered for {}",
                event.account_id()
            );
        }
    }
}

fn update_account(
    cache: &Rc<RefCell<Cache>>,
    inner: &Rc<RefCell<PortfolioState>>,
    event: &AccountState,
) {
    if let Err(e) = cache.borrow_mut().update_account_state(event) {
        log::error!("Failed to update account state: {e}");
        return;
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

fn snapshot_timer_name(account_id: &AccountId) -> String {
    format!("portfolio_snapshot.{account_id}")
}

fn update_snapshot_timer_state(
    cache: &Rc<RefCell<Cache>>,
    clock: &Rc<RefCell<dyn Clock>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    account_id: &AccountId,
) {
    if config.snapshot_interval_ms.is_none() {
        return;
    }

    let current_count = cache
        .borrow()
        .positions_open(None, None, None, Some(account_id), None)
        .len();

    let prev_count = inner
        .borrow()
        .account_open_positions
        .get(account_id)
        .copied()
        .unwrap_or(0);

    inner
        .borrow_mut()
        .account_open_positions
        .insert(*account_id, current_count);

    if prev_count == 0 && current_count > 0 {
        arm_snapshot_timer(cache, clock, inner, config, account_id);
    } else if prev_count > 0 && current_count == 0 {
        clock
            .borrow_mut()
            .cancel_timer(&snapshot_timer_name(account_id));
    }
}

fn arm_snapshot_timer(
    cache: &Rc<RefCell<Cache>>,
    clock: &Rc<RefCell<dyn Clock>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    account_id: &AccountId,
) {
    let interval_ms = match config.snapshot_interval_ms {
        Some(ms) if ms > 0 => ms,
        _ => return,
    };
    let interval_ns = interval_ms * NANOSECONDS_IN_MILLISECOND;
    let timer_name = snapshot_timer_name(account_id);
    let account_id = *account_id;

    let cache_weak = Rc::downgrade(cache);
    let clock_weak = Rc::downgrade(clock);
    let inner_weak = Rc::downgrade(inner);

    let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(move |event| {
        let cache = match cache_weak.upgrade() {
            Some(c) => c,
            None => return,
        };
        let clock = match clock_weak.upgrade() {
            Some(c) => c,
            None => return,
        };
        let inner = match inner_weak.upgrade() {
            Some(i) => i,
            None => return,
        };
        emit_snapshot(&cache, &clock, &inner, config, &account_id, event.ts_event);
    });

    if let Err(e) = clock.borrow_mut().set_timer_ns(
        &timer_name,
        interval_ns,
        None,
        None,
        Some(TimeEventCallback::from(callback)),
        Some(true),
        Some(false),
    ) {
        log::error!("Failed to arm portfolio snapshot timer for {account_id}: {e}");
    }
}

fn emit_snapshot(
    cache: &Rc<RefCell<Cache>>,
    clock: &Rc<RefCell<dyn Clock>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    account_id: &AccountId,
    ts_event: nautilus_core::UnixNanos,
) {
    let mut portfolio = Portfolio {
        cache: cache.clone(),
        clock: clock.clone(),
        inner: inner.clone(),
        config,
    };

    let mut snapshot = match portfolio.build_snapshot(account_id) {
        Some(snapshot) => snapshot,
        None => return,
    };
    // Stamp the snapshot with the timer's scheduled fire time so the cadence
    // is preserved even if the dispatcher batches or runs late. ts_init stays
    // the construction time set by build_snapshot.
    snapshot.ts_event = ts_event;

    msgbus::publish_portfolio_snapshot(format!("events.portfolio.{account_id}").into(), &snapshot);

    let mut inner_mut = inner.borrow_mut();
    push_bounded(
        &mut inner_mut.portfolio_snapshots,
        *account_id,
        snapshot,
        SNAPSHOT_BUFFER_CAP,
    );
}

/// Appends `snapshot` onto the per-account ring, dropping the oldest entry when at `cap`.
fn push_bounded(
    snapshots: &mut AHashMap<AccountId, VecDeque<PortfolioSnapshot>>,
    account_id: AccountId,
    snapshot: PortfolioSnapshot,
    cap: usize,
) {
    let ring = snapshots.entry(account_id).or_default();
    if ring.len() == cap {
        ring.pop_front();
    }
    ring.push_back(snapshot);
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{enums::AccountType, identifiers::AccountId};
    use rstest::rstest;

    use super::*;

    fn mk_snapshot(seq: u64) -> PortfolioSnapshot {
        PortfolioSnapshot::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            UUID4::new(),
            UnixNanos::from(seq),
            UnixNanos::from(seq),
        )
    }

    #[rstest]
    fn push_bounded_drops_oldest_when_at_cap() {
        let account_id = AccountId::new("SIM-001");
        let mut snapshots: AHashMap<AccountId, VecDeque<PortfolioSnapshot>> = AHashMap::new();

        for seq in 0..5 {
            push_bounded(&mut snapshots, account_id, mk_snapshot(seq), 3);
        }

        let ring = snapshots.get(&account_id).expect("ring exists");
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.front().unwrap().ts_event, UnixNanos::from(2));
        assert_eq!(ring.back().unwrap().ts_event, UnixNanos::from(4));
    }
}
