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

//! Provides a `SimulatedExchange` venue for backtesting on historical data.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
    cell::RefCell,
    collections::{BinaryHeap, HashMap, VecDeque},
    rc::Rc,
};

use nautilus_common::{cache::Cache, clock::Clock};
use nautilus_core::{
    UnixNanos,
    correctness::{FAILED, check_equal},
};
use nautilus_execution::{
    client::ExecutionClient,
    matching_engine::{config::OrderMatchingEngineConfig, engine::OrderMatchingEngine},
    messages::TradingCommand,
    models::{fee::FeeModelAny, fill::FillModel, latency::LatencyModel},
};
use nautilus_model::{
    accounts::AccountAny,
    data::{
        Bar, Data, InstrumentStatus, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API,
        QuoteTick, TradeTick,
    },
    enums::{AccountType, BookType, OmsType},
    identifiers::{InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    orders::PassiveOrderAny,
    types::{AccountBalance, Currency, Money, Price},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use crate::modules::SimulationModule;

/// Represents commands with simulated network latency in a min-heap priority queue.
/// The commands are ordered by timestamp for FIFO processing, with the
/// earliest timestamp having the highest priority in the queue.
#[derive(Debug, Eq, PartialEq)]
struct InflightCommand {
    ts: UnixNanos,
    counter: u32,
    command: TradingCommand,
}

impl InflightCommand {
    const fn new(ts: UnixNanos, counter: u32, command: TradingCommand) -> Self {
        Self {
            ts,
            counter,
            command,
        }
    }
}

impl Ord for InflightCommand {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering for min-heap (earliest timestamp first then lowest counter)
        other
            .ts
            .cmp(&self.ts)
            .then_with(|| other.counter.cmp(&self.counter))
    }
}

impl PartialOrd for InflightCommand {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub struct SimulatedExchange {
    pub id: Venue,
    pub oms_type: OmsType,
    pub account_type: AccountType,
    starting_balances: Vec<Money>,
    book_type: BookType,
    default_leverage: Decimal,
    exec_client: Option<Rc<dyn ExecutionClient>>,
    pub base_currency: Option<Currency>,
    fee_model: FeeModelAny,
    fill_model: FillModel,
    latency_model: Option<LatencyModel>,
    instruments: HashMap<InstrumentId, InstrumentAny>,
    matching_engines: HashMap<InstrumentId, OrderMatchingEngine>,
    leverages: HashMap<InstrumentId, Decimal>,
    modules: Vec<Box<dyn SimulationModule>>,
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    message_queue: VecDeque<TradingCommand>,
    inflight_queue: BinaryHeap<InflightCommand>,
    inflight_counter: HashMap<UnixNanos, u32>,
    frozen_account: bool,
    bar_execution: bool,
    reject_stop_orders: bool,
    support_gtd_orders: bool,
    support_contingent_orders: bool,
    use_position_ids: bool,
    use_random_ids: bool,
    use_reduce_only: bool,
    use_message_queue: bool,
}

impl SimulatedExchange {
    /// Creates a new [`SimulatedExchange`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        venue: Venue,
        oms_type: OmsType,
        account_type: AccountType,
        starting_balances: Vec<Money>,
        base_currency: Option<Currency>,
        default_leverage: Decimal,
        leverages: HashMap<InstrumentId, Decimal>,
        modules: Vec<Box<dyn SimulationModule>>,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
        fill_model: FillModel,
        fee_model: FeeModelAny,
        book_type: BookType,
        latency_model: Option<LatencyModel>,
        frozen_account: Option<bool>,
        bar_execution: Option<bool>,
        reject_stop_orders: Option<bool>,
        support_gtd_orders: Option<bool>,
        support_contingent_orders: Option<bool>,
        use_position_ids: Option<bool>,
        use_random_ids: Option<bool>,
        use_reduce_only: Option<bool>,
        use_message_queue: Option<bool>,
    ) -> anyhow::Result<Self> {
        if starting_balances.is_empty() {
            anyhow::bail!("Starting balances must be provided")
        }
        if base_currency.is_some() && starting_balances.len() > 1 {
            anyhow::bail!("single-currency account has multiple starting currencies")
        }
        // TODO register and load modules
        Ok(Self {
            id: venue,
            oms_type,
            account_type,
            starting_balances,
            book_type,
            default_leverage,
            exec_client: None,
            base_currency,
            fee_model,
            fill_model,
            latency_model,
            instruments: HashMap::new(),
            matching_engines: HashMap::new(),
            leverages,
            modules,
            clock,
            cache,
            message_queue: VecDeque::new(),
            inflight_queue: BinaryHeap::new(),
            inflight_counter: HashMap::new(),
            frozen_account: frozen_account.unwrap_or(false),
            bar_execution: bar_execution.unwrap_or(true),
            reject_stop_orders: reject_stop_orders.unwrap_or(true),
            support_gtd_orders: support_gtd_orders.unwrap_or(true),
            support_contingent_orders: support_contingent_orders.unwrap_or(true),
            use_position_ids: use_position_ids.unwrap_or(true),
            use_random_ids: use_random_ids.unwrap_or(false),
            use_reduce_only: use_reduce_only.unwrap_or(true),
            use_message_queue: use_message_queue.unwrap_or(true),
        })
    }

    pub fn register_client(&mut self, client: Rc<dyn ExecutionClient>) {
        self.exec_client = Some(client);
    }

    pub fn set_fill_model(&mut self, fill_model: FillModel) {
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

    pub const fn set_latency_model(&mut self, latency_model: LatencyModel) {
        self.latency_model = Some(latency_model);
    }

    pub fn initialize_account(&mut self) {
        self.generate_fresh_account_state();
    }

    pub fn add_instrument(&mut self, instrument: InstrumentAny) -> anyhow::Result<()> {
        check_equal(
            instrument.id().venue,
            self.id,
            "Venue of instrument id",
            "Venue of simulated exchange",
        )
        .expect(FAILED);

        if self.account_type == AccountType::Cash
            && (matches!(instrument, InstrumentAny::CryptoPerpetual(_))
                || matches!(instrument, InstrumentAny::CryptoFuture(_)))
        {
            anyhow::bail!("Cash account cannot trade futures or perpetuals")
        }

        self.instruments.insert(instrument.id(), instrument.clone());

        let matching_engine_config = OrderMatchingEngineConfig::new(
            self.bar_execution,
            self.reject_stop_orders,
            self.support_gtd_orders,
            self.support_contingent_orders,
            self.use_position_ids,
            self.use_random_ids,
            self.use_reduce_only,
        );
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

    #[must_use]
    pub fn best_bid_price(&self, instrument_id: InstrumentId) -> Option<Price> {
        self.matching_engines
            .get(&instrument_id)
            .and_then(OrderMatchingEngine::best_bid_price)
    }

    #[must_use]
    pub fn best_ask_price(&self, instrument_id: InstrumentId) -> Option<Price> {
        self.matching_engines
            .get(&instrument_id)
            .and_then(OrderMatchingEngine::best_ask_price)
    }

    pub fn get_book(&self, instrument_id: InstrumentId) -> Option<&OrderBook> {
        self.matching_engines
            .get(&instrument_id)
            .map(OrderMatchingEngine::get_book)
    }

    #[must_use]
    pub fn get_matching_engine(
        &self,
        instrument_id: &InstrumentId,
    ) -> Option<&OrderMatchingEngine> {
        self.matching_engines.get(instrument_id)
    }

    #[must_use]
    pub const fn get_matching_engines(&self) -> &HashMap<InstrumentId, OrderMatchingEngine> {
        &self.matching_engines
    }

    #[must_use]
    pub fn get_books(&self) -> HashMap<InstrumentId, OrderBook> {
        let mut books = HashMap::new();
        for (instrument_id, matching_engine) in &self.matching_engines {
            books.insert(*instrument_id, matching_engine.get_book().clone());
        }
        books
    }

    #[must_use]
    pub fn get_open_orders(&self, instrument_id: Option<InstrumentId>) -> Vec<PassiveOrderAny> {
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

    #[must_use]
    pub fn get_open_bid_orders(&self, instrument_id: Option<InstrumentId>) -> Vec<PassiveOrderAny> {
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

    #[must_use]
    pub fn get_open_ask_orders(&self, instrument_id: Option<InstrumentId>) -> Vec<PassiveOrderAny> {
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

    #[must_use]
    pub fn get_account(&self) -> Option<AccountAny> {
        self.exec_client
            .as_ref()
            .map(|client| client.get_account().unwrap())
    }

    pub fn adjust_account(&mut self, adjustment: Money) {
        if self.frozen_account {
            // Nothing to adjust
            return;
        }

        if let Some(exec_client) = &self.exec_client {
            let venue = exec_client.venue();
            println!("Adjusting account for venue {venue}");
            if let Some(account) = self.cache.borrow().account_for_venue(&venue) {
                match account.balance(Some(adjustment.currency)) {
                    Some(balance) => {
                        let mut current_balance = *balance;
                        current_balance.total += adjustment;
                        current_balance.free += adjustment;

                        let margins = match account {
                            AccountAny::Margin(margin_account) => margin_account.margins.clone(),
                            _ => HashMap::new(),
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

    pub fn send(&mut self, command: TradingCommand) {
        if !self.use_message_queue {
            self.process_trading_command(command);
        } else if self.latency_model.is_none() {
            self.message_queue.push_back(command);
        } else {
            let (ts, counter) = self.generate_inflight_command(&command);
            self.inflight_queue
                .push(InflightCommand::new(ts, counter, command));
        }
    }

    pub fn generate_inflight_command(&mut self, command: &TradingCommand) -> (UnixNanos, u32) {
        if let Some(latency_model) = &self.latency_model {
            let ts = match command {
                TradingCommand::SubmitOrder(_) | TradingCommand::SubmitOrderList(_) => {
                    command.ts_init() + latency_model.insert_latency_nanos
                }
                TradingCommand::ModifyOrder(_) => {
                    command.ts_init() + latency_model.update_latency_nanos
                }
                TradingCommand::CancelOrder(_)
                | TradingCommand::CancelAllOrders(_)
                | TradingCommand::BatchCancelOrders(_) => {
                    command.ts_init() + latency_model.delete_latency_nanos
                }
                _ => panic!("Invalid command was {command}"),
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

    pub fn process_order_book_delta(&mut self, delta: OrderBookDelta) {
        for module in &self.modules {
            module.pre_process(Data::Delta(delta));
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
            matching_engine.process_order_book_delta(&delta);
        } else {
            panic!("Matching engine should be initialized");
        }
    }

    pub fn process_order_book_deltas(&mut self, deltas: OrderBookDeltas) {
        for module in &self.modules {
            module.pre_process(Data::Deltas(OrderBookDeltas_API::new(deltas.clone())));
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
            matching_engine.process_order_book_deltas(&deltas);
        } else {
            panic!("Matching engine should be initialized");
        }
    }

    pub fn process_quote_tick(&mut self, quote: &QuoteTick) {
        for module in &self.modules {
            module.pre_process(Data::Quote(quote.to_owned()));
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

    pub fn process_trade_tick(&mut self, trade: &TradeTick) {
        for module in &self.modules {
            module.pre_process(Data::Trade(trade.to_owned()));
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

    pub fn process_bar(&mut self, bar: Bar) {
        for module in &self.modules {
            module.pre_process(Data::Bar(bar));
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

    pub fn process_instrument_status(&mut self, status: InstrumentStatus) {
        // TODO add module preprocessing

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

    pub fn process(&mut self, ts_now: UnixNanos) {
        // TODO implement correct clock fixed time setting self.clock.set_time(ts_now);

        // Process inflight commands
        while let Some(inflight) = self.inflight_queue.peek() {
            if inflight.ts > ts_now {
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

    pub fn reset(&mut self) {
        for module in &self.modules {
            module.reset();
        }

        self.generate_fresh_account_state();

        for matching_engine in self.matching_engines.values_mut() {
            matching_engine.reset();
        }

        // TODO Clear the inflight and message queues
        log::info!("Resetting exchange state");
    }

    pub fn process_trading_command(&mut self, command: TradingCommand) {
        if let Some(matching_engine) = self.matching_engines.get_mut(&command.instrument_id()) {
            let account_id = if let Some(exec_client) = &self.exec_client {
                exec_client.account_id()
            } else {
                panic!("Execution client should be initialized");
            };
            match command {
                TradingCommand::SubmitOrder(mut command) => {
                    matching_engine.process_order(&mut command.order, account_id);
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
                TradingCommand::SubmitOrderList(mut command) => {
                    for order in &mut command.order_list.orders {
                        matching_engine.process_order(order, account_id);
                    }
                }
                _ => {}
            }
        } else {
            panic!("Matching engine should be initialized");
        }
    }

    pub fn generate_fresh_account_state(&self) {
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

        // Set leverages
        if let Some(AccountAny::Margin(mut margin_account)) = self.get_account() {
            margin_account.set_default_leverage(self.default_leverage.to_f64().unwrap());

            // Set instrument specific leverages
            for (instrument_id, leverage) in &self.leverages {
                margin_account.set_leverage(*instrument_id, leverage.to_f64().unwrap());
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::{BinaryHeap, HashMap},
        rc::Rc,
        sync::LazyLock,
    };

    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        msgbus::{
            self,
            stubs::{get_message_saving_handler, get_saved_messages},
        },
    };
    use nautilus_core::{AtomicTime, UUID4, UnixNanos};
    use nautilus_execution::{
        messages::{SubmitOrder, TradingCommand},
        models::{
            fee::{FeeModelAny, MakerTakerFeeModel},
            fill::FillModel,
            latency::LatencyModel,
        },
    };
    use nautilus_model::{
        accounts::{AccountAny, MarginAccount},
        data::{
            Bar, BarType, BookOrder, InstrumentStatus, OrderBookDelta, OrderBookDeltas, QuoteTick,
            TradeTick,
        },
        enums::{
            AccountType, AggressorSide, BookAction, BookType, MarketStatus, MarketStatusAction,
            OmsType, OrderSide, OrderType,
        },
        events::AccountState,
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, Venue,
            VenueOrderId,
        },
        instruments::{CryptoPerpetual, InstrumentAny, stubs::crypto_perpetual_ethusdt},
        orders::OrderTestBuilder,
        types::{AccountBalance, Currency, Money, Price, Quantity},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use crate::{
        exchange::{InflightCommand, SimulatedExchange},
        execution_client::BacktestExecutionClient,
    };

    static ATOMIC_TIME: LazyLock<AtomicTime> =
        LazyLock::new(|| AtomicTime::new(true, UnixNanos::default()));

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
                HashMap::new(),
                vec![],
                cache.clone(),
                clock,
                FillModel::default(),
                FeeModelAny::MakerTaker(MakerTakerFeeModel),
                book_type,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        ));

        let clock = TestClock::new();
        let execution_client = BacktestExecutionClient::new(
            TraderId::default(),
            AccountId::default(),
            exchange.clone(),
            cache.clone(),
            Rc::new(RefCell::new(clock)),
            None,
            None,
        );
        exchange
            .borrow_mut()
            .register_client(Rc::new(execution_client));

        exchange
    }

    fn create_submit_order_command(ts_init: UnixNanos) -> TradingCommand {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_id)
            .quantity(Quantity::from(1))
            .build();
        TradingCommand::SubmitOrder(
            SubmitOrder::new(
                TraderId::default(),
                ClientId::default(),
                StrategyId::default(),
                instrument_id,
                ClientOrderId::default(),
                VenueOrderId::default(),
                order,
                None,
                None,
                UUID4::default(),
                ts_init,
            )
            .unwrap(),
        )
    }

    #[rstest]
    #[should_panic(
        expected = r#"Condition failed: 'Venue of instrument id' value of BINANCE was not equal to 'Venue of simulated exchange' value of SIM"#
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
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // process tick
        let quote_tick = QuoteTick::new(
            crypto_perpetual_ethusdt.id,
            Price::from("1000"),
            Price::from("1001"),
            Quantity::from(1),
            Quantity::from(1),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        exchange.borrow_mut().process_quote_tick(&quote_tick);

        let best_bid_price = exchange
            .borrow()
            .best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1000")));
        let best_ask_price = exchange
            .borrow()
            .best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask_price, Some(Price::from("1001")));
    }

    #[rstest]
    fn test_exchange_process_trade_tick(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // process tick
        let trade_tick = TradeTick::new(
            crypto_perpetual_ethusdt.id,
            Price::from("1000"),
            Quantity::from(1),
            AggressorSide::Buyer,
            TradeId::from("1"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        exchange.borrow_mut().process_trade_tick(&trade_tick);

        let best_bid_price = exchange
            .borrow()
            .best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1000")));
        let best_ask = exchange
            .borrow()
            .best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask, Some(Price::from("1000")));
    }

    #[rstest]
    fn test_exchange_process_bar_last_bar_spec(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let exchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // process bar
        let bar = Bar::new(
            BarType::from("ETHUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            Price::from("1500.00"),
            Price::from("1505.00"),
            Price::from("1490.00"),
            Price::from("1502.00"),
            Quantity::from(100),
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
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

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
            Quantity::from(100),
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        let bar_ask = Bar::new(
            BarType::from("ETHUSDT-PERP.BINANCE-1-MINUTE-ASK-EXTERNAL"),
            Price::from("1501.00"),
            Price::from("1506.00"),
            Price::from("1491.00"),
            Price::from("1503.00"),
            Quantity::from(100),
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
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // create order book delta at both bid and ask with incremented ts init and sequence
        let delta_buy = OrderBookDelta::new(
            crypto_perpetual_ethusdt.id,
            BookAction::Add,
            BookOrder::new(OrderSide::Buy, Price::from("1000.00"), Quantity::from(1), 1),
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
                Quantity::from(1),
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
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        // create two sell order book deltas with same timestamps and higher sequence
        let delta_sell_1 = OrderBookDelta::new(
            crypto_perpetual_ethusdt.id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("1000.00"),
                Quantity::from(3),
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
                Quantity::from(1),
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
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

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
        let handler = get_message_saving_handler::<AccountState>(None);
        msgbus::register(Ustr::from("Portfolio.update_account"), handler.clone());
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
        let messages = get_saved_messages::<AccountState>(handler);
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
        let inflight1 = InflightCommand::new(
            UnixNanos::from(100),
            1,
            create_submit_order_command(UnixNanos::from(100)),
        );
        let inflight2 = InflightCommand::new(
            UnixNanos::from(200),
            2,
            create_submit_order_command(UnixNanos::from(200)),
        );
        let inflight3 = InflightCommand::new(
            UnixNanos::from(100),
            2,
            create_submit_order_command(UnixNanos::from(100)),
        );

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

        assert_eq!(first.ts, UnixNanos::from(100));
        assert_eq!(first.counter, 1);
        assert_eq!(second.ts, UnixNanos::from(100));
        assert_eq!(second.counter, 2);
        assert_eq!(third.ts, UnixNanos::from(200));
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

        let command1 = create_submit_order_command(UnixNanos::from(100));
        let command2 = create_submit_order_command(UnixNanos::from(200));

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
        let latency_model = LatencyModel::new(
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
        exchange.borrow_mut().set_latency_model(latency_model);

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        exchange.borrow_mut().add_instrument(instrument).unwrap();

        let command1 = create_submit_order_command(UnixNanos::from(100));
        let command2 = create_submit_order_command(UnixNanos::from(150));
        exchange.borrow_mut().send(command1);
        exchange.borrow_mut().send(command2);

        // Verify that inflight queue has 2 commands and message queue is empty
        assert_eq!(exchange.borrow().message_queue.len(), 0);
        assert_eq!(exchange.borrow().inflight_queue.len(), 2);
        // First inflight command should have timestamp at 100 and 200 insert latency
        assert_eq!(
            exchange.borrow().inflight_queue.iter().nth(0).unwrap().ts,
            UnixNanos::from(300)
        );
        // Second inflight command should have timestamp at 150 and 200 insert latency
        assert_eq!(
            exchange.borrow().inflight_queue.iter().nth(1).unwrap().ts,
            UnixNanos::from(350)
        );

        // Process at timestamp 350, and test that only first command is processed
        exchange.borrow_mut().process(UnixNanos::from(320));
        assert_eq!(exchange.borrow().message_queue.len(), 0);
        assert_eq!(exchange.borrow().inflight_queue.len(), 1);
        assert_eq!(
            exchange.borrow().inflight_queue.iter().nth(0).unwrap().ts,
            UnixNanos::from(350)
        );
    }
}
