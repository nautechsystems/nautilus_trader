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

use std::{
    any::Any,
    cell::{RefCell, UnsafeCell},
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::Arc,
};

use log::LevelFilter;
use nautilus_model::{
    data::{Bar, OrderBookDelta, QuoteTick, TradeTick},
    identifiers::TraderId,
    instruments::{CurrencyPair, stubs::audusd_sim},
    orderbook::OrderBook,
};
use rstest::{fixture, rstest};
use ustr::Ustr;

use super::{Actor, DataActor, DataActorCore, data_actor::DataActorConfig};
use crate::{
    actor::registry::{get_actor_unchecked, register_actor},
    cache::Cache,
    clock::{Clock, TestClock},
    enums::ComponentState,
    logging::{logger::LogGuard, logging_is_initialized},
    msgbus::{
        self,
        switchboard::{MessagingSwitchboard, get_quotes_topic, get_trades_topic},
    },
    testing::init_logger_for_testing,
};

struct TestDataActor {
    core: DataActorCore,
    pub received_data: Vec<String>, // Use string for simplicity
    pub received_books: Vec<OrderBook>,
    pub _received_deltas: Vec<OrderBookDelta>,
    pub received_quotes: Vec<QuoteTick>,
    pub received_trades: Vec<TradeTick>,
    pub _received_bars: Vec<Bar>,
}

impl Deref for TestDataActor {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for TestDataActor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl Actor for TestDataActor {
    fn id(&self) -> Ustr {
        self.core.actor_id.inner()
    }

    fn handle(&mut self, msg: &dyn Any) {
        // Let the core handle message routing
        self.core.handle(msg);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Implement DataActor trait overriding handlers as required
impl DataActor for TestDataActor {
    fn state(&self) -> ComponentState {
        self.core.state()
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting actor"); // Custom log
        Ok(())
    }

    fn on_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        self.received_data.push(format!("{data:?}"));
        Ok(())
    }

    fn on_book(&mut self, book: &OrderBook) -> anyhow::Result<()> {
        self.received_books.push(book.clone());
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.received_quotes.push(*quote);
        Ok(())
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        self.received_trades.push(*trade);
        Ok(())
    }
}

// Custom functionality as required
impl TestDataActor {
    pub fn new(
        config: DataActorConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> Self {
        Self {
            core: DataActorCore::new(config, cache, clock),
            received_data: Vec::new(),
            received_books: Vec::new(),
            _received_deltas: Vec::new(),
            received_quotes: Vec::new(),
            received_trades: Vec::new(),
            _received_bars: Vec::new(),
        }
    }
    #[allow(dead_code)] // TODO: Under development
    pub fn custom_function(&mut self) {}
}

#[fixture]
pub fn clock() -> Rc<RefCell<TestClock>> {
    Rc::new(RefCell::new(TestClock::new()))
}

#[fixture]
pub fn cache() -> Rc<RefCell<Cache>> {
    Rc::new(RefCell::new(Cache::new(None, None)))
}

#[fixture]
fn switchboard() -> Arc<MessagingSwitchboard> {
    Arc::new(MessagingSwitchboard::default())
}

#[fixture]
fn trader_id() -> TraderId {
    TraderId::from("TRADER-000")
}

#[fixture]
fn test_logging() -> Option<LogGuard> {
    // TODO: Using u8 for now due FFI (change when Cython gone)
    if logging_is_initialized() == 1 {
        return None;
    }

    Some(init_logger_for_testing(Some(LevelFilter::Trace)).unwrap())
}

fn register_data_actor(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) -> Ustr {
    let config = DataActorConfig::default();
    let mut actor = TestDataActor::new(config, cache, clock);
    let actor_id = actor.actor_id;
    actor.set_trader_id(trader_id);
    actor.initialize().unwrap();

    let actor_rc = Rc::new(UnsafeCell::new(actor));
    register_actor(actor_rc);
    actor_id.inner()
}

#[rstest]
fn test_subscribe_and_receive_quotes(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_quotes::<TestDataActor>(audusd_sim.id, None, None);

    let topic = get_quotes_topic(audusd_sim.id);
    let quote = QuoteTick::default();
    msgbus::publish(&topic, &quote);
    msgbus::publish(&topic, &quote);

    assert_eq!(actor.received_quotes.len(), 2);
}

#[rstest]
fn test_unsubscribe_quotes(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let _guard = test_logging();
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_quotes::<TestDataActor>(audusd_sim.id, None, None);

    let topic = get_quotes_topic(audusd_sim.id);
    let quote = QuoteTick::default();
    msgbus::publish(&topic, &quote);
    msgbus::publish(&topic, &quote);

    actor.unsubscribe_quotes::<TestDataActor>(audusd_sim.id, None, None);

    // Publish more quotes
    msgbus::publish(&topic, &quote);
    msgbus::publish(&topic, &quote);

    // Actor should not receive new quotes
    assert_eq!(actor.received_quotes.len(), 2);
}

#[rstest]
fn test_subscribe_and_receive_trades(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let _guard = test_logging();
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_trades::<TestDataActor>(audusd_sim.id, None, None);

    let topic = get_trades_topic(audusd_sim.id);
    let trade = TradeTick::default();
    msgbus::publish(&topic, &trade);
    msgbus::publish(&topic, &trade);

    assert_eq!(actor.received_trades.len(), 2);
}
