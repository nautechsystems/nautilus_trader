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

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use nautilus_common::{cache::Cache, msgbus::MessageBus};
use nautilus_core::{
    correctness::{check_equal, FAILED},
    AtomicTime, UnixNanos,
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
    instruments::InstrumentAny,
    orderbook::OrderBook,
    orders::PassiveOrderAny,
    types::{AccountBalance, Currency, Money, Price},
};
use rust_decimal::{prelude::ToPrimitive, Decimal};

use crate::modules::SimulationModule;

pub struct SimulatedExchange {
    id: Venue,
    oms_type: OmsType,
    account_type: AccountType,
    starting_balances: Vec<Money>,
    book_type: BookType,
    default_leverage: Decimal,
    exec_client: Option<ExecutionClient>,
    fee_model: FeeModelAny,
    fill_model: FillModel,
    latency_model: LatencyModel,
    instruments: HashMap<InstrumentId, InstrumentAny>,
    matching_engines: HashMap<InstrumentId, OrderMatchingEngine>,
    leverages: HashMap<InstrumentId, Decimal>,
    modules: Vec<Box<dyn SimulationModule>>,
    clock: &'static AtomicTime,
    msgbus: Rc<RefCell<MessageBus>>,
    cache: Rc<RefCell<Cache>>,
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
        msgbus: Rc<RefCell<MessageBus>>, // TODO add portfolio
        cache: Rc<RefCell<Cache>>,
        clock: &'static AtomicTime,
        fill_model: FillModel,
        fee_model: FeeModelAny,
        latency_model: LatencyModel,
        book_type: BookType,
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
            fee_model,
            fill_model,
            latency_model,
            instruments: HashMap::new(),
            matching_engines: HashMap::new(),
            leverages,
            modules,
            clock,
            msgbus,
            cache,
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

    pub fn register_client(&mut self, client: ExecutionClient) {
        let client_id = client.client_id;
        self.exec_client = Some(client);
        log::info!("Registered ExecutionClient: {client_id}");
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

    pub fn set_latency_model(&mut self, latency_model: LatencyModel) {
        self.latency_model = latency_model;
        log::info!("Setting latency model to {}", self.latency_model);
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
            self.clock,
            Rc::clone(&self.msgbus),
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
    pub fn get_matching_engine(&self, instrument_id: InstrumentId) -> Option<&OrderMatchingEngine> {
        self.matching_engines.get(&instrument_id)
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
            let venue = exec_client.venue;
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
                                    self.clock.get_time_ns(),
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

    pub fn send(&self, _command: TradingCommand) {
        todo!("send")
    }

    pub fn generate_inflight_command(&self, _command: TradingCommand) {
        todo!("generate inflight command")
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

    pub fn process(&mut self, _ts_now: UnixNanos) {
        todo!("process")
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
                exec_client.account_id
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
                TradingCommand::QueryOrder(ref command) => {
                    matching_engine.process_query_order(command, account_id);
                }
                TradingCommand::SubmitOrderList(mut command) => {
                    for order in &mut command.order_list.orders {
                        matching_engine.process_order(order, account_id);
                    }
                }
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
                .generate_account_state(balances, vec![], true, self.clock.get_time_ns())
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
    use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::LazyLock};

    use nautilus_common::{
        cache::Cache,
        msgbus::{
            stubs::{get_message_saving_handler, get_saved_messages},
            MessageBus,
        },
    };
    use nautilus_core::{AtomicTime, UnixNanos, UUID4};
    use nautilus_execution::{
        client::ExecutionClient,
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
            OmsType, OrderSide,
        },
        events::AccountState,
        identifiers::{AccountId, ClientId, TradeId, TraderId, Venue},
        instruments::{stubs::crypto_perpetual_ethusdt, CryptoPerpetual, InstrumentAny},
        types::{AccountBalance, Currency, Money, Price, Quantity},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use crate::exchange::SimulatedExchange;

    static ATOMIC_TIME: LazyLock<AtomicTime> =
        LazyLock::new(|| AtomicTime::new(true, UnixNanos::default()));

    fn get_exchange(
        venue: Venue,
        account_type: AccountType,
        book_type: BookType,
        msgbus: Option<Rc<RefCell<MessageBus>>>,
        cache: Option<Rc<RefCell<Cache>>>,
    ) -> SimulatedExchange {
        let msgbus = msgbus.unwrap_or(Rc::new(RefCell::new(MessageBus::default())));
        let cache = cache.unwrap_or(Rc::new(RefCell::new(Cache::default())));

        let mut exchange = SimulatedExchange::new(
            venue,
            OmsType::Netting,
            account_type,
            vec![Money::new(1000.0, Currency::USD())],
            None,
            1.into(),
            HashMap::new(),
            vec![],
            msgbus.clone(),
            cache.clone(),
            &ATOMIC_TIME,
            FillModel::default(),
            FeeModelAny::MakerTaker(MakerTakerFeeModel),
            LatencyModel,
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
        )
        .unwrap();

        let execution_client = ExecutionClient::new(
            TraderId::default(),
            ClientId::default(),
            venue,
            OmsType::Netting,
            AccountId::default(),
            account_type,
            None,
            &ATOMIC_TIME,
            cache,
            msgbus,
        );
        exchange.register_client(execution_client);

        exchange
    }

    #[rstest]
    #[should_panic(
        expected = r#"Condition failed: 'Venue of instrument id' value of BINANCE was not equal to 'Venue of simulated exchange' value of SIM"#
    )]
    fn test_venue_mismatch_between_exchange_and_instrument(
        crypto_perpetual_ethusdt: CryptoPerpetual,
    ) {
        let mut exchange: SimulatedExchange = get_exchange(
            Venue::new("SIM"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        exchange.add_instrument(instrument).unwrap();
    }

    #[rstest]
    #[should_panic(expected = "Cash account cannot trade futures or perpetuals")]
    fn test_cash_account_trading_futures_or_perpetuals(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let mut exchange: SimulatedExchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Cash,
            BookType::L1_MBP,
            None,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        exchange.add_instrument(instrument).unwrap();
    }

    #[rstest]
    fn test_exchange_process_quote_tick(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let mut exchange: SimulatedExchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.add_instrument(instrument).unwrap();

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
        exchange.process_quote_tick(&quote_tick);

        let best_bid_price = exchange.best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1000")));
        let best_ask_price = exchange.best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask_price, Some(Price::from("1001")));
    }

    #[rstest]
    fn test_exchange_process_trade_tick(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let mut exchange: SimulatedExchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.add_instrument(instrument).unwrap();

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
        exchange.process_trade_tick(&trade_tick);

        let best_bid_price = exchange.best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1000")));
        let best_ask = exchange.best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask, Some(Price::from("1000")));
    }

    #[rstest]
    fn test_exchange_process_bar_last_bar_spec(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let mut exchange: SimulatedExchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.add_instrument(instrument).unwrap();

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
        exchange.process_bar(bar);

        // this will be processed as ticks so both bid and ask will be the same as close of the bar
        let best_bid_price = exchange.best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1502.00")));
        let best_ask_price = exchange.best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask_price, Some(Price::from("1502.00")));
    }

    #[rstest]
    fn test_exchange_process_bar_bid_ask_bar_spec(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let mut exchange: SimulatedExchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L1_MBP,
            None,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.add_instrument(instrument).unwrap();

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
        exchange.process_bar(bar_bid);
        exchange.process_bar(bar_ask);

        // current bid and ask prices will be the corresponding close of the ask and bid bar
        let best_bid_price = exchange.best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1502.00")));
        let best_ask_price = exchange.best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask_price, Some(Price::from("1503.00")));
    }

    #[rstest]
    fn test_exchange_process_orderbook_delta(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let mut exchange: SimulatedExchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L2_MBP,
            None,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.add_instrument(instrument).unwrap();

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
        exchange.process_order_book_delta(delta_buy);
        exchange.process_order_book_delta(delta_sell);

        let book = exchange.get_book(crypto_perpetual_ethusdt.id).unwrap();
        assert_eq!(book.count, 2);
        assert_eq!(book.sequence, 1);
        assert_eq!(book.ts_last, UnixNanos::from(2));
        let best_bid_price = exchange.best_bid_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_bid_price, Some(Price::from("1000.00")));
        let best_ask_price = exchange.best_ask_price(crypto_perpetual_ethusdt.id);
        assert_eq!(best_ask_price, Some(Price::from("1001.00")));
    }

    #[rstest]
    fn test_exchange_process_orderbook_deltas(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let mut exchange: SimulatedExchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L2_MBP,
            None,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.add_instrument(instrument).unwrap();

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
        exchange.process_order_book_deltas(orderbook_deltas);

        let book = exchange.get_book(crypto_perpetual_ethusdt.id).unwrap();
        assert_eq!(book.count, 2);
        assert_eq!(book.sequence, 1);
        assert_eq!(book.ts_last, UnixNanos::from(1));
        let best_bid_price = exchange.best_bid_price(crypto_perpetual_ethusdt.id);
        // no bid orders in orderbook deltas
        assert_eq!(best_bid_price, None);
        let best_ask_price = exchange.best_ask_price(crypto_perpetual_ethusdt.id);
        // best ask price is the first order in orderbook deltas
        assert_eq!(best_ask_price, Some(Price::from("1000.00")));
    }

    #[rstest]
    fn test_exchange_process_instrument_status(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let mut exchange: SimulatedExchange = get_exchange(
            Venue::new("BINANCE"),
            AccountType::Margin,
            BookType::L2_MBP,
            None,
            None,
        );
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        // register instrument
        exchange.add_instrument(instrument).unwrap();

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

        exchange.process_instrument_status(instrument_status);

        let matching_engine = exchange
            .get_matching_engine(crypto_perpetual_ethusdt.id)
            .unwrap();
        assert_eq!(matching_engine.market_status, MarketStatus::Closed);
    }

    #[rstest]
    fn test_accounting() {
        let account_type = AccountType::Margin;
        let mut msgbus = MessageBus::default();
        let mut cache = Cache::default();
        let handler = get_message_saving_handler::<AccountState>(None);
        msgbus.register(Ustr::from("Portfolio.update_account"), handler.clone());
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

        let mut exchange = get_exchange(
            Venue::new("SIM"),
            account_type,
            BookType::L2_MBP,
            Some(Rc::new(RefCell::new(msgbus))),
            Some(Rc::new(RefCell::new(cache))),
        );
        exchange.initialize_account();

        // Test adjust account, increase balance by 500 USD
        exchange.adjust_account(Money::from("500 USD"));

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
}
