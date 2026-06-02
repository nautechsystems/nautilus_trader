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
use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::{Clock, TestClock},
    messages::execution::{ModifyOrder, TradingCommand},
    msgbus::{self, MessagingSwitchboard, switchboard},
};
use nautilus_core::{
    UUID4, UnixNanos,
    correctness::{CorrectnessResultExt, FAILED, check_equal},
};
use nautilus_execution::{
    matching_core::RestingOrder,
    matching_engine::{config::OrderMatchingEngineConfig, engine::OrderMatchingEngine},
    models::{fee::FeeModelAny, fill::FillModelAny, latency::LatencyModel},
};
use nautilus_model::{
    accounts::{Account, AccountAny, margin_model::MarginModelAny},
    data::{
        Bar, Data, FundingRateUpdate, InstrumentClose, InstrumentStatus, OrderBookDelta,
        OrderBookDeltas, OrderBookDeltas_API, OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{AccountType, AggressorSide, BookType, OmsType, OrderStatus, PositionAdjustmentType},
    events::{FundingSettlement, OrderEventAny, OrderUpdated, PositionAdjusted, PositionEvent},
    identifiers::{AccountId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    orders::{Order, OrderAny},
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    config::SimulatedVenueConfig,
    modules::{ExchangeContext, SimulationModule},
};

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
    pending_funding_rates: AHashMap<InstrumentId, FundingRateUpdate>,
    funding_settled_through: AHashMap<InstrumentId, UnixNanos>,
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
    allow_cash_borrowing: bool,
    frozen_account: bool,
    queue_position: bool,
    oto_full_trigger: bool,
    price_protection_points: u32,
    liquidation_enabled: bool,
    liquidation_trigger_ratio: f64,
    liquidation_cancel_open_orders: bool,
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
    /// Creates a new [`SimulatedExchange`] instance from a venue configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `starting_balances` is empty.
    /// - `base_currency` is `Some` but `starting_balances` contains multiple currencies.
    pub fn new(
        config: SimulatedVenueConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Self> {
        if config.starting_balances.is_empty() {
            anyhow::bail!("Starting balances must be provided")
        }

        if config.base_currency.is_some() && config.starting_balances.len() > 1 {
            anyhow::bail!("single-currency account has multiple starting currencies")
        }

        let default_leverage = config.default_leverage.unwrap_or_else(|| {
            if config.account_type == AccountType::Margin {
                Decimal::from(10)
            } else {
                Decimal::from(1)
            }
        });

        Ok(Self {
            id: config.venue,
            oms_type: config.oms_type,
            account_type: config.account_type,
            base_currency: config.base_currency,
            starting_balances: config.starting_balances,
            book_type: config.book_type,
            default_leverage,
            exec_client: None,
            fee_model: config.fee_model,
            fill_model: config.fill_model,
            latency_model: config.latency_model,
            instruments: AHashMap::new(),
            matching_engines: AHashMap::new(),
            settlement_prices: config.settlement_prices,
            pending_funding_rates: AHashMap::new(),
            funding_settled_through: AHashMap::new(),
            leverages: config.leverages,
            margin_model: config.margin_model,
            modules: config.modules,
            clock,
            cache,
            message_queue: VecDeque::new(),
            inflight_queue: BinaryHeap::new(),
            inflight_counter: AHashMap::new(),
            bar_execution: config.bar_execution,
            bar_adaptive_high_low_ordering: config.bar_adaptive_high_low_ordering,
            trade_execution: config.trade_execution,
            liquidity_consumption: config.liquidity_consumption,
            reject_stop_orders: config.reject_stop_orders,
            support_gtd_orders: config.support_gtd_orders,
            support_contingent_orders: config.support_contingent_orders,
            use_position_ids: config.use_position_ids,
            use_random_ids: config.use_random_ids,
            use_reduce_only: config.use_reduce_only,
            use_message_queue: config.use_message_queue,
            use_market_order_acks: config.use_market_order_acks,
            allow_cash_borrowing: config.allow_cash_borrowing,
            frozen_account: config.frozen_account,
            queue_position: config.queue_position,
            oto_full_trigger: config.oto_full_trigger,
            price_protection_points: config.price_protection_points,
            liquidation_enabled: config.liquidation_enabled,
            liquidation_trigger_ratio: config.liquidation_trigger_ratio,
            liquidation_cancel_open_orders: config.liquidation_cancel_open_orders,
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

    /// Returns the configured book type for this venue.
    #[must_use]
    pub const fn book_type(&self) -> BookType {
        self.book_type
    }

    /// Returns an iterator over the instrument IDs registered with this exchange.
    pub fn instrument_ids(&self) -> impl Iterator<Item = &InstrumentId> {
        self.instruments.keys()
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

    // panics-doc-ok (transitive via expect_display on venue mismatch)
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
        .expect_display(FAILED);

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

        let matching_engine_config = OrderMatchingEngineConfig::builder()
            .bar_execution(self.bar_execution)
            .bar_adaptive_high_low_ordering(self.bar_adaptive_high_low_ordering)
            .trade_execution(self.trade_execution)
            .liquidity_consumption(self.liquidity_consumption)
            .reject_stop_orders(self.reject_stop_orders)
            .support_gtd_orders(self.support_gtd_orders)
            .support_contingent_orders(self.support_contingent_orders)
            .use_position_ids(self.use_position_ids)
            .use_random_ids(self.use_random_ids)
            .use_reduce_only(self.use_reduce_only)
            .use_market_order_acks(self.use_market_order_acks)
            .queue_position(self.queue_position)
            .oto_full_trigger(self.oto_full_trigger)
            .maybe_price_protection_points(price_protection)
            .build();
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
    pub fn get_open_orders(&self, instrument_id: Option<InstrumentId>) -> Vec<RestingOrder> {
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
    pub fn get_open_bid_orders(&self, instrument_id: Option<InstrumentId>) -> Vec<RestingOrder> {
        instrument_id
            .and_then(|id| {
                self.matching_engines
                    .get(&id)
                    .map(|engine| engine.get_open_bid_orders())
            })
            .unwrap_or_else(|| {
                self.matching_engines
                    .values()
                    .flat_map(|engine| engine.get_open_bid_orders())
                    .collect()
            })
    }

    /// Returns all open ask orders, optionally filtered by instrument ID.
    #[must_use]
    pub fn get_open_ask_orders(&self, instrument_id: Option<InstrumentId>) -> Vec<RestingOrder> {
        instrument_id
            .and_then(|id| {
                self.matching_engines
                    .get(&id)
                    .map(|engine| engine.get_open_ask_orders())
            })
            .unwrap_or_else(|| {
                self.matching_engines
                    .values()
                    .flat_map(|engine| engine.get_open_ask_orders())
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
            let account_state = {
                let cache = self.cache.borrow();
                if let Some(account) = cache.account_for_venue(&venue) {
                    match account.balance(Some(adjustment.currency)) {
                        Some(balance) => {
                            let mut current_balance = *balance;
                            current_balance.total = current_balance.total + adjustment;
                            current_balance.free = current_balance.free + adjustment;

                            let margins = match &*account {
                                AccountAny::Margin(margin_account) => {
                                    margin_account.margins.clone()
                                }
                                _ => IndexMap::new(),
                            };

                            Some((
                                vec![current_balance],
                                margins.values().copied().collect(),
                                self.clock.borrow().timestamp_ns(),
                            ))
                        }
                        None => {
                            log::error!(
                                "Cannot adjust account: no balance for currency {}",
                                adjustment.currency
                            );
                            None
                        }
                    }
                } else {
                    log::error!("Cannot adjust account: no account for venue {venue}");
                    None
                }
            };

            if let Some((balances, margins, ts_event)) = account_state {
                exec_client
                    .generate_account_state(balances, margins, true, ts_event)
                    .unwrap();
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

    /// Returns the latest arrival timestamp across all latency-deferred
    /// inflight commands, or `None` when the inflight queue is empty.
    ///
    /// Used at shutdown to advance the clock past the configured `LatencyModel`
    /// delay so trailing commands (those emitted on the final data tick or
    /// in `on_stop`) settle before the engines stop.
    #[must_use]
    pub fn max_inflight_command_ts(&self) -> Option<UnixNanos> {
        self.inflight_queue.iter().map(|c| c.timestamp).max()
    }

    /// Iterates all matching engines so newly submitted orders can match
    /// against the current market state.
    pub fn iterate_matching_engines(&mut self, ts_now: UnixNanos) {
        for matching_engine in self.matching_engines.values_mut() {
            matching_engine.iterate(ts_now, AggressorSide::NoAggressor);
        }
    }

    /// Processes instrument expirations due at the given timestamp.
    pub fn process_instrument_expirations(&mut self, ts_now: UnixNanos) {
        for matching_engine in self.matching_engines.values_mut() {
            if matching_engine
                .instrument
                .expiration_ns()
                .is_some_and(|expiration_ns| ts_now >= expiration_ns)
            {
                matching_engine.process_instrument_expiration(ts_now);
            }
        }
    }

    /// Returns unprocessed instrument expirations for timer scheduling.
    #[must_use]
    pub fn instrument_expirations(&self) -> Vec<(InstrumentId, UnixNanos)> {
        self.matching_engines
            .values()
            .filter(|matching_engine| !matching_engine.is_expiration_processed())
            .filter_map(|matching_engine| {
                matching_engine
                    .instrument
                    .expiration_ns()
                    .filter(|expiration_ns| *expiration_ns > UnixNanos::default())
                    .map(|expiration_ns| (matching_engine.instrument.id(), expiration_ns))
            })
            .collect()
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
    pub fn process_order_book_deltas(&mut self, deltas: &OrderBookDeltas) {
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
            matching_engine.process_order_book_deltas(deltas).unwrap();
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
        for module in &self.modules {
            module.pre_process(&Data::InstrumentStatus(status));
        }

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

    /// Processes a funding rate update.
    ///
    /// Returns the funding boundary timestamp when the engine should schedule a settlement.
    pub fn process_funding_rate(&mut self, funding_rate: FundingRateUpdate) -> Option<UnixNanos> {
        for module in &self.modules {
            module.pre_process(&Data::FundingRateUpdate(funding_rate));
        }

        if let Some(next_funding_ns) = funding_rate.next_funding_ns {
            if next_funding_ns <= self.clock.borrow().timestamp_ns() {
                self.pending_funding_rates
                    .remove(&funding_rate.instrument_id);
                self.settle_funding_rate(funding_rate, next_funding_ns);
                return None;
            }

            self.pending_funding_rates
                .insert(funding_rate.instrument_id, funding_rate);
            return Some(next_funding_ns);
        }

        if Self::is_interval_funding_boundary(&funding_rate) {
            self.settle_funding_rate(funding_rate, funding_rate.ts_event);
        } else {
            log::debug!(
                "Funding rate update for {} does not define a settlement boundary",
                funding_rate.instrument_id
            );
        }

        None
    }

    /// Processes a scheduled funding settlement for the instrument.
    pub fn process_funding_settlement(&mut self, instrument_id: InstrumentId, ts_event: UnixNanos) {
        let Some(funding_rate) = self.pending_funding_rates.remove(&instrument_id) else {
            return;
        };

        if funding_rate
            .next_funding_ns
            .is_some_and(|next_funding_ns| next_funding_ns > ts_event)
        {
            self.pending_funding_rates
                .insert(funding_rate.instrument_id, funding_rate);
            return;
        }

        self.settle_funding_rate(funding_rate, ts_event);
    }

    fn settle_funding_rate(&mut self, funding_rate: FundingRateUpdate, ts_event: UnixNanos) {
        if self
            .funding_settled_through
            .get(&funding_rate.instrument_id)
            .is_some_and(|settled_through| *settled_through >= ts_event)
        {
            return;
        }

        let Some(exec_client) = &self.exec_client else {
            log::warn!(
                "Cannot settle funding for {}: execution client is not registered",
                funding_rate.instrument_id
            );
            return;
        };
        let account_id = exec_client.account_id();

        if !self
            .matching_engines
            .contains_key(&funding_rate.instrument_id)
        {
            let instrument = {
                let cache = self.cache.as_ref().borrow();
                cache.instrument(&funding_rate.instrument_id).cloned()
            };

            if let Some(instrument) = instrument {
                if let Err(e) = self.add_instrument(instrument) {
                    log::error!(
                        "Cannot settle funding for {}: failed to add instrument: {e}",
                        funding_rate.instrument_id
                    );
                    return;
                }
            } else {
                log::warn!(
                    "Cannot settle funding for {}: no matching engine or instrument",
                    funding_rate.instrument_id
                );
                return;
            }
        }

        let Some(settlement_price) = self.funding_settlement_price(funding_rate.instrument_id)
        else {
            log::warn!(
                "Cannot settle funding for {}: no mark price or top-of-book price",
                funding_rate.instrument_id
            );
            return;
        };

        let open_positions: Vec<Position> = {
            let cache = self.cache.borrow();
            cache
                .positions_open(
                    Some(&self.id),
                    Some(&funding_rate.instrument_id),
                    None,
                    Some(&account_id),
                    None,
                )
                .into_iter()
                .map(|position| position.cloned())
                .collect()
        };

        self.funding_settled_through
            .insert(funding_rate.instrument_id, ts_event);

        if open_positions.is_empty() {
            return;
        }

        let currency = open_positions[0].settlement_currency;
        let ts_init = self.clock.borrow().timestamp_ns();
        let settlement = FundingSettlement::new(
            msgbus::get_message_bus().borrow().trader_id,
            funding_rate.instrument_id,
            account_id,
            funding_rate.rate,
            settlement_price,
            currency,
            UUID4::new(),
            ts_event,
            ts_init,
        );
        let settlement_topic = switchboard::get_funding_settlement_topic(settlement.instrument_id);
        msgbus::publish_any(settlement_topic, &settlement);

        let mut account_adjustments: AHashMap<Currency, Decimal> = AHashMap::new();
        let mut position_events = Vec::new();

        {
            let mut cache = self.cache.borrow_mut();

            for mut position in open_positions {
                let notional = position.notional_value(settlement_price);
                let side = if position.signed_qty > 0.0 {
                    -Decimal::ONE
                } else {
                    Decimal::ONE
                };
                let amount = notional.as_decimal() * funding_rate.rate * side;

                let pnl_change = match Money::from_decimal(amount, notional.currency) {
                    Ok(money) => money,
                    Err(e) => {
                        log::error!(
                            "Cannot settle funding for position {}: invalid funding amount: {e}",
                            position.id
                        );
                        continue;
                    }
                };

                let adjustment = PositionAdjusted::new(
                    settlement.trader_id,
                    position.strategy_id,
                    position.instrument_id,
                    position.id,
                    position.account_id,
                    PositionAdjustmentType::Funding,
                    None,
                    Some(pnl_change),
                    Some(Ustr::from(&format!(
                        "funding_settlement:{}",
                        settlement.event_id
                    ))),
                    UUID4::new(),
                    settlement.ts_event,
                    settlement.ts_init,
                );
                position.apply_adjustment(adjustment);

                if let Err(e) = cache.update_position(&position) {
                    log::error!(
                        "Cannot update position {} after funding settlement: {e}",
                        position.id
                    );
                    continue;
                }

                account_adjustments
                    .entry(pnl_change.currency)
                    .and_modify(|current| *current += pnl_change.as_decimal())
                    .or_insert_with(|| pnl_change.as_decimal());
                position_events.push(PositionEvent::PositionAdjusted(adjustment));
            }
        }

        for (currency, amount) in account_adjustments {
            match Money::from_decimal(amount, currency) {
                Ok(adjustment) => self.adjust_account(adjustment),
                Err(e) => log::error!("Cannot apply funding account adjustment: {e}"),
            }
        }

        for event in position_events {
            let PositionEvent::PositionAdjusted(adjustment) = &event else {
                continue;
            };
            let topic = switchboard::get_event_positions_topic(adjustment.strategy_id);
            msgbus::publish_position_event(topic, &event);
        }
    }

    fn funding_settlement_price(&self, instrument_id: InstrumentId) -> Option<Price> {
        if let Some(mark_price) = self.cache.borrow().mark_price(&instrument_id) {
            return Some(mark_price.value);
        }

        let bid = self.best_bid_price(instrument_id)?;
        let ask = self.best_ask_price(instrument_id)?;
        let midpoint = (bid.as_decimal() + ask.as_decimal()) / Decimal::from(2);
        Price::from_decimal_dp(midpoint, bid.precision.max(ask.precision)).ok()
    }

    fn is_interval_funding_boundary(funding_rate: &FundingRateUpdate) -> bool {
        let Some(interval_mins) = funding_rate.interval else {
            return false;
        };
        let interval_ns = u64::from(interval_mins) * 60 * 1_000_000_000;
        interval_ns > 0 && funding_rate.ts_event.as_u64().is_multiple_of(interval_ns)
    }

    /// Advances the exchange clock and processes all pending inflight and queued trading commands
    /// up to `ts_now`.
    ///
    /// # Panics
    ///
    /// Panics if the exchange clock is not a [`TestClock`] or popping an inflight command fails
    /// during processing.
    pub fn process(&mut self, ts_now: UnixNanos) {
        self.set_clock_time(ts_now);

        while let Some(inflight) = self.inflight_queue.peek() {
            if inflight.timestamp > ts_now {
                break;
            }
            let inflight = self.inflight_queue.pop().unwrap();
            let timestamp = inflight.timestamp;
            self.message_queue.push_back(inflight.command);
            self.inflight_counter.remove(&timestamp);
        }

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
        if !self.account_at_starting_balances() {
            self.generate_fresh_account_state();
        }

        for module in &self.modules {
            module.reset();
        }

        for matching_engine in self.matching_engines.values_mut() {
            matching_engine.reset();
        }

        self.pending_funding_rates.clear();
        self.funding_settled_through.clear();
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

    /// Checks if any margin accounts have breached maintenance margin and liquidates open
    /// positions when the trigger threshold is met.
    ///
    /// Liquidation is scoped to the breached settlement currency: only positions whose
    /// instrument settles in the same currency as the breached margin account are closed.
    /// Positions settled in other currencies remain open, isolating the liquidation to
    /// the currency whose equity fell below the maintenance threshold.
    ///
    /// > **Note**: A future `cross_margin_mode` venue configuration could extend this to
    /// > liquidate all positions across all settlement currencies simultaneously.
    pub fn process_liquidations(&mut self, ts_now: UnixNanos) {
        if !self.liquidation_enabled {
            return;
        }

        if self.frozen_account {
            return;
        }

        let account = {
            let cache = self.cache.borrow();
            cache.account_for_venue_owned(&self.id)
        };
        let Some(account) = account else { return };
        let AccountAny::Margin(margin_account) = &account else {
            return;
        };
        let account_id = margin_account.id();

        let currencies: Vec<Currency> = margin_account.currencies();

        let open_positions: Vec<Position> = {
            let cache = self.cache.borrow();
            cache
                .positions_open(Some(&self.id), None, None, None, None)
                .into_iter()
                .map(|p| p.cloned())
                .collect()
        };

        // Pre-bucket position indices by settlement currency to avoid repeated full scans.
        let mut positions_by_currency: AHashMap<Currency, Vec<usize>> = AHashMap::new();
        for (i, p) in open_positions.iter().enumerate() {
            positions_by_currency
                .entry(p.settlement_currency)
                .or_default()
                .push(i);
        }

        for currency in currencies {
            let Some(balance) = margin_account.balance(Some(currency)) else {
                continue;
            };
            let balance_f64 = balance.total.as_f64();

            let Some(indices) = positions_by_currency.get(&currency) else {
                continue;
            };

            let (upnl_f64, all_priced) = {
                let cache = self.cache.borrow();
                let mut upnl = 0.0_f64;
                let mut all_priced = true;

                for &i in indices {
                    let p = &open_positions[i];
                    match cache.calculate_unrealized_pnl(p) {
                        Some(pnl) => upnl += pnl.as_f64(),
                        None => {
                            all_priced = false;
                            break;
                        }
                    }
                }
                (upnl, all_priced)
            };

            if !all_priced {
                continue; // defer until all positions are priced
            }

            let equity = balance_f64 + upnl_f64;
            let maintenance = margin_account.total_maintenance_margin(currency).as_f64();

            if maintenance == 0.0 {
                continue;
            }

            let threshold = maintenance * self.liquidation_trigger_ratio;

            if equity > threshold {
                continue;
            }

            log::warn!(
                "LIQUIDATION triggered for account {} currency {}: equity={:.4} <= threshold={:.4} (maintenance={:.4} x ratio={})",
                account_id,
                currency,
                equity,
                threshold,
                maintenance,
                self.liquidation_trigger_ratio
            );

            for matching_engine in self.matching_engines.values_mut() {
                matching_engine.liquidate_open_positions(
                    ts_now,
                    self.liquidation_cancel_open_orders,
                    currency,
                );
            }

            break;
        }
    }

    fn process_trading_command(&mut self, command: TradingCommand) {
        let instrument_id = command.instrument_id();
        assert!(
            self.matching_engines.contains_key(&instrument_id),
            "Matching engine not found for instrument {instrument_id}",
        );

        if let TradingCommand::ModifyOrder(ref command) = command
            && self.process_modify_submitted_order(command)
        {
            return;
        }

        if let Some(matching_engine) = self.matching_engines.get_mut(&instrument_id) {
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
                        .map(|o| o.clone())
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
            panic!("Matching engine not found for instrument {instrument_id}");
        }
    }

    fn process_modify_submitted_order(&self, command: &ModifyOrder) -> bool {
        let Some(order) = self
            .cache
            .borrow()
            .order(&command.client_order_id)
            .map(|o| o.clone())
        else {
            return false;
        };

        let modifies_submitted_order = matches!(order.status(), OrderStatus::Submitted)
            || (matches!(order.status(), OrderStatus::PendingUpdate)
                && order
                    .previous_status()
                    .is_some_and(|status| matches!(status, OrderStatus::Submitted)));

        if !modifies_submitted_order {
            return false;
        }

        self.generate_order_updated(
            &order,
            command.quantity.unwrap_or_else(|| order.quantity()),
            command.price.or_else(|| order.price()),
            command.trigger_price.or_else(|| order.trigger_price()),
        );
        true
    }

    fn generate_order_updated(
        &self,
        order: &OrderAny,
        quantity: Quantity,
        price: Option<Price>,
        trigger_price: Option<Price>,
    ) {
        let ts_now = self.clock.borrow().timestamp_ns();
        let event = OrderEventAny::Updated(OrderUpdated::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            quantity,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            order.venue_order_id(),
            order.account_id(),
            price,
            trigger_price,
            None,
            order.is_quote_quantity(),
        ));
        Self::dispatch_order_event(event);
    }

    fn dispatch_order_event(event: OrderEventAny) {
        msgbus::send_order_event(MessagingSwitchboard::exec_engine_process(), event);
    }

    fn account_at_starting_balances(&self) -> bool {
        let Some(account) = self.get_account() else {
            return false;
        };

        let balances = account.balances();

        for starting in &self.starting_balances {
            let Some(balance) = balances.get(&starting.currency) else {
                return false;
            };

            if balance.total != *starting || balance.free != *starting {
                return false;
            }
        }

        true
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

        let calculate_account_state = !self.frozen_account;

        if let Some(mut account) = self.get_account() {
            account.set_calculate_account_state(calculate_account_state);

            match &mut account {
                AccountAny::Margin(margin_account) => {
                    margin_account.set_default_leverage(self.default_leverage);
                    for (instrument_id, leverage) in &self.leverages {
                        margin_account.set_leverage(*instrument_id, *leverage);
                    }

                    if let Some(model) = &self.margin_model {
                        margin_account.set_margin_model(model.clone());
                    }
                }
                AccountAny::Cash(cash_account) => {
                    cash_account.allow_borrowing = self.allow_cash_borrowing;
                }
                AccountAny::Betting(_) => {}
            }

            self.cache.borrow_mut().update_account(&account).unwrap();
        }
    }
}
