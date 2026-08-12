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

#![warn(clippy::clone_on_ref_ptr)]

use std::{
    cell::RefCell,
    collections::{BTreeSet, VecDeque},
    fmt::Debug,
    rc::Rc,
};

use ahash::{AHashMap, AHashSet};
use indexmap::{IndexMap, IndexSet};
use nautilus_analysis::{analyzer::PortfolioAnalyzer, snapshot::PortfolioStatistics};
use nautilus_common::{
    cache::{AccountLookupError, AccountRef, Cache},
    clock::Clock,
    enums::LogColor,
    msgbus::{self, MessagingSwitchboard, TypedHandler, TypedIntoHandler},
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    UUID4, UnixNanos, WeakCell,
    datetime::{NANOSECONDS_IN_DAY, NANOSECONDS_IN_MILLISECOND},
};
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

// Sized for post-run backtest analysis (e.g. ~11 days at 1s cadence, or years
// at per-minute cadence), long-lived live deployments should consume snapshots
// via the message bus instead of relying on this buffer.
const SNAPSHOT_BUFFER_CAP: usize = 1_000_000;

struct PortfolioState {
    accounts: AccountsManager,
    analyzer: PortfolioAnalyzer,
    unrealized_pnls: IndexMap<InstrumentId, Money>,
    realized_pnls: IndexMap<InstrumentId, Money>,
    recorded_closed_position_cycles: AHashSet<(PositionId, UnixNanos)>,
    snapshot_sum_per_position: AHashMap<PositionId, Money>,
    snapshot_last_per_position: AHashMap<PositionId, Money>,
    snapshot_currency_mismatches: AHashSet<PositionId>,
    snapshot_aggregation_overflows: AHashSet<PositionId>,
    snapshot_processed_counts: AHashMap<PositionId, usize>,
    snapshot_processed_revisions: AHashMap<PositionId, u64>,
    snapshot_account_ids: AHashMap<PositionId, AccountId>,
    net_positions: IndexMap<InstrumentId, Decimal>,
    pending_calcs: AHashSet<InstrumentId>,
    bar_close_prices: AHashMap<InstrumentId, Price>,
    last_prices: AHashMap<(InstrumentId, PositionSide), Price>,
    last_xrates: AHashMap<(Venue, Currency, Currency), Decimal>,
    stale_prices: AHashSet<(InstrumentId, PositionSide)>,
    stale_xrates: AHashSet<(Venue, Currency, Currency)>,
    initialized: bool,
    last_account_state_log_ts: AHashMap<AccountId, u64>,
    min_account_state_logging_interval_ns: u64,
    venues_missing_price: AHashMap<Venue, AHashMap<Option<AccountId>, AHashSet<InstrumentId>>>,
    account_open_positions: AHashMap<AccountId, usize>,
    equity_curve_accounts: AHashSet<AccountId>,
    equity_curve_finalized: bool,
    portfolio_snapshots: AHashMap<AccountId, VecDeque<PortfolioSnapshot>>,
    pre_position_fill_events: AHashSet<UUID4>,
}

#[derive(Clone, Copy)]
enum OrderUpdateSource {
    Endpoint,
    Topic,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkValueMode {
    Gross,
    Equity,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UnrealizedPnlError {
    MissingInput,
    Invalid,
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
            recorded_closed_position_cycles: AHashSet::new(),
            snapshot_sum_per_position: AHashMap::new(),
            snapshot_last_per_position: AHashMap::new(),
            snapshot_currency_mismatches: AHashSet::new(),
            snapshot_aggregation_overflows: AHashSet::new(),
            snapshot_processed_counts: AHashMap::new(),
            snapshot_processed_revisions: AHashMap::new(),
            snapshot_account_ids: AHashMap::new(),
            net_positions: IndexMap::new(),
            pending_calcs: AHashSet::new(),
            bar_close_prices: AHashMap::new(),
            last_prices: AHashMap::new(),
            last_xrates: AHashMap::new(),
            stale_prices: AHashSet::new(),
            stale_xrates: AHashSet::new(),
            initialized: false,
            last_account_state_log_ts: AHashMap::new(),
            min_account_state_logging_interval_ns,
            venues_missing_price: AHashMap::new(),
            account_open_positions: AHashMap::new(),
            equity_curve_accounts: AHashSet::new(),
            equity_curve_finalized: false,
            portfolio_snapshots: AHashMap::new(),
            pre_position_fill_events: AHashSet::new(),
        }
    }

    fn reset(&mut self) {
        log::debug!("RESETTING");
        self.net_positions.clear();
        self.unrealized_pnls.clear();
        self.realized_pnls.clear();
        self.recorded_closed_position_cycles.clear();
        self.snapshot_sum_per_position.clear();
        self.snapshot_last_per_position.clear();
        self.snapshot_currency_mismatches.clear();
        self.snapshot_aggregation_overflows.clear();
        self.snapshot_processed_counts.clear();
        self.snapshot_processed_revisions.clear();
        self.snapshot_account_ids.clear();
        self.pending_calcs.clear();
        self.bar_close_prices.clear();
        self.last_prices.clear();
        self.last_xrates.clear();
        self.stale_prices.clear();
        self.stale_xrates.clear();
        self.last_account_state_log_ts.clear();
        self.venues_missing_price.clear();
        self.account_open_positions.clear();
        self.equity_curve_accounts.clear();
        self.equity_curve_finalized = false;
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
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        config: Option<PortfolioConfig>,
    ) -> Self {
        let config = config.unwrap_or_default();
        let inner = Rc::new(RefCell::new(PortfolioState::new(
            Rc::clone(&clock),
            Rc::clone(&cache),
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
            clock: Rc::clone(&self.clock),
            cache: Rc::clone(&self.cache),
            inner: Rc::clone(&self.inner),
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
            let cache = Rc::clone(cache);
            let clock = Rc::clone(clock);
            let inner = WeakCell::clone(&inner_weak);

            TypedHandler::from(move |event: &AccountState| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    update_account(&clock, &cache, &inner_rc, config, event);
                }
            })
        };

        let update_position_handler = {
            let cache = Rc::clone(cache);
            let clock = Rc::clone(clock);
            let inner = WeakCell::clone(&inner_weak);
            TypedHandler::from(move |event: &PositionEvent| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    update_position(&cache, &clock, &inner_rc, config, event);
                }
            })
        };

        let update_quote_handler = {
            let cache = Rc::clone(cache);
            let clock = Rc::clone(clock);
            let inner = WeakCell::clone(&inner_weak);
            TypedHandler::from(move |quote: &QuoteTick| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    update_quote_tick(&cache, &clock, &inner_rc, config, quote);
                }
            })
        };

        let update_bar_handler = {
            let cache = Rc::clone(cache);
            let clock = Rc::clone(clock);
            let inner = WeakCell::clone(&inner_weak);
            TypedHandler::from(move |bar: &Bar| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    update_bar(&cache, &clock, &inner_rc, config, bar);
                }
            })
        };

        let update_mark_price_handler = {
            let cache = Rc::clone(cache);
            let clock = Rc::clone(clock);
            let inner = WeakCell::clone(&inner_weak);
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
            let cache = Rc::clone(cache);
            let inner = WeakCell::clone(&inner_weak);
            TypedHandler::from(move |event: &OrderEventAny| {
                if let Some(inner_rc) = inner.upgrade() {
                    let inner_rc: Rc<RefCell<PortfolioState>> = inner_rc.into();
                    on_order_event(&cache, &inner_rc, event);
                }
            })
        };

        let endpoint = MessagingSwitchboard::portfolio_update_account();
        msgbus::register_account_state_endpoint(endpoint, update_account_handler.clone());

        let update_order_endpoint_handler = {
            let cache = Rc::clone(cache);
            let clock = Rc::clone(clock);
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
        let (snapshot_accounts, equity_curve_accounts) = {
            let inner = self.inner.borrow();
            (
                inner
                    .account_open_positions
                    .keys()
                    .copied()
                    .collect::<Vec<_>>(),
                inner
                    .equity_curve_accounts
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
            )
        };

        for account_id in snapshot_accounts {
            self.clock
                .borrow_mut()
                .cancel_timer(&snapshot_timer_name(account_id));
        }

        for account_id in equity_curve_accounts {
            self.clock
                .borrow_mut()
                .cancel_timer(&equity_curve_timer_name(account_id));
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
    pub fn instrument_initial_margins(&self, venue: &Venue) -> IndexMap<InstrumentId, Money> {
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
    pub fn instrument_maintenance_margins(&self, venue: &Venue) -> IndexMap<InstrumentId, Money> {
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
        target_currency: Option<Currency>,
    ) -> Option<IndexMap<Currency, Money>> {
        let (unrealized_pnls, unpriced) =
            self.unrealized_pnls_with_missing(*venue, account_id, target_currency)?;

        if unpriced.is_empty() {
            Some(unrealized_pnls)
        } else {
            None
        }
    }

    fn unrealized_pnls_with_missing(
        &self,
        venue: Venue,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Option<(IndexMap<Currency, Money>, AHashSet<InstrumentId>)> {
        let instrument_ids = {
            let cache = self.cache.borrow();
            let positions = cache.positions_open(Some(&venue), None, None, account_id, None);

            if positions.is_empty() {
                return Some((IndexMap::new(), AHashSet::new()));
            }

            // IndexSet preserves the deterministic order of cache.positions
            // through the dedup so the returned currency map iterates in a
            // stable order across runs.
            let instrument_ids: IndexSet<InstrumentId> =
                positions.iter().map(|p| p.instrument_id).collect();

            instrument_ids
        };

        let mut unrealized_pnls: IndexMap<Currency, Money> = IndexMap::new();
        let mut unpriced: AHashSet<InstrumentId> = AHashSet::new();

        for instrument_id in instrument_ids {
            match self.unrealized_pnls_by_account(&instrument_id, None, account_id, target_currency)
            {
                Ok(pnls) => {
                    for pnl in pnls {
                        checked_add_money_map(&mut unrealized_pnls, pnl, "unrealized PnLs")?;
                    }
                }
                Err(UnrealizedPnlError::MissingInput) => {
                    unpriced.insert(instrument_id);
                }
                Err(UnrealizedPnlError::Invalid) => return None,
            }
        }

        if account_id.is_some() {
            self.update_missing_price_state(venue, account_id.copied(), &unpriced);
        }

        Some((unrealized_pnls, unpriced))
    }

    /// Returns the realized PnLs for all positions at the given venue.
    ///
    /// Calculates total realized profit and loss from closed positions.
    #[must_use]
    pub fn realized_pnls(
        &mut self,
        venue: &Venue,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Option<IndexMap<Currency, Money>> {
        let instrument_ids = {
            let cache = self.cache.borrow();
            let positions = cache.positions(Some(venue), None, None, account_id, None);

            if positions.is_empty() {
                return Some(IndexMap::new()); // Nothing to calculate
            }

            let instrument_ids: IndexSet<InstrumentId> =
                positions.iter().map(|p| p.instrument_id).collect();

            instrument_ids
        };

        let mut realized_pnls: IndexMap<Currency, Money> = IndexMap::new();

        for instrument_id in instrument_ids {
            self.ensure_snapshot_pnls_cached_for(&instrument_id);
            for pnl in self.realized_pnls_by_account(&instrument_id, account_id, target_currency)? {
                checked_add_money_map(&mut realized_pnls, pnl, "realized PnLs")?;
            }
        }

        Some(realized_pnls)
    }

    #[must_use]
    pub fn net_exposures(
        &self,
        venue: &Venue,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Option<IndexMap<Currency, Money>> {
        let cache = self.cache.borrow();

        if let Some(id) = account_id
            && cache.account(id).is_none()
        {
            log::error!("Cannot calculate net exposures: no account for {id}");
            return None;
        }

        if account_id.is_none() && cache.account_for_venue(venue).is_none() {
            let has_position_account = cache
                .positions(Some(venue), None, None, None, None)
                .iter()
                .any(|position| cache.account(&position.account_id).is_some());
            if !has_position_account {
                log::error!("Cannot calculate net exposures: no account registered for {venue}");
                return None;
            }
        }

        let instrument_ids: IndexSet<InstrumentId> = {
            let positions_open = cache.positions_open(Some(venue), None, None, account_id, None);
            if positions_open.is_empty() {
                return Some(IndexMap::new()); // Nothing to calculate
            }
            positions_open
                .iter()
                .map(|position| position.instrument_id)
                .collect()
        };
        drop(cache);

        let mut net_exposures = IndexMap::new();

        for instrument_id in instrument_ids {
            let exposure = self.net_exposure(&instrument_id, None, account_id, target_currency)?;
            if exposure.is_zero() {
                continue;
            }
            checked_add_money_map(&mut net_exposures, exposure, "net exposures")?;
        }

        Some(net_exposures)
    }

    #[must_use]
    pub fn unrealized_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        self.unrealized_pnl_for_account(instrument_id, None, None, None)
    }

    #[must_use]
    pub fn unrealized_pnl_for_account(
        &mut self,
        instrument_id: &InstrumentId,
        price: Option<Price>,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Option<Money> {
        let use_cache = price.is_none() && account_id.is_none() && target_currency.is_none();
        if use_cache {
            let has_open_position = !self
                .cache
                .borrow()
                .positions_open(None, Some(instrument_id), None, None, None)
                .is_empty();

            if !has_open_position
                && let Some(pnl) = self
                    .inner
                    .borrow()
                    .unrealized_pnls
                    .get(instrument_id)
                    .copied()
            {
                return Some(pnl);
            }
        }

        let pnl = self
            .aggregate_unrealized_pnl_by_account(instrument_id, price, account_id, target_currency)
            .ok()?;

        if use_cache {
            self.inner
                .borrow_mut()
                .unrealized_pnls
                .insert(*instrument_id, pnl);
        }
        Some(pnl)
    }

    #[must_use]
    pub fn realized_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        self.realized_pnl_for_account(instrument_id, None, None)
    }

    #[must_use]
    pub fn realized_pnl_for_account(
        &mut self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Option<Money> {
        self.ensure_snapshot_pnls_cached_for(instrument_id);

        let use_cache = account_id.is_none() && target_currency.is_none();
        let pnl =
            self.aggregate_realized_pnl_by_account(instrument_id, account_id, target_currency)?;

        if use_cache {
            self.inner
                .borrow_mut()
                .realized_pnls
                .insert(*instrument_id, pnl);
        }
        Some(pnl)
    }

    /// Returns the total PnL for the given instrument ID.
    ///
    /// Total PnL = Realized PnL + Unrealized PnL
    #[must_use]
    pub fn total_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        self.total_pnl_for_account(instrument_id, None, None, None)
    }

    #[must_use]
    pub fn total_pnl_for_account(
        &mut self,
        instrument_id: &InstrumentId,
        price: Option<Price>,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Option<Money> {
        let realized = self.realized_pnl_for_account(instrument_id, account_id, target_currency)?;
        let unrealized =
            self.unrealized_pnl_for_account(instrument_id, price, account_id, target_currency)?;

        checked_add_money(realized, unrealized, "total PnL")
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
        target_currency: Option<Currency>,
    ) -> Option<IndexMap<Currency, Money>> {
        let realized_pnls = self.realized_pnls(venue, account_id, target_currency)?;
        let unrealized_pnls = self.unrealized_pnls(venue, account_id, target_currency)?;

        let mut total_pnls = realized_pnls;
        for unrealized in unrealized_pnls.into_values() {
            checked_add_money_map(&mut total_pnls, unrealized, "total PnLs")?;
        }

        Some(total_pnls)
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
        self.mark_values_with_mode(*venue, account_id, MarkValueMode::Gross)
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
                None => cache.account_for_venue(venue).or_else(|| {
                    cache
                        .positions(Some(venue), None, None, None, None)
                        .into_iter()
                        .next()
                        .and_then(|p| cache.account(&p.account_id))
                }),
            };

            match account {
                Some(account) => {
                    let equity: IndexMap<Currency, Decimal> = account
                        .balances_total()
                        .into_iter()
                        .map(|(c, m)| (c, m.as_decimal()))
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
                self.clear_missing_price_state(*venue, account_id.copied());
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
                        None => {
                            self.calculate_unrealized_pnl(&instrument_id, None, account_id, None)
                        }
                    };

                    match pnl {
                        Some(pnl) => {
                            *equity.entry(pnl.currency).or_insert(Decimal::ZERO) +=
                                pnl.as_decimal();
                        }
                        None => {
                            unpriced.insert(instrument_id);
                        }
                    }
                }
                self.update_missing_price_state(*venue, account_id.copied(), &unpriced);
            }
        } else if self.accumulate_mark_values(
            *venue,
            account_id,
            &mut equity,
            &mut unpriced,
            MarkValueMode::Equity,
        ) {
            self.update_missing_price_state(*venue, account_id.copied(), &unpriced);
        } else {
            self.clear_missing_price_state(*venue, account_id.copied());
        }

        decimal_map_to_money(equity)
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
        let (open_venues, open_instrument_ids, open_price_keys) = {
            let cache = self.cache.borrow();
            let positions = cache.positions_open(None, None, None, Some(account_id), None);
            let venues: AHashSet<Venue> = positions
                .iter()
                .map(|position| position.instrument_id.venue)
                .collect();
            let instrument_ids: AHashSet<InstrumentId> = positions
                .iter()
                .map(|position| position.instrument_id)
                .collect();
            let price_keys: AHashSet<(InstrumentId, PositionSide)> = positions
                .iter()
                .map(|position| (position.instrument_id, position.side))
                .collect();
            (venues, instrument_ids, price_keys)
        };
        let all_venues: AHashSet<Venue> = self
            .cache
            .borrow()
            .positions(None, None, None, Some(account_id), None)
            .iter()
            .map(|p| p.instrument_id.venue)
            .collect();
        let mut unrealized: IndexMap<Currency, Money> = IndexMap::new();
        let mut realized: IndexMap<Currency, Money> = IndexMap::new();
        let mut equity: IndexMap<Currency, Money> = account.balances_total().into_iter().collect();
        let mut snapshot_unpriced = AHashSet::new();

        for venue in &open_venues {
            let (unrealized_pnls, venue_unpriced) =
                self.unrealized_pnls_with_missing(*venue, Some(account_id), None)?;
            snapshot_unpriced.extend(venue_unpriced);

            for money in unrealized_pnls.into_values() {
                checked_add_money_map(&mut unrealized, money, "snapshot unrealized PnL")?;
            }
        }

        for venue in &all_venues {
            let realized_pnls = match self.realized_pnls(venue, Some(account_id), None) {
                Some(pnls) => pnls,
                None if !self.has_nonzero_realized_pnl(*venue, *account_id) => IndexMap::new(),
                None => return None,
            };

            for money in realized_pnls.into_values() {
                checked_add_money_map(&mut realized, money, "snapshot realized PnL")?;
            }
        }

        match &account {
            AccountAny::Margin(_) => {
                for value in unrealized.values() {
                    checked_add_money_map(&mut equity, *value, "snapshot equity")?;
                }
            }
            AccountAny::Cash(_) | AccountAny::Betting(_) => {
                for venue in &open_venues {
                    for money in self
                        .mark_values_with_mode(*venue, Some(account_id), MarkValueMode::Equity)
                        .into_values()
                    {
                        checked_add_money_map(&mut equity, money, "snapshot equity")?;
                    }
                    snapshot_unpriced
                        .extend(self.missing_price_instruments_for_account(*venue, *account_id));
                }
            }
        }

        let base_currency_equity = if self.config.convert_to_account_base_currency {
            account
                .base_currency()
                .and_then(|currency| equity.get(&currency).copied())
        } else {
            None
        };
        let (mut stale_instruments, mut stale_currencies, mut unpriced_instruments) = {
            let inner = self.inner.borrow();
            let stale_instruments = inner
                .stale_prices
                .iter()
                .filter(|key| open_price_keys.contains(key))
                .map(|(instrument_id, _)| *instrument_id)
                .collect::<AHashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let stale_currencies = account
                .base_currency()
                .map_or_else(Vec::new, |base_currency| {
                    inner
                        .stale_xrates
                        .iter()
                        .filter(|(venue, _, target)| {
                            open_venues.contains(venue) && *target == base_currency
                        })
                        .map(|(_, source, _)| *source)
                        .collect::<AHashSet<_>>()
                        .into_iter()
                        .collect()
                });
            let unpriced_instruments: Vec<InstrumentId> = snapshot_unpriced
                .iter()
                .filter(|instrument_id| open_instrument_ids.contains(instrument_id))
                .copied()
                .collect();
            (stale_instruments, stale_currencies, unpriced_instruments)
        };
        stale_instruments.sort_unstable();
        stale_currencies.sort_unstable_by_key(|currency| currency.code);
        unpriced_instruments.sort_unstable();
        let is_stale = !stale_instruments.is_empty()
            || !stale_currencies.is_empty()
            || !unpriced_instruments.is_empty();

        let unrealized_pnls: Vec<Money> = unrealized.into_values().collect();
        let realized_pnls: Vec<Money> = realized.into_values().collect();
        let total_equity: Vec<Money> = equity.into_values().collect();

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
            base_currency_equity,
            is_stale,
            stale_instruments,
            stale_currencies,
            unpriced_instruments,
            UUID4::new(),
            ts_now,
            ts_now,
        ))
    }

    fn has_nonzero_realized_pnl(&self, venue: Venue, account_id: AccountId) -> bool {
        let cache = self.cache.borrow();
        cache
            .positions(Some(&venue), None, None, Some(&account_id), None)
            .iter()
            .any(|position| position.realized_pnl.is_some_and(|pnl| !pnl.is_zero()))
            || cache
                .position_snapshots(None, Some(&account_id))
                .iter()
                .any(|position| {
                    position.instrument_id.venue == venue
                        && position.realized_pnl.is_some_and(|pnl| !pnl.is_zero())
                })
    }

    /// Returns the recorded portfolio snapshots for the given account, in order of emission.
    ///
    /// With `equity_curve` enabled, snapshots are recorded at account registration, every
    /// UTC midnight including while flat, and shutdown. Setting `snapshot_interval_ms` adds
    /// fine-grained samples while the account holds an open position. The ring is bounded;
    /// long-lived live deployments should consume snapshots via the message bus instead of
    /// relying on this buffer. Cleared on [`Portfolio::reset`].
    #[must_use]
    pub fn snapshots(&self, account_id: &AccountId) -> Vec<PortfolioSnapshot> {
        self.inner
            .borrow()
            .portfolio_snapshots
            .get(account_id)
            .map(|ring| ring.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Records one final equity-curve sample for every registered account and stops its timer.
    ///
    /// Has no effect when `equity_curve` is disabled. Calling this method more than once
    /// before [`Portfolio::reset`] has no effect.
    pub fn finalize_equity_curve(&mut self) {
        if !self.config.equity_curve {
            return;
        }

        let account_ids = {
            let mut inner = self.inner.borrow_mut();
            if inner.equity_curve_finalized {
                return;
            }
            inner.equity_curve_finalized = true;
            inner
                .equity_curve_accounts
                .iter()
                .copied()
                .collect::<Vec<_>>()
        };
        let ts_event = self.clock.borrow().timestamp_ns();

        for account_id in account_ids {
            emit_snapshot(
                &self.cache,
                &self.clock,
                &self.inner,
                self.config,
                account_id,
                ts_event,
            );
            self.clock
                .borrow_mut()
                .cancel_timer(&equity_curve_timer_name(account_id));
        }
    }

    /// Returns the instruments currently flagged as unpriceable for the given venue and account.
    ///
    /// An entry is added the first time [`Portfolio::mark_values`] cannot value an open position
    /// because its price is missing or its notional is invalid (after also emitting a warn log),
    /// and removed once the instrument can be valued again so a subsequent drop re-warns.
    #[must_use]
    pub fn missing_price_instruments(
        &self,
        venue: &Venue,
        account_id: Option<&AccountId>,
    ) -> Vec<InstrumentId> {
        let inner = self.inner.borrow();
        let observations = inner.venues_missing_price.get(venue);
        let mut ids: Vec<InstrumentId> = match account_id {
            Some(account_id) => observations
                .and_then(|observations| observations.get(&Some(*account_id)))
                .map(|ids| ids.iter().copied().collect())
                .unwrap_or_default(),
            None => observations
                .map(|observations| {
                    observations
                        .values()
                        .flat_map(|ids| ids.iter().copied())
                        .collect::<AHashSet<_>>()
                        .into_iter()
                        .collect()
                })
                .unwrap_or_default(),
        };
        // Sort so the public Vec is deterministic even though the underlying
        // tracking set is AHash-backed.
        ids.sort();
        ids
    }

    fn missing_price_instruments_for_account(
        &self,
        venue: Venue,
        account_id: AccountId,
    ) -> AHashSet<InstrumentId> {
        self.inner
            .borrow()
            .venues_missing_price
            .get(&venue)
            .and_then(|observations| observations.get(&Some(account_id)))
            .cloned()
            .unwrap_or_default()
    }

    fn update_missing_price_state(
        &self,
        venue: Venue,
        account_id: Option<AccountId>,
        unpriced: &AHashSet<InstrumentId>,
    ) {
        let mut inner = self.inner.borrow_mut();
        let tracked: AHashSet<InstrumentId> = inner
            .venues_missing_price
            .get(&venue)
            .into_iter()
            .flat_map(|observations| observations.values())
            .flatten()
            .copied()
            .collect();

        // Sort first so the warn-log sequence is deterministic across runs.
        let mut ids: Vec<InstrumentId> = unpriced.iter().copied().collect();
        ids.sort();
        for instrument_id in ids {
            if !tracked.contains(&instrument_id) {
                log::warn!(
                    "Cannot value open position {instrument_id}; ensure its notional inputs are \
                    valid and subscribe to quotes, trades, or bars for continuous mark-to-market \
                    equity"
                );
            }
        }

        let remove_venue = {
            let observations = inner.venues_missing_price.entry(venue).or_default();
            if unpriced.is_empty() {
                observations.remove(&account_id);
            } else {
                observations.insert(account_id, unpriced.clone());
            }
            observations.is_empty()
        };

        if remove_venue {
            inner.venues_missing_price.remove(&venue);
        }
    }

    fn clear_missing_price_state(&self, venue: Venue, account_id: Option<AccountId>) {
        let mut inner = self.inner.borrow_mut();
        let Some(account_id) = account_id else {
            inner.venues_missing_price.remove(&venue);
            return;
        };
        let remove_venue = if let Some(observations) = inner.venues_missing_price.get_mut(&venue) {
            observations.remove(&Some(account_id));
            observations.is_empty()
        } else {
            false
        };

        if remove_venue {
            inner.venues_missing_price.remove(&venue);
        }
    }

    fn mark_values_with_mode(
        &self,
        venue: Venue,
        account_id: Option<&AccountId>,
        mode: MarkValueMode,
    ) -> IndexMap<Currency, Money> {
        let mut values: IndexMap<Currency, Decimal> = IndexMap::new();
        let mut unpriced: AHashSet<InstrumentId> = AHashSet::new();

        if self.accumulate_mark_values(venue, account_id, &mut values, &mut unpriced, mode) {
            self.update_missing_price_state(venue, account_id.copied(), &unpriced);
        } else {
            self.clear_missing_price_state(venue, account_id.copied());
        }

        decimal_map_to_money(values)
    }

    // Returns `true` if at least one open position was seen (priced or not),
    // `false` if the venue is flat. Unpriced instruments are written to
    // `unpriced` for the caller to flow into `update_missing_price_state`.
    fn accumulate_mark_values(
        &self,
        venue: Venue,
        account_id: Option<&AccountId>,
        values: &mut IndexMap<Currency, Decimal>,
        unpriced: &mut AHashSet<InstrumentId>,
        mode: MarkValueMode,
    ) -> bool {
        let cache = self.cache.borrow();
        let positions = cache.positions_open(Some(&venue), None, None, account_id, None);

        if positions.is_empty() {
            return false;
        }

        let valuation_account = match account_id {
            Some(id) => cache.account(id),
            None => cache
                .account_for_venue(&venue)
                .or_else(|| positions.first().and_then(|p| cache.account(&p.account_id))),
        };
        let equity_account_id = if mode == MarkValueMode::Equity {
            valuation_account.as_ref().map(|a| a.id())
        } else {
            None
        };
        let mut xrate_cache: AHashMap<Currency, Option<Decimal>> = AHashMap::new();

        for position in positions {
            let sign = match position.side {
                PositionSide::Long => Decimal::ONE,
                PositionSide::Short => Decimal::NEGATIVE_ONE,
                PositionSide::Flat | PositionSide::NoPositionSide => continue,
            };

            let instrument = match cache.instrument(&position.instrument_id) {
                Some(i) => i,
                None => {
                    unpriced.insert(position.instrument_id);
                    continue;
                }
            };

            let position_account = cache.account(&position.account_id);
            let base_currency_is_credited = mode == MarkValueMode::Equity
                && equity_account_id == Some(position.account_id)
                && position_account.as_ref().is_some_and(|account| {
                    matches!(&**account, AccountAny::Cash(_))
                        && account.base_currency().is_none()
                        && position.base_currency.is_some_and(|base| {
                            position.settlement_currency != base
                                && account.balances().contains_key(&base)
                        })
                });

            if base_currency_is_credited {
                continue;
            }

            let price = match self.get_price(&position) {
                Some(p) => p,
                None => {
                    unpriced.insert(position.instrument_id);
                    continue;
                }
            };

            let notional = match position.try_notional_value(price) {
                Ok(notional) => notional,
                Err(e) => {
                    log::error!(
                        "Cannot calculate mark value: invalid notional value for {}: {e}",
                        position.instrument_id
                    );
                    unpriced.insert(position.instrument_id);
                    continue;
                }
            };
            let cost_currency = notional.currency;
            let (xrate, currency) = if self.config.convert_to_account_base_currency
                && let Some(account) = valuation_account.as_ref()
                && let Some(base_currency) = account.base_currency()
            {
                let xrate_opt = *xrate_cache.entry(cost_currency).or_insert_with(|| {
                    self.calculate_xrate_to_base(instrument, account, cost_currency)
                });
                let xrate = match xrate_opt {
                    Some(x) => x,
                    None => {
                        unpriced.insert(position.instrument_id);
                        continue;
                    }
                };
                (xrate, base_currency)
            } else {
                (Decimal::ONE, cost_currency)
            };

            // Sum exact Decimals; the caller rounds once so sub-precision positions survive
            let value = notional.as_decimal() * xrate * sign;
            *values.entry(currency).or_insert(Decimal::ZERO) += value;
        }

        true
    }

    #[must_use]
    pub fn net_exposure(
        &self,
        instrument_id: &InstrumentId,
        price: Option<Price>,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Option<Money> {
        let cache = self.cache.borrow();

        let instrument = if let Some(instrument) = cache.instrument(instrument_id) {
            instrument
        } else {
            log::error!("Cannot calculate net exposure: no instrument for {instrument_id}");
            return None;
        };

        if let Some(account_id) = account_id
            && cache.account(account_id).is_none()
        {
            log::error!("Cannot calculate net exposure: no account for {account_id}");
            return None;
        }

        let positions_open =
            cache.positions_open(None, Some(instrument_id), None, account_id, None);

        if positions_open.is_empty() {
            return Some(Money::zero(
                target_currency.unwrap_or_else(|| instrument.cost_currency()),
            ));
        }

        let mut net_exposure = Decimal::ZERO;
        let mut output_currency = target_currency;
        let mut native_currency: Option<Currency> = None;

        for position in &positions_open {
            let sign = match position.side {
                PositionSide::Long => Decimal::ONE,
                PositionSide::Short => Decimal::NEGATIVE_ONE,
                _ => {
                    log::error!(
                        "Cannot calculate net exposure: position is flat for {}",
                        position.instrument_id
                    );
                    continue; // Nothing to calculate
                }
            };

            // Get account for THIS position
            let account = if let Some(account) = cache.account(&position.account_id) {
                account
            } else if account_id.is_none()
                && let Some(account) = cache.account_for_venue(&instrument.id().venue)
            {
                account
            } else {
                log::error!(
                    "Cannot calculate net exposure: no account for {}",
                    position.account_id
                );
                return None;
            };

            let base_currency = self.conversion_base_currency(&account);

            // Validate consistent base currency across accounts when the caller did not select a
            // target. An explicit target provides the common aggregation currency instead.
            if target_currency.is_none()
                && let Some(base) = base_currency
            {
                match output_currency {
                    None => {
                        output_currency = Some(base);
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

            let price = price.or_else(|| self.get_price(position))?;
            let notional_value = match position.try_notional_value(price) {
                Ok(notional) => notional,
                Err(e) => {
                    log::error!(
                        "Cannot calculate net exposure: invalid notional value for {}: {e}",
                        position.instrument_id
                    );
                    return None;
                }
            };
            let source_currency = notional_value.currency;

            if target_currency.is_some() {
                match native_currency {
                    None => native_currency = Some(source_currency),
                    Some(first) if first != source_currency => {
                        log::error!(
                            "Cannot calculate net exposure: positions have different cost \
                            currencies ({first} vs {source_currency})"
                        );
                        return None;
                    }
                    _ => {}
                }

                let Some(signed) = notional_value.as_decimal().checked_mul(sign) else {
                    log::error!("Cannot calculate net exposure: signed notional overflow");
                    return None;
                };
                let Some(updated) = net_exposure.checked_add(signed) else {
                    log::error!("Cannot calculate net exposure: total overflow");
                    return None;
                };
                net_exposure = updated;
                continue;
            }

            if base_currency.is_none() {
                match output_currency {
                    None => output_currency = Some(source_currency),
                    Some(first) if first != source_currency => {
                        log::error!(
                            "Cannot calculate net exposure: positions have different cost \
                            currencies ({first} vs {source_currency})"
                        );
                        return None;
                    }
                    _ => {}
                }
            }

            let xrate = self.calculate_xrate_to_base(instrument, &account, source_currency);
            let xrate = if let Some(xrate) = xrate {
                xrate
            } else {
                log::error!(
                    "Cannot calculate net exposures: insufficient data for {}/{:?}",
                    source_currency,
                    account.base_currency()
                );
                return None;
            };

            let Some(converted) = notional_value.as_decimal().checked_mul(xrate) else {
                log::error!("Cannot calculate net exposure: currency conversion overflow");
                return None;
            };
            let Some(signed) = converted.checked_mul(sign) else {
                log::error!("Cannot calculate net exposure: signed notional overflow");
                return None;
            };
            let Some(updated) = net_exposure.checked_add(signed) else {
                log::error!("Cannot calculate net exposure: total overflow");
                return None;
            };
            net_exposure = updated;
        }

        // Net exposure is reported as a magnitude once opposing sides are netted
        let mut net_exposure = if net_exposure.is_sign_negative() {
            let Some(value) = net_exposure.checked_mul(Decimal::NEGATIVE_ONE) else {
                log::error!("Cannot calculate net exposure: absolute value overflow");
                return None;
            };
            value
        } else {
            net_exposure
        };

        if let Some(target_currency) = target_currency {
            if net_exposure == Decimal::ZERO {
                return Some(Money::zero(target_currency));
            }

            let source_currency = native_currency.unwrap_or_else(|| instrument.cost_currency());
            let Some(xrate) = self.calculate_xrate(
                instrument.id().venue,
                source_currency,
                target_currency,
                false,
            ) else {
                log::error!(
                    "Cannot calculate net exposure: insufficient data for {source_currency}/{target_currency}"
                );
                return None;
            };
            let Some(converted) = net_exposure.checked_mul(xrate) else {
                log::error!("Cannot calculate net exposure: currency conversion overflow");
                return None;
            };
            net_exposure = converted.round_dp(u32::from(target_currency.precision));
        }

        let output_currency = output_currency.unwrap_or_else(|| instrument.cost_currency());
        match Money::from_decimal(net_exposure, output_currency) {
            Ok(money) => Some(money),
            Err(e) => {
                log::error!("Cannot calculate net exposure: {e}");
                None
            }
        }
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
    pub fn is_net_flat(&self, instrument_id: &InstrumentId) -> bool {
        self.inner
            .borrow()
            .net_positions
            .get(instrument_id)
            .copied()
            .map_or_else(|| true, |net_position| net_position == Decimal::ZERO)
    }

    #[must_use]
    pub fn is_completely_net_flat(&self) -> bool {
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

            // Ordered so margin recalculation materializes any unreported balance currency
            // in a stable sequence; the account balance map preserves insertion order.
            let mut instruments = BTreeSet::new();

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
            let mut by_account: IndexMap<Option<AccountId>, Vec<&OrderAny>> = IndexMap::new();
            for order in orders_open {
                by_account
                    .entry(order.account_id())
                    .or_default()
                    .push(order);
            }

            for (account_id, orders) in by_account {
                let account = {
                    let cache = self.cache.borrow();
                    match resolve_account_for_instrument(
                        &cache,
                        &instrument.id(),
                        account_id.as_ref(),
                    ) {
                        Some(account) => account.cloned(),
                        None => {
                            log::error!(
                                "Cannot update initial (order) margin: no account registered for {}",
                                instrument.id().venue
                            );
                            initialized = false;
                            continue;
                        }
                    }
                };

                let result = self.inner.borrow_mut().accounts.update_orders(
                    &account,
                    instrument,
                    &orders,
                    self.clock.borrow().timestamp_ns(),
                );

                match result {
                    Some((updated_account, _)) => {
                        self.cache
                            .borrow_mut()
                            .update_account(&updated_account)
                            .unwrap();
                    }
                    None => initialized = false,
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

        // Ordered for the same reason as `initialize_orders`
        let mut instruments = BTreeSet::new();
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

            let position_refs: Vec<&Position> = positions_open.iter().collect();
            self.update_net_position(&instrument_id, &position_refs);

            if let Some(calculated_unrealized_pnl) =
                self.calculate_unrealized_pnl(&instrument_id, None, None, None)
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

            if let Some(calculated_realized_pnl) =
                self.calculate_realized_pnl(&instrument_id, None, None)
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

            let instrument = {
                let cache = self.cache.borrow();
                let Some(instrument) = cache.instrument(&instrument_id).cloned() else {
                    log::error!(
                        "Cannot update maintenance (position) margin: no instrument found for {instrument_id}"
                    );
                    initialized = false;
                    break;
                };
                instrument
            };

            let mut by_account: IndexMap<AccountId, Vec<&Position>> = IndexMap::new();
            for position in &positions_open {
                by_account
                    .entry(position.account_id)
                    .or_default()
                    .push(position);
            }

            for (account_id, positions) in by_account {
                let account = {
                    let cache = self.cache.borrow();
                    let Some(account) = cache.account(&account_id).map(|a| a.cloned()) else {
                        log::error!(
                            "Cannot update maintenance (position) margin: no account registered for {account_id}"
                        );
                        initialized = false;
                        continue;
                    };
                    account
                };
                let AccountAny::Margin(margin_account) = account else {
                    continue;
                };

                let result = self.inner.borrow_mut().accounts.update_positions(
                    &margin_account,
                    &instrument,
                    positions,
                    self.clock.borrow().timestamp_ns(),
                );

                match result {
                    Some((updated_account, _)) => {
                        self.cache
                            .borrow_mut()
                            .update_account(&AccountAny::Margin(updated_account))
                            .unwrap();
                    }
                    None => initialized = false,
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
                    account_id,
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
        update_account(&self.clock, &self.cache, &self.inner, self.config, event);
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

    /// Returns realized PnLs recorded during portfolio event processing.
    ///
    /// Each record is `(position_id, ts_event, realized_pnl)`.
    #[must_use]
    pub fn recorded_realized_pnls(&self) -> AHashMap<Currency, Vec<(PositionId, UnixNanos, f64)>> {
        self.inner.borrow().analyzer.recorded_realized_pnls.clone()
    }

    /// Computes an owned [`PortfolioStatistics`] snapshot from the portfolio's current cache state.
    ///
    /// Aggregates balances across every account, includes cached positions and their snapshots, and
    /// merges close-time PnLs recorded during processing. Recomputes on each call; callers on hot paths
    /// should invoke it sparingly.
    #[must_use]
    pub fn statistics(&self) -> PortfolioStatistics {
        let cache = self.cache.borrow();
        let accounts = cache.accounts_all_owned();
        let positions: Vec<Position> = cache
            .positions(None, None, None, None, None)
            .into_iter()
            .map(|p| p.cloned())
            .collect();
        let mut snapshots = Vec::new();
        for position in &positions {
            snapshots.extend(cache.position_snapshots(Some(&position.id), None));
        }

        let inner = self.inner.borrow();
        let recorded = inner.analyzer.recorded_realized_pnls.clone();
        let portfolio_snapshots = inner
            .portfolio_snapshots
            .values()
            .flat_map(|ring| ring.iter())
            .collect::<Vec<_>>();
        PortfolioAnalyzer::from_accounts_with_snapshots(
            &accounts,
            &positions,
            &snapshots,
            portfolio_snapshots,
            recorded,
        )
        .statistics()
    }

    /// Updates portfolio calculations based on a position event.
    ///
    /// Recalculates net positions, unrealized PnL, and margin requirements.
    pub fn update_position(&mut self, event: &PositionEvent) {
        update_position(&self.cache, &self.clock, &self.inner, self.config, event);
    }

    fn update_net_position(&self, instrument_id: &InstrumentId, positions: &[&Position]) {
        let mut net_position = Decimal::ZERO;

        for open_position in positions {
            log::debug!("open_position: {}", *open_position);
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

    fn aggregate_unrealized_pnl_by_account(
        &self,
        instrument_id: &InstrumentId,
        price: Option<Price>,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Result<Money, UnrealizedPnlError> {
        let pnls =
            self.unrealized_pnls_by_account(instrument_id, price, account_id, target_currency)?;
        let mut total: Option<Money> = None;
        for pnl in pnls {
            total = Some(match total {
                Some(total) => checked_add_money(total, pnl, "PnL aggregation")
                    .ok_or(UnrealizedPnlError::Invalid)?,
                None => pnl,
            });
        }

        total.ok_or(UnrealizedPnlError::Invalid)
    }

    fn unrealized_pnls_by_account(
        &self,
        instrument_id: &InstrumentId,
        price: Option<Price>,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Result<Vec<Money>, UnrealizedPnlError> {
        let mut account_ids = if let Some(account_id) = account_id {
            vec![*account_id]
        } else {
            let cache = self.cache.borrow();
            cache
                .positions_open(None, Some(instrument_id), None, None, None)
                .iter()
                .map(|position| position.account_id)
                .collect::<Vec<_>>()
        };

        account_ids.sort();
        account_ids.dedup();
        if account_ids.is_empty() {
            let cache = self.cache.borrow();
            let account_id = if let Some(account_id) = account_id {
                cache.account(account_id).map(|account| account.id())
            } else {
                cache
                    .account_for_venue(&instrument_id.venue)
                    .map(|account| account.id())
            }
            .ok_or(UnrealizedPnlError::Invalid)?;
            account_ids.push(account_id);
        }

        let mut pnls = Vec::with_capacity(account_ids.len());
        for account_id in account_ids {
            let pnl = self.calculate_unrealized_pnl_result(
                instrument_id,
                price,
                Some(&account_id),
                target_currency,
            )?;
            pnls.push(pnl);
        }

        Ok(pnls)
    }

    fn aggregate_realized_pnl_by_account(
        &self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Option<Money> {
        let pnls = self.realized_pnls_by_account(instrument_id, account_id, target_currency)?;
        let mut total: Option<Money> = None;
        for pnl in pnls {
            total = Some(match total {
                Some(total) => checked_add_money(total, pnl, "PnL aggregation")?,
                None => pnl,
            });
        }

        total
    }

    fn realized_pnls_by_account(
        &self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Option<Vec<Money>> {
        let mut account_ids = if let Some(account_id) = account_id {
            vec![*account_id]
        } else {
            let cache = self.cache.borrow();
            cache
                .positions(None, Some(instrument_id), None, None, None)
                .iter()
                .map(|position| position.account_id)
                .collect::<Vec<_>>()
        };

        if account_id.is_none() {
            let inner = self.inner.borrow();
            account_ids.extend(
                self.cache
                    .borrow()
                    .position_snapshot_ids(instrument_id)
                    .iter()
                    .filter_map(|position_id| inner.snapshot_account_ids.get(position_id))
                    .copied(),
            );
        }

        account_ids.sort();
        account_ids.dedup();
        if account_ids.is_empty() {
            let cache = self.cache.borrow();
            let account_id = if let Some(account_id) = account_id {
                cache.account(account_id).map(|account| account.id())
            } else {
                cache
                    .account_for_venue(&instrument_id.venue)
                    .map(|account| account.id())
            }?;
            account_ids.push(account_id);
        }

        let mut pnls = Vec::with_capacity(account_ids.len());
        for account_id in account_ids {
            let pnl =
                self.calculate_realized_pnl(instrument_id, Some(&account_id), target_currency)?;
            pnls.push(pnl);
        }

        Some(pnls)
    }

    fn calculate_unrealized_pnl(
        &self,
        instrument_id: &InstrumentId,
        price: Option<Price>,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Option<Money> {
        self.calculate_unrealized_pnl_result(instrument_id, price, account_id, target_currency)
            .ok()
    }

    fn calculate_unrealized_pnl_result(
        &self,
        instrument_id: &InstrumentId,
        price: Option<Price>,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Result<Money, UnrealizedPnlError> {
        let cache = self.cache.borrow();
        let account = resolve_account_for_instrument(&cache, instrument_id, account_id);
        let account = if let Some(account) = account {
            account
        } else {
            log::error!(
                "Cannot calculate unrealized PnL: no account for {} / {account_id:?}",
                instrument_id.venue,
            );
            return Err(UnrealizedPnlError::Invalid);
        };

        let instrument = if let Some(instrument) = cache.instrument(instrument_id) {
            instrument
        } else {
            log::error!("Cannot calculate unrealized PnL: no instrument for {instrument_id}");
            return Err(UnrealizedPnlError::Invalid);
        };

        let conversion_currency =
            target_currency.or_else(|| self.conversion_base_currency(&account));
        let allow_stale_xrate = target_currency.is_none();
        let mut output_currency = conversion_currency;

        let positions_open =
            cache.positions_open(None, Some(instrument_id), None, account_id, None);

        if positions_open.is_empty() {
            return Ok(Money::zero(
                output_currency.unwrap_or_else(|| instrument.cost_currency()),
            ));
        }

        let mut total_pnl = Decimal::ZERO;

        for position in positions_open {
            if position.instrument_id != *instrument_id {
                continue; // Nothing to calculate
            }

            if position.side == PositionSide::Flat {
                continue; // Nothing to calculate
            }

            let price = if let Some(price) = price.or_else(|| self.get_price(&position)) {
                price
            } else {
                log::debug!("Cannot calculate unrealized PnL: no prices for {instrument_id}");
                self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                return Err(UnrealizedPnlError::MissingInput);
            };

            let position_pnl = match position.try_unrealized_pnl(price) {
                Ok(pnl) => pnl,
                Err(e) => {
                    log::error!(
                        "Cannot calculate unrealized PnL for {}: {e}",
                        position.instrument_id
                    );
                    self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                    return Err(UnrealizedPnlError::Invalid);
                }
            };
            let source_currency = position_pnl.currency;
            let currency = conversion_currency.unwrap_or(source_currency);
            match output_currency {
                None => output_currency = Some(currency),
                Some(first) if first != currency => {
                    log::error!(
                        "Cannot calculate unrealized PnL: positions have different output \
                        currencies ({first} vs {currency})"
                    );
                    return Err(UnrealizedPnlError::Invalid);
                }
                _ => {}
            }

            let mut pnl = position_pnl.as_decimal();

            if let Some(conversion_currency) = conversion_currency {
                let xrate = if let Some(xrate) = self.calculate_xrate(
                    instrument.id().venue,
                    source_currency,
                    conversion_currency,
                    allow_stale_xrate,
                ) {
                    xrate
                } else {
                    log::warn!(
                        // TODO: Improve logging
                        "Cannot calculate unrealized PnL: insufficient data for \
                        {source_currency}/{conversion_currency}"
                    );
                    self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                    return Err(UnrealizedPnlError::MissingInput);
                };

                let Some(converted) = pnl.checked_mul(xrate) else {
                    log::error!("Cannot calculate unrealized PnL: currency conversion overflow");
                    self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                    return Err(UnrealizedPnlError::Invalid);
                };
                pnl = converted.round_dp(u32::from(currency.precision));
            }

            let Some(updated_total) = total_pnl.checked_add(pnl) else {
                log::error!("Cannot calculate unrealized PnL: total overflow");
                self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                return Err(UnrealizedPnlError::Invalid);
            };
            total_pnl = updated_total;
        }

        let currency = output_currency.unwrap_or_else(|| instrument.cost_currency());
        match Money::from_decimal(total_pnl, currency) {
            Ok(money) => Ok(money),
            Err(e) => {
                log::error!("Cannot calculate unrealized PnL: {e}");
                Err(UnrealizedPnlError::Invalid)
            }
        }
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

        // Detect purge/reset (count regression) or a settle that replaced frames without moving
        // the count, both of which invalidate the cached per-position aggregates
        for position_id in &snapshot_position_ids {
            let curr_count = self.cache.borrow().position_snapshot_count(position_id);
            let curr_revision = self.cache.borrow().position_snapshot_revision(position_id);
            let prev_count = self
                .inner
                .borrow()
                .snapshot_processed_counts
                .get(position_id)
                .copied()
                .unwrap_or(0);
            let prev_revision = self
                .inner
                .borrow()
                .snapshot_processed_revisions
                .get(position_id)
                .copied()
                .unwrap_or(0);

            if prev_count > curr_count || prev_revision != curr_revision {
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
                let snapshot_revision = self.cache.borrow().position_snapshot_revision(position_id);
                let snapshots = self
                    .cache
                    .borrow()
                    .position_snapshots(Some(position_id), None);

                let mut sum_pnl: Option<Money> = None;
                let mut last_pnl: Option<Money> = None;
                let mut snapshot_account_id: Option<AccountId> = None;
                let mut currency_mismatch = false;
                let mut aggregation_overflow = false;

                for snapshot in snapshots {
                    snapshot_account_id.get_or_insert(snapshot.account_id);
                    if let Some(realized_pnl) = snapshot.realized_pnl {
                        if let Some(sum) = sum_pnl {
                            if sum.currency == realized_pnl.currency {
                                if let Some(updated) = sum.checked_add(realized_pnl) {
                                    sum_pnl = Some(updated);
                                } else {
                                    aggregation_overflow = true;
                                }
                            } else {
                                currency_mismatch = true;
                            }
                        } else {
                            sum_pnl = Some(realized_pnl);
                        }
                        last_pnl = Some(realized_pnl);
                    }
                }

                let mut inner = self.inner.borrow_mut();

                if !aggregation_overflow && let Some(sum) = sum_pnl {
                    inner.snapshot_sum_per_position.insert(*position_id, sum);

                    if let Some(last) = last_pnl {
                        inner.snapshot_last_per_position.insert(*position_id, last);
                    }
                } else {
                    inner.snapshot_sum_per_position.remove(position_id);
                    inner.snapshot_last_per_position.remove(position_id);
                }

                if currency_mismatch {
                    inner.snapshot_currency_mismatches.insert(*position_id);
                } else {
                    inner.snapshot_currency_mismatches.remove(position_id);
                }

                if aggregation_overflow {
                    inner.snapshot_aggregation_overflows.insert(*position_id);
                } else {
                    inner.snapshot_aggregation_overflows.remove(position_id);
                }

                if let Some(account_id) = snapshot_account_id {
                    inner.snapshot_account_ids.insert(*position_id, account_id);
                } else {
                    inner.snapshot_account_ids.remove(position_id);
                }

                inner
                    .snapshot_processed_counts
                    .insert(*position_id, snapshot_count);
                inner
                    .snapshot_processed_revisions
                    .insert(*position_id, snapshot_revision);
            }
            self.inner
                .borrow_mut()
                .realized_pnls
                .shift_remove(instrument_id);
        } else {
            let mut cache_changed = false;
            // Incremental path: only process new snapshots
            for position_id in &snapshot_position_ids {
                // Compare raw frame counts first so untouched positions skip any
                // allocation/serde cost on repeated PnL refreshes.
                let curr_count = self.cache.borrow().position_snapshot_count(position_id);
                let curr_revision = self.cache.borrow().position_snapshot_revision(position_id);
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
                cache_changed = true;

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
                let mut currency_mismatch = self
                    .inner
                    .borrow()
                    .snapshot_currency_mismatches
                    .contains(position_id);
                let mut aggregation_overflow = self
                    .inner
                    .borrow()
                    .snapshot_aggregation_overflows
                    .contains(position_id);

                let new_snapshots = self
                    .cache
                    .borrow()
                    .position_snapshots_from(position_id, prev_count);

                for snapshot in new_snapshots {
                    snapshot_account_id.get_or_insert(snapshot.account_id);
                    if let Some(realized_pnl) = snapshot.realized_pnl {
                        if let Some(sum) = sum_pnl {
                            if sum.currency == realized_pnl.currency {
                                if let Some(updated) = sum.checked_add(realized_pnl) {
                                    sum_pnl = Some(updated);
                                } else {
                                    aggregation_overflow = true;
                                }
                            } else {
                                currency_mismatch = true;
                            }
                        } else {
                            sum_pnl = Some(realized_pnl);
                        }
                        last_pnl = Some(realized_pnl);
                    }
                }

                let mut inner = self.inner.borrow_mut();

                if !aggregation_overflow && let Some(sum) = sum_pnl {
                    inner.snapshot_sum_per_position.insert(*position_id, sum);

                    if let Some(last) = last_pnl {
                        inner.snapshot_last_per_position.insert(*position_id, last);
                    }
                }

                if currency_mismatch {
                    inner.snapshot_currency_mismatches.insert(*position_id);
                }

                if aggregation_overflow {
                    inner.snapshot_aggregation_overflows.insert(*position_id);
                    inner.snapshot_sum_per_position.remove(position_id);
                    inner.snapshot_last_per_position.remove(position_id);
                }

                if let Some(account_id) = snapshot_account_id
                    && !inner.snapshot_account_ids.contains_key(position_id)
                {
                    inner.snapshot_account_ids.insert(*position_id, account_id);
                }

                inner
                    .snapshot_processed_counts
                    .insert(*position_id, curr_count);
                inner
                    .snapshot_processed_revisions
                    .insert(*position_id, curr_revision);
            }

            if cache_changed {
                self.inner
                    .borrow_mut()
                    .realized_pnls
                    .shift_remove(instrument_id);
            }
        }
    }

    fn calculate_realized_pnl(
        &self,
        instrument_id: &InstrumentId,
        account_id: Option<&AccountId>,
        target_currency: Option<Currency>,
    ) -> Option<Money> {
        // Ensure snapshot PnLs are cached for this instrument
        self.ensure_snapshot_pnls_cached_for(instrument_id);

        let cache = self.cache.borrow();
        let account = resolve_account_for_instrument(&cache, instrument_id, account_id);
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

        if snapshot_position_ids.iter().any(|position_id| {
            self.inner
                .borrow()
                .snapshot_currency_mismatches
                .contains(position_id)
        }) {
            log::error!(
                "Cannot calculate realized PnL: snapshots for {instrument_id} contain mixed \
                cost currencies"
            );
            return None;
        }

        if snapshot_position_ids.iter().any(|position_id| {
            self.inner
                .borrow()
                .snapshot_aggregation_overflows
                .contains(position_id)
        }) {
            log::error!(
                "Cannot calculate realized PnL: snapshot aggregation for {instrument_id} exceeds Money bounds"
            );
            return None;
        }

        let conversion_currency =
            target_currency.or_else(|| self.conversion_base_currency(&account));
        let allow_stale_xrate = target_currency.is_none();
        let currency = conversion_currency.unwrap_or_else(|| {
            positions
                .first()
                .map(|position| position.settlement_currency)
                .or_else(|| {
                    let inner = self.inner.borrow();
                    snapshot_position_ids.iter().find_map(|position_id| {
                        inner
                            .snapshot_sum_per_position
                            .get(position_id)
                            .or_else(|| inner.snapshot_last_per_position.get(position_id))
                            .map(|pnl| pnl.currency)
                    })
                })
                .unwrap_or_else(|| instrument.cost_currency())
        });

        // Check if we need to use NETTING OMS logic
        let is_netting = positions
            .iter()
            .any(|p| cache.oms_type(&p.id) == Some(OmsType::Netting));

        let mut total_pnl = Decimal::ZERO;

        if is_netting && !snapshot_position_ids.is_empty() {
            // NETTING OMS: Apply 3-case rule for position cycles

            for position_id in &snapshot_position_ids {
                let position = positions.iter().find(|p| p.id == *position_id);
                let sum_pnl = self
                    .inner
                    .borrow()
                    .snapshot_sum_per_position
                    .get(position_id)
                    .copied();

                // A closed position whose final cycle was snapshotted carries that cycle both in
                // its last frame and in its own realized PnL, which the loop below adds; drop
                // the frame here so the cycle lands once.
                let sum_pnl = if let Some(sum_pnl) = sum_pnl {
                    let closed_position_pnl = position
                        .filter(|position| !position.is_open())
                        .and_then(|position| position.realized_pnl);
                    let last_pnl = self
                        .inner
                        .borrow()
                        .snapshot_last_per_position
                        .get(position_id)
                        .copied();

                    Some(match (closed_position_pnl, last_pnl) {
                        (Some(realized_pnl), Some(last_pnl)) if last_pnl == realized_pnl => {
                            match sum_pnl.checked_sub(last_pnl) {
                                Some(remaining) => remaining,
                                None => {
                                    log::error!(
                                        "Cannot calculate realized PnL: snapshot adjustment exceeds Money bounds"
                                    );
                                    return None;
                                }
                            }
                        }
                        _ => sum_pnl,
                    })
                } else {
                    None
                };

                if let Some(sum_pnl) = sum_pnl {
                    if !pnl_currency_is_compatible(conversion_currency, currency, sum_pnl.currency)
                    {
                        return None;
                    }

                    let mut pnl = sum_pnl.as_decimal();

                    if let Some(conversion_currency) = conversion_currency {
                        let xrate = if let Some(xrate) = self.calculate_xrate(
                            instrument.id().venue,
                            sum_pnl.currency,
                            conversion_currency,
                            allow_stale_xrate,
                        ) {
                            xrate
                        } else {
                            log::warn!(
                                "Cannot calculate realized PnL: insufficient exchange rate data for {}/{}, marking as pending calculation",
                                sum_pnl.currency,
                                conversion_currency
                            );
                            self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                            return None;
                        };

                        pnl = self.checked_convert_realized_pnl(
                            pnl,
                            xrate,
                            currency,
                            *instrument_id,
                        )?;
                    }

                    total_pnl = self.checked_add_realized_pnl(total_pnl, pnl, *instrument_id)?;
                }
            }

            // Add realized PnL from current active positions
            for position in positions {
                if position.instrument_id != *instrument_id {
                    continue;
                }

                if let Some(realized_pnl) = position.realized_pnl {
                    if !pnl_currency_is_compatible(
                        conversion_currency,
                        currency,
                        realized_pnl.currency,
                    ) {
                        return None;
                    }

                    let mut pnl = realized_pnl.as_decimal();

                    if let Some(conversion_currency) = conversion_currency {
                        let xrate = if let Some(xrate) = self.calculate_xrate(
                            instrument.id().venue,
                            realized_pnl.currency,
                            conversion_currency,
                            allow_stale_xrate,
                        ) {
                            xrate
                        } else {
                            log::warn!(
                                "Cannot calculate realized PnL: insufficient exchange rate data for {}/{}, marking as pending calculation",
                                realized_pnl.currency,
                                conversion_currency
                            );
                            self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                            return None;
                        };

                        pnl = self.checked_convert_realized_pnl(
                            pnl,
                            xrate,
                            currency,
                            *instrument_id,
                        )?;
                    }

                    total_pnl = self.checked_add_realized_pnl(total_pnl, pnl, *instrument_id)?;
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
                    if !pnl_currency_is_compatible(conversion_currency, currency, sum_pnl.currency)
                    {
                        return None;
                    }

                    let mut pnl = sum_pnl.as_decimal();

                    if let Some(conversion_currency) = conversion_currency {
                        let xrate = if let Some(xrate) = self.calculate_xrate(
                            instrument.id().venue,
                            sum_pnl.currency,
                            conversion_currency,
                            allow_stale_xrate,
                        ) {
                            xrate
                        } else {
                            log::warn!(
                                "Cannot calculate realized PnL: insufficient exchange rate data for {}/{}, marking as pending calculation",
                                sum_pnl.currency,
                                conversion_currency
                            );
                            self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                            return None;
                        };

                        pnl = self.checked_convert_realized_pnl(
                            pnl,
                            xrate,
                            currency,
                            *instrument_id,
                        )?;
                    }

                    total_pnl = self.checked_add_realized_pnl(total_pnl, pnl, *instrument_id)?;
                }
            }

            // Add realized PnL from current positions
            for position in positions {
                if position.instrument_id != *instrument_id {
                    continue;
                }

                if let Some(realized_pnl) = position.realized_pnl {
                    if !pnl_currency_is_compatible(
                        conversion_currency,
                        currency,
                        realized_pnl.currency,
                    ) {
                        return None;
                    }

                    let mut pnl = realized_pnl.as_decimal();

                    if let Some(conversion_currency) = conversion_currency {
                        let xrate = if let Some(xrate) = self.calculate_xrate(
                            instrument.id().venue,
                            realized_pnl.currency,
                            conversion_currency,
                            allow_stale_xrate,
                        ) {
                            xrate
                        } else {
                            log::warn!(
                                "Cannot calculate realized PnL: insufficient exchange rate data for {}/{}, marking as pending calculation",
                                realized_pnl.currency,
                                conversion_currency
                            );
                            self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                            return None;
                        };

                        pnl = self.checked_convert_realized_pnl(
                            pnl,
                            xrate,
                            currency,
                            *instrument_id,
                        )?;
                    }

                    total_pnl = self.checked_add_realized_pnl(total_pnl, pnl, *instrument_id)?;
                }
            }
        }

        match Money::from_decimal(total_pnl, currency) {
            Ok(money) => Some(money),
            Err(e) => {
                log::error!("Cannot calculate realized PnL: {e}");
                None
            }
        }
    }

    fn checked_convert_realized_pnl(
        &self,
        pnl: Decimal,
        xrate: Decimal,
        currency: Currency,
        instrument_id: InstrumentId,
    ) -> Option<Decimal> {
        let Some(converted) = pnl.checked_mul(xrate) else {
            log::error!("Cannot calculate realized PnL: currency conversion overflow");
            self.inner.borrow_mut().pending_calcs.insert(instrument_id);
            return None;
        };
        Some(converted.round_dp(u32::from(currency.precision)))
    }

    fn checked_add_realized_pnl(
        &self,
        total: Decimal,
        pnl: Decimal,
        instrument_id: InstrumentId,
    ) -> Option<Decimal> {
        let Some(total) = total.checked_add(pnl) else {
            log::error!("Cannot calculate realized PnL: total overflow");
            self.inner.borrow_mut().pending_calcs.insert(instrument_id);
            return None;
        };
        Some(total)
    }

    fn get_price(&self, position: &Position) -> Option<Price> {
        let cache = self.cache.borrow();
        let instrument_id = &position.instrument_id;

        let price_type = match position.side {
            PositionSide::Long => PriceType::Bid,
            PositionSide::Short => PriceType::Ask,
            _ => {
                log::error!(
                    "Cannot get price for invalid position side {}",
                    position.side
                );
                return None;
            }
        };
        let is_valid = |price: &Price| price.as_decimal() > Decimal::ZERO;
        let mark_price = if self.config.use_mark_prices {
            cache.mark_price(instrument_id).map(|mark| mark.value)
        } else {
            None
        };
        let current = mark_price
            .filter(is_valid)
            .or_else(|| cache.price(instrument_id, price_type).filter(is_valid))
            .or_else(|| cache.price(instrument_id, PriceType::Last).filter(is_valid))
            .or_else(|| {
                self.inner
                    .borrow()
                    .bar_close_prices
                    .get(instrument_id)
                    .filter(|price| is_valid(price))
                    .copied()
            });
        drop(cache);

        let key = (*instrument_id, position.side);
        let mut inner = self.inner.borrow_mut();
        if let Some(price) = current {
            inner.last_prices.insert(key, price);
            inner.stale_prices.remove(&key);
            Some(price)
        } else if let Some(price) = inner.last_prices.get(&key).copied() {
            inner.stale_prices.insert(key);
            Some(price)
        } else {
            inner.stale_prices.remove(&key);
            None
        }
    }

    fn calculate_xrate_to_base(
        &self,
        instrument: &InstrumentAny,
        account: &AccountAny,
        source_currency: Currency,
    ) -> Option<Decimal> {
        if !self.config.convert_to_account_base_currency {
            return Some(Decimal::ONE); // No conversion needed
        }

        let base_currency = match account.base_currency() {
            Some(base_currency) => base_currency,
            None => return Some(Decimal::ONE),
        };

        self.calculate_xrate(instrument.id().venue, source_currency, base_currency, true)
    }

    fn calculate_xrate(
        &self,
        venue: Venue,
        source_currency: Currency,
        target_currency: Currency,
        allow_stale: bool,
    ) -> Option<Decimal> {
        if source_currency == target_currency {
            return Some(Decimal::ONE);
        }

        let cache = self.cache.borrow();
        let mark_xrate = if self.config.use_mark_xrates {
            cache
                .get_mark_xrate(source_currency, target_currency)
                .and_then(|xrate| Decimal::try_from(xrate).ok())
        } else {
            None
        };
        let current = mark_xrate
            .filter(|xrate| *xrate > Decimal::ZERO)
            .or_else(|| {
                cache
                    .get_xrate(venue, source_currency, target_currency, PriceType::Mid)
                    .filter(|xrate| *xrate > Decimal::ZERO)
            });
        drop(cache);

        let key = (venue, source_currency, target_currency);
        let mut inner = self.inner.borrow_mut();
        if let Some(xrate) = current {
            inner.last_xrates.insert(key, xrate);
            inner.stale_xrates.remove(&key);
            Some(xrate)
        } else if allow_stale && let Some(xrate) = inner.last_xrates.get(&key).copied() {
            inner.stale_xrates.insert(key);
            Some(xrate)
        } else {
            inner.stale_xrates.remove(&key);
            None
        }
    }

    // Pairs with `calculate_xrate_to_base`, which yields a unit rate when conversion is disabled:
    // the output currency must ignore the account base currency for the same reason, otherwise a
    // native cost-currency amount is labelled with a currency it was never converted into.
    fn conversion_base_currency(&self, account: &AccountAny) -> Option<Currency> {
        if self.config.convert_to_account_base_currency {
            account.base_currency()
        } else {
            None
        }
    }
}

fn checked_add_money(lhs: Money, rhs: Money, context: &str) -> Option<Money> {
    if lhs.currency != rhs.currency {
        log::error!(
            "Cannot calculate {context}: currency mismatch {} vs {}",
            lhs.currency,
            rhs.currency
        );
        return None;
    }

    match lhs.checked_add(rhs) {
        Some(total) => Some(total),
        None => {
            log::error!("Cannot calculate {context}: total exceeds Money bounds");
            None
        }
    }
}

fn checked_add_money_map(
    totals: &mut IndexMap<Currency, Money>,
    money: Money,
    context: &str,
) -> Option<()> {
    let currency = money.currency;
    match totals.get_mut(&currency) {
        Some(total) => *total = checked_add_money(*total, money, context)?,
        None => {
            totals.insert(currency, money);
        }
    }
    Some(())
}

fn pnl_currency_is_compatible(
    base_currency: Option<Currency>,
    output_currency: Currency,
    source_currency: Currency,
) -> bool {
    if base_currency.is_none() && source_currency != output_currency {
        log::error!(
            "Cannot calculate realized PnL: records have different cost currencies \
            ({output_currency} vs {source_currency})"
        );
        false
    } else {
        true
    }
}

fn decimal_map_to_money(map: IndexMap<Currency, Decimal>) -> IndexMap<Currency, Money> {
    map.into_iter()
        .filter_map(
            |(currency, amount)| match Money::from_decimal(amount, currency) {
                Ok(money) => Some((currency, money)),
                Err(e) => {
                    log::error!("Cannot convert {currency} amount to Money: {e}");
                    None
                }
            },
        )
        .collect()
}

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

/// Account for an instrument. For broker-routed instruments the account lives
/// under the broker venue (e.g. `IB`) while the instrument carries the exchange
/// MIC (e.g. `IBIS`); on venue miss, fall back to the position-owning account.
fn resolve_account_for_instrument<'a>(
    cache: &'a Cache,
    instrument_id: &InstrumentId,
    account_id: Option<&AccountId>,
) -> Option<AccountRef<'a>> {
    match account_id {
        Some(id) => cache.account(id),
        None => cache.account_for_venue(&instrument_id.venue).or_else(|| {
            cache
                .positions(None, Some(instrument_id), None, None, None)
                .into_iter()
                .next()
                .and_then(|p| cache.account(&p.account_id))
        }),
    }
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

    let instrument = match cache.borrow().instrument(instrument_id) {
        Some(instrument) => instrument.clone(),
        None => {
            log::error!("Cannot update tick: no instrument found for {instrument_id}");
            return;
        }
    };

    let mut by_account: IndexMap<AccountId, (Vec<OrderAny>, Vec<Position>)> = IndexMap::new();
    {
        let cache_ref = cache.borrow();
        for order in cache_ref
            .orders_open(None, Some(instrument_id), None, None, None)
            .iter()
            .map(|o| (*o).clone())
        {
            if let Some(account_id) = order.account_id() {
                by_account.entry(account_id).or_default().0.push(order);
            }
        }

        for position in cache_ref
            .positions_open(None, Some(instrument_id), None, None, None)
            .iter()
            .map(|p| (*p).clone())
        {
            by_account
                .entry(position.account_id)
                .or_default()
                .1
                .push(position);
        }

        if by_account.is_empty()
            && let Some(account) =
                resolve_account_for_instrument(&cache_ref, instrument_id, None).map(|a| a.cloned())
        {
            by_account.entry(account.id()).or_default();
        }
    }

    if by_account.is_empty() {
        log::error!(
            "Cannot update tick: no account registered for {}",
            instrument_id.venue
        );
        return;
    }

    let ts_event = clock.borrow().timestamp_ns();
    let mut ok = true;
    let mut any_margin = false;

    for (account_id, (orders, positions)) in by_account {
        let Some(mut account) = cache.borrow().account(&account_id).map(|a| a.cloned()) else {
            log::error!("Cannot update tick: no account registered for {account_id}");
            ok = false;
            continue;
        };

        let orders_refs: Vec<&OrderAny> = orders.iter().collect();
        let mut account_updated = inner
            .borrow()
            .accounts
            .update_orders_in_place(&mut account, &instrument, &orders_refs, ts_event)
            .is_some();

        if !account_updated {
            ok = false;
        }

        if let AccountAny::Margin(margin_account) = &mut account {
            any_margin = true;

            if inner
                .borrow()
                .accounts
                .update_positions_in_place(
                    margin_account,
                    &instrument,
                    positions.iter().collect(),
                    ts_event,
                )
                .is_some()
            {
                account_updated = true;
            } else {
                ok = false;
            }
        }

        if account_updated {
            cache.borrow_mut().update_account(&account).unwrap();
        }
    }

    let portfolio_clone = Portfolio {
        clock: Rc::clone(clock),
        cache: Rc::clone(cache),
        inner: Rc::clone(inner),
        config,
    };

    let result_unrealized_pnl: Option<Money> =
        portfolio_clone.calculate_unrealized_pnl(instrument_id, None, None, None);

    if ok && (!any_margin || result_unrealized_pnl.is_some()) {
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

        let account = match cache_ref.try_account(&account_id) {
            Ok(account) => account,
            Err(e) => {
                log::error!("Cannot update order: {e}");
                return;
            }
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
            | OrderEventAny::Expired(_)
            | OrderEventAny::Rejected(_)
            | OrderEventAny::Updated(_)
            | OrderEventAny::Filled(_)
            | OrderEventAny::FillVoided(_) => {}
            _ => {
                return;
            }
        }

        let order = cache_ref.order(&event.client_order_id());
        if order.is_none() && !matches!(event, OrderEventAny::Filled(_)) {
            log::error!(
                "Cannot update order: {} not found in the cache",
                event.client_order_id()
            );
            return; // No Order Found
        }

        if matches!(event, OrderEventAny::Rejected(_))
            && order.is_some_and(|order| order.order_type() != OrderType::StopLimit)
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
            .orders_open(
                None,
                Some(&event.instrument_id()),
                None,
                Some(&account_id),
                None,
            )
            .iter()
            .map(|o| (*o).clone())
            .collect();

        (instrument, orders_open)
    };

    // No cache borrow held: AccountsManager borrows cache internally for xrate lookups.
    let mut working_account = match cache.borrow_mut().take_account(&account_id) {
        Some(account) => account,
        None => {
            log::error!(
                "Cannot update order: {}",
                AccountLookupError::not_found(account_id)
            );
            return;
        }
    };

    if let OrderEventAny::Filled(order_filled) = event {
        if !instrument.is_spread() {
            let (post_balance, _state) =
                inner
                    .borrow()
                    .accounts
                    .update_balances(working_account, &instrument, order_filled);
            working_account = post_balance;
        }

        cache.borrow_mut().cache_account_owned(working_account);

        let portfolio_clone = Portfolio {
            clock: Rc::clone(clock),
            cache: Rc::clone(cache),
            inner: Rc::clone(inner),
            config,
        };

        match portfolio_clone.calculate_unrealized_pnl(
            &order_filled.instrument_id,
            None,
            Some(&account_id),
            None,
        ) {
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
    } else if let OrderEventAny::FillVoided(fill_voided) = event {
        cache.borrow_mut().cache_account_owned(working_account);

        let portfolio = Portfolio {
            clock: Rc::clone(clock),
            cache: Rc::clone(cache),
            inner: Rc::clone(inner),
            config,
        };
        {
            let cache_ref = cache.borrow();
            let positions =
                cache_ref.positions_open(None, Some(&fill_voided.instrument_id), None, None, None);
            let positions: Vec<&Position> = positions.iter().map(|position| &**position).collect();
            portfolio.update_net_position(&fill_voided.instrument_id, &positions);
        }

        if let Some(pnl) =
            portfolio.calculate_unrealized_pnl(&fill_voided.instrument_id, None, None, None)
        {
            inner
                .borrow_mut()
                .unrealized_pnls
                .insert(fill_voided.instrument_id, pnl);
        } else {
            inner
                .borrow_mut()
                .unrealized_pnls
                .shift_remove(&fill_voided.instrument_id);
        }

        if let Some(pnl) = portfolio.calculate_realized_pnl(&fill_voided.instrument_id, None, None)
        {
            inner
                .borrow_mut()
                .realized_pnls
                .insert(fill_voided.instrument_id, pnl);
        } else {
            inner
                .borrow_mut()
                .realized_pnls
                .shift_remove(&fill_voided.instrument_id);
        }

        log::debug!("Updated {event}");
        return;
    }

    let orders_open_refs: Vec<&OrderAny> = orders_open.iter().collect();
    let account_state = inner.borrow().accounts.update_orders_in_place(
        &mut working_account,
        &instrument,
        &orders_open_refs,
        clock.borrow().timestamp_ns(),
    );

    let is_fill = matches!(event, OrderEventAny::Filled(_));
    let suppress_margin_fill_account_state =
        is_fill && matches!(working_account, AccountAny::Margin(_));
    let publish_account_state = !matches!(source, OrderUpdateSource::Endpoint) && !is_fill;

    if !publish_account_state
        && !suppress_margin_fill_account_state
        && let Some(account_state) = account_state.as_ref()
        && let Err(e) = working_account.apply(account_state.clone())
    {
        log::error!("Cannot apply generated account state: {e}");
    }

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
        if publish_account_state {
            msgbus::publish_account_state(
                format!("events.account.{updated_account_id}").into(),
                &account_state,
            );
        }
    } else {
        log::debug!("Added pending calculation for {}", instrument.id());
        inner.borrow_mut().pending_calcs.insert(instrument.id());
    }

    log::debug!("Updated {event}");
}

fn on_order_event(
    cache: &Rc<RefCell<Cache>>,
    inner: &Rc<RefCell<PortfolioState>>,
    event: &OrderEventAny,
) {
    if let OrderEventAny::Filled(order_filled) = event {
        inner
            .borrow_mut()
            .pre_position_fill_events
            .remove(&order_filled.event_id);
        return;
    }

    let account_id = match event.account_id() {
        Some(account_id) => account_id,
        None => return,
    };

    match event {
        OrderEventAny::Accepted(_)
        | OrderEventAny::Canceled(_)
        | OrderEventAny::Expired(_)
        | OrderEventAny::Rejected(_)
        | OrderEventAny::Updated(_) => {}
        _ => return,
    }

    let account_state = cache
        .borrow()
        .account(&account_id)
        .and_then(|account| account.last_event());

    if let Some(account_state) = account_state {
        msgbus::publish_account_state(
            format!("events.account.{account_id}").into(),
            &account_state,
        );
    }
}

/// Result of peeking at the cached account inside [`update_position`]: only a margin account
/// with `calculate_account_state` set needs the owned recompute path.
enum AccountPeek {
    MarginRecompute,
    LastEvent(Option<AccountState>),
    Missing,
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

    update_snapshot_timer_state(cache, clock, inner, config, account_id);

    let portfolio_clone = Portfolio {
        clock: Rc::clone(clock),
        cache: Rc::clone(cache),
        inner: Rc::clone(inner),
        config,
    };

    {
        let cache_ref = cache.borrow();
        let refs = cache_ref.positions_open(None, Some(&instrument_id), None, None, None);
        log::debug!("position fresh from cache -> {refs:?}");
        let positions: Vec<&Position> = refs.iter().map(|r| &**r).collect();
        portfolio_clone.update_net_position(&instrument_id, &positions);
    }

    record_closed_position_pnl(cache, inner, config, event);

    if let Some(calculated_unrealized_pnl) =
        portfolio_clone.calculate_unrealized_pnl(&instrument_id, None, None, None)
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
        portfolio_clone.calculate_realized_pnl(&instrument_id, None, None)
    {
        inner
            .borrow_mut()
            .realized_pnls
            .insert(event.instrument_id(), calculated_realized_pnl);
    } else {
        inner
            .borrow_mut()
            .realized_pnls
            .shift_remove(&event.instrument_id());
        log::warn!(
            "Failed to calculate realized PnL for {}, marking as pending",
            event.instrument_id()
        );
        inner
            .borrow_mut()
            .pending_calcs
            .insert(event.instrument_id());
    }

    // Peek under a borrow: the account event log grows per fill, so a clone here was O(n)
    let peek = {
        let cache_ref = cache.borrow();
        match cache_ref.account(&account_id) {
            Some(account) => match &*account {
                AccountAny::Margin(margin_account) if margin_account.calculate_account_state => {
                    AccountPeek::MarginRecompute
                }
                account => AccountPeek::LastEvent(account.last_event()),
            },
            None => AccountPeek::Missing,
        }
    };
    let account_state_to_publish = match peek {
        AccountPeek::MarginRecompute => {
            recompute_margin_account(cache, clock, inner, account_id, &instrument_id)
        }
        AccountPeek::LastEvent(last_event) => last_event,
        AccountPeek::Missing => {
            log::error!(
                "Cannot update position: no account registered for {}",
                event.account_id()
            );
            None
        }
    };

    if let Some(account_state) = account_state_to_publish {
        msgbus::publish_account_state(
            format!("events.account.{account_id}").into(),
            &account_state,
        );
    }
}

/// Recalculates the margin account for `instrument_id` from the currently open positions.
///
/// Moves the account out of the cache for the recompute instead of cloning it, then moves it
/// back without a database write when the recompute produces no new state.
fn recompute_margin_account(
    cache: &Rc<RefCell<Cache>>,
    clock: &Rc<RefCell<dyn Clock>>,
    inner: &Rc<RefCell<PortfolioState>>,
    account_id: AccountId,
    instrument_id: &InstrumentId,
) -> Option<AccountState> {
    let instrument = { cache.borrow().instrument(instrument_id).cloned() };
    let Some(instrument) = instrument else {
        log::error!("Cannot update position: no instrument found for {instrument_id}");
        let cache_ref = cache.borrow();
        return cache_ref
            .account(&account_id)
            .and_then(|account| account.last_event());
    };

    // Bind the taken account so the mutable cache borrow drops before the recompute
    let taken_account = cache.borrow_mut().take_account(&account_id);
    let mut account = taken_account?;
    let AccountAny::Margin(margin_account) = &mut account else {
        // The caller peeked a margin account, so this only restores an unexpected account type
        return restore_cached_account(cache, account);
    };

    let recomputed = {
        let cache_ref = cache.borrow();
        let refs =
            cache_ref.positions_open(None, Some(instrument_id), None, Some(&account_id), None);
        let positions: Vec<&Position> = refs.iter().map(|r| &**r).collect();
        inner.borrow_mut().accounts.update_positions_in_place(
            margin_account,
            &instrument,
            positions,
            clock.borrow().timestamp_ns(),
        )
    };

    match recomputed {
        Some(account_state) => {
            cache.borrow_mut().update_account_owned(account).unwrap();
            Some(account_state)
        }
        None => restore_cached_account(cache, account),
    }
}

/// Returns the `account` to the cache without a database write and reports its last state event.
fn restore_cached_account(cache: &Rc<RefCell<Cache>>, account: AccountAny) -> Option<AccountState> {
    let last_event = account.last_event();
    cache.borrow_mut().cache_account_owned(account);

    last_event
}

fn record_closed_position_pnl(
    cache: &Rc<RefCell<Cache>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    event: &PositionEvent,
) {
    let position_id = match event {
        PositionEvent::PositionOpened(event) => event.position_id,
        PositionEvent::PositionChanged(event) => event.position_id,
        PositionEvent::PositionClosed(event) => event.position_id,
        PositionEvent::PositionAdjusted(event) => event.position_id,
    };

    let cache_ref = cache.borrow();
    let Some(position) = cache_ref.position(&position_id) else {
        return;
    };

    if !position.is_closed() {
        return;
    }

    let Some(realized_pnl) = position.realized_pnl else {
        return;
    };

    let mut inner_ref = inner.borrow_mut();

    if !inner_ref
        .recorded_closed_position_cycles
        .insert((position.id, position.ts_opened))
    {
        return;
    }

    let converted_pnl =
        converted_realized_pnl(&cache_ref, config, event, position_id, realized_pnl);

    let ts_event = position.ts_last;
    inner_ref
        .analyzer
        .record_trade(&position.id, ts_event, &realized_pnl);

    if let Some(converted_pnl) = converted_pnl {
        inner_ref
            .analyzer
            .record_trade(&position.id, ts_event, &converted_pnl);
    }
}

fn converted_realized_pnl(
    cache_ref: &Cache,
    config: PortfolioConfig,
    event: &PositionEvent,
    position_id: PositionId,
    realized_pnl: Money,
) -> Option<Money> {
    let account = cache_ref.account(&event.account_id())?;
    let base_currency = account.base_currency()?;

    if realized_pnl.currency == base_currency {
        return None;
    }

    let xrate = if config.use_mark_xrates {
        cache_ref
            .get_mark_xrate(realized_pnl.currency, base_currency)
            .and_then(|xrate| Decimal::try_from(xrate).ok())
    } else {
        cache_ref.get_xrate(
            event.instrument_id().venue,
            realized_pnl.currency,
            base_currency,
            PriceType::Mid,
        )
    };

    let Some(xrate) = xrate else {
        log::warn!(
            "Cannot record account-currency realized PnL for {position_id}: conversion failed from {} to {base_currency}",
            realized_pnl.currency
        );
        return None;
    };

    let amount = (realized_pnl.as_decimal() * xrate).round_dp(u32::from(base_currency.precision));
    match Money::from_decimal(amount, base_currency) {
        Ok(amount) => Some(amount),
        Err(e) => {
            log::warn!("Cannot record account-currency realized PnL for {position_id}: {e}");
            None
        }
    }
}

fn update_account(
    clock: &Rc<RefCell<dyn Clock>>,
    cache: &Rc<RefCell<Cache>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    event: &AccountState,
) {
    let already_applied = {
        cache
            .borrow()
            .account(&event.account_id)
            .and_then(|account| account.last_event())
            .is_some_and(|last_event| last_event.event_id == event.event_id)
    };

    if !already_applied && let Err(e) = cache.borrow_mut().update_account_state(event) {
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

        // Saturating: an out-of-order event carrying an earlier `ts_init` keeps the throttle
        // engaged rather than wrapping into an interval that always logs.
        if last_ts == 0
            || current_ts.saturating_sub(last_ts) >= inner_ref.min_account_state_logging_interval_ns
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
    drop(inner_ref);

    register_equity_curve_account(clock, cache, inner, config, event.account_id);
}

fn equity_curve_timer_name(account_id: AccountId) -> String {
    format!("portfolio_equity_curve.{account_id}")
}

fn register_equity_curve_account(
    clock: &Rc<RefCell<dyn Clock>>,
    cache: &Rc<RefCell<Cache>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    account_id: AccountId,
) {
    if !config.equity_curve {
        return;
    }

    let is_new = {
        let mut inner = inner.borrow_mut();
        if inner.equity_curve_finalized {
            return;
        }
        inner.equity_curve_accounts.insert(account_id)
    };

    if !is_new {
        return;
    }

    arm_equity_curve_timer(clock, cache, inner, config, account_id);
    let ts_event = clock.borrow().timestamp_ns();
    emit_snapshot(cache, clock, inner, config, account_id, ts_event);
}

fn arm_equity_curve_timer(
    clock: &Rc<RefCell<dyn Clock>>,
    cache: &Rc<RefCell<Cache>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    account_id: AccountId,
) {
    let ts_now = clock.borrow().timestamp_ns().as_u64();
    let Some(next_day) = (ts_now / NANOSECONDS_IN_DAY)
        .checked_add(1)
        .and_then(|day| day.checked_mul(NANOSECONDS_IN_DAY))
    else {
        log::error!("Failed to calculate next equity curve sample for {account_id}");
        return;
    };
    let timer_name = equity_curve_timer_name(account_id);
    let cache_weak = Rc::downgrade(cache);
    let clock_weak = Rc::downgrade(clock);
    let inner_weak = Rc::downgrade(inner);

    let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(move |event| {
        let Some(cache) = cache_weak.upgrade() else {
            return;
        };
        let Some(clock) = clock_weak.upgrade() else {
            return;
        };
        let Some(inner) = inner_weak.upgrade() else {
            return;
        };
        emit_snapshot(&cache, &clock, &inner, config, account_id, event.ts_event);
    });

    if let Err(e) = clock.borrow_mut().set_timer_ns(
        &timer_name,
        NANOSECONDS_IN_DAY,
        Some(UnixNanos::from(next_day)),
        None,
        Some(TimeEventCallback::from(callback)),
        Some(false),
        Some(true),
    ) {
        log::error!("Failed to arm portfolio equity curve timer for {account_id}: {e}");
    }
}

fn snapshot_timer_name(account_id: AccountId) -> String {
    format!("portfolio_snapshot.{account_id}")
}

fn update_snapshot_timer_state(
    cache: &Rc<RefCell<Cache>>,
    clock: &Rc<RefCell<dyn Clock>>,
    inner: &Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
    account_id: AccountId,
) {
    if config.snapshot_interval_ms.is_none() {
        return;
    }

    let current_count = cache
        .borrow()
        .positions_open(None, None, None, Some(&account_id), None)
        .len();

    let prev_count = inner
        .borrow()
        .account_open_positions
        .get(&account_id)
        .copied()
        .unwrap_or(0);

    inner
        .borrow_mut()
        .account_open_positions
        .insert(account_id, current_count);

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
    account_id: AccountId,
) {
    let interval_ms = match config.snapshot_interval_ms {
        Some(ms) if ms > 0 => ms,
        _ => return,
    };
    let interval_ns = interval_ms * NANOSECONDS_IN_MILLISECOND;
    let timer_name = snapshot_timer_name(account_id);

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
        emit_snapshot(&cache, &clock, &inner, config, account_id, event.ts_event);
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
    account_id: AccountId,
    ts_event: nautilus_core::UnixNanos,
) {
    let mut portfolio = Portfolio {
        cache: Rc::clone(cache),
        clock: Rc::clone(clock),
        inner: Rc::clone(inner),
        config,
    };

    let mut snapshot = match portfolio.build_snapshot(&account_id) {
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
        account_id,
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
            None,
            false,
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
