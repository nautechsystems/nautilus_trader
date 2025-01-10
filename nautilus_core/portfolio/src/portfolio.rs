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
    any::Any,
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

use nautilus_analysis::{
    analyzer::PortfolioAnalyzer,
    statistics::{
        expectancy::Expectancy, long_ratio::LongRatio, loser_max::MaxLoser, loser_min::MinLoser,
        profit_factor::ProfitFactor, returns_avg::ReturnsAverage,
        returns_avg_loss::ReturnsAverageLoss, returns_avg_win::ReturnsAverageWin,
        returns_volatility::ReturnsVolatility, risk_return_ratio::RiskReturnRatio,
        sharpe_ratio::SharpeRatio, sortino_ratio::SortinoRatio, win_rate::WinRate,
        winner_avg::AvgWinner, winner_max::MaxWinner, winner_min::MinWinner,
    },
};
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    messages::data::DataResponse,
    msgbus::{
        handler::{MessageHandler, ShareableMessageHandler},
        MessageBus,
    },
};
use nautilus_model::{
    accounts::AccountAny,
    data::{Data, QuoteTick},
    enums::{OrderSide, OrderType, PositionSide, PriceType},
    events::{position::PositionEvent, AccountState, OrderEventAny},
    identifiers::{InstrumentId, Venue},
    instruments::InstrumentAny,
    orders::OrderAny,
    position::Position,
    types::{Currency, Money, Price},
};
use rust_decimal::{
    prelude::{FromPrimitive, ToPrimitive},
    Decimal,
};
use ustr::Ustr;
use uuid::Uuid;

use crate::manager::AccountsManager;

struct UpdateQuoteTickHandler {
    id: Ustr,
    callback: Box<dyn Fn(&QuoteTick)>,
}

impl MessageHandler for UpdateQuoteTickHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&QuoteTick>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct UpdateOrderHandler {
    id: Ustr,
    callback: Box<dyn Fn(&OrderEventAny)>,
}

impl MessageHandler for UpdateOrderHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&OrderEventAny>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct UpdatePositionHandler {
    id: Ustr,
    callback: Box<dyn Fn(&PositionEvent)>,
}

impl MessageHandler for UpdatePositionHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&PositionEvent>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct UpdateAccountHandler {
    id: Ustr,
    callback: Box<dyn Fn(&AccountState)>,
}

impl MessageHandler for UpdateAccountHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&AccountState>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct PortfolioState {
    accounts: AccountsManager,
    analyzer: PortfolioAnalyzer,
    unrealized_pnls: HashMap<InstrumentId, Money>,
    realized_pnls: HashMap<InstrumentId, Money>,
    net_positions: HashMap<InstrumentId, Decimal>,
    pending_calcs: HashSet<InstrumentId>,
    initialized: bool,
}

impl PortfolioState {
    fn new(clock: Rc<RefCell<dyn Clock>>, cache: Rc<RefCell<Cache>>) -> Self {
        let mut analyzer = PortfolioAnalyzer::new();
        analyzer.register_statistic(Arc::new(MaxWinner {}));
        analyzer.register_statistic(Arc::new(AvgWinner {}));
        analyzer.register_statistic(Arc::new(MinWinner {}));
        analyzer.register_statistic(Arc::new(MinLoser {}));
        analyzer.register_statistic(Arc::new(MaxLoser {}));
        analyzer.register_statistic(Arc::new(Expectancy {}));
        analyzer.register_statistic(Arc::new(WinRate {}));
        analyzer.register_statistic(Arc::new(ReturnsVolatility::new(None)));
        analyzer.register_statistic(Arc::new(ReturnsAverage {}));
        analyzer.register_statistic(Arc::new(ReturnsAverageLoss {}));
        analyzer.register_statistic(Arc::new(ReturnsAverageWin {}));
        analyzer.register_statistic(Arc::new(SharpeRatio::new(None)));
        analyzer.register_statistic(Arc::new(SortinoRatio::new(None)));
        analyzer.register_statistic(Arc::new(ProfitFactor {}));
        analyzer.register_statistic(Arc::new(RiskReturnRatio {}));
        analyzer.register_statistic(Arc::new(LongRatio::new(None)));

        Self {
            accounts: AccountsManager::new(clock, cache),
            analyzer,
            unrealized_pnls: HashMap::new(),
            realized_pnls: HashMap::new(),
            net_positions: HashMap::new(),
            pending_calcs: HashSet::new(),
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
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    inner: Rc<RefCell<PortfolioState>>,
}

impl Portfolio {
    pub fn new(
        msgbus: Rc<RefCell<MessageBus>>,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> Self {
        let inner = Rc::new(RefCell::new(PortfolioState::new(
            clock.clone(),
            cache.clone(),
        )));

        Self::register_message_handlers(
            msgbus.clone(),
            cache.clone(),
            clock.clone(),
            inner.clone(),
        );

        Self {
            clock,
            cache,
            msgbus,
            inner,
        }
    }

    fn register_message_handlers(
        msgbus: Rc<RefCell<MessageBus>>,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
        inner: Rc<RefCell<PortfolioState>>,
    ) {
        let update_account_handler = {
            let cache = cache.clone();
            ShareableMessageHandler(Rc::new(UpdateAccountHandler {
                id: Ustr::from(&Uuid::new_v4().to_string()),
                callback: Box::new(move |event: &AccountState| {
                    update_account(cache.clone(), event);
                }),
            }))
        };

        let update_position_handler = {
            let cache = cache.clone();
            let msgbus = msgbus.clone();
            let clock = clock.clone();
            let inner = inner.clone();
            ShareableMessageHandler(Rc::new(UpdatePositionHandler {
                id: Ustr::from(&Uuid::new_v4().to_string()),
                callback: Box::new(move |event: &PositionEvent| {
                    update_position(
                        cache.clone(),
                        msgbus.clone(),
                        clock.clone(),
                        inner.clone(),
                        event,
                    );
                }),
            }))
        };

        let update_quote_handler = {
            let cache = cache.clone();
            let msgbus = msgbus.clone();
            let clock = clock.clone();
            let inner = inner.clone();
            ShareableMessageHandler(Rc::new(UpdateQuoteTickHandler {
                id: Ustr::from(&Uuid::new_v4().to_string()),
                callback: Box::new(move |quote: &QuoteTick| {
                    update_quote_tick(
                        cache.clone(),
                        msgbus.clone(),
                        clock.clone(),
                        inner.clone(),
                        quote,
                    );
                }),
            }))
        };

        let update_order_handler = {
            let cache = cache;
            let msgbus = msgbus.clone();
            let clock = clock.clone();
            let inner = inner;
            ShareableMessageHandler(Rc::new(UpdateOrderHandler {
                id: Ustr::from(&Uuid::new_v4().to_string()),
                callback: Box::new(move |event: &OrderEventAny| {
                    update_order(
                        cache.clone(),
                        msgbus.clone(),
                        clock.clone(),
                        inner.clone(),
                        event,
                    );
                }),
            }))
        };

        let mut borrowed_msgbus = msgbus.borrow_mut();
        borrowed_msgbus.register("Portfolio.update_account", update_account_handler.clone());

        borrowed_msgbus.subscribe("data.quotes.*", update_quote_handler, Some(10));
        borrowed_msgbus.subscribe("events.order.*", update_order_handler, Some(10));
        borrowed_msgbus.subscribe("events.position.*", update_position_handler, Some(10));
        borrowed_msgbus.subscribe("events.account.*", update_account_handler, Some(10));
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
                log::error!(
                    "Cannot get balances locked: no account generated for {}",
                    venue
                );
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
                    "Cannot get initial (order) margins: no account registered for {}",
                    venue
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
                    "Cannot get maintenance (position) margins: no account registered for {}",
                    venue
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
            let borrowed_cache = self.cache.borrow();
            let positions = borrowed_cache.positions(Some(venue), None, None, None);

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
            let borrowed_cache = self.cache.borrow();
            let positions = borrowed_cache.positions(Some(venue), None, None, None);

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
        let borrowed_cache = self.cache.borrow();
        let account = if let Some(account) = borrowed_cache.account_for_venue(venue) {
            account
        } else {
            log::error!(
                "Cannot calculate net exposures: no account registered for {}",
                venue
            );
            return None; // Cannot calculate
        };

        let positions_open = borrowed_cache.positions_open(Some(venue), None, None, None);
        if positions_open.is_empty() {
            return Some(HashMap::new()); // Nothing to calculate
        }

        let mut net_exposures: HashMap<Currency, f64> = HashMap::new();

        for position in positions_open {
            let instrument =
                if let Some(instrument) = borrowed_cache.instrument(&position.instrument_id) {
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

            let last = self.get_last_price(position)?;
            let xrate = self.calculate_xrate_to_base(instrument, account, position.entry);
            if xrate == 0.0 {
                log::error!(
                    "Cannot calculate net exposures: insufficient data for {}/{:?}",
                    instrument.settlement_currency(),
                    account.base_currency()
                );
                return None; // Cannot calculate
            }

            let settlement_currency = account
                .base_currency()
                .unwrap_or_else(|| instrument.settlement_currency());

            let net_exposure = instrument
                .calculate_notional_value(position.quantity, last, None)
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
        let borrowed_cache = self.cache.borrow();
        let account = if let Some(account) = borrowed_cache.account_for_venue(&instrument_id.venue)
        {
            account
        } else {
            log::error!(
                "Cannot calculate net exposure: no account registered for {}",
                instrument_id.venue
            );
            return None;
        };

        let instrument = if let Some(instrument) = borrowed_cache.instrument(instrument_id) {
            instrument
        } else {
            log::error!(
                "Cannot calculate net exposure: no instrument for {}",
                instrument_id
            );
            return None;
        };

        let positions_open = borrowed_cache.positions_open(
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
            let last = self.get_last_price(position)?;
            let xrate = self.calculate_xrate_to_base(instrument, account, position.entry);
            if xrate == 0.0 {
                log::error!(
                    "Cannot calculate net exposure: insufficient data for {}/{:?}",
                    instrument.settlement_currency(),
                    account.base_currency()
                );
                return None;
            }

            let notional_value = instrument
                .calculate_notional_value(position.quantity, last, None)
                .as_f64();

            net_exposure += notional_value * xrate;
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
            let borrowed_cache = self.cache.borrow();
            let all_orders_open = borrowed_cache.orders_open(None, None, None, None);

            let mut instruments_with_orders = Vec::new();
            let mut instruments = HashSet::new();

            for order in &all_orders_open {
                instruments.insert(order.instrument_id());
            }

            for instrument_id in instruments {
                if let Some(instrument) = borrowed_cache.instrument(&instrument_id) {
                    let orders = borrowed_cache
                        .orders_open(None, Some(&instrument_id), None, None)
                        .into_iter()
                        .cloned()
                        .collect::<Vec<OrderAny>>();
                    instruments_with_orders.push((instrument.clone(), orders));
                } else {
                    log::error!(
                        "Cannot update initial (order) margin: no instrument found for {}",
                        instrument_id
                    );
                    initialized = false;
                    break;
                }
            }
            instruments_with_orders
        };

        for (instrument, orders_open) in &orders_and_instruments {
            let mut borrowed_cache = self.cache.borrow_mut();
            let account =
                if let Some(account) = borrowed_cache.account_for_venue(&instrument.id().venue) {
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
                    borrowed_cache.add_account(updated_account).unwrap(); // Temp Fix to update the mutated account
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
            let borrowed_cache = self.cache.borrow();
            all_positions_open = borrowed_cache
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
                let borrowed_cache = self.cache.borrow();
                borrowed_cache
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

            let borrowed_cache = self.cache.borrow();
            let account =
                if let Some(account) = borrowed_cache.account_for_venue(&instrument_id.venue) {
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

            let mut borrowed_cache = self.cache.borrow_mut();
            let instrument = if let Some(instrument) = borrowed_cache.instrument(&instrument_id) {
                instrument
            } else {
                log::error!(
                    "Cannot update maintenance (position) margin: no instrument found for {}",
                    instrument_id
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
                    borrowed_cache
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
            self.msgbus.clone(),
            self.clock.clone(),
            self.inner.clone(),
            quote,
        );
    }

    pub fn update_account(&mut self, event: &AccountState) {
        update_account(self.cache.clone(), event);
    }

    pub fn update_order(&mut self, event: &OrderEventAny) {
        update_order(
            self.cache.clone(),
            self.msgbus.clone(),
            self.clock.clone(),
            self.inner.clone(),
            event,
        );
    }

    pub fn update_position(&mut self, event: &PositionEvent) {
        update_position(
            self.cache.clone(),
            self.msgbus.clone(),
            self.clock.clone(),
            self.inner.clone(),
            event,
        );
    }

    // -- INTERNAL --------------------------------------------------------------------------------

    fn update_net_position(&mut self, instrument_id: &InstrumentId, positions_open: Vec<Position>) {
        let mut net_position = Decimal::ZERO;

        for open_position in positions_open {
            log::debug!("open_position: {}", open_position);
            net_position += Decimal::from_f64(open_position.signed_qty).unwrap_or(Decimal::ZERO);
        }

        let existing_position = self.net_position(instrument_id);
        if existing_position != net_position {
            self.inner
                .borrow_mut()
                .net_positions
                .insert(*instrument_id, net_position);
            log::info!("{} net_position={}", instrument_id, net_position);
        }
    }

    fn calculate_unrealized_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        let borrowed_cache = self.cache.borrow();

        let account = if let Some(account) = borrowed_cache.account_for_venue(&instrument_id.venue)
        {
            account
        } else {
            log::error!(
                "Cannot calculate unrealized PnL: no account registered for {}",
                instrument_id.venue
            );
            return None;
        };

        let instrument = if let Some(instrument) = borrowed_cache.instrument(instrument_id) {
            instrument
        } else {
            log::error!(
                "Cannot calculate unrealized PnL: no instrument for {}",
                instrument_id
            );
            return None;
        };

        let currency = account
            .base_currency()
            .unwrap_or_else(|| instrument.settlement_currency());

        let positions_open = borrowed_cache.positions_open(
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

            let last = if let Some(price) = self.get_last_price(position) {
                price
            } else {
                log::debug!(
                    "Cannot calculate unrealized PnL: no prices for {}",
                    instrument_id
                );
                self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                return None; // Cannot calculate
            };

            let mut pnl = position.unrealized_pnl(last).as_f64();

            if let Some(base_currency) = account.base_currency() {
                let xrate = self.calculate_xrate_to_base(instrument, account, position.entry);

                if xrate == 0.0 {
                    log::debug!(
                        "Cannot calculate unrealized PnL: insufficient data for {}/{}",
                        instrument.settlement_currency(),
                        base_currency
                    );
                    self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                    return None;
                }

                let scale = 10f64.powi(currency.precision.into());
                pnl = ((pnl * xrate) * scale).round() / scale;
            }

            total_pnl += pnl;
        }

        Some(Money::new(total_pnl, currency))
    }

    fn calculate_realized_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        let borrowed_cache = self.cache.borrow();

        let account = if let Some(account) = borrowed_cache.account_for_venue(&instrument_id.venue)
        {
            account
        } else {
            log::error!(
                "Cannot calculate realized PnL: no account registered for {}",
                instrument_id.venue
            );
            return None;
        };

        let instrument = if let Some(instrument) = borrowed_cache.instrument(instrument_id) {
            instrument
        } else {
            log::error!(
                "Cannot calculate realized PnL: no instrument for {}",
                instrument_id
            );
            return None;
        };

        let currency = account
            .base_currency()
            .unwrap_or_else(|| instrument.settlement_currency());

        let positions = borrowed_cache.positions(
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

            if position.side == PositionSide::Flat {
                continue; // Nothing to calculate
            }

            let mut pnl = position.realized_pnl?.as_f64();

            if let Some(base_currency) = account.base_currency() {
                let xrate = self.calculate_xrate_to_base(instrument, account, position.entry);

                if xrate == 0.0 {
                    log::debug!(
                        "Cannot calculate realized PnL: insufficient data for {}/{}",
                        instrument.settlement_currency(),
                        base_currency
                    );
                    self.inner.borrow_mut().pending_calcs.insert(*instrument_id);
                    return None; // Cannot calculate
                }

                let scale = 10f64.powi(currency.precision.into());
                pnl = ((pnl * xrate) * scale).round() / scale;
            }

            total_pnl += pnl;
        }

        Some(Money::new(total_pnl, currency))
    }

    fn get_last_price(&self, position: &Position) -> Option<Price> {
        let price_type = match position.side {
            PositionSide::Long => PriceType::Bid,
            PositionSide::Short => PriceType::Ask,
            _ => panic!("invalid `PositionSide`, was {}", position.side),
        };

        let borrowed_cache = self.cache.borrow();

        borrowed_cache
            .price(&position.instrument_id, price_type)
            .or_else(|| borrowed_cache.price(&position.instrument_id, PriceType::Last))
    }

    fn calculate_xrate_to_base(
        &self,
        instrument: &InstrumentAny,
        account: &AccountAny,
        side: OrderSide,
    ) -> f64 {
        match account.base_currency() {
            Some(base_currency) => {
                let price_type = if side == OrderSide::Buy {
                    PriceType::Bid
                } else {
                    PriceType::Ask
                };

                self.cache
                    .borrow()
                    .get_xrate(
                        instrument.id().venue,
                        instrument.settlement_currency(),
                        base_currency,
                        price_type,
                    )
                    .to_f64()
                    .unwrap_or_else(|| {
                        log::error!(
                            "Failed to get/convert xrate for instrument {} from {} to {}",
                            instrument.id(),
                            instrument.settlement_currency(),
                            base_currency
                        );
                        1.0
                    })
            }
            None => 1.0, // No conversion needed
        }
    }
}

// Helper functions
fn update_quote_tick(
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    clock: Rc<RefCell<dyn Clock>>,
    inner: Rc<RefCell<PortfolioState>>,
    quote: &QuoteTick,
) {
    inner
        .borrow_mut()
        .unrealized_pnls
        .remove(&quote.instrument_id);

    if inner.borrow().initialized || !inner.borrow().pending_calcs.contains(&quote.instrument_id) {
        return;
    }

    let result_init;
    let mut result_maint = None;

    let account = {
        let borrowed_cache = cache.borrow();
        let account =
            if let Some(account) = borrowed_cache.account_for_venue(&quote.instrument_id.venue) {
                account
            } else {
                log::error!(
                    "Cannot update tick: no account registered for {}",
                    quote.instrument_id.venue
                );
                return;
            };

        let mut borrowed_cache = cache.borrow_mut();
        let instrument = if let Some(instrument) = borrowed_cache.instrument(&quote.instrument_id) {
            instrument.clone()
        } else {
            log::error!(
                "Cannot update tick: no instrument found for {}",
                quote.instrument_id
            );
            return;
        };

        // Clone the orders and positions to own the data
        let orders_open: Vec<OrderAny> = borrowed_cache
            .orders_open(None, Some(&quote.instrument_id), None, None)
            .iter()
            .map(|o| (*o).clone())
            .collect();

        let positions_open: Vec<Position> = borrowed_cache
            .positions_open(None, Some(&quote.instrument_id), None, None)
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
        msgbus,
        inner: inner.clone(),
    };

    let result_unrealized_pnl: Option<Money> =
        portfolio_clone.calculate_unrealized_pnl(&quote.instrument_id);

    if result_init.is_some()
        && (matches!(account, AccountAny::Cash(_))
            || (result_maint.is_some() && result_unrealized_pnl.is_some()))
    {
        inner
            .borrow_mut()
            .pending_calcs
            .remove(&quote.instrument_id);
        if inner.borrow().pending_calcs.is_empty() {
            inner.borrow_mut().initialized = true;
        }
    }
}

fn update_order(
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
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
        log::error!(
            "Cannot update order: no account registered for {}",
            account_id
        );
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
            msgbus: msgbus.clone(),
            inner: inner.clone(),
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
        msgbus.borrow().publish(
            &Ustr::from(&format!("events.account.{}", account.id())),
            &account_state,
        );
    } else {
        log::debug!("Added pending calculation for {}", instrument.id());
        inner.borrow_mut().pending_calcs.insert(instrument.id());
    }

    log::debug!("Updated {}", event);
}

fn update_position(
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
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

    log::debug!("postion fresh from cache -> {:?}", positions_open);

    let mut portfolio_clone = Portfolio {
        clock: clock.clone(),
        cache: cache.clone(),
        msgbus,
        inner: inner.clone(),
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
        };

        let borrowed_cache = cache.borrow();
        let instrument = if let Some(instrument) = borrowed_cache.instrument(&instrument_id) {
            instrument
        } else {
            log::error!(
                "Cannot update position: no instrument found for {}",
                instrument_id
            );
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
            log::error!("Failed to update account: {}", e);
            return;
        }
    } else {
        let account = match AccountAny::from_events(vec![event.clone()]) {
            Ok(account) => account,
            Err(e) => {
                log::error!("Failed to create account: {}", e);
                return;
            }
        };

        if let Err(e) = borrowed_cache.add_account(account) {
            log::error!("Failed to add account: {}", e);
            return;
        }
    }

    log::info!("Updated {}", event);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock, msgbus::MessageBus};
    use nautilus_core::{UnixNanos, UUID4};
    use nautilus_model::{
        data::QuoteTick,
        enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderType},
        events::{
            account::stubs::cash_account_state,
            order::stubs::{order_accepted, order_filled, order_submitted},
            AccountState, OrderAccepted, OrderEventAny, OrderFilled, OrderSubmitted,
            PositionChanged, PositionClosed, PositionEvent, PositionOpened,
        },
        identifiers::{
            stubs::{account_id, uuid4},
            AccountId, ClientOrderId, PositionId, StrategyId, Symbol, TradeId, VenueOrderId,
        },
        instruments::{
            stubs::{audusd_sim, currency_pair_btcusdt, default_fx_ccy, ethusdt_bitmex},
            CryptoPerpetual, CurrencyPair, InstrumentAny,
        },
        orders::{OrderAny, OrderTestBuilder},
        position::Position,
        types::{AccountBalance, Currency, Money, Price, Quantity},
    };
    use rstest::{fixture, rstest};
    use rust_decimal::{prelude::FromPrimitive, Decimal};

    use super::Portfolio;

    #[fixture]
    fn msgbus() -> MessageBus {
        MessageBus::default()
    }

    #[fixture]
    fn simple_cache() -> Cache {
        Cache::new(None, None)
    }

    #[fixture]
    fn clock() -> TestClock {
        TestClock::new()
    }

    #[fixture]
    fn venue() -> Venue {
        Venue::new("SIM")
    }

    #[fixture]
    fn instrument_audusd(audusd_sim: CurrencyPair) -> InstrumentAny {
        InstrumentAny::CurrencyPair(audusd_sim)
    }

    #[fixture]
    fn instrument_gbpusd() -> InstrumentAny {
        InstrumentAny::CurrencyPair(default_fx_ccy(
            Symbol::from("GBP/USD"),
            Some(Venue::from("SIM")),
        ))
    }

    #[fixture]
    fn instrument_btcusdt(currency_pair_btcusdt: CurrencyPair) -> InstrumentAny {
        InstrumentAny::CurrencyPair(currency_pair_btcusdt)
    }

    #[fixture]
    fn instrument_ethusdt(ethusdt_bitmex: CryptoPerpetual) -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(ethusdt_bitmex)
    }

    #[fixture]
    fn portfolio(
        msgbus: MessageBus,
        mut simple_cache: Cache,
        clock: TestClock,
        instrument_audusd: InstrumentAny,
        instrument_gbpusd: InstrumentAny,
        instrument_btcusdt: InstrumentAny,
        instrument_ethusdt: InstrumentAny,
    ) -> Portfolio {
        simple_cache.add_instrument(instrument_audusd).unwrap();
        simple_cache.add_instrument(instrument_gbpusd).unwrap();
        simple_cache.add_instrument(instrument_btcusdt).unwrap();
        simple_cache.add_instrument(instrument_ethusdt).unwrap();

        Portfolio::new(
            Rc::new(RefCell::new(msgbus)),
            Rc::new(RefCell::new(simple_cache)),
            Rc::new(RefCell::new(clock)),
        )
    }

    use std::collections::HashMap;

    use nautilus_model::identifiers::Venue;

    // Helpers
    fn get_cash_account(accountid: Option<&str>) -> AccountState {
        AccountState::new(
            match accountid {
                Some(account_id_str) => AccountId::new(account_id_str),
                None => account_id(),
            },
            AccountType::Cash,
            vec![
                AccountBalance::new(
                    Money::new(10.00000000, Currency::BTC()),
                    Money::new(0.00000000, Currency::BTC()),
                    Money::new(10.00000000, Currency::BTC()),
                ),
                AccountBalance::new(
                    Money::new(10.000, Currency::USD()),
                    Money::new(0.000, Currency::USD()),
                    Money::new(10.000, Currency::USD()),
                ),
                AccountBalance::new(
                    Money::new(100000.000, Currency::USDT()),
                    Money::new(0.000, Currency::USDT()),
                    Money::new(100000.000, Currency::USDT()),
                ),
                AccountBalance::new(
                    Money::new(20.000, Currency::ETH()),
                    Money::new(0.000, Currency::ETH()),
                    Money::new(20.000, Currency::ETH()),
                ),
            ],
            vec![],
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        )
    }

    fn get_margin_account(accountid: Option<&str>) -> AccountState {
        AccountState::new(
            match accountid {
                Some(account_id_str) => AccountId::new(account_id_str),
                None => account_id(),
            },
            AccountType::Margin,
            vec![
                AccountBalance::new(
                    Money::new(10.000, Currency::BTC()),
                    Money::new(0.000, Currency::BTC()),
                    Money::new(10.000, Currency::BTC()),
                ),
                AccountBalance::new(
                    Money::new(20.000, Currency::ETH()),
                    Money::new(0.000, Currency::ETH()),
                    Money::new(20.000, Currency::ETH()),
                ),
                AccountBalance::new(
                    Money::new(100000.000, Currency::USDT()),
                    Money::new(0.000, Currency::USDT()),
                    Money::new(100000.000, Currency::USDT()),
                ),
                AccountBalance::new(
                    Money::new(10.000, Currency::USD()),
                    Money::new(0.000, Currency::USD()),
                    Money::new(10.000, Currency::USD()),
                ),
                AccountBalance::new(
                    Money::new(10.000, Currency::GBP()),
                    Money::new(0.000, Currency::GBP()),
                    Money::new(10.000, Currency::GBP()),
                ),
            ],
            Vec::new(),
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        )
    }

    fn get_quote_tick(
        instrument: &InstrumentAny,
        bid: f64,
        ask: f64,
        bid_size: f64,
        ask_size: f64,
    ) -> QuoteTick {
        QuoteTick::new(
            instrument.id(),
            Price::new(bid, 0),
            Price::new(ask, 0),
            Quantity::new(bid_size, 0),
            Quantity::new(ask_size, 0),
            0.into(),
            0.into(),
        )
    }

    fn submit_order(order: &OrderAny) -> OrderSubmitted {
        order_submitted(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            account_id(),
            uuid4(),
        )
    }

    fn accept_order(order: &OrderAny) -> OrderAccepted {
        order_accepted(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            account_id(),
            order.venue_order_id().unwrap_or(VenueOrderId::new("1")),
            uuid4(),
        )
    }

    fn fill_order(order: &OrderAny) -> OrderFilled {
        order_filled(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            uuid4(),
        )
    }

    fn get_open_position(position: &Position) -> PositionOpened {
        PositionOpened {
            trader_id: position.trader_id,
            strategy_id: position.strategy_id,
            instrument_id: position.instrument_id,
            position_id: position.id,
            account_id: position.account_id,
            opening_order_id: position.opening_order_id,
            entry: position.entry,
            side: position.side,
            signed_qty: position.signed_qty,
            quantity: position.quantity,
            last_qty: position.quantity,
            last_px: Price::new(position.avg_px_open, 0),
            currency: position.settlement_currency,
            avg_px_open: position.avg_px_open,
            event_id: UUID4::new(),
            ts_event: 0.into(),
            ts_init: 0.into(),
        }
    }

    fn get_changed_position(position: &Position) -> PositionChanged {
        PositionChanged {
            trader_id: position.trader_id,
            strategy_id: position.strategy_id,
            instrument_id: position.instrument_id,
            position_id: position.id,
            account_id: position.account_id,
            opening_order_id: position.opening_order_id,
            entry: position.entry,
            side: position.side,
            signed_qty: position.signed_qty,
            quantity: position.quantity,
            last_qty: position.quantity,
            last_px: Price::new(position.avg_px_open, 0),
            currency: position.settlement_currency,
            avg_px_open: position.avg_px_open,
            ts_event: 0.into(),
            ts_init: 0.into(),
            peak_quantity: position.quantity,
            avg_px_close: Some(position.avg_px_open),
            realized_return: position.avg_px_open,
            realized_pnl: Some(Money::new(10.0, Currency::USD())),
            unrealized_pnl: Money::new(10.0, Currency::USD()),
            event_id: UUID4::new(),
            ts_opened: 0.into(),
        }
    }

    fn get_close_position(position: &Position) -> PositionClosed {
        PositionClosed {
            trader_id: position.trader_id,
            strategy_id: position.strategy_id,
            instrument_id: position.instrument_id,
            position_id: position.id,
            account_id: position.account_id,
            opening_order_id: position.opening_order_id,
            entry: position.entry,
            side: position.side,
            signed_qty: position.signed_qty,
            quantity: position.quantity,
            last_qty: position.quantity,
            last_px: Price::new(position.avg_px_open, 0),
            currency: position.settlement_currency,
            avg_px_open: position.avg_px_open,
            ts_event: 0.into(),
            ts_init: 0.into(),
            peak_quantity: position.quantity,
            avg_px_close: Some(position.avg_px_open),
            realized_return: position.avg_px_open,
            realized_pnl: Some(Money::new(10.0, Currency::USD())),
            unrealized_pnl: Money::new(10.0, Currency::USD()),
            closing_order_id: Some(ClientOrderId::new("SSD")),
            duration: 0,
            event_id: UUID4::new(),
            ts_opened: 0.into(),
            ts_closed: None,
        }
    }

    // Tests
    #[rstest]
    fn test_account_when_account_returns_the_account_facade(mut portfolio: Portfolio) {
        let account_id = "BINANCE-1513111";
        let state = get_cash_account(Some(account_id));

        portfolio.update_account(&state);

        let borrowed_cache = portfolio.cache.borrow_mut();
        let account = borrowed_cache.account(&AccountId::new(account_id)).unwrap();
        assert_eq!(account.id().get_issuer(), "BINANCE".into());
        assert_eq!(account.id().get_issuers_id(), "1513111");
    }

    #[rstest]
    fn test_balances_locked_when_no_account_for_venue_returns_none(
        portfolio: Portfolio,
        venue: Venue,
    ) {
        let result = portfolio.balances_locked(&venue);
        assert_eq!(result, HashMap::new());
    }

    #[rstest]
    fn test_margins_init_when_no_account_for_venue_returns_none(
        portfolio: Portfolio,
        venue: Venue,
    ) {
        let result = portfolio.margins_init(&venue);
        assert_eq!(result, HashMap::new());
    }

    #[rstest]
    fn test_margins_maint_when_no_account_for_venue_returns_none(
        portfolio: Portfolio,
        venue: Venue,
    ) {
        let result = portfolio.margins_maint(&venue);
        assert_eq!(result, HashMap::new());
    }

    #[rstest]
    fn test_unrealized_pnl_for_instrument_when_no_instrument_returns_none(
        mut portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
    ) {
        let result = portfolio.unrealized_pnl(&instrument_audusd.id());
        assert!(result.is_none());
    }

    #[rstest]
    fn test_unrealized_pnl_for_venue_when_no_account_returns_empty_dict(
        mut portfolio: Portfolio,
        venue: Venue,
    ) {
        let result = portfolio.unrealized_pnls(&venue);
        assert_eq!(result, HashMap::new());
    }

    #[rstest]
    fn test_realized_pnl_for_instrument_when_no_instrument_returns_none(
        mut portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
    ) {
        let result = portfolio.realized_pnl(&instrument_audusd.id());
        assert!(result.is_none());
    }

    #[rstest]
    fn test_realized_pnl_for_venue_when_no_account_returns_empty_dict(
        mut portfolio: Portfolio,
        venue: Venue,
    ) {
        let result = portfolio.realized_pnls(&venue);
        assert_eq!(result, HashMap::new());
    }

    #[rstest]
    fn test_net_position_when_no_positions_returns_zero(
        portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
    ) {
        let result = portfolio.net_position(&instrument_audusd.id());
        assert_eq!(result, Decimal::ZERO);
    }

    #[rstest]
    fn test_net_exposures_when_no_positions_returns_none(portfolio: Portfolio, venue: Venue) {
        let result = portfolio.net_exposures(&venue);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_is_net_long_when_no_positions_returns_false(
        portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
    ) {
        let result = portfolio.is_net_long(&instrument_audusd.id());
        assert!(!result);
    }

    #[rstest]
    fn test_is_net_short_when_no_positions_returns_false(
        portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
    ) {
        let result = portfolio.is_net_short(&instrument_audusd.id());
        assert!(!result);
    }

    #[rstest]
    fn test_is_flat_when_no_positions_returns_true(
        portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
    ) {
        let result = portfolio.is_flat(&instrument_audusd.id());
        assert!(result);
    }

    #[rstest]
    fn test_is_completely_flat_when_no_positions_returns_true(portfolio: Portfolio) {
        let result = portfolio.is_completely_flat();
        assert!(result);
    }

    #[rstest]
    fn test_open_value_when_no_account_returns_none(portfolio: Portfolio, venue: Venue) {
        let result = portfolio.net_exposures(&venue);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_update_tick(mut portfolio: Portfolio, instrument_audusd: InstrumentAny) {
        let tick = get_quote_tick(&instrument_audusd, 1.25, 1.251, 1.0, 1.0);
        portfolio.update_quote_tick(&tick);
        assert!(portfolio.unrealized_pnl(&instrument_audusd.id()).is_none());
    }

    //TODO: FIX: It should return an error
    #[rstest]
    fn test_exceed_free_balance_single_currency_raises_account_balance_negative_exception(
        mut portfolio: Portfolio,
        cash_account_state: AccountState,
        instrument_audusd: InstrumentAny,
    ) {
        portfolio.update_account(&cash_account_state);

        let mut order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1000000"))
            .build();

        portfolio
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        let order_submitted = submit_order(&order);
        order
            .apply(OrderEventAny::Submitted(order_submitted))
            .unwrap();

        portfolio.update_order(&OrderEventAny::Submitted(order_submitted));

        let order_filled = fill_order(&order);
        order.apply(OrderEventAny::Filled(order_filled)).unwrap();
        portfolio.update_order(&OrderEventAny::Filled(order_filled));
    }

    // TODO: It should return an error
    #[rstest]
    fn test_exceed_free_balance_multi_currency_raises_account_balance_negative_exception(
        mut portfolio: Portfolio,
        cash_account_state: AccountState,
        instrument_audusd: InstrumentAny,
    ) {
        portfolio.update_account(&cash_account_state);

        let account = portfolio
            .cache
            .borrow_mut()
            .account_for_venue(&Venue::from("SIM"))
            .unwrap()
            .clone();

        // Create Order
        let mut order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("3.0"))
            .build();

        portfolio
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        let order_submitted = submit_order(&order);
        order
            .apply(OrderEventAny::Submitted(order_submitted))
            .unwrap();
        portfolio.update_order(&OrderEventAny::Submitted(order_submitted));

        // Assert
        assert_eq!(
            account.balances().iter().next().unwrap().1.total.as_f64(),
            1525000.00
        );
    }

    #[rstest]
    fn test_update_orders_open_cash_account(
        mut portfolio: Portfolio,
        cash_account_state: AccountState,
        instrument_audusd: InstrumentAny,
    ) {
        portfolio.update_account(&cash_account_state);

        // Create Order
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .price(Price::new(50000.0, 0))
            .build();

        portfolio
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        let order_submitted = submit_order(&order);
        order
            .apply(OrderEventAny::Submitted(order_submitted))
            .unwrap();
        portfolio.update_order(&OrderEventAny::Submitted(order_submitted));

        // ACCEPTED
        let order_accepted = accept_order(&order);
        order
            .apply(OrderEventAny::Accepted(order_accepted))
            .unwrap();
        portfolio.update_order(&OrderEventAny::Accepted(order_accepted));

        assert_eq!(
            portfolio
                .balances_locked(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            25000.0
        );
    }

    #[rstest]
    fn test_update_orders_open_margin_account(
        mut portfolio: Portfolio,
        instrument_btcusdt: InstrumentAny,
    ) {
        let account_state = get_margin_account(Some("BINANCE-01234"));
        portfolio.update_account(&account_state);

        // Create Order
        let mut order1 = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100.0"))
            .price(Price::new(55.0, 1))
            .trigger_price(Price::new(35.0, 1))
            .build();

        let order2 = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1000.0"))
            .price(Price::new(45.0, 1))
            .trigger_price(Price::new(30.0, 1))
            .build();

        portfolio
            .cache
            .borrow_mut()
            .add_order(order1.clone(), None, None, true)
            .unwrap();

        portfolio
            .cache
            .borrow_mut()
            .add_order(order2, None, None, true)
            .unwrap();

        let order_submitted = submit_order(&order1);
        order1
            .apply(OrderEventAny::Submitted(order_submitted))
            .unwrap();
        portfolio.cache.borrow_mut().update_order(&order1).unwrap();

        // Push status to Accepted
        let order_accepted = accept_order(&order1);
        order1
            .apply(OrderEventAny::Accepted(order_accepted))
            .unwrap();
        portfolio.cache.borrow_mut().update_order(&order1).unwrap();

        // TODO: Replace with Execution Engine once implemented.
        portfolio
            .cache
            .borrow_mut()
            .add_order(order1.clone(), None, None, true)
            .unwrap();

        let order_filled1 = fill_order(&order1);
        order1.apply(OrderEventAny::Filled(order_filled1)).unwrap();

        // Act
        let last = get_quote_tick(&instrument_btcusdt, 25001.0, 25002.0, 15.0, 12.0);
        portfolio.update_quote_tick(&last);
        portfolio.initialize_orders();

        // Assert
        assert_eq!(
            portfolio
                .margins_init(&Venue::from("BINANCE"))
                .get(&instrument_btcusdt.id())
                .unwrap()
                .as_f64(),
            10.5
        );
    }

    #[rstest]
    fn test_order_accept_updates_margin_init(
        mut portfolio: Portfolio,
        instrument_btcusdt: InstrumentAny,
    ) {
        let account_state = get_margin_account(Some("BINANCE-01234"));
        portfolio.update_account(&account_state);

        // Create Order
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(ClientOrderId::new("55"))
            .instrument_id(instrument_btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100.0"))
            .price(Price::new(5.0, 0))
            .build();

        portfolio
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();

        let order_submitted = submit_order(&order);
        order
            .apply(OrderEventAny::Submitted(order_submitted))
            .unwrap();
        portfolio.cache.borrow_mut().update_order(&order).unwrap();

        let order_accepted = accept_order(&order);
        order
            .apply(OrderEventAny::Accepted(order_accepted))
            .unwrap();
        portfolio.cache.borrow_mut().update_order(&order).unwrap();

        // TODO: Replace with Execution Engine once implemented.
        portfolio
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();

        // Act
        portfolio.initialize_orders();

        // Assert
        assert_eq!(
            portfolio
                .margins_init(&Venue::from("BINANCE"))
                .get(&instrument_btcusdt.id())
                .unwrap()
                .as_f64(),
            1.5
        );
    }

    #[rstest]
    fn test_update_positions(mut portfolio: Portfolio, instrument_audusd: InstrumentAny) {
        let account_state = get_cash_account(None);
        portfolio.update_account(&account_state);

        // Create Order
        let mut order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.50"))
            .build();

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("10.50"))
            .build();

        portfolio
            .cache
            .borrow_mut()
            .add_order(order1.clone(), None, None, true)
            .unwrap();
        portfolio
            .cache
            .borrow_mut()
            .add_order(order2.clone(), None, None, true)
            .unwrap();

        let order1_submitted = submit_order(&order1);
        order1
            .apply(OrderEventAny::Submitted(order1_submitted))
            .unwrap();
        portfolio.update_order(&OrderEventAny::Submitted(order1_submitted));

        // ACCEPTED
        let order1_accepted = accept_order(&order1);
        order1
            .apply(OrderEventAny::Accepted(order1_accepted))
            .unwrap();
        portfolio.update_order(&OrderEventAny::Accepted(order1_accepted));

        let mut fill1 = fill_order(&order1);
        fill1.position_id = Some(PositionId::new("SSD"));

        let mut fill2 = fill_order(&order2);
        fill2.trade_id = TradeId::new("2");

        let mut position1 = Position::new(&instrument_audusd, fill1);
        position1.apply(&fill2);

        let order3 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("10.00"))
            .build();

        let mut fill3 = fill_order(&order3);
        fill3.position_id = Some(PositionId::new("SSsD"));

        let position2 = Position::new(&instrument_audusd, fill3);

        // Update the last quote
        let last = get_quote_tick(&instrument_audusd, 250001.0, 250002.0, 1.0, 1.0);

        // Act
        portfolio
            .cache
            .borrow_mut()
            .add_position(position1, OmsType::Hedging)
            .unwrap();
        portfolio
            .cache
            .borrow_mut()
            .add_position(position2, OmsType::Hedging)
            .unwrap();
        portfolio.cache.borrow_mut().add_quote(last).unwrap();
        portfolio.initialize_positions();
        portfolio.update_quote_tick(&last);

        // Assert
        assert!(portfolio.is_net_long(&instrument_audusd.id()));
    }

    #[rstest]
    fn test_opening_one_long_position_updates_portfolio(
        mut portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
    ) {
        let account_state = get_margin_account(None);
        portfolio.update_account(&account_state);

        // Create Order
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.00"))
            .build();

        let mut fill = fill_order(&order);
        fill.position_id = Some(PositionId::new("SSD"));

        // Update the last quote
        let last = get_quote_tick(&instrument_audusd, 10510.0, 10511.0, 1.0, 1.0);
        portfolio.cache.borrow_mut().add_quote(last).unwrap();
        portfolio.update_quote_tick(&last);

        let position = Position::new(&instrument_audusd, fill);

        // Act
        portfolio
            .cache
            .borrow_mut()
            .add_position(position.clone(), OmsType::Hedging)
            .unwrap();

        let position_opened = get_open_position(&position);
        portfolio.update_position(&PositionEvent::PositionOpened(position_opened));

        // Assert
        assert_eq!(
            portfolio
                .net_exposures(&Venue::from("SIM"))
                .unwrap()
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            10510.0
        );
        assert_eq!(
            portfolio
                .unrealized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            -6445.89
        );
        assert_eq!(
            portfolio
                .realized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            0.0
        );
        assert_eq!(
            portfolio
                .net_exposure(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            10510.0
        );
        assert_eq!(
            portfolio
                .unrealized_pnl(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            -6445.89
        );
        assert_eq!(
            portfolio
                .realized_pnl(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            0.0
        );
        assert_eq!(
            portfolio.net_position(&instrument_audusd.id()),
            Decimal::new(561, 3)
        );
        assert!(portfolio.is_net_long(&instrument_audusd.id()));
        assert!(!portfolio.is_net_short(&instrument_audusd.id()));
        assert!(!portfolio.is_flat(&instrument_audusd.id()));
        assert!(!portfolio.is_completely_flat());
    }

    #[rstest]
    fn test_opening_one_short_position_updates_portfolio(
        mut portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
    ) {
        let account_state = get_margin_account(None);
        portfolio.update_account(&account_state);

        // Create Order
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("2"))
            .build();

        let fill = OrderFilled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("1"),
            order.order_side(),
            order.order_type(),
            order.quantity(),
            Price::new(10.0, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("SSD")),
            Some(Money::from("12.2 USD")),
        );

        // Update the last quote
        let last = get_quote_tick(&instrument_audusd, 15510.15, 15510.25, 13.0, 4.0);

        portfolio.cache.borrow_mut().add_quote(last).unwrap();
        portfolio.update_quote_tick(&last);

        let position = Position::new(&instrument_audusd, fill);

        // Act
        portfolio
            .cache
            .borrow_mut()
            .add_position(position.clone(), OmsType::Hedging)
            .unwrap();

        let position_opened = get_open_position(&position);
        portfolio.update_position(&PositionEvent::PositionOpened(position_opened));

        // Assert
        assert_eq!(
            portfolio
                .net_exposures(&Venue::from("SIM"))
                .unwrap()
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            31020.0
        );
        assert_eq!(
            portfolio
                .unrealized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            -31000.0
        );
        assert_eq!(
            portfolio
                .realized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            -12.2
        );
        assert_eq!(
            portfolio
                .net_exposure(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            31020.0
        );
        assert_eq!(
            portfolio
                .unrealized_pnl(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            -31000.0
        );
        assert_eq!(
            portfolio
                .realized_pnl(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            -12.2
        );
        assert_eq!(
            portfolio.net_position(&instrument_audusd.id()),
            Decimal::new(-2, 0)
        );

        assert!(!portfolio.is_net_long(&instrument_audusd.id()));
        assert!(portfolio.is_net_short(&instrument_audusd.id()));
        assert!(!portfolio.is_flat(&instrument_audusd.id()));
        assert!(!portfolio.is_completely_flat());
    }

    #[rstest]
    fn test_opening_positions_with_multi_asset_account(
        mut portfolio: Portfolio,
        instrument_btcusdt: InstrumentAny,
        instrument_ethusdt: InstrumentAny,
    ) {
        let account_state = get_margin_account(Some("BITMEX-01234"));
        portfolio.update_account(&account_state);

        let last_ethusd = get_quote_tick(&instrument_ethusdt, 376.05, 377.10, 16.0, 25.0);
        let last_btcusd = get_quote_tick(&instrument_btcusdt, 10500.05, 10501.51, 2.54, 0.91);

        portfolio.cache.borrow_mut().add_quote(last_ethusd).unwrap();
        portfolio.cache.borrow_mut().add_quote(last_btcusd).unwrap();
        portfolio.update_quote_tick(&last_ethusd);
        portfolio.update_quote_tick(&last_btcusd);

        // Create Order
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_ethusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10000"))
            .build();

        let fill = OrderFilled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("1"),
            order.order_side(),
            order.order_type(),
            order.quantity(),
            Price::new(376.0, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("SSD")),
            Some(Money::from("12.2 USD")),
        );

        let position = Position::new(&instrument_ethusdt, fill);

        // Act
        portfolio
            .cache
            .borrow_mut()
            .add_position(position.clone(), OmsType::Hedging)
            .unwrap();

        let position_opened = get_open_position(&position);
        portfolio.update_position(&PositionEvent::PositionOpened(position_opened));

        // Assert
        assert_eq!(
            portfolio
                .net_exposures(&Venue::from("BITMEX"))
                .unwrap()
                .get(&Currency::ETH())
                .unwrap()
                .as_f64(),
            26.59574468
        );
        assert_eq!(
            portfolio
                .unrealized_pnls(&Venue::from("BITMEX"))
                .get(&Currency::ETH())
                .unwrap()
                .as_f64(),
            0.0
        );
        // TODO: fix
        // assert_eq!(
        //     portfolio
        //         .margins_maint(&Venue::from("SIM"))
        //         .get(&instrument_audusd.id())
        //         .unwrap()
        //         .as_f64(),
        //     0.0
        // );
        assert_eq!(
            portfolio
                .net_exposure(&instrument_ethusdt.id())
                .unwrap()
                .as_f64(),
            26.59574468
        );
    }

    #[rstest]
    fn test_market_value_when_insufficient_data_for_xrate_returns_none(
        mut portfolio: Portfolio,
        instrument_btcusdt: InstrumentAny,
        instrument_ethusdt: InstrumentAny,
    ) {
        let account_state = get_margin_account(Some("BITMEX-01234"));
        portfolio.update_account(&account_state);

        // Create Order
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_ethusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .build();

        let fill = OrderFilled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("1"),
            order.order_side(),
            order.order_type(),
            order.quantity(),
            Price::new(376.05, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("SSD")),
            Some(Money::from("12.2 USD")),
        );

        let last_ethusd = get_quote_tick(&instrument_ethusdt, 376.05, 377.10, 16.0, 25.0);
        let last_xbtusd = get_quote_tick(&instrument_btcusdt, 50000.00, 50000.00, 1.0, 1.0);

        let position = Position::new(&instrument_ethusdt, fill);
        let position_opened = get_open_position(&position);

        // Act
        portfolio.update_position(&PositionEvent::PositionOpened(position_opened));
        portfolio
            .cache
            .borrow_mut()
            .add_position(position, OmsType::Hedging)
            .unwrap();
        portfolio.cache.borrow_mut().add_quote(last_ethusd).unwrap();
        portfolio.cache.borrow_mut().add_quote(last_xbtusd).unwrap();
        portfolio.update_quote_tick(&last_ethusd);
        portfolio.update_quote_tick(&last_xbtusd);

        // Assert
        assert_eq!(
            portfolio
                .net_exposures(&Venue::from("BITMEX"))
                .unwrap()
                .get(&Currency::ETH())
                .unwrap()
                .as_f64(),
            0.26595745
        );
    }

    #[rstest]
    fn test_opening_several_positions_updates_portfolio(
        mut portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
        instrument_gbpusd: InstrumentAny,
    ) {
        let account_state = get_margin_account(None);
        portfolio.update_account(&account_state);

        let last_audusd = get_quote_tick(&instrument_audusd, 0.80501, 0.80505, 1.0, 1.0);
        let last_gbpusd = get_quote_tick(&instrument_gbpusd, 1.30315, 1.30317, 1.0, 1.0);

        portfolio.cache.borrow_mut().add_quote(last_audusd).unwrap();
        portfolio.cache.borrow_mut().add_quote(last_gbpusd).unwrap();
        portfolio.update_quote_tick(&last_audusd);
        portfolio.update_quote_tick(&last_gbpusd);

        // Create Order
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .build();

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_gbpusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .build();

        portfolio
            .cache
            .borrow_mut()
            .add_order(order1.clone(), None, None, true)
            .unwrap();
        portfolio
            .cache
            .borrow_mut()
            .add_order(order2.clone(), None, None, true)
            .unwrap();

        let fill1 = OrderFilled::new(
            order1.trader_id(),
            order1.strategy_id(),
            order1.instrument_id(),
            order1.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("1"),
            order1.order_side(),
            order1.order_type(),
            order1.quantity(),
            Price::new(376.05, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("SSD")),
            Some(Money::from("12.2 USD")),
        );
        let fill2 = OrderFilled::new(
            order2.trader_id(),
            order2.strategy_id(),
            order2.instrument_id(),
            order2.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("1"),
            order2.order_side(),
            order2.order_type(),
            order2.quantity(),
            Price::new(376.05, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("SSD")),
            Some(Money::from("12.2 USD")),
        );

        portfolio.cache.borrow_mut().update_order(&order1).unwrap();
        portfolio.cache.borrow_mut().update_order(&order2).unwrap();

        let position1 = Position::new(&instrument_audusd, fill1);
        let position2 = Position::new(&instrument_gbpusd, fill2);

        let position_opened1 = get_open_position(&position1);
        let position_opened2 = get_open_position(&position2);

        // Act
        portfolio
            .cache
            .borrow_mut()
            .add_position(position1, OmsType::Hedging)
            .unwrap();
        portfolio
            .cache
            .borrow_mut()
            .add_position(position2, OmsType::Hedging)
            .unwrap();
        portfolio.update_position(&PositionEvent::PositionOpened(position_opened1));
        portfolio.update_position(&PositionEvent::PositionOpened(position_opened2));

        // Assert
        assert_eq!(
            portfolio
                .net_exposures(&Venue::from("SIM"))
                .unwrap()
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            100000.0
        );

        assert_eq!(
            portfolio
                .unrealized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            -37500000.0
        );

        assert_eq!(
            portfolio
                .realized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            -12.2
        );
        // FIX: TODO: should not be empty
        assert_eq!(portfolio.margins_maint(&Venue::from("SIM")), HashMap::new());
        assert_eq!(
            portfolio
                .net_exposure(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            100000.0
        );
        assert_eq!(
            portfolio
                .net_exposure(&instrument_gbpusd.id())
                .unwrap()
                .as_f64(),
            100000.0
        );
        assert_eq!(
            portfolio
                .unrealized_pnl(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            0.0
        );
        assert_eq!(
            portfolio
                .unrealized_pnl(&instrument_gbpusd.id())
                .unwrap()
                .as_f64(),
            -37500000.0
        );
        assert_eq!(
            portfolio
                .realized_pnl(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            0.0
        );
        assert_eq!(
            portfolio
                .realized_pnl(&instrument_gbpusd.id())
                .unwrap()
                .as_f64(),
            -12.2
        );
        assert_eq!(
            portfolio.net_position(&instrument_audusd.id()),
            Decimal::from_f64(100000.0).unwrap()
        );
        assert_eq!(
            portfolio.net_position(&instrument_gbpusd.id()),
            Decimal::from_f64(100000.0).unwrap()
        );
        assert!(portfolio.is_net_long(&instrument_audusd.id()));
        assert!(!portfolio.is_net_short(&instrument_audusd.id()));
        assert!(!portfolio.is_flat(&instrument_audusd.id()));
        assert!(!portfolio.is_completely_flat());
    }

    #[rstest]
    fn test_modifying_position_updates_portfolio(
        mut portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
    ) {
        let account_state = get_margin_account(None);
        portfolio.update_account(&account_state);

        let last_audusd = get_quote_tick(&instrument_audusd, 0.80501, 0.80505, 1.0, 1.0);
        portfolio.cache.borrow_mut().add_quote(last_audusd).unwrap();
        portfolio.update_quote_tick(&last_audusd);

        // Create Order
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .build();

        let fill1 = OrderFilled::new(
            order1.trader_id(),
            order1.strategy_id(),
            order1.instrument_id(),
            order1.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("1"),
            order1.order_side(),
            order1.order_type(),
            order1.quantity(),
            Price::new(376.05, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("SSD")),
            Some(Money::from("12.2 USD")),
        );

        let mut position1 = Position::new(&instrument_audusd, fill1);
        portfolio
            .cache
            .borrow_mut()
            .add_position(position1.clone(), OmsType::Hedging)
            .unwrap();
        let position_opened1 = get_open_position(&position1);
        portfolio.update_position(&PositionEvent::PositionOpened(position_opened1));

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("50000"))
            .build();

        let fill2 = OrderFilled::new(
            order2.trader_id(),
            order2.strategy_id(),
            order2.instrument_id(),
            order2.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("2"),
            order2.order_side(),
            order2.order_type(),
            order2.quantity(),
            Price::new(1.00, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("SSD")),
            Some(Money::from("1.2 USD")),
        );

        position1.apply(&fill2);
        let position1_changed = get_changed_position(&position1);

        // Act
        portfolio.update_position(&PositionEvent::PositionChanged(position1_changed));

        // Assert
        assert_eq!(
            portfolio
                .net_exposures(&Venue::from("SIM"))
                .unwrap()
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            100000.0
        );

        assert_eq!(
            portfolio
                .unrealized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            -37500000.0
        );

        assert_eq!(
            portfolio
                .realized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            -12.2
        );
        // FIX: TODO: should not be empty
        assert_eq!(portfolio.margins_maint(&Venue::from("SIM")), HashMap::new());
        assert_eq!(
            portfolio
                .net_exposure(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            100000.0
        );
        assert_eq!(
            portfolio
                .unrealized_pnl(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            -37500000.0
        );
        assert_eq!(
            portfolio
                .realized_pnl(&instrument_audusd.id())
                .unwrap()
                .as_f64(),
            -12.2
        );
        assert_eq!(
            portfolio.net_position(&instrument_audusd.id()),
            Decimal::from_f64(100000.0).unwrap()
        );
        assert!(portfolio.is_net_long(&instrument_audusd.id()));
        assert!(!portfolio.is_net_short(&instrument_audusd.id()));
        assert!(!portfolio.is_flat(&instrument_audusd.id()));
        assert!(!portfolio.is_completely_flat());
        assert_eq!(
            portfolio.unrealized_pnls(&Venue::from("BINANCE")),
            HashMap::new()
        );
        assert_eq!(
            portfolio.realized_pnls(&Venue::from("BINANCE")),
            HashMap::new()
        );
        assert_eq!(portfolio.net_exposures(&Venue::from("BINANCE")), None);
    }

    #[rstest]
    fn test_closing_position_updates_portfolio(
        mut portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
    ) {
        let account_state = get_margin_account(None);
        portfolio.update_account(&account_state);

        let last_audusd = get_quote_tick(&instrument_audusd, 0.80501, 0.80505, 1.0, 1.0);
        portfolio.cache.borrow_mut().add_quote(last_audusd).unwrap();
        portfolio.update_quote_tick(&last_audusd);

        // Create Order
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .build();

        let fill1 = OrderFilled::new(
            order1.trader_id(),
            order1.strategy_id(),
            order1.instrument_id(),
            order1.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("1"),
            order1.order_side(),
            order1.order_type(),
            order1.quantity(),
            Price::new(376.05, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("SSD")),
            Some(Money::from("12.2 USD")),
        );

        let mut position1 = Position::new(&instrument_audusd, fill1);
        portfolio
            .cache
            .borrow_mut()
            .add_position(position1.clone(), OmsType::Hedging)
            .unwrap();
        let position_opened1 = get_open_position(&position1);
        portfolio.update_position(&PositionEvent::PositionOpened(position_opened1));

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("50000"))
            .build();

        let fill2 = OrderFilled::new(
            order2.trader_id(),
            order2.strategy_id(),
            order2.instrument_id(),
            order2.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("2"),
            order2.order_side(),
            order2.order_type(),
            order2.quantity(),
            Price::new(1.00, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("SSD")),
            Some(Money::from("1.2 USD")),
        );

        position1.apply(&fill2);
        portfolio
            .cache
            .borrow_mut()
            .update_position(&position1)
            .unwrap();

        // Act
        let position1_closed = get_close_position(&position1);
        portfolio.update_position(&PositionEvent::PositionClosed(position1_closed));

        // Assert
        assert_eq!(
            portfolio
                .net_exposures(&Venue::from("SIM"))
                .unwrap()
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            100000.00
        );
        assert_eq!(
            portfolio
                .unrealized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            -37500000.00
        );
        assert_eq!(
            portfolio
                .realized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            -12.2
        );
        assert_eq!(portfolio.margins_maint(&Venue::from("SIM")), HashMap::new());
    }

    #[rstest]
    fn test_several_positions_with_different_instruments_updates_portfolio(
        mut portfolio: Portfolio,
        instrument_audusd: InstrumentAny,
        instrument_gbpusd: InstrumentAny,
    ) {
        let account_state = get_margin_account(None);
        portfolio.update_account(&account_state);

        // Create Order
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .build();
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .build();
        let order3 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_gbpusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .build();
        let order4 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_gbpusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("100000"))
            .build();

        let fill1 = OrderFilled::new(
            order1.trader_id(),
            StrategyId::new("S-1"),
            order1.instrument_id(),
            order1.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("1"),
            order1.order_side(),
            order1.order_type(),
            order1.quantity(),
            Price::new(1.0, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("P-1")),
            None,
        );
        let fill2 = OrderFilled::new(
            order2.trader_id(),
            StrategyId::new("S-1"),
            order2.instrument_id(),
            order2.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("2"),
            order2.order_side(),
            order2.order_type(),
            order2.quantity(),
            Price::new(1.0, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("P-2")),
            None,
        );
        let fill3 = OrderFilled::new(
            order3.trader_id(),
            StrategyId::new("S-1"),
            order3.instrument_id(),
            order3.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("3"),
            order3.order_side(),
            order3.order_type(),
            order3.quantity(),
            Price::new(1.0, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("P-3")),
            None,
        );
        let fill4 = OrderFilled::new(
            order4.trader_id(),
            StrategyId::new("S-1"),
            order4.instrument_id(),
            order4.client_order_id(),
            VenueOrderId::new("123456"),
            AccountId::new("SIM-001"),
            TradeId::new("4"),
            order4.order_side(),
            order4.order_type(),
            order4.quantity(),
            Price::new(1.0, 0),
            Currency::USD(),
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::new("P-4")),
            None,
        );

        let position1 = Position::new(&instrument_audusd, fill1);
        let position2 = Position::new(&instrument_audusd, fill2);
        let mut position3 = Position::new(&instrument_gbpusd, fill3);

        let last_audusd = get_quote_tick(&instrument_audusd, 0.80501, 0.80505, 1.0, 1.0);
        let last_gbpusd = get_quote_tick(&instrument_gbpusd, 1.30315, 1.30317, 1.0, 1.0);

        portfolio.cache.borrow_mut().add_quote(last_audusd).unwrap();
        portfolio.cache.borrow_mut().add_quote(last_gbpusd).unwrap();
        portfolio.update_quote_tick(&last_audusd);
        portfolio.update_quote_tick(&last_gbpusd);

        portfolio
            .cache
            .borrow_mut()
            .add_position(position1.clone(), OmsType::Hedging)
            .unwrap();
        portfolio
            .cache
            .borrow_mut()
            .add_position(position2.clone(), OmsType::Hedging)
            .unwrap();
        portfolio
            .cache
            .borrow_mut()
            .add_position(position3.clone(), OmsType::Hedging)
            .unwrap();

        let position_opened1 = get_open_position(&position1);
        let position_opened2 = get_open_position(&position2);
        let position_opened3 = get_open_position(&position3);

        portfolio.update_position(&PositionEvent::PositionOpened(position_opened1));
        portfolio.update_position(&PositionEvent::PositionOpened(position_opened2));
        portfolio.update_position(&PositionEvent::PositionOpened(position_opened3));

        let position_closed3 = get_close_position(&position3);
        position3.apply(&fill4);
        portfolio
            .cache
            .borrow_mut()
            .add_position(position3.clone(), OmsType::Hedging)
            .unwrap();
        portfolio.update_position(&PositionEvent::PositionClosed(position_closed3));

        // Assert
        assert_eq!(
            portfolio
                .net_exposures(&Venue::from("SIM"))
                .unwrap()
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            200000.00
        );
        assert_eq!(
            portfolio
                .unrealized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            0.0
        );
        assert_eq!(
            portfolio
                .realized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            0.0
        );
        // FIX: TODO: should not be empty
        assert_eq!(portfolio.margins_maint(&Venue::from("SIM")), HashMap::new());
    }
}
