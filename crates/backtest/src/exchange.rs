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

//! Provides a `SimulatedExchange` venue for backtesting on historical data.

use std::{
    cell::RefCell,
    collections::{BinaryHeap, VecDeque},
    fmt::Debug,
    rc::Rc,
};

use ahash::AHashMap;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::{Clock, TestClock},
    messages::execution::TradingCommand,
};
use nautilus_core::{
    UnixNanos,
    correctness::{FAILED, check_equal},
};
use nautilus_execution::{
    matching_core::OrderMatchInfo,
    matching_engine::{config::OrderMatchingEngineConfig, engine::OrderMatchingEngine},
    models::{fee::FeeModelAny, fill::FillModelAny, latency::LatencyModel},
};
use nautilus_model::{
    accounts::{AccountAny, margin_model::MarginModelAny},
    data::{
        Bar, Data, InstrumentClose, InstrumentStatus, OrderBookDelta, OrderBookDeltas,
        OrderBookDeltas_API, OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{AccountType, AggressorSide, BookType, OmsType},
    identifiers::{AccountId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    orders::{Order, OrderAny},
    types::{AccountBalance, Currency, Money, Price},
};
use rust_decimal::Decimal;

use crate::modules::{ExchangeContext, SimulationModule};

/// Represents commands with simulated network latency in a min-heap priority queue.
/// The commands are ordered by timestamp for FIFO processing, with the
/// earliest timestamp having the highest priority in the queue.
#[derive(Debug, Eq, PartialEq)]
struct InflightCommand {
    timestamp: UnixNanos,
    counter: u32,
    command: TradingCommand,
}

impl InflightCommand {
    const fn new(timestamp: UnixNanos, counter: u32, command: TradingCommand) -> Self {
        Self {
            timestamp,
            counter,
            command,
        }
    }
}

impl Ord for InflightCommand {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering for min-heap (earliest timestamp first then lowest counter)
        other
            .timestamp
            .cmp(&self.timestamp)
            .then_with(|| other.counter.cmp(&self.counter))
    }
}

impl PartialOrd for InflightCommand {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Simulated exchange venue for realistic trading execution during backtesting.
///
/// The `SimulatedExchange` provides a simulation of a trading venue,
/// including order matching engines, account management, and realistic execution
/// models. It maintains order books, processes market data, and executes trades
/// with configurable latency and fill models to accurately simulate real market
/// conditions during backtesting.
///
/// Key features:
/// - Multi-instrument order matching with realistic execution
/// - Configurable fee, fill, and latency models
/// - Support for various order types and execution options
/// - Account balance and position management
/// - Market data processing and order book maintenance
/// - Simulation modules for custom venue behaviors
pub struct SimulatedExchange {
    /// The venue identifier.
    pub id: Venue,
    /// The order management system type.
    pub oms_type: OmsType,
    /// The account type for the venue.
    pub account_type: AccountType,
    /// The optional base currency for single-currency accounts.
    pub base_currency: Option<Currency>,
    starting_balances: Vec<Money>,
    book_type: BookType,
    default_leverage: Decimal,
    exec_client: Option<Rc<dyn ExecutionClient>>,
    fee_model: FeeModelAny,
    fill_model: FillModelAny,
    latency_model: Option<Box<dyn LatencyModel>>,
    instruments: AHashMap<InstrumentId, InstrumentAny>,
    matching_engines: AHashMap<InstrumentId, OrderMatchingEngine>,
    settlement_prices: AHashMap<InstrumentId, Price>,
    leverages: AHashMap<InstrumentId, Decimal>,
    margin_model: Option<MarginModelAny>,
    modules: Vec<Box<dyn SimulationModule>>,
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    message_queue: VecDeque<TradingCommand>,
    inflight_queue: BinaryHeap<InflightCommand>,
    inflight_counter: AHashMap<UnixNanos, u32>,
    bar_execution: bool,
    bar_adaptive_high_low_ordering: bool,
    trade_execution: bool,
    liquidity_consumption: bool,
    reject_stop_orders: bool,
    support_gtd_orders: bool,
    support_contingent_orders: bool,
    use_position_ids: bool,
    use_random_ids: bool,
    use_reduce_only: bool,
    use_message_queue: bool,
    use_market_order_acks: bool,
    _allow_cash_borrowing: bool,
    frozen_account: bool,
    queue_position: bool,
    oto_full_trigger: bool,
    price_protection_points: u32,
}

impl Debug for SimulatedExchange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SimulatedExchange))
            .field("id", &self.id)
            .field("account_type", &self.account_type)
            .finish()
    }
}

impl SimulatedExchange {
    /// Creates a new [`SimulatedExchange`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `starting_balances` is empty.
    /// - `base_currency` is `Some` but `starting_balances` contains multiple currencies.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        venue: Venue,
        oms_type: OmsType,
        account_type: AccountType,
        starting_balances: Vec<Money>,
        base_currency: Option<Currency>,
        default_leverage: Decimal,
        leverages: AHashMap<InstrumentId, Decimal>,
        margin_model: Option<MarginModelAny>,
        modules: Vec<Box<dyn SimulationModule>>,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
        fill_model: FillModelAny,
        fee_model: FeeModelAny,
        book_type: BookType,
        latency_model: Option<Box<dyn LatencyModel>>,
        bar_execution: Option<bool>,
        bar_adaptive_high_low_ordering: Option<bool>,
        trade_execution: Option<bool>,
        liquidity_consumption: Option<bool>,
        reject_stop_orders: Option<bool>,
        support_gtd_orders: Option<bool>,
        support_contingent_orders: Option<bool>,
        use_position_ids: Option<bool>,
        use_random_ids: Option<bool>,
        use_reduce_only: Option<bool>,
        use_message_queue: Option<bool>,
        use_market_order_acks: Option<bool>,
        allow_cash_borrowing: Option<bool>,
        frozen_account: Option<bool>,
        queue_position: Option<bool>,
        oto_full_trigger: Option<bool>,
        price_protection_points: Option<u32>,
    ) -> anyhow::Result<Self> {
        if starting_balances.is_empty() {
            anyhow::bail!("Starting balances must be provided")
        }

        if base_currency.is_some() && starting_balances.len() > 1 {
            anyhow::bail!("single-currency account has multiple starting currencies")
        }
        Ok(Self {
            id: venue,
            oms_type,
            account_type,
            base_currency,
            starting_balances,
            book_type,
            default_leverage,
            exec_client: None,
            fee_model,
            fill_model,
            latency_model,
            instruments: AHashMap::new(),
            matching_engines: AHashMap::new(),
            settlement_prices: AHashMap::new(),
            leverages,
            margin_model,
            modules,
            clock,
            cache,
            message_queue: VecDeque::new(),
            inflight_queue: BinaryHeap::new(),
            inflight_counter: AHashMap::new(),
            bar_execution: bar_execution.unwrap_or(true),
            bar_adaptive_high_low_ordering: bar_adaptive_high_low_ordering.unwrap_or(false),
            trade_execution: trade_execution.unwrap_or(true),
            liquidity_consumption: liquidity_consumption.unwrap_or(true),
            reject_stop_orders: reject_stop_orders.unwrap_or(true),
            support_gtd_orders: support_gtd_orders.unwrap_or(true),
            support_contingent_orders: support_contingent_orders.unwrap_or(true),
            use_position_ids: use_position_ids.unwrap_or(true),
            use_random_ids: use_random_ids.unwrap_or(false),
            use_reduce_only: use_reduce_only.unwrap_or(true),
            use_message_queue: use_message_queue.unwrap_or(true),
            use_market_order_acks: use_market_order_acks.unwrap_or(false),
            _allow_cash_borrowing: allow_cash_borrowing.unwrap_or(false),
            frozen_account: frozen_account.unwrap_or(false),
            queue_position: queue_position.unwrap_or(false),
            oto_full_trigger: oto_full_trigger.unwrap_or(false),
            price_protection_points: price_protection_points.unwrap_or(0),
        })
    }

    /// Registers the execution client for the exchange.
    pub fn register_client(&mut self, client: Rc<dyn ExecutionClient>) {
        self.exec_client = Some(client);
    }

    /// Sets the fill model for the exchange.
    pub fn set_fill_model(&mut self, fill_model: FillModelAny) {
        for matching_engine in self.matching_engines.values_mut() {
            matching_engine.set_fill_model(fill_model.clone());
            log::info!(
                "Setting fill model for {} to {}",
                matching_engine.venue,
                self.fill_model
            );
        }
        self.fill_model = fill_model;
    }

    /// Sets the latency model for the exchange.
    pub fn set_latency_model(&mut self, latency_model: Box<dyn LatencyModel>) {
        self.latency_model = Some(latency_model);
    }

    /// Sets the settlement price for the given instrument.
    pub fn set_settlement_price(&mut self, instrument_id: InstrumentId, price: Price) {
        self.settlement_prices.insert(instrument_id, price);
    }

    pub fn initialize_account(&mut self) {
        self.generate_fresh_account_state();
    }

    /// Loads non-emulated open orders from the cache into matching engines.
    pub fn load_open_orders(&mut self) {
        let mut open_orders: Vec<(OrderAny, AccountId)> = {
            let cache = self.cache.as_ref().borrow();
            cache
                .orders_open(Some(&self.id), None, None, None, None)
                .into_iter()
                .filter(|order| !order.is_emulated())
                .filter_map(|order| {
                    order
                        .account_id()
                        .map(|account_id| (order.clone(), account_id))
                })
                .collect()
        };

        // Sort for deterministic insertion order
        open_orders.sort_by(|(a, _), (b, _)| {
            a.ts_init()
                .cmp(&b.ts_init())
                .then_with(|| a.client_order_id().cmp(&b.client_order_id()))
        });

        for (mut order, account_id) in open_orders {
            let instrument_id = order.instrument_id();
            if let Some(matching_engine) = self.matching_engines.get_mut(&instrument_id) {
                matching_engine.process_order(&mut order, account_id);
            } else {
                log::error!(
                    "No matching engine for {instrument_id} to load open order {}",
                    order.client_order_id()
                );
            }
        }
    }

    /// Adds an instrument to the simulated exchange and initializes its matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the exchange account type is `Cash` and the instrument is a `CryptoPerpetual` or `CryptoFuture`.
    ///
    /// # Panics
    ///
    /// Panics if the instrument cannot be added to the exchange.
    pub fn add_instrument(&mut self, instrument: InstrumentAny) -> anyhow::Result<()> {
        check_equal(
            &instrument.id().venue,
            &self.id,
            "Venue of instrument id",
            "Venue of simulated exchange",
        )
        .expect(FAILED);

        if self.account_type == AccountType::Cash
            && (matches!(instrument, InstrumentAny::CryptoPerpetual(_))
                || matches!(instrument, InstrumentAny::CryptoFuture(_))
                || matches!(instrument, InstrumentAny::PerpetualContract(_)))
        {
            anyhow::bail!("Cash account cannot trade futures or perpetuals")
        }

        self.instruments.insert(instrument.id(), instrument.clone());

        let price_protection = if self.price_protection_points == 0 {
            None
        } else {
            Some(self.price_protection_points)
        };

        let matching_engine_config = OrderMatchingEngineConfig::new(
            self.bar_execution,
            self.bar_adaptive_high_low_ordering,
            self.trade_execution,
            self.liquidity_consumption,
            self.reject_stop_orders,
            self.support_gtd_orders,
            self.support_contingent_orders,
            self.use_position_ids,
            self.use_random_ids,
            self.use_reduce_only,
            self.use_market_order_acks,
            self.queue_position,
            self.oto_full_trigger,
        )
        .with_price_protection_points(price_protection);
        let instrument_id = instrument.id();
        let matching_engine = OrderMatchingEngine::new(
            instrument,
            self.instruments.len() as u32,
            self.fill_model.clone(),
            self.fee_model.clone(),
            self.book_type,
            self.oms_type,
            self.account_type,
            self.clock.clone(),
            Rc::clone(&self.cache),
            matching_engine_config,
        );
        self.matching_engines.insert(instrument_id, matching_engine);

        log::info!("Added instrument {instrument_id} and created matching engine");
        Ok(())
    }

    /// Returns the best bid price for the given instrument, if available.
    #[must_use]
    pub fn best_bid_price(&self, instrument_id: InstrumentId) -> Option<Price> {
        self.matching_engines
            .get(&instrument_id)
            .and_then(OrderMatchingEngine::best_bid_price)
    }

    /// Returns the best ask price for the given instrument, if available.
    #[must_use]
    pub fn best_ask_price(&self, instrument_id: InstrumentId) -> Option<Price> {
        self.matching_engines
            .get(&instrument_id)
            .and_then(OrderMatchingEngine::best_ask_price)
    }

    /// Returns a reference to the order book for the given instrument, if available.
    pub fn get_book(&self, instrument_id: InstrumentId) -> Option<&OrderBook> {
        self.matching_engines
            .get(&instrument_id)
            .map(OrderMatchingEngine::get_book)
    }

    /// Returns a reference to the matching engine for the given instrument, if available.
    #[must_use]
    pub fn get_matching_engine(
        &self,
        instrument_id: &InstrumentId,
    ) -> Option<&OrderMatchingEngine> {
        self.matching_engines.get(instrument_id)
    }

    /// Returns a reference to all matching engines keyed by instrument ID.
    #[must_use]
    pub const fn get_matching_engines(&self) -> &AHashMap<InstrumentId, OrderMatchingEngine> {
        &self.matching_engines
    }

    /// Returns all order books keyed by instrument ID.
    #[must_use]
    pub fn get_books(&self) -> AHashMap<InstrumentId, OrderBook> {
        let mut books = AHashMap::new();
        for (instrument_id, matching_engine) in &self.matching_engines {
            books.insert(*instrument_id, matching_engine.get_book().clone());
        }
        books
    }

    /// Returns all open orders, optionally filtered by instrument ID.
    #[must_use]
    pub fn get_open_orders(&self, instrument_id: Option<InstrumentId>) -> Vec<OrderMatchInfo> {
        instrument_id
            .and_then(|id| {
                self.matching_engines
                    .get(&id)
                    .map(OrderMatchingEngine::get_open_orders)
            })
            .unwrap_or_else(|| {
                self.matching_engines
                    .values()
                    .flat_map(OrderMatchingEngine::get_open_orders)
                    .collect()
            })
    }

    /// Returns all open bid orders, optionally filtered by instrument ID.
    #[must_use]
    pub fn get_open_bid_orders(&self, instrument_id: Option<InstrumentId>) -> Vec<OrderMatchInfo> {
        instrument_id
            .and_then(|id| {
                self.matching_engines
                    .get(&id)
                    .map(|engine| engine.get_open_bid_orders().to_vec())
            })
            .unwrap_or_else(|| {
                self.matching_engines
                    .values()
                    .flat_map(|engine| engine.get_open_bid_orders().to_vec())
                    .collect()
            })
    }

    /// Returns all open ask orders, optionally filtered by instrument ID.
    #[must_use]
    pub fn get_open_ask_orders(&self, instrument_id: Option<InstrumentId>) -> Vec<OrderMatchInfo> {
        instrument_id
            .and_then(|id| {
                self.matching_engines
                    .get(&id)
                    .map(|engine| engine.get_open_ask_orders().to_vec())
            })
            .unwrap_or_else(|| {
                self.matching_engines
                    .values()
                    .flat_map(|engine| engine.get_open_ask_orders().to_vec())
                    .collect()
            })
    }

    /// Returns the account for this exchange, if an execution client is registered.
    #[must_use]
    pub fn get_account(&self) -> Option<AccountAny> {
        self.exec_client
            .as_ref()
            .and_then(|client| client.get_account())
    }

    /// Returns a reference to the cache.
    #[must_use]
    pub fn cache(&self) -> &Rc<RefCell<Cache>> {
        &self.cache
    }

    /// Adjusts the account balance by the given amount.
    ///
    /// # Panics
    ///
    /// Panics if generating account state fails during adjustment.
    pub fn adjust_account(&mut self, adjustment: Money) {
        if self.frozen_account {
            // Nothing to adjust
            return;
        }

        if let Some(exec_client) = &self.exec_client {
            let venue = exec_client.venue();
            log::debug!("Adjusting account for venue {venue}");
            if let Some(account) = self.cache.borrow().account_for_venue(&venue) {
                match account.balance(Some(adjustment.currency)) {
                    Some(balance) => {
                        let mut current_balance = *balance;
                        current_balance.total = current_balance.total + adjustment;
                        current_balance.free = current_balance.free + adjustment;

                        let margins = match account {
                            AccountAny::Margin(margin_account) => margin_account.margins.clone(),
                            _ => AHashMap::new(),
                        };

                        if let Some(exec_client) = &self.exec_client {
                            exec_client
                                .generate_account_state(
                                    vec![current_balance],
                                    margins.values().copied().collect(),
                                    true,
                                    self.clock.borrow().timestamp_ns(),
                                )
                                .unwrap();
                        }
                    }
                    None => {
                        log::error!(
                            "Cannot adjust account: no balance for currency {}",
                            adjustment.currency
                        );
                    }
                }
            } else {
                log::error!("Cannot adjust account: no account for venue {venue}");
            }
        }
    }

    /// Returns whether there are pending commands at or before `ts_now`.
    #[must_use]
    pub fn has_pending_commands(&self, ts_now: UnixNanos) -> bool {
        if !self.message_queue.is_empty() {
            return true;
        }
        self.inflight_queue
            .peek()
            .is_some_and(|inflight| inflight.timestamp <= ts_now)
    }

    /// Iterates all matching engines so newly submitted orders can match
    /// against the current market state.
    pub fn iterate_matching_engines(&mut self, ts_now: UnixNanos) {
        for matching_engine in self.matching_engines.values_mut() {
            matching_engine.iterate(ts_now, AggressorSide::NoAggressor);
        }
    }

    /// Advances the exchange clock to the given timestamp so that any event
    /// generators (modules, account state) see the correct time even when
    /// no commands are pending.
    ///
    /// # Panics
    ///
    /// Panics if the clock is not a [`TestClock`].
    pub fn set_clock_time(&self, ts_now: UnixNanos) {
        let mut clock_ref = self.clock.borrow_mut();
        let test_clock = clock_ref
            .as_any_mut()
            .downcast_mut::<TestClock>()
            .expect("SimulatedExchange requires TestClock");
        test_clock.set_time(ts_now);
    }

    /// Sends a trading command to the exchange for processing.
    pub fn send(&mut self, command: TradingCommand) {
        if !self.use_message_queue {
            self.process_trading_command(command);
        } else if self.latency_model.is_none() {
            self.message_queue.push_back(command);
        } else {
            let (timestamp, counter) = self.generate_inflight_command(&command);
            self.inflight_queue
                .push(InflightCommand::new(timestamp, counter, command));
        }
    }

    fn generate_inflight_command(&mut self, command: &TradingCommand) -> (UnixNanos, u32) {
        if let Some(latency_model) = &self.latency_model {
            let ts = match command {
                TradingCommand::SubmitOrder(_) | TradingCommand::SubmitOrderList(_) => {
                    command.ts_init() + latency_model.get_insert_latency()
                }
                TradingCommand::ModifyOrder(_) => {
                    command.ts_init() + latency_model.get_update_latency()
                }
                TradingCommand::CancelOrder(_)
                | TradingCommand::CancelAllOrders(_)
                | TradingCommand::BatchCancelOrders(_) => {
                    command.ts_init() + latency_model.get_delete_latency()
                }
                _ => panic!("Cannot handle command: {command:?}"),
            };

            let counter = self
                .inflight_counter
                .entry(ts)
                .and_modify(|e| *e += 1)
                .or_insert(1);

            (ts, *counter)
        } else {
            panic!("Latency model should be initialized");
        }
    }

    /// Processes a single order book delta.
    ///
    /// # Panics
    ///
    /// Panics if adding a missing instrument during delta processing fails.
    pub fn process_order_book_delta(&mut self, delta: OrderBookDelta) {
        for module in &self.modules {
            module.pre_process(&Data::Delta(delta));
        }

        if !self.matching_engines.contains_key(&delta.instrument_id) {
            let instrument = {
                let cache = self.cache.as_ref().borrow();
                cache.instrument(&delta.instrument_id).cloned()
            };

            if let Some(instrument) = instrument {
                self.add_instrument(instrument).unwrap();
            } else {
                panic!(
                    "No matching engine found for instrument {}",
                    delta.instrument_id
                );
            }
        }

        if let Some(matching_engine) = self.matching_engines.get_mut(&delta.instrument_id) {
            matching_engine.process_order_book_delta(&delta).unwrap();
        } else {
            panic!("Matching engine should be initialized");
        }
    }

    /// Processes a batch of order book deltas.
    ///
    /// # Panics
    ///
    /// Panics if adding a missing instrument during deltas processing fails.
    pub fn process_order_book_deltas(&mut self, deltas: OrderBookDeltas) {
        for module in &self.modules {
            module.pre_process(&Data::Deltas(OrderBookDeltas_API::new(deltas.clone())));
        }

        if !self.matching_engines.contains_key(&deltas.instrument_id) {
            let instrument = {
                let cache = self.cache.as_ref().borrow();
                cache.instrument(&deltas.instrument_id).cloned()
            };

            if let Some(instrument) = instrument {
                self.add_instrument(instrument).unwrap();
            } else {
                panic!(
                    "No matching engine found for instrument {}",
                    deltas.instrument_id
                );
            }
        }

        if let Some(matching_engine) = self.matching_engines.get_mut(&deltas.instrument_id) {
            matching_engine.process_order_book_deltas(&deltas).unwrap();
        } else {
            panic!("Matching engine should be initialized");
        }
    }

    /// Processes an L2 order book depth snapshot.
    ///
    /// # Panics
    ///
    /// Panics if adding a missing instrument during depth10 processing fails.
    pub fn process_order_book_depth10(&mut self, depth: &OrderBookDepth10) {
        for module in &self.modules {
            module.pre_process(&Data::Depth10(Box::new(*depth)));
        }

        if !self.matching_engines.contains_key(&depth.instrument_id) {
            let instrument = {
                let cache = self.cache.as_ref().borrow();
                cache.instrument(&depth.instrument_id).cloned()
            };

            if let Some(instrument) = instrument {
                self.add_instrument(instrument).unwrap();
            } else {
                panic!(
                    "No matching engine found for instrument {}",
                    depth.instrument_id
                );
            }
        }

        if let Some(matching_engine) = self.matching_engines.get_mut(&depth.instrument_id) {
            matching_engine.process_order_book_depth10(depth).unwrap();
        } else {
            panic!("Matching engine should be initialized");
        }
    }

    /// Processes a quote tick and updates the matching engine.
    ///
    /// # Panics
    ///
    /// Panics if adding a missing instrument during quote tick processing fails.
    pub fn process_quote_tick(&mut self, quote: &QuoteTick) {
        for module in &self.modules {
            module.pre_process(&Data::Quote(*quote));
        }

        if !self.matching_engines.contains_key(&quote.instrument_id) {
            let instrument = {
                let cache = self.cache.as_ref().borrow();
                cache.instrument(&quote.instrument_id).cloned()
            };

            if let Some(instrument) = instrument {
                self.add_instrument(instrument).unwrap();
            } else {
                panic!(
                    "No matching engine found for instrument {}",
                    quote.instrument_id
                );
            }
        }

        if let Some(matching_engine) = self.matching_engines.get_mut(&quote.instrument_id) {
            matching_engine.process_quote_tick(quote);
        } else {
            panic!("Matching engine should be initialized");
        }
    }

    /// Processes a trade tick and updates the matching engine.
    ///
    /// # Panics
    ///
    /// Panics if adding a missing instrument during trade tick processing fails.
    pub fn process_trade_tick(&mut self, trade: &TradeTick) {
        for module in &self.modules {
            module.pre_process(&Data::Trade(*trade));
        }

        if !self.matching_engines.contains_key(&trade.instrument_id) {
            let instrument = {
                let cache = self.cache.as_ref().borrow();
                cache.instrument(&trade.instrument_id).cloned()
            };

            if let Some(instrument) = instrument {
                self.add_instrument(instrument).unwrap();
            } else {
                panic!(
                    "No matching engine found for instrument {}",
                    trade.instrument_id
                );
            }
        }

        if let Some(matching_engine) = self.matching_engines.get_mut(&trade.instrument_id) {
            matching_engine.process_trade_tick(trade);
        } else {
            panic!("Matching engine should be initialized");
        }
    }

    /// Processes a bar and updates the matching engine.
    ///
    /// # Panics
    ///
    /// Panics if adding a missing instrument during bar processing fails.
    pub fn process_bar(&mut self, bar: Bar) {
        for module in &self.modules {
            module.pre_process(&Data::Bar(bar));
        }

        if !self.matching_engines.contains_key(&bar.instrument_id()) {
            let instrument = {
                let cache = self.cache.as_ref().borrow();
                cache.instrument(&bar.instrument_id()).cloned()
            };

            if let Some(instrument) = instrument {
                self.add_instrument(instrument).unwrap();
            } else {
                panic!(
                    "No matching engine found for instrument {}",
                    bar.instrument_id()
                );
            }
        }

        if let Some(matching_engine) = self.matching_engines.get_mut(&bar.instrument_id()) {
            matching_engine.process_bar(&bar);
        } else {
            panic!("Matching engine should be initialized");
        }
    }

    /// Processes an instrument status update.
    ///
    /// # Panics
    ///
    /// Panics if adding a missing instrument during instrument status processing fails.
    pub fn process_instrument_status(&mut self, status: InstrumentStatus) {
        if !self.matching_engines.contains_key(&status.instrument_id) {
            let instrument = {
                let cache = self.cache.as_ref().borrow();
                cache.instrument(&status.instrument_id).cloned()
            };

            if let Some(instrument) = instrument {
                self.add_instrument(instrument).unwrap();
            } else {
                panic!(
                    "No matching engine found for instrument {}",
                    status.instrument_id
                );
            }
        }

        if let Some(matching_engine) = self.matching_engines.get_mut(&status.instrument_id) {
            matching_engine.process_status(status.action);
        } else {
            panic!("Matching engine should be initialized");
        }
    }

    /// Processes an instrument close event.
    ///
    /// # Panics
    ///
    /// Panics if adding a missing instrument during instrument close processing fails.
    pub fn process_instrument_close(&mut self, close: InstrumentClose) {
        for module in &self.modules {
            module.pre_process(&Data::InstrumentClose(close));
        }

        if !self.matching_engines.contains_key(&close.instrument_id) {
            let instrument = {
                let cache = self.cache.as_ref().borrow();
                cache.instrument(&close.instrument_id).cloned()
            };

            if let Some(instrument) = instrument {
                self.add_instrument(instrument).unwrap();
            } else {
                panic!(
                    "No matching engine found for instrument {}",
                    close.instrument_id
                );
            }
        }

        if let Some(matching_engine) = self.matching_engines.get_mut(&close.instrument_id) {
            if let Some(price) = self.settlement_prices.get(&close.instrument_id) {
                matching_engine.set_settlement_price(*price);
            }
            matching_engine.process_instrument_close(close);
        } else {
            panic!("Matching engine should be initialized");
        }
    }

    /// Processes all pending inflight and queued trading commands up to `ts_now`.
    ///
    /// # Panics
    ///
    /// Panics if popping an inflight command fails during processing.
    pub fn process(&mut self, ts_now: UnixNanos) {
        // Clock is advanced by BacktestEngine::settle_venues before entering
        // the settlement loop, so we don't set it here.

        // Process inflight commands
        while let Some(inflight) = self.inflight_queue.peek() {
            if inflight.timestamp > ts_now {
                // Future commands remain in the queue
                break;
            }
            // We get the inflight command, remove it from the queue and process it
            let inflight = self.inflight_queue.pop().unwrap();
            self.process_trading_command(inflight.command);
        }

        // Process regular message queue
        while let Some(command) = self.message_queue.pop_front() {
            self.process_trading_command(command);
        }
    }

    /// Runs all simulation modules for the given timestamp.
    ///
    /// Must be called once per time step after all command queues have fully
    /// settled, not inside the settle loop.
    pub fn process_modules(&mut self, ts_now: UnixNanos) {
        let adjustments = {
            let cache = self.cache.borrow();
            let ctx = ExchangeContext {
                venue: self.id,
                base_currency: self.base_currency,
                instruments: &self.instruments,
                matching_engines: &self.matching_engines,
                cache: &cache,
            };
            self.modules
                .iter()
                .flat_map(|m| m.process(ts_now, &ctx))
                .collect::<Vec<Money>>()
        };

        for adjustment in adjustments {
            self.adjust_account(adjustment);
        }
    }

    /// Resets the exchange to its initial state.
    pub fn reset(&mut self) {
        for module in &self.modules {
            module.reset();
        }

        self.generate_fresh_account_state();

        for matching_engine in self.matching_engines.values_mut() {
            matching_engine.reset();
        }

        self.settlement_prices.clear();
        self.message_queue.clear();
        self.inflight_queue.clear();

        log::info!("Resetting exchange state");
    }

    /// Logs diagnostic information from all simulation modules.
    pub fn log_diagnostics(&self) {
        for module in &self.modules {
            module.log_diagnostics();
        }
    }

    fn process_trading_command(&mut self, command: TradingCommand) {
        if let Some(matching_engine) = self.matching_engines.get_mut(&command.instrument_id()) {
            let account_id = if let Some(exec_client) = &self.exec_client {
                exec_client.account_id()
            } else {
                panic!("Execution client should be initialized");
            };
            match command {
                TradingCommand::SubmitOrder(command) => {
                    let mut order = self
                        .cache
                        .borrow()
                        .order(&command.client_order_id)
                        .cloned()
                        .expect("Order must exist in cache");
                    matching_engine.process_order(&mut order, account_id);
                }
                TradingCommand::ModifyOrder(ref command) => {
                    matching_engine.process_modify(command, account_id);
                }
                TradingCommand::CancelOrder(ref command) => {
                    matching_engine.process_cancel(command, account_id);
                }
                TradingCommand::CancelAllOrders(ref command) => {
                    matching_engine.process_cancel_all(command, account_id);
                }
                TradingCommand::BatchCancelOrders(ref command) => {
                    matching_engine.process_batch_cancel(command, account_id);
                }
                TradingCommand::SubmitOrderList(ref command) => {
                    let mut orders: Vec<OrderAny> = self
                        .cache
                        .borrow()
                        .orders_for_ids(&command.order_list.client_order_ids, command);

                    for order in &mut orders {
                        matching_engine.process_order(order, account_id);
                    }
                }
                _ => {}
            }
        } else {
            panic!(
                "Matching engine not found for instrument {}",
                command.instrument_id()
            );
        }
    }

    fn generate_fresh_account_state(&self) {
        let balances: Vec<AccountBalance> = self
            .starting_balances
            .iter()
            .map(|money| AccountBalance::new(*money, Money::zero(money.currency), *money))
            .collect();

        if let Some(exec_client) = &self.exec_client {
            exec_client
                .generate_account_state(balances, vec![], true, self.clock.borrow().timestamp_ns())
                .unwrap();
        }

        if let Some(AccountAny::Margin(mut margin_account)) = self.get_account() {
            margin_account.set_default_leverage(self.default_leverage);
            for (instrument_id, leverage) in &self.leverages {
                margin_account.set_leverage(*instrument_id, *leverage);
            }

            if let Some(model) = &self.margin_model {
                margin_account.set_margin_model(model.clone());
            }
            self.cache
                .borrow_mut()
                .update_account(AccountAny::Margin(margin_account))
                .unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::BinaryHeap,
        rc::Rc,
    };

    use ahash::AHashMap;
    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        messages::execution::{SubmitOrder, TradingCommand},
        msgbus::{self, stubs::get_typed_message_saving_handler},
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_execution::models::{
        fee::{FeeModelAny, MakerTakerFeeModel},
        fill::FillModelAny,
        latency::StaticLatencyModel,
    };
    use nautilus_model::{
        accounts::{AccountAny, MarginAccount},
        data::{
            Bar, BarType, BookOrder, Data, InstrumentStatus, OrderBookDelta, OrderBookDeltas,
            QuoteTick, TradeTick,
        },
        enums::{
            AccountType, AggressorSide, BookAction, BookType, MarketStatus, MarketStatusAction,
            OmsType, OrderSide, OrderType,
        },
        events::AccountState,
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, Venue,
        },
        instruments::{
            CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt,
        },
        orders::{Order, OrderAny, OrderTestBuilder},
        stubs::TestDefault,
        types::{AccountBalance, Currency, Money, Price, Quantity},
    };
    use rstest::rstest;

    use crate::{
        exchange::{InflightCommand, SimulatedExchange},
        execution_client::BacktestExecutionClient,
        modules::{ExchangeContext, SimulationModule},
    };

    fn get_exchange(
        venue: Venue,
        account_type: AccountType,
        book_type: BookType,
        cache: Option<Rc<RefCell<Cache>>>,
    ) -> Rc<RefCell<SimulatedExchange>> {
        let cache = cache.unwrap_or(Rc::new(RefCell::new(Cache::default())));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let exchange = Rc::new(RefCell::new(
            SimulatedExchange::new(
                venue,
                OmsType::Netting,
                account_type,
                vec![Money::new(1000.0, Currency::USD())],
                None,
                1.into(),
                AHashMap::new(),
                None, // margin_model
                vec![],
                cache.clone(),
                clock,
                FillModelAny::default(),
                FeeModelAny::MakerTaker(MakerTakerFeeModel),
                book_type,
                None, // latency_model
                None, // bar_execution
                None, // bar_adaptive_high_low_ordering
                None, // trade_execution
                None, // liquidity_consumption
                None, // reject_stop_orders
                None, // support_gtd_orders
                None, // support_contingent_orders
                None, // use_position_ids
                None, // use_random_ids
                None, // use_reduce_only
                None, // use_message_queue
                None, // use_market_order_acks
                None, // allow_cash_borrowing
                None, // frozen_account
                None, // queue_position
                None, // oto_full_trigger
                None, // price_protection_points
            )
            .unwrap(),
        ));

        let clock = TestClock::new();
        let execution_client = BacktestExecutionClient::new(
            TraderId::test_default(),
            AccountId::test_default(),
            exchange.clone(),
            cache,
            Rc::new(RefCell::new(clock)),
            None,
            None,
        );
        exchange
            .borrow_mut()
            .register_client(Rc::new(execution_client));

        exchange
    }

    fn create_submit_order_command(
        ts_init: UnixNanos,
        client_order_id: &str,
    ) -> (OrderAny, TradingCommand) {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::new(client_order_id))
            .quantity(Quantity::from(1))
            .build();
        let command = TradingCommand::SubmitOrder(SubmitOrder::new(
            TraderId::test_default(),
            None,
            StrategyId::test_default(),
            instrument_id,
            order.client_order_id(),
            order.init_event().clone(),
            None,
            None,
            None, // params
            UUID4::default(),
            ts_init,
        ));
        (order, command)
    }

    #[rstest]
    #[should_panic(
        expected = "Condition failed: 'Venue of instrument id' value of BINANCE was not equal to 'Venue of simulated exchange' value of SIM"
    )]
    fn test_venue_mismatch_between_exchange_and_instrument(
        crypto_perpetual_ethusdt: CryptoPerpetual,
    ) {
        let exchange = get_exchange(
            Venue::new("SIM"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        exchange.borrow_mut().add_instrument(instrument).unwrap();
    }

    #[rstest]
    #[should_panic(expected = "Cash account cannot trade futures or perpetuals")]
    fn test_cash_account_trading_futures_or_perpetuals(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Cash,
            BookType::L1_MBP,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        exchange.borrow_mut().add_instrument(instrument).unwrap();
    }

    #[rstest]
    fn test_exchange_process_quote_tick(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // process tick
        let quote_tick = QuoteTick::new(
            crypto_perpetual_ethusdt.id,
            Price::from("1000.00"),
            Price::from("1001.00"),
            Quantity::from("1.000"),
            Quantity::from("1.000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        exchange.borrow_mut().process_quote_tick(&quote_tick);

        let best_bid_price = exchange
            .borrow()
            .best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1000.00")));
        let best_ask_price = exchange
            .borrow()
            .best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask_price, Some(Price::from("1001.00")));
    }

    #[rstest]
    fn test_exchange_process_trade_tick(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // process tick
        let trade_tick = TradeTick::new(
            crypto_perpetual_ethusdt.id,
            Price::from("1000.00"),
            Quantity::from("1.000"),
            AggressorSide::Buyer,
            TradeId::from("1"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        exchange.borrow_mut().process_trade_tick(&trade_tick);

        let best_bid_price = exchange
            .borrow()
            .best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1000.00")));
        let best_ask = exchange
            .borrow()
            .best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask, Some(Price::from("1000.00")));
    }

    #[rstest]
    fn test_exchange_process_bar_last_bar_spec(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // process bar
        let bar = Bar::new(
            BarType::from("ETHUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            Price::from("1500.00"),
            Price::from("1505.00"),
            Price::from("1490.00"),
            Price::from("1502.00"),
            Quantity::from("100.000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        exchange.borrow_mut().process_bar(bar);

        // this will be processed as ticks so both bid and ask will be the same as close of the bar
        let best_bid_price = exchange
            .borrow()
            .best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1502.00")));
        let best_ask_price = exchange
            .borrow()
            .best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask_price, Some(Price::from("1502.00")));
    }

    #[rstest]
    fn test_exchange_process_bar_bid_ask_bar_spec(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // create both bid and ask based bars
        // add +1 on ask to make sure it is different from bid
        let bar_bid = Bar::new(
            BarType::from("ETHUSDT-PERP.BINANCE-1-MINUTE-BID-EXTERNAL"),
            Price::from("1500.00"),
            Price::from("1505.00"),
            Price::from("1490.00"),
            Price::from("1502.00"),
            Quantity::from("100.000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        let bar_ask = Bar::new(
            BarType::from("ETHUSDT-PERP.BINANCE-1-MINUTE-ASK-EXTERNAL"),
            Price::from("1501.00"),
            Price::from("1506.00"),
            Price::from("1491.00"),
            Price::from("1503.00"),
            Quantity::from("100.000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        );

        // process them
        exchange.borrow_mut().process_bar(bar_bid);
        exchange.borrow_mut().process_bar(bar_ask);

        // current bid and ask prices will be the corresponding close of the ask and bid bar
        let best_bid_price = exchange
            .borrow()
            .best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1502.00")));
        let best_ask_price = exchange
            .borrow()
            .best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask_price, Some(Price::from("1503.00")));
    }

    #[rstest]
    fn test_exchange_process_orderbook_delta(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L2_MBP,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // create order book delta at both bid and ask with incremented ts init and sequence
        let delta_buy = OrderBookDelta::new(
            crypto_perpetual_ethusdt.id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("1000.00"),
                Quantity::from("1.000"),
                1,
            ),
            0,
            0,
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        let delta_sell = OrderBookDelta::new(
            crypto_perpetual_ethusdt.id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("1001.00"),
                Quantity::from("1.000"),
                1,
            ),
            0,
            1,
            UnixNanos::from(2),
            UnixNanos::from(2),
        );

        // process both deltas
        exchange.borrow_mut().process_order_book_delta(delta_buy);
        exchange.borrow_mut().process_order_book_delta(delta_sell);

        let book = exchange
            .borrow()
            .get_book(crypto_perpetual_ethusdt.id)
            .unwrap()
            .clone();
        assert_eq!(book.update_count, 2);
        assert_eq!(book.sequence, 1);
        assert_eq!(book.ts_last, UnixNanos::from(2));
        let best_bid_price = exchange
            .borrow()
            .best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1000.00")));
        let best_ask_price = exchange
            .borrow()
            .best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask_price, Some(Price::from("1001.00")));
    }

    #[rstest]
    fn test_exchange_process_orderbook_deltas(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L2_MBP,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // create two sell order book deltas with same timestamps and higher sequence
        let delta_sell_1 = OrderBookDelta::new(
            crypto_perpetual_ethusdt.id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("1000.00"),
                Quantity::from("3.000"),
                1,
            ),
            0,
            0,
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        let delta_sell_2 = OrderBookDelta::new(
            crypto_perpetual_ethusdt.id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("1001.00"),
                Quantity::from("1.000"),
                1,
            ),
            0,
            1,
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        let orderbook_deltas = OrderBookDeltas::new(
            crypto_perpetual_ethusdt.id,
            vec![delta_sell_1, delta_sell_2],
        );

        // process both deltas
        exchange
            .borrow_mut()
            .process_order_book_deltas(orderbook_deltas);

        let book = exchange
            .borrow()
            .get_book(crypto_perpetual_ethusdt.id)
            .unwrap()
            .clone();
        assert_eq!(book.update_count, 2);
        assert_eq!(book.sequence, 1);
        assert_eq!(book.ts_last, UnixNanos::from(1));
        let best_bid_price = exchange
            .borrow()
            .best_bid_price(crypto_perpetual_ethusdt.id);
        // no bid orders in orderbook deltas
        assert_eq!(best_bid_price, None);
        let best_ask_price = exchange
            .borrow()
            .best_ask_price(crypto_perpetual_ethusdt.id);
        // best ask price is the first order in orderbook deltas
        assert_eq!(best_ask_price, Some(Price::from("1000.00")));
    }

    #[rstest]
    fn test_exchange_process_instrument_status(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L2_MBP,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        let instrument_status = InstrumentStatus::new(
            crypto_perpetual_ethusdt.id,
            MarketStatusAction::Close, // close the market
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
            None,
            None,
            None,
            None,
        );

        exchange
            .borrow_mut()
            .process_instrument_status(instrument_status);

        let market_status = exchange
            .borrow()
            .get_matching_engine(&crypto_perpetual_ethusdt.id)
            .unwrap()
            .market_status;
        assert_eq!(market_status, MarketStatus::Closed);
    }

    #[rstest]
    fn test_accounting() {
        let account_type = AccountType::Margin;
        let mut cache = Cache::default();
        let (handler, saving_handler) = get_typed_message_saving_handler::<AccountState>(None);
        msgbus::register_account_state_endpoint("Portfolio.update_account".into(), handler);
        let margin_account = MarginAccount::new(
            AccountState::new(
                AccountId::from("SIM-001"),
                account_type,
                vec![AccountBalance::new(
                    Money::from("1000 USD"),
                    Money::from("0 USD"),
                    Money::from("1000 USD"),
                )],
                vec![],
                false,
                UUID4::default(),
                UnixNanos::default(),
                UnixNanos::default(),
                None,
            ),
            false,
        );
        let () = cache
            .add_account(AccountAny::Margin(margin_account))
            .unwrap();
        // build indexes
        cache.build_index();

        let exchange = get_exchange(
            Venue::new("SIM"),
            account_type,
            BookType::L2_MBP,
            Some(Rc::new(RefCell::new(cache))),
        );
        exchange.borrow_mut().initialize_account();

        // Test adjust account, increase balance by 500 USD
        exchange.borrow_mut().adjust_account(Money::from("500 USD"));

        // Check if we received two messages, one for initial account state and one for adjusted account state
        let messages = saving_handler.get_messages();
        assert_eq!(messages.len(), 2);
        let account_state_first = messages.first().unwrap();
        let account_state_second = messages.last().unwrap();

        assert_eq!(account_state_first.balances.len(), 1);
        let current_balance = account_state_first.balances[0];
        assert_eq!(current_balance.free, Money::new(1000.0, Currency::USD()));
        assert_eq!(current_balance.locked, Money::new(0.0, Currency::USD()));
        assert_eq!(current_balance.total, Money::new(1000.0, Currency::USD()));

        assert_eq!(account_state_second.balances.len(), 1);
        let current_balance = account_state_second.balances[0];
        assert_eq!(current_balance.free, Money::new(1500.0, Currency::USD()));
        assert_eq!(current_balance.locked, Money::new(0.0, Currency::USD()));
        assert_eq!(current_balance.total, Money::new(1500.0, Currency::USD()));
    }

    #[rstest]
    fn test_inflight_commands_binary_heap_ordering_respecting_timestamp_counter() {
        // Create 3 inflight commands with different timestamps and counters
        let (_, cmd1) = create_submit_order_command(UnixNanos::from(100), "O-1");
        let (_, cmd2) = create_submit_order_command(UnixNanos::from(200), "O-2");
        let (_, cmd3) = create_submit_order_command(UnixNanos::from(100), "O-3");

        let inflight1 = InflightCommand::new(UnixNanos::from(100), 1, cmd1);
        let inflight2 = InflightCommand::new(UnixNanos::from(200), 2, cmd2);
        let inflight3 = InflightCommand::new(UnixNanos::from(100), 2, cmd3);

        // Create a binary heap and push the inflight commands
        let mut inflight_heap = BinaryHeap::new();
        inflight_heap.push(inflight1);
        inflight_heap.push(inflight2);
        inflight_heap.push(inflight3);

        // Pop the inflight commands and check if they are in the correct order
        // by our custom ordering with counter and timestamp
        let first = inflight_heap.pop().unwrap();
        let second = inflight_heap.pop().unwrap();
        let third = inflight_heap.pop().unwrap();

        assert_eq!(first.timestamp, UnixNanos::from(100));
        assert_eq!(first.counter, 1);
        assert_eq!(second.timestamp, UnixNanos::from(100));
        assert_eq!(second.counter, 2);
        assert_eq!(third.timestamp, UnixNanos::from(200));
        assert_eq!(third.counter, 2);
    }

    #[rstest]
    fn test_process_without_latency_model(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L2_MBP,
            None,
        );

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        let (order1, command1) = create_submit_order_command(UnixNanos::from(100), "O-1");
        let (order2, command2) = create_submit_order_command(UnixNanos::from(200), "O-2");

        exchange
            .borrow()
            .cache()
            .borrow_mut()
            .add_order(order1, None, None, false)
            .unwrap();
        exchange
            .borrow()
            .cache()
            .borrow_mut()
            .add_order(order2, None, None, false)
            .unwrap();

        exchange.borrow_mut().send(command1);
        exchange.borrow_mut().send(command2);

        // Verify that message queue has 2 commands and inflight queue is empty
        // as we are not using latency model
        assert_eq!(exchange.borrow().message_queue.len(), 2);
        assert_eq!(exchange.borrow().inflight_queue.len(), 0);

        // Process command and check that queues is empty
        exchange.borrow_mut().process(UnixNanos::from(300));
        assert_eq!(exchange.borrow().message_queue.len(), 0);
        assert_eq!(exchange.borrow().inflight_queue.len(), 0);
    }

    #[rstest]
    fn test_process_with_latency_model(crypto_perpetual_ethusdt: CryptoPerpetual) {
        // StaticLatencyModel adds base_latency to each operation latency
        // base=100, insert=200 -> effective insert latency = 300
        let latency_model = StaticLatencyModel::new(
            UnixNanos::from(100),
            UnixNanos::from(200),
            UnixNanos::from(300),
            UnixNanos::from(100),
        );
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L2_MBP,
            None,
        );
        exchange
            .borrow_mut()
            .set_latency_model(Box::new(latency_model));

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        let (order1, command1) = create_submit_order_command(UnixNanos::from(100), "O-1");
        let (order2, command2) = create_submit_order_command(UnixNanos::from(150), "O-2");

        exchange
            .borrow()
            .cache()
            .borrow_mut()
            .add_order(order1, None, None, false)
            .unwrap();
        exchange
            .borrow()
            .cache()
            .borrow_mut()
            .add_order(order2, None, None, false)
            .unwrap();

        exchange.borrow_mut().send(command1);
        exchange.borrow_mut().send(command2);

        // Verify that inflight queue has 2 commands and message queue is empty
        assert_eq!(exchange.borrow().message_queue.len(), 0);
        assert_eq!(exchange.borrow().inflight_queue.len(), 2);
        // First inflight command: ts_init=100 + effective_insert_latency=300 = 400
        assert_eq!(
            exchange
                .borrow()
                .inflight_queue
                .iter()
                .next()
                .unwrap()
                .timestamp,
            UnixNanos::from(400)
        );
        // Second inflight command: ts_init=150 + effective_insert_latency=300 = 450
        assert_eq!(
            exchange
                .borrow()
                .inflight_queue
                .iter()
                .nth(1)
                .unwrap()
                .timestamp,
            UnixNanos::from(450)
        );

        // Process at timestamp 420, and test that only first command is processed
        exchange.borrow_mut().process(UnixNanos::from(420));
        assert_eq!(exchange.borrow().message_queue.len(), 0);
        assert_eq!(exchange.borrow().inflight_queue.len(), 1);
        assert_eq!(
            exchange
                .borrow()
                .inflight_queue
                .iter()
                .next()
                .unwrap()
                .timestamp,
            UnixNanos::from(450)
        );
    }

    #[rstest]
    fn test_process_iterates_matching_engines_after_commands(
        crypto_perpetual_ethusdt: CryptoPerpetual,
    ) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            Some(cache.clone()),
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        let instrument_id = instrument.id();
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        let quote = QuoteTick::new(
            instrument_id,
            Price::from("1000.00"),
            Price::from("1001.00"),
            Quantity::from("1.000"),
            Quantity::from("1.000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        exchange.borrow_mut().process_quote_tick(&quote);

        // Create a passive buy limit below the ask (should NOT fill)
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::new("O-LIMIT-1"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.000"))
            .price(Price::from("999.00"))
            .build();

        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        let command = TradingCommand::SubmitOrder(SubmitOrder::new(
            TraderId::test_default(),
            None,
            StrategyId::test_default(),
            instrument_id,
            order.client_order_id(),
            order.init_event().clone(),
            None,
            None,
            None,
            UUID4::default(),
            UnixNanos::from(1),
        ));
        exchange.borrow_mut().send(command);

        exchange.borrow_mut().process(UnixNanos::from(1));

        let open_orders = exchange.borrow().get_open_orders(Some(instrument_id));
        assert_eq!(open_orders.len(), 1);
        assert_eq!(
            open_orders[0].client_order_id,
            ClientOrderId::new("O-LIMIT-1")
        );
    }

    #[derive(Clone)]
    struct MockModuleCounts {
        pre_process: Rc<Cell<u32>>,
        process: Rc<Cell<u32>>,
        reset: Rc<Cell<u32>>,
        log_diagnostics: Rc<Cell<u32>>,
    }

    impl MockModuleCounts {
        fn new() -> Self {
            Self {
                pre_process: Rc::new(Cell::new(0)),
                process: Rc::new(Cell::new(0)),
                reset: Rc::new(Cell::new(0)),
                log_diagnostics: Rc::new(Cell::new(0)),
            }
        }
    }

    struct MockSimulationModule {
        counts: MockModuleCounts,
    }

    impl MockSimulationModule {
        fn new(counts: MockModuleCounts) -> Self {
            Self { counts }
        }
    }

    impl SimulationModule for MockSimulationModule {
        fn pre_process(&self, _data: &Data) {
            self.counts
                .pre_process
                .set(self.counts.pre_process.get() + 1);
        }

        fn process(&self, _ts_now: UnixNanos, _ctx: &ExchangeContext) -> Vec<Money> {
            self.counts.process.set(self.counts.process.get() + 1);
            Vec::new()
        }

        fn log_diagnostics(&self) {
            self.counts
                .log_diagnostics
                .set(self.counts.log_diagnostics.get() + 1);
        }

        fn reset(&self) {
            self.counts.reset.set(self.counts.reset.get() + 1);
        }
    }

    fn get_exchange_with_module(
        venue: Venue,
        counts: MockModuleCounts,
    ) -> Rc<RefCell<SimulatedExchange>> {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        // Register msgbus handler so generate_account_state works during reset
        let (handler, _saving_handler) = get_typed_message_saving_handler::<AccountState>(None);
        msgbus::register_account_state_endpoint("Portfolio.update_account".into(), handler);

        let modules: Vec<Box<dyn SimulationModule>> =
            vec![Box::new(MockSimulationModule::new(counts))];

        let exchange = Rc::new(RefCell::new(
            SimulatedExchange::new(
                venue,
                OmsType::Netting,
                AccountType::Margin,
                vec![Money::new(1000.0, Currency::USD())],
                None,
                1.into(),
                AHashMap::new(),
                None, // margin_model
                modules,
                cache.clone(),
                clock,
                FillModelAny::default(),
                FeeModelAny::MakerTaker(MakerTakerFeeModel),
                BookType::L1_MBP,
                None, // latency_model
                None, // bar_execution
                None, // bar_adaptive_high_low_ordering
                None, // trade_execution
                None, // liquidity_consumption
                None, // reject_stop_orders
                None, // support_gtd_orders
                None, // support_contingent_orders
                None, // use_position_ids
                None, // use_random_ids
                None, // use_reduce_only
                None, // use_message_queue
                None, // use_market_order_acks
                None, // allow_cash_borrowing
                None, // frozen_account
                None, // queue_position
                None, // oto_full_trigger
                None, // price_protection_points
            )
            .unwrap(),
        ));

        let exec_clock = TestClock::new();
        let execution_client = BacktestExecutionClient::new(
            TraderId::test_default(),
            AccountId::test_default(),
            exchange.clone(),
            cache,
            Rc::new(RefCell::new(exec_clock)),
            None,
            None,
        );
        exchange
            .borrow_mut()
            .register_client(Rc::new(execution_client));

        exchange
    }

    #[rstest]
    fn test_module_pre_process_called_on_quote(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let counts = MockModuleCounts::new();
        let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        let quote = QuoteTick::new(
            crypto_perpetual_ethusdt.id,
            Price::from("1000.00"),
            Price::from("1001.00"),
            Quantity::from("1.000"),
            Quantity::from("1.000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        exchange.borrow_mut().process_quote_tick(&quote);

        assert_eq!(counts.pre_process.get(), 1);
        assert_eq!(counts.process.get(), 0);
    }

    #[rstest]
    fn test_module_process_not_called_by_process(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let counts = MockModuleCounts::new();
        let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // process() drains commands but does not run modules
        exchange.borrow_mut().process(UnixNanos::from(100));

        assert_eq!(counts.process.get(), 0);
    }

    #[rstest]
    fn test_module_process_called_by_process_modules(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let counts = MockModuleCounts::new();
        let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        exchange.borrow_mut().process_modules(UnixNanos::from(100));

        assert_eq!(counts.process.get(), 1);
    }

    #[rstest]
    fn test_module_reset_called_on_reset(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let counts = MockModuleCounts::new();
        let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // Pre-populate account in cache so generate_fresh_account_state succeeds
        let margin_account = MarginAccount::new(
            AccountState::new(
                AccountId::test_default(),
                AccountType::Margin,
                vec![AccountBalance::new(
                    Money::from("1000 USD"),
                    Money::from("0 USD"),
                    Money::from("1000 USD"),
                )],
                vec![],
                false,
                UUID4::default(),
                UnixNanos::default(),
                UnixNanos::default(),
                None,
            ),
            false,
        );
        exchange
            .borrow()
            .cache()
            .borrow_mut()
            .add_account(AccountAny::Margin(margin_account))
            .unwrap();

        exchange.borrow_mut().reset();

        assert_eq!(counts.reset.get(), 1);
    }

    #[rstest]
    fn test_module_log_diagnostics(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let counts = MockModuleCounts::new();
        let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        exchange.borrow().log_diagnostics();

        assert_eq!(counts.log_diagnostics.get(), 1);
    }

    #[rstest]
    fn test_module_pre_process_and_process_call_order(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let counts = MockModuleCounts::new();
        let exchange = get_exchange_with_module(Venue::new("BINANCE"), counts.clone());
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // pre_process called per data item, process_modules called separately
        let quote = QuoteTick::new(
            crypto_perpetual_ethusdt.id,
            Price::from("1000.00"),
            Price::from("1001.00"),
            Quantity::from("1.000"),
            Quantity::from("1.000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        exchange.borrow_mut().process_quote_tick(&quote);
        exchange.borrow_mut().process_quote_tick(&quote);
        exchange.borrow_mut().process(UnixNanos::from(100));
        exchange.borrow_mut().process_modules(UnixNanos::from(100));

        assert_eq!(counts.pre_process.get(), 2);
        assert_eq!(counts.process.get(), 1);
    }
}
