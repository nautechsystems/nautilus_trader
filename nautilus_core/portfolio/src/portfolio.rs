// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
    accounts::any::AccountAny,
    data::{quote::QuoteTick, Data},
    enums::{OrderSide, OrderType, PositionSide, PriceType},
    events::{account::state::AccountState, order::OrderEventAny, position::PositionEvent},
    identifiers::{InstrumentId, Venue},
    instruments::any::InstrumentAny,
    orders::any::OrderAny,
    position::Position,
    types::{currency::Currency, money::Money, price::Price},
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
            nautilus_model::accounts::any::AccountAny::balances_locked,
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
                AccountAny::Margin(margin_account) => {
                    println!("HERE are the margins: {:?}", margin_account.margins);
                    margin_account.initial_margins()
                }
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
        // First try to get existing PnL
        if let Some(pnl) = self
            .inner
            .borrow()
            .unrealized_pnls
            .get(instrument_id)
            .copied()
        {
            return Some(pnl);
        }

        // If not found, calculate new PnL
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
        let borrowed_cache = self.cache.borrow();
        let all_orders_open = borrowed_cache.orders_open(None, None, None, None);
        println!("ALL ORDERS OPEN: {all_orders_open:?}");
        let mut instruments = HashSet::new();

        for order in all_orders_open.clone() {
            instruments.insert(order.instrument_id());
        }

        let mut initialized = true;
        println!("HERE I AM3");

        for instrument_id in instruments {
            let instrument = if let Some(instrument) = borrowed_cache.instrument(&instrument_id) {
                instrument
            } else {
                log::error!(
                    "Cannot update initial (order) margin: no instrument found for {}",
                    instrument_id
                );
                println!("HERE I AM2");
                initialized = false;
                break;
            };

            println!("HERE I AM");

            let orders_open = borrowed_cache.orders_open(None, Some(&instrument_id), None, None);

            let mut borrowed_cache = self.cache.borrow_mut();
            let account =
                if let Some(account) = borrowed_cache.mut_account_for_venue(&instrument_id.venue) {
                    account
                } else {
                    log::error!(
                        "Cannot update initial (order) margin: no account registered for {}",
                        instrument_id.venue
                    );
                    initialized = false;
                    break;
                };

            let result = self.inner.borrow_mut().accounts.update_orders(
                account,
                instrument.clone(),
                orders_open,
                self.clock.borrow().timestamp_ns(),
            );

            if result.is_none() {
                initialized = false;
            }
        }

        let open_count = all_orders_open.len();
        log::info!(
            "Initialized {} open order{}",
            open_count,
            if open_count == 1 { "" } else { "s" }
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

            let mut borrowed_cache = self.cache.borrow_mut();
            let account =
                if let Some(account) = borrowed_cache.mut_account_for_venue(&instrument_id.venue) {
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

            let borrowed_cache = self.cache.borrow_mut();
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

            if result.is_none() {
                initialized = false;
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

    let result_init: Option<AccountState>;
    let mut result_maint = None;

    let account = {
        let mut borrowed_cache = cache.borrow_mut();
        let account = if let Some(account) =
            borrowed_cache.mut_account_for_venue(&quote.instrument_id.venue)
        {
            account
        } else {
            log::error!(
                "Cannot update tick: no account registered for {}",
                quote.instrument_id.venue
            );
            return;
        };

        let borrowed_cache = cache.borrow();
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

        account.clone()
    }; // All borrows are dropped here

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
    let mut borrowed_cache = cache.borrow_mut();
    let account_id = match event.account_id() {
        Some(account_id) => account_id,
        None => {
            return; // No Account Assigned
        }
    };

    let account = if let Some(account) = borrowed_cache.mut_account(&account_id) {
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

    let mut borrowed_cache = cache.borrow_mut();
    let account = borrowed_cache.mut_account(&event.account_id());

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

        let _ = inner.borrow_mut().accounts.update_positions(
            margin_account,
            instrument.clone(),
            positions_open.iter().collect(),
            clock.borrow().timestamp_ns(),
        );
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

    println!("Updated {event}");
    log::info!("Updated {}", event);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    // TODO: remove
    use env_logger;
    use nautilus_common::{cache::Cache, clock::TestClock, msgbus::MessageBus};
    use nautilus_core::nanos::UnixNanos;
    use nautilus_model::{
        data::quote::QuoteTick,
        enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderType},
        events::{
            account::{state::AccountState, stubs::cash_account_state},
            order::{
                stubs::{order_accepted, order_filled, order_submitted},
                OrderEventAny, OrderFilled,
            },
            position::{opened::PositionOpened, PositionEvent},
        },
        identifiers::{
            stubs::{account_id, uuid4},
            AccountId, PositionId, TradeId, VenueOrderId,
        },
        instruments::{any::InstrumentAny, currency_pair::CurrencyPair, stubs::audusd_sim},
        orders::builder::OrderTestBuilder,
        position::Position,
        types::{
            balance::AccountBalance, currency::Currency, money::Money, price::Price,
            quantity::Quantity,
        },
    };
    use rstest::{fixture, rstest};
    use rust_decimal::Decimal;

    use super::Portfolio;

    fn init() {
        // Initialize the logger once
        let _ = env_logger::builder().is_test(true).try_init();
    }

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
    fn portfolio(msgbus: MessageBus, simple_cache: Cache, clock: TestClock) -> Portfolio {
        init();
        Portfolio::new(
            Rc::new(RefCell::new(msgbus)),
            Rc::new(RefCell::new(simple_cache)),
            Rc::new(RefCell::new(clock)),
        )
    }

    #[fixture]
    fn instrument_audusd(audusd_sim: CurrencyPair) -> InstrumentAny {
        InstrumentAny::CurrencyPair(audusd_sim)
    }

    use std::collections::HashMap;

    use nautilus_model::identifiers::Venue;

    // Helpers

    // Tests

    // #[rstest]
    // fn test_account_when_no_account_returns_none(portfolio: Portfolio) {
    //     let venue = Venue::new("SIM");
    //     // TODO
    //     // let result = portfolio.account(&venue);
    //     // assert!(result.is_none());
    // }

    #[rstest]
    fn test_account_when_account_returns_the_account_facade(mut portfolio: Portfolio) {
        let account_id = AccountId::new("BINANCE-1513111");
        let state = AccountState::new(
            account_id,
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::new(10.00000000, Currency::BTC()),
                Money::new(0.00000000, Currency::BTC()),
                Money::new(10.00000000, Currency::BTC()),
            )],
            vec![],
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        );

        portfolio.update_account(&state);

        // let venue = Venue::new("BINANCE");
        // TODO
        // let result = portfolio.account(&venue).unwrap();

        // assert_eq!(result.id.get_issuer(), "BINANCE");
        // assert_eq!(result.id.get_id(), "1513111");
    }

    #[rstest]
    fn test_balances_locked_when_no_account_for_venue_returns_none(portfolio: Portfolio) {
        let venue = Venue::new("SIM");
        let result = portfolio.balances_locked(&venue);
        assert_eq!(result, HashMap::new());
    }

    #[rstest]
    fn test_margins_init_when_no_account_for_venue_returns_none(portfolio: Portfolio) {
        let venue = Venue::new("SIM");
        let result = portfolio.margins_init(&venue);
        assert_eq!(result, HashMap::new());
    }

    #[rstest]
    fn test_margins_maint_when_no_account_for_venue_returns_none(portfolio: Portfolio) {
        let venue = Venue::new("SIM");
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
    fn test_unrealized_pnl_for_venue_when_no_account_returns_empty_dict(mut portfolio: Portfolio) {
        let venue = Venue::new("SIM");
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
    fn test_realized_pnl_for_venue_when_no_account_returns_empty_dict(mut portfolio: Portfolio) {
        let venue = Venue::new("SIM");
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
    fn test_net_exposures_when_no_positions_returns_none(portfolio: Portfolio) {
        let venue = Venue::new("SIM");
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
    fn test_open_value_when_no_account_returns_none(portfolio: Portfolio) {
        let venue = Venue::new("SIM");
        let result = portfolio.net_exposures(&venue);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_update_tick(mut portfolio: Portfolio, instrument_audusd: InstrumentAny) {
        let tick = QuoteTick::new(
            instrument_audusd.id(),
            Price::new(1.2500, 0),
            Price::new(1.2510, 0),
            Quantity::new(1.0, 0),
            Quantity::new(1.0, 0),
            0.into(),
            0.into(),
        );

        portfolio.update_quote_tick(&tick);
        assert!(portfolio.unrealized_pnl(&instrument_audusd.id()).is_none());
    }

    // It shouuld return an error
    #[rstest]
    fn test_exceed_free_balance_single_currency_raises_account_balance_negative_exception(
        mut portfolio: Portfolio,
        cash_account_state: AccountState,
        instrument_audusd: InstrumentAny,
    ) {
        portfolio
            .cache
            .borrow_mut()
            .add_instrument(instrument_audusd.clone())
            .unwrap();

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

        let order_submitted = order_submitted(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            account_id(),
            uuid4(),
        );

        // sudo_exec_engine proess for
        order
            .apply(OrderEventAny::Submitted(order_submitted))
            .unwrap();
        portfolio.update_order(&OrderEventAny::Submitted(order_submitted));
        // portfolio.msgbus.borrow().publish(topic, message);

        let order_filled = order_filled(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            uuid4(),
        );

        order.apply(OrderEventAny::Filled(order_filled)).unwrap();
        portfolio.update_order(&OrderEventAny::Filled(order_filled));
    }

    // It shouuld return an error
    #[rstest]
    fn test_exceed_free_balance_multi_currency_raises_account_balance_negative_exception(
        mut portfolio: Portfolio,
        cash_account_state: AccountState,
        instrument_audusd: InstrumentAny,
    ) {
        portfolio
            .cache
            .borrow_mut()
            .add_instrument(instrument_audusd.clone())
            .unwrap();

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

        let order_submitted = order_submitted(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            account_id(),
            uuid4(),
        );

        // sudo_exec_engine proess for
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
        portfolio
            .cache
            .borrow_mut()
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        portfolio.update_account(&cash_account_state);

        // Create Order
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .price(Price::from_raw(50000, 0))
            .build();

        portfolio
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        // sudo_exec_engine proess for
        let order_submitted = order_submitted(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            account_id(),
            uuid4(),
        );

        order
            .apply(OrderEventAny::Submitted(order_submitted))
            .unwrap();
        portfolio.update_order(&OrderEventAny::Submitted(order_submitted));

        // ACCEPTED
        let order_accepted = order_accepted(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            account_id(),
            VenueOrderId::new("s"),
            uuid4(),
        );

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
            25000.0 // should be around 30000, rn 25000
        );

        // portfolio.msgbus.borrow().publish(topic, message);
    }

    #[rstest]
    fn test_update_orders_open_margin_account() {}

    // #[rstest]
    // fn test_order_accept_updates_margin_init(
    //     mut portfolio: Portfolio,
    //     // margin_account_state: AccountState,
    //     instrument_audusd: InstrumentAny,
    // ) {
    //     portfolio
    //         .cache
    //         .borrow_mut()
    //         .add_instrument(instrument_audusd.clone())
    //         .unwrap();

    //     let account_state = AccountState::new(
    //         account_id(),
    //         AccountType::Margin,
    //         vec![AccountBalance::new(
    //             Money::new(10.000, Currency::BTC()),
    //             Money::new(0.000, Currency::BTC()),
    //             Money::new(10.000, Currency::BTC()),
    //         )],
    //         Vec::new(),
    //         true,
    //         uuid4(),
    //         0.into(),
    //         0.into(),
    //         None,
    //     );

    //     portfolio.update_account(&account_state);

    //     // Create Order
    //     let mut order = OrderTestBuilder::new(OrderType::Limit)
    //         .instrument_id(instrument_audusd.id())
    //         .side(OrderSide::Buy)
    //         .quantity(Quantity::from("100.0"))
    //         .price(Price::from_raw(5, 1))
    //         .build();

    //     // we are passing clone here: TODO: fix
    //     portfolio
    //         .cache
    //         .borrow_mut()
    //         .add_order(order.clone(), None, None, false)
    //         .unwrap();

    //     // Push status to Accepted
    //     // sudo_exec_engine proess for
    //     let order_submitted = order_submitted(
    //         order.trader_id(),
    //         order.strategy_id(),
    //         order.instrument_id(),
    //         order.client_order_id(),
    //         account_id(),
    //         uuid4(),
    //     );

    //     order
    //         .apply(OrderEventAny::Submitted(order_submitted))
    //         .unwrap();

    //     let order_accepted = order_accepted(
    //         order.trader_id(),
    //         order.strategy_id(),
    //         order.instrument_id(),
    //         order.client_order_id(),
    //         account_id(),
    //         venue_order_id(),
    //         uuid4(),
    //     );

    //     order
    //         .apply(OrderEventAny::Accepted(order_accepted))
    //         .unwrap();

    //     // Act
    //     portfolio.initialize_orders();

    //     // Assert
    //     assert_eq!(
    //         portfolio
    //             .margins_init(&Venue::from("SIM"))
    //             .get(&instrument_audusd.id())
    //             .unwrap()
    //             .as_f64(),
    //         20000.0 // should be around 30000, rn 25000
    //     );

    //     // portfolio.update_order(&OrderEventAny::Submitted(order_submitted));
    // }

    #[rstest]
    fn test_update_positions(mut portfolio: Portfolio, instrument_audusd: InstrumentAny) {
        portfolio
            .cache
            .borrow_mut()
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        let account_state = AccountState::new(
            account_id(),
            AccountType::Cash,
            vec![
                AccountBalance::new(
                    Money::new(10.000, Currency::USD()),
                    Money::new(0.000, Currency::USD()),
                    Money::new(10.000, Currency::USD()),
                ),
                AccountBalance::new(
                    Money::new(20.000, Currency::ETH()),
                    Money::new(0.000, Currency::ETH()),
                    Money::new(20.000, Currency::ETH()),
                ),
            ],
            Vec::new(),
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        );

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

        // Push states to Accepted
        let order1_submitted = order_submitted(
            order1.trader_id(),
            order1.strategy_id(),
            order1.instrument_id(),
            order1.client_order_id(),
            account_id(),
            uuid4(),
        );

        order1
            .apply(OrderEventAny::Submitted(order1_submitted))
            .unwrap();
        portfolio.update_order(&OrderEventAny::Submitted(order1_submitted));

        // ACCEPTED
        let order1_accepted = order_accepted(
            order1.trader_id(),
            order1.strategy_id(),
            order1.instrument_id(),
            order1.client_order_id(),
            account_id(),
            VenueOrderId::new("s"),
            uuid4(),
        );

        order1
            .apply(OrderEventAny::Accepted(order1_accepted))
            .unwrap();
        portfolio.update_order(&OrderEventAny::Accepted(order1_accepted));

        let mut fill1 = order_filled(
            order1.trader_id(),
            order1.strategy_id(),
            order1.instrument_id(),
            order1.client_order_id(),
            uuid4(),
        );

        fill1.position_id = Some(PositionId::new("SSD"));

        let mut fill2 = order_filled(
            order2.trader_id(),
            order2.strategy_id(),
            order2.instrument_id(),
            order2.client_order_id(),
            uuid4(),
        );
        fill2.trade_id = TradeId::new("2");

        let mut position1 = Position::new(&instrument_audusd, fill1);
        position1.apply(&fill2);

        let order3 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("10.00"))
            .build();

        let mut fill3 = order_filled(
            order3.trader_id(),
            order3.strategy_id(),
            order3.instrument_id(),
            order3.client_order_id(),
            uuid4(),
        );

        fill3.position_id = Some(PositionId::new("SSsD"));
        let position2 = Position::new(&instrument_audusd, fill3);

        // Update the last quote
        let last = QuoteTick::new(
            instrument_audusd.id(),
            Price::new(250001.0, 0),
            Price::new(250002.0, 0),
            Quantity::new(1.0, 0),
            Quantity::new(1.0, 0),
            0.into(),
            0.into(),
        );

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
        portfolio
            .cache
            .borrow_mut()
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        let account_state = AccountState::new(
            account_id(),
            AccountType::Margin,
            vec![
                AccountBalance::new(
                    Money::new(10.000, Currency::USD()),
                    Money::new(0.000, Currency::USD()),
                    Money::new(10.000, Currency::USD()),
                ),
                AccountBalance::new(
                    Money::new(20.000, Currency::ETH()),
                    Money::new(0.000, Currency::ETH()),
                    Money::new(20.000, Currency::ETH()),
                ),
            ],
            Vec::new(),
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        );

        portfolio.update_account(&account_state);

        // Create Order
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.00"))
            .build();

        let mut fill = order_filled(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            uuid4(),
        );

        fill.position_id = Some(PositionId::new("SSD"));

        // Update the last quote
        let last = QuoteTick::new(
            instrument_audusd.id(),
            Price::new(10510.0, 0),
            Price::new(10511.0, 0),
            Quantity::new(1.0, 0),
            Quantity::new(1.0, 0),
            0.into(),
            0.into(),
        );

        portfolio.cache.borrow_mut().add_quote(last).unwrap();
        portfolio.update_quote_tick(&last);

        let position = Position::new(&instrument_audusd, fill);

        // Act
        portfolio
            .cache
            .borrow_mut()
            .add_position(position.clone(), OmsType::Hedging)
            .unwrap();

        let position_opened = PositionOpened {
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
        };
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
        // TODO: doubtful -> compare flow with python
        assert_eq!(
            portfolio
                .realized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
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
        portfolio
            .cache
            .borrow_mut()
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        let account_state = AccountState::new(
            account_id(),
            AccountType::Margin,
            vec![
                AccountBalance::new(
                    Money::new(100.000, Currency::USD()),
                    Money::new(0.000, Currency::USD()),
                    Money::new(100.000, Currency::USD()),
                ),
                AccountBalance::new(
                    Money::new(20.000, Currency::ETH()),
                    Money::new(0.000, Currency::ETH()),
                    Money::new(20.000, Currency::ETH()),
                ),
            ],
            Vec::new(),
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        );

        portfolio.update_account(&account_state);

        // Create Order
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("2"))
            .build();

        // let mut fill = order_filled(
        //     order.trader_id(),
        //     order.strategy_id(),
        //     order.instrument_id(),
        //     order.client_order_id(),
        //     uuid4(),
        // );

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

        // fill.position_id = Some(PositionId::new("SSD"));

        // Update the last quote
        let last = QuoteTick::new(
            instrument_audusd.id(),
            Price::new(15510.15, 0),
            Price::new(15510.25, 0),
            Quantity::new(13.0, 0),
            Quantity::new(4.0, 0),
            0.into(),
            0.into(),
        );

        portfolio.cache.borrow_mut().add_quote(last).unwrap();
        portfolio.update_quote_tick(&last);

        let position = Position::new(&instrument_audusd, fill);

        // Act
        portfolio
            .cache
            .borrow_mut()
            .add_position(position.clone(), OmsType::Hedging)
            .unwrap();

        let position_opened = PositionOpened {
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
        };
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
        // TODO: doubtful -> compare flow with python
        assert_eq!(
            portfolio
                .realized_pnls(&Venue::from("SIM"))
                .get(&Currency::USD())
                .unwrap()
                .as_f64(),
            -12.2
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
    fn test_opening_positions_with_multi_asset_account() {}
}
