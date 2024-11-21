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

// Under development
// improve error handling: TODO

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

pub struct Portfolio {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    accounts: AccountsManager,
    analyzer: PortfolioAnalyzer,
    unrealized_pnls: HashMap<InstrumentId, Money>,
    realized_pnls: HashMap<InstrumentId, Money>,
    net_positions: HashMap<InstrumentId, Decimal>,
    pending_calcs: HashSet<InstrumentId>,
    initialized: bool,
}

impl Portfolio {
    pub fn new(
        msgbus: Rc<RefCell<MessageBus>>,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> Self {
        let mut analyzer = PortfolioAnalyzer::new();

        // Register default statistics
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

        let portfolio = Self {
            clock: clock.clone(),
            cache: cache.clone(),
            msgbus: msgbus.clone(),
            accounts: AccountsManager::new(clock, cache.clone()),
            analyzer,
            unrealized_pnls: HashMap::new(),
            realized_pnls: HashMap::new(),
            net_positions: HashMap::new(),
            pending_calcs: HashSet::new(),
            initialized: false,
        };

        let portfolio_rc = Rc::new(RefCell::new(portfolio));
        Self::register_message_handlers(&msgbus, &cache, &portfolio_rc);

        Rc::try_unwrap(portfolio_rc)
            .map_err(|_| "Failed to unwrap Portfolio")
            .unwrap()
            .into_inner()
    }

    fn register_message_handlers(
        msgbus: &Rc<RefCell<MessageBus>>,
        cache: &Rc<RefCell<Cache>>,
        portfolio: &Rc<RefCell<Self>>,
    ) {
        // previously-used method
        let update_account_handler = {
            let cache = cache.clone();
            ShareableMessageHandler(Rc::new(UpdateAccountHandler {
                id: Ustr::from("TODO:FIX-ME"),
                callback: Box::new(move |event: &AccountState| {
                    let mut borrowed_cache = cache.borrow_mut();
                    if let Some(existing) = borrowed_cache.account(&event.account_id) {
                        let mut account = existing.clone();
                        account.apply(event.clone());
                        borrowed_cache.update_account(account.clone()).unwrap();
                    } else {
                        let account = AccountAny::from_events(vec![event.clone()]).unwrap();
                        borrowed_cache.add_account(account).unwrap();
                    };
                    log::info!("Updated account {}", event);
                }),
            }))
        };

        // new-experimental method
        let update_position_handler = {
            let portfolio = portfolio.clone();
            ShareableMessageHandler(Rc::new(UpdatePositionHandler {
                id: Ustr::from("TODO:FIX-ME"),
                callback: Box::new(move |event: &PositionEvent| {
                    portfolio.borrow_mut().update_position(event);
                }),
            }))
        };

        let update_quote_handler = {
            let portfolio = portfolio.clone();
            ShareableMessageHandler(Rc::new(UpdateQuoteTickHandler {
                id: Ustr::from("TODO:FIX-ME"),
                callback: Box::new(move |quote: &QuoteTick| {
                    portfolio.borrow_mut().update_quote_tick(quote);
                }),
            }))
        };

        let update_order_handler = {
            let portfolio = portfolio.clone();
            ShareableMessageHandler(Rc::new(UpdateOrderHandler {
                id: Ustr::from("TODO:FIX-ME"),
                callback: Box::new(move |event: &OrderEventAny| {
                    portfolio.borrow_mut().update_order(event);
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

        self.net_positions.clear();
        self.unrealized_pnls.clear();
        self.realized_pnls.clear();
        self.pending_calcs.clear();
        self.analyzer.reset();

        log::debug!("READY");
    }

    // -- QUERIES ---------------------------------------------------------------------------------

    #[must_use]
    pub const fn is_initialized(&self) -> bool {
        self.initialized
    }

    #[must_use]
    pub const fn analyzer(&self) -> &PortfolioAnalyzer {
        &self.analyzer
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
            if let Some(&pnl) = self.unrealized_pnls.get(&instrument_id) {
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
            if let Some(&pnl) = self.realized_pnls.get(&instrument_id) {
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
        self.unrealized_pnls
            .get(instrument_id)
            .copied()
            .or_else(|| {
                let pnl = self.calculate_unrealized_pnl(instrument_id)?;
                self.unrealized_pnls.insert(*instrument_id, pnl);
                Some(pnl)
            })
    }

    #[must_use]
    pub fn realized_pnl(&mut self, instrument_id: &InstrumentId) -> Option<Money> {
        self.realized_pnls.get(instrument_id).copied().or_else(|| {
            let pnl = self.calculate_realized_pnl(instrument_id)?;
            self.realized_pnls.insert(*instrument_id, pnl);
            Some(pnl)
        })
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
        self.net_positions
            .get(instrument_id)
            .copied()
            .unwrap_or(Decimal::ZERO)
    }

    #[must_use]
    pub fn is_net_long(&self, instrument_id: &InstrumentId) -> bool {
        self.net_positions
            .get(instrument_id)
            .copied()
            .map_or_else(|| false, |net_position| net_position > Decimal::ZERO)
    }

    #[must_use]
    pub fn is_net_short(&self, instrument_id: &InstrumentId) -> bool {
        self.net_positions
            .get(instrument_id)
            .copied()
            .map_or_else(|| false, |net_position| net_position < Decimal::ZERO)
    }

    #[must_use]
    pub fn is_flat(&self, instrument_id: &InstrumentId) -> bool {
        self.net_positions
            .get(instrument_id)
            .copied()
            .map_or_else(|| true, |net_position| net_position == Decimal::ZERO)
    }

    #[must_use]
    pub fn is_completely_flat(&self) -> bool {
        for net_position in self.net_positions.values() {
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
        let mut instruments = HashSet::new();

        for order in all_orders_open.clone() {
            instruments.insert(order.instrument_id());
        }

        let mut initialized = true;

        for instrument_id in instruments {
            let instrument = if let Some(instrument) = borrowed_cache.instrument(&instrument_id) {
                instrument
            } else {
                log::error!(
                    "Cannot update initial (order) margin: no instrument found for {}",
                    instrument_id
                );
                initialized = false;
                break;
            };

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

            let result = self.accounts.update_orders(
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
        self.initialized = initialized;
    }

    pub fn initialize_positions(&mut self) {
        self.unrealized_pnls.clear();
        self.realized_pnls.clear();
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

            let calculated_unrealized_pnl = self.calculate_unrealized_pnl(&instrument_id).unwrap();
            let calculated_realized_pnl = self.calculate_realized_pnl(&instrument_id).unwrap();

            self.unrealized_pnls
                .insert(instrument_id, calculated_unrealized_pnl);
            self.realized_pnls
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

            let result = self.accounts.update_positions(
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
        self.initialized = initialized;
        log::info!(
            "Initialized {} open position{}",
            open_count,
            if open_count == 1 { "" } else { "s" }
        );
    }

    pub fn update_quote_tick(&mut self, quote: &QuoteTick) {
        self.unrealized_pnls.remove(&quote.instrument_id);

        if self.initialized || !self.pending_calcs.contains(&quote.instrument_id) {
            return;
        }

        let result_init: Option<AccountState>;
        let mut result_maint = None;

        let account = {
            let mut borrowed_cache = self.cache.borrow_mut();
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

            let borrowed_cache = self.cache.borrow();
            let instrument =
                if let Some(instrument) = borrowed_cache.instrument(&quote.instrument_id) {
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

            result_init = self.accounts.update_orders(
                account,
                instrument.clone(),
                orders_open.iter().collect(),
                self.clock.borrow().timestamp_ns(),
            );

            if let AccountAny::Margin(margin_account) = account {
                result_maint = self.accounts.update_positions(
                    margin_account,
                    instrument,
                    positions_open.iter().collect(),
                    self.clock.borrow().timestamp_ns(),
                );
            }

            account.clone()
        }; // All borrows are dropped here

        let result_unrealized_pnl: Option<Money> =
            self.calculate_unrealized_pnl(&quote.instrument_id);

        if result_init.is_some()
            && (matches!(account, AccountAny::Cash(_))
                || (result_maint.is_some() && result_unrealized_pnl.is_some()))
        {
            self.pending_calcs.remove(&quote.instrument_id);
            if self.pending_calcs.is_empty() {
                self.initialized = true;
            }
        }
    }

    pub fn update_account(&mut self, event: &AccountState) {
        let mut borrowed_cache = self.cache.borrow_mut();

        if let Some(existing) = borrowed_cache.account(&event.account_id) {
            let mut account = existing.clone();
            account.apply(event.clone());
            borrowed_cache.update_account(account.clone()).unwrap();
        } else {
            let account = AccountAny::from_events(vec![event.clone()]).unwrap();
            borrowed_cache.add_account(account).unwrap();
        };

        log::info!("Updated {}", event);
    }

    pub fn update_order(&mut self, event: &OrderEventAny) {
        let mut borrowed_cache = self.cache.borrow_mut();
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

        let borrowed_cache = self.cache.borrow();
        let order = if let Some(order) = borrowed_cache.order(&event.client_order_id()) {
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

        let instrument =
            if let Some(instrument_id) = borrowed_cache.instrument(&event.instrument_id()) {
                instrument_id
            } else {
                log::error!(
                    "Cannot update order: no instrument found for {}",
                    event.instrument_id()
                );
                return;
            };

        // TODO: fix borrowing issue
        // if let OrderEventAny::Filled(order_filled) = event {
        //     let _ =
        //         self.accounts
        //             .update_balances(account.clone(), instrument.clone(), *order_filled);

        //     let unrealized_pnl = self.calculate_unrealized_pnl(&order_filled.instrument_id);
        //     self.unrealized_pnls
        //         .insert(event.instrument_id(), unrealized_pnl.unwrap());
        // }

        let orders_open =
            borrowed_cache.orders_open(None, Some(&event.instrument_id()), None, None);

        let account_state = self.accounts.update_orders(
            account,
            instrument.clone(),
            orders_open,
            self.clock.borrow().timestamp_ns(),
        );

        if let Some(account_state) = account_state {
            self.msgbus.borrow().publish(
                &Ustr::from(&format!("events.account.{}", account.id())),
                &account_state,
            );
        } else {
            log::debug!("Added pending calculation for {}", instrument.id());
            self.pending_calcs.insert(instrument.id());
        }

        log::debug!("Updated {}", event);
    }

    pub fn update_position(&mut self, event: &PositionEvent) {
        let instrument_id = event.instrument_id();

        let positions_open: Vec<Position> = {
            let borrowed_cache = self.cache.borrow();

            borrowed_cache
                .positions_open(None, Some(&instrument_id), None, None)
                .iter()
                .map(|o| (*o).clone())
                .collect()
        };

        self.update_net_position(&instrument_id, positions_open.clone());

        let calculated_unrealized_pnl = self.calculate_unrealized_pnl(&instrument_id).unwrap();
        let calculated_realized_pnl = self.calculate_realized_pnl(&instrument_id).unwrap();

        self.unrealized_pnls
            .insert(event.instrument_id(), calculated_unrealized_pnl);
        self.realized_pnls
            .insert(event.instrument_id(), calculated_realized_pnl);

        let mut borrowed_cache = self.cache.borrow_mut();
        let account = borrowed_cache.mut_account(&event.account_id());

        if let Some(AccountAny::Margin(margin_account)) = account {
            if !margin_account.calculate_account_state {
                return; // Nothing to calculate
            };

            let borrowed_cache = self.cache.borrow();
            let instrument = if let Some(instrument) = borrowed_cache.instrument(&instrument_id) {
                instrument
            } else {
                log::error!(
                    "Cannot update position: no instrument found for {}",
                    instrument_id
                );
                return;
            };

            let _ = self.accounts.update_positions(
                margin_account,
                instrument.clone(),
                positions_open.iter().collect(),
                self.clock.borrow().timestamp_ns(),
            );
        } else if account.is_none() {
            log::error!(
                "Cannot update position: no account registered for {}",
                event.account_id()
            );
        }
    }

    // -- INTERNAL --------------------------------------------------------------------------------

    fn update_net_position(&mut self, instrument_id: &InstrumentId, positions_open: Vec<Position>) {
        let mut net_position = Decimal::ZERO;

        for open_position in positions_open {
            net_position += Decimal::from_f64(open_position.signed_qty).unwrap_or(Decimal::ZERO);
        }

        let existing_position = self.net_position(instrument_id);
        if existing_position != net_position {
            self.net_positions.insert(*instrument_id, net_position);
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
                self.pending_calcs.insert(*instrument_id);
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
                    self.pending_calcs.insert(*instrument_id);
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
                    self.pending_calcs.insert(*instrument_id);
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
                    // todo: improve error
                    .expect("Fails to convert Decimal to f64")
            }
            None => 1.0, // No conversion needed
        }
    }
}
