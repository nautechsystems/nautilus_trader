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
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    rc::Rc,
    str::FromStr,
    sync::Arc,
};

use bytes::Bytes;
use indexmap::IndexMap;
use log::LevelFilter;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, DataType, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
        close::InstrumentClose, stubs::*,
    },
    enums::{BookAction, BookType, OrderSide},
    identifiers::{ClientId, TraderId, Venue},
    instruments::{CurrencyPair, InstrumentAny, stubs::*},
    orderbook::OrderBook,
    types::{Price, Quantity},
};
use rstest::*;
use ustr::Ustr;
#[cfg(feature = "defi")]
use {
    alloy_primitives::Address,
    nautilus_model::defi::{Block, Blockchain, Pool, PoolLiquidityUpdate, PoolSwap},
};

use super::{Actor, DataActor, DataActorCore, data_actor::DataActorConfig};
#[cfg(feature = "defi")]
use crate::defi::switchboard::{get_defi_blocks_topic, get_defi_pool_swaps_topic};
use crate::{
    actor::registry::{get_actor, get_actor_unchecked, register_actor},
    cache::Cache,
    clock::TestClock,
    component::Component,
    logging::{logger::LogGuard, logging_is_initialized},
    messages::data::{
        BarsResponse, BookResponse, CustomDataResponse, InstrumentResponse, InstrumentsResponse,
        QuotesResponse, TradesResponse,
    },
    msgbus::{
        self, MessageBus, get_message_bus,
        switchboard::{
            MessagingSwitchboard, get_bars_topic, get_book_deltas_topic, get_book_snapshots_topic,
            get_custom_topic, get_funding_rate_topic, get_index_price_topic,
            get_instrument_close_topic, get_instrument_status_topic, get_instrument_topic,
            get_instruments_topic, get_mark_price_topic, get_quotes_topic, get_trades_topic,
        },
    },
    runner::{SyncDataCommandSender, set_data_cmd_sender},
    testing::init_logger_for_testing,
    timer::TimeEvent,
};

#[derive(Debug)]
struct TestDataActor {
    core: DataActorCore,
    pub received_time_events: Vec<TimeEvent>,
    pub received_instruments: Vec<InstrumentAny>,
    pub received_data: Vec<String>, // Use string for simplicity
    pub received_books: Vec<OrderBook>,
    pub received_deltas: Vec<OrderBookDelta>,
    pub received_quotes: Vec<QuoteTick>,
    pub received_trades: Vec<TradeTick>,
    pub received_bars: Vec<Bar>,
    pub received_mark_prices: Vec<MarkPriceUpdate>,
    pub received_index_prices: Vec<IndexPriceUpdate>,
    pub received_funding_rates: Vec<FundingRateUpdate>,
    pub received_status: Vec<InstrumentStatus>,
    pub received_closes: Vec<InstrumentClose>,
    #[cfg(feature = "defi")]
    pub received_blocks: Vec<Block>,
    #[cfg(feature = "defi")]
    pub received_pools: Vec<Pool>,
    #[cfg(feature = "defi")]
    pub received_pool_swaps: Vec<PoolSwap>,
    #[cfg(feature = "defi")]
    pub received_pool_liquidity_updates: Vec<PoolLiquidityUpdate>,
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

impl DataActor for TestDataActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting actor"); // Custom log
        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        self.received_time_events.push(event.clone());
        Ok(())
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        self.received_instruments.push(instrument.clone());
        Ok(())
    }

    fn on_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        self.received_data.push(format!("{data:?}"));
        Ok(())
    }

    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        self.received_deltas.extend(&deltas.deltas);
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

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        self.received_bars.push(*bar);
        Ok(())
    }

    fn on_historical_quotes(&mut self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        // Push to common received vec
        self.received_quotes.extend(quotes);
        Ok(())
    }

    fn on_historical_trades(&mut self, trades: &[TradeTick]) -> anyhow::Result<()> {
        // Push to common received vec
        self.received_trades.extend(trades);
        Ok(())
    }

    fn on_historical_bars(&mut self, bars: &[Bar]) -> anyhow::Result<()> {
        // Push to common received vec
        self.received_bars.extend(bars);
        Ok(())
    }

    fn on_historical_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        self.received_data.push(format!("{data:?}"));
        Ok(())
    }

    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        self.received_mark_prices.push(*mark_price);
        Ok(())
    }

    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        self.received_index_prices.push(*index_price);
        Ok(())
    }

    fn on_funding_rate(&mut self, funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        self.received_funding_rates.push(*funding_rate);
        Ok(())
    }

    fn on_instrument_status(&mut self, status: &InstrumentStatus) -> anyhow::Result<()> {
        self.received_status.push(*status);
        Ok(())
    }

    fn on_instrument_close(&mut self, close: &InstrumentClose) -> anyhow::Result<()> {
        self.received_closes.push(*close);
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn on_block(&mut self, block: &Block) -> anyhow::Result<()> {
        self.received_blocks.push(block.clone());
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn on_pool(&mut self, pool: &Pool) -> anyhow::Result<()> {
        self.received_pools.push(pool.clone());
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn on_pool_swap(&mut self, swap: &PoolSwap) -> anyhow::Result<()> {
        self.received_pool_swaps.push(swap.clone());
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn on_pool_liquidity_update(&mut self, update: &PoolLiquidityUpdate) -> anyhow::Result<()> {
        self.received_pool_liquidity_updates.push(update.clone());
        Ok(())
    }
}

// Custom functionality as required
impl TestDataActor {
    pub fn new(config: DataActorConfig) -> Self {
        Self {
            core: DataActorCore::new(config),
            received_time_events: Vec::new(),
            received_instruments: Vec::new(),
            received_data: Vec::new(),
            received_books: Vec::new(),
            received_deltas: Vec::new(),
            received_quotes: Vec::new(),
            received_trades: Vec::new(),
            received_bars: Vec::new(),
            received_mark_prices: Vec::new(),
            received_index_prices: Vec::new(),
            received_funding_rates: Vec::new(),
            received_status: Vec::new(),
            received_closes: Vec::new(),
            #[cfg(feature = "defi")]
            received_blocks: Vec::new(),
            #[cfg(feature = "defi")]
            received_pools: Vec::new(),
            #[cfg(feature = "defi")]
            received_pool_swaps: Vec::new(),
            #[cfg(feature = "defi")]
            received_pool_liquidity_updates: Vec::new(),
        }
    }

    #[allow(dead_code, reason = "TODO: Under development")]
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
    // Avoid reinitializing logger if already set
    if logging_is_initialized() {
        return None;
    }

    Some(init_logger_for_testing(Some(LevelFilter::Trace)).unwrap())
}

/// A simple Actor implementation for testing.
#[derive(Debug)]
struct DummyActor {
    id_str: Ustr,
    count: usize,
}
impl DummyActor {
    fn new<S: AsRef<str>>(s: S) -> Self {
        Self {
            id_str: Ustr::from_str(s.as_ref()).unwrap(),
            count: 0,
        }
    }
}
impl Actor for DummyActor {
    fn id(&self) -> Ustr {
        self.id_str
    }
    fn handle(&mut self, _msg: &dyn std::any::Any) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn register_data_actor(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) -> Ustr {
    // Set up sync data command sender for tests
    let sender = SyncDataCommandSender;
    set_data_cmd_sender(Arc::new(sender));

    let config = DataActorConfig::default();
    // Ensure clean message bus state for this actor's subscriptions
    let bus = get_message_bus();
    *bus.borrow_mut() = MessageBus::default();
    let mut actor = TestDataActor::new(config);
    actor.register(trader_id, clock, cache).unwrap();

    let actor_id = actor.actor_id();

    register_actor(actor);
    actor_id.inner()
}

/// Helper to register a dummy actor and return its Rc.
fn register_dummy(name: &str) -> Rc<UnsafeCell<dyn Actor>> {
    let actor = DummyActor::new(name);
    register_actor(actor)
}

#[rstest]
#[case("actor-001")]
#[case("actor-002")]
fn test_register_and_get(#[case] name: &str) {
    let rc = register_dummy(name);
    // Retrieve by id
    let id = unsafe { &*rc.get() }.id();
    let found = get_actor(&id).expect("actor not found");
    // Should be same Rc pointer
    assert!(Rc::ptr_eq(&rc, &found));
}

#[rstest]
fn test_get_nonexistent() {
    let id = Ustr::from_str("no_such_actor").unwrap();
    assert!(get_actor(&id).is_none());
}

#[should_panic(expected = "Actor for")]
#[rstest]
fn test_get_actor_unchecked_panic() {
    let id = Ustr::from_str("unknown").unwrap();
    // Should panic due to missing actor
    let _: &mut DummyActor = get_actor_unchecked(&id);
}

#[rstest]
fn test_get_actor_unchecked_mutate() {
    let name = "mutant";
    let _rc = register_dummy(name);
    let id = Ustr::from_str(name).unwrap();
    // Mutate via unchecked
    let actor_ref: &mut DummyActor = get_actor_unchecked(&id);
    actor_ref.count = 42;
    // Read back via unchecked again
    let actor_ref2: &mut DummyActor = get_actor_unchecked(&id);
    assert_eq!(actor_ref2.count, 42);
}

#[rstest]
fn test_subscribe_and_receive_custom_data(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let data_type = DataType::new(stringify!(String), None);
    actor.subscribe_data(data_type.clone(), None, None);

    let topic = get_custom_topic(&data_type);
    let data = String::from("CustomData-01");
    msgbus::publish(topic, &data);
    let data = String::from("CustomData-02");
    msgbus::publish(topic, &data);

    assert_eq!(actor.received_data.len(), 2);
}

#[rstest]
fn test_unsubscribe_custom_data(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let data_type = DataType::new(stringify!(String), None);
    actor.subscribe_data(data_type.clone(), None, None);

    let topic = get_custom_topic(&data_type);
    let data = String::from("CustomData-01");
    msgbus::publish(topic, &data);
    let data = String::from("CustomData-02");
    msgbus::publish(topic, &data);

    actor.unsubscribe_data(data_type, None, None);

    // Publish more data
    let data = String::from("CustomData-03");
    msgbus::publish(topic, &data);
    let data = String::from("CustomData-04");
    msgbus::publish(topic, &data);

    // Actor should not receive new data
    assert_eq!(actor.received_data.len(), 2);
}

#[rstest]
fn test_subscribe_and_receive_book_deltas(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_book_deltas(audusd_sim.id, BookType::L2_MBP, None, None, false, None);

    let topic = get_book_deltas_topic(audusd_sim.id);

    let order = BookOrder::new(
        OrderSide::Buy,
        Price::from("1.00000"),
        Quantity::from("100000"),
        123456,
    );
    let delta = OrderBookDelta::new(
        audusd_sim.id,
        BookAction::Add,
        order,
        0,
        1,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let deltas = OrderBookDeltas::new(audusd_sim.id, vec![delta]);

    msgbus::publish(topic, &deltas);

    assert_eq!(actor.received_deltas.len(), 1);
}

#[rstest]
fn test_unsubscribe_book_deltas(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_book_deltas(audusd_sim.id, BookType::L2_MBP, None, None, false, None);

    let topic = get_book_deltas_topic(audusd_sim.id);

    let order = BookOrder::new(
        OrderSide::Buy,
        Price::from("1.00000"),
        Quantity::from("100000"),
        123456,
    );
    let delta = OrderBookDelta::new(
        audusd_sim.id,
        BookAction::Add,
        order,
        0,
        1,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let deltas = OrderBookDeltas::new(audusd_sim.id, vec![delta]);

    msgbus::publish(topic, &deltas);

    // Unsubscribe
    actor.unsubscribe_book_deltas(audusd_sim.id, None, None);

    let delta2 = OrderBookDelta::new(
        audusd_sim.id,
        BookAction::Add,
        order,
        0,
        2,
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    let deltas2 = OrderBookDeltas::new(audusd_sim.id, vec![delta2]);

    // Publish again
    msgbus::publish(topic, &deltas2);

    // Should still only have one delta
    assert_eq!(actor.received_deltas.len(), 1);
}

#[rstest]
fn test_subscribe_and_receive_book_at_interval(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let book_type = BookType::L2_MBP;
    let interval_ms = NonZeroUsize::new(1_000).unwrap();

    actor.subscribe_book_at_interval(audusd_sim.id, book_type, None, interval_ms, None, None);

    let topic = get_book_snapshots_topic(audusd_sim.id, interval_ms);
    let book = OrderBook::new(audusd_sim.id, book_type);

    msgbus::publish(topic, &book);

    assert_eq!(actor.received_books.len(), 1);
}

#[rstest]
fn test_unsubscribe_book_at_interval(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let book_type = BookType::L2_MBP;
    let interval_ms = NonZeroUsize::new(1_000).unwrap();

    actor.subscribe_book_at_interval(audusd_sim.id, book_type, None, interval_ms, None, None);

    let topic = get_book_snapshots_topic(audusd_sim.id, interval_ms);
    let book = OrderBook::new(audusd_sim.id, book_type);

    msgbus::publish(topic, &book);

    assert_eq!(actor.received_books.len(), 1);

    actor.unsubscribe_book_at_interval(audusd_sim.id, interval_ms, None, None);

    // Publish more book refs
    msgbus::publish(topic, &book);
    msgbus::publish(topic, &book);

    // Should still only have one book
    assert_eq!(actor.received_books.len(), 1);
}

#[rstest]
fn test_subscribe_and_receive_quotes(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_quotes(audusd_sim.id, None, None);

    let topic = get_quotes_topic(audusd_sim.id);
    let quote = QuoteTick::default();
    msgbus::publish(topic, &quote);
    msgbus::publish(topic, &quote);

    assert_eq!(actor.received_quotes.len(), 2);
}

#[rstest]
fn test_unsubscribe_quotes(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_quotes(audusd_sim.id, None, None);

    let topic = get_quotes_topic(audusd_sim.id);
    let quote = QuoteTick::default();
    msgbus::publish(topic, &quote);
    msgbus::publish(topic, &quote);

    actor.unsubscribe_quotes(audusd_sim.id, None, None);

    // Publish more quotes
    msgbus::publish(topic, &quote);
    msgbus::publish(topic, &quote);

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
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_trades(audusd_sim.id, None, None);

    let topic = get_trades_topic(audusd_sim.id);
    let trade = TradeTick::default();
    msgbus::publish(topic, &trade);
    msgbus::publish(topic, &trade);

    assert_eq!(actor.received_trades.len(), 2);
}

#[rstest]
fn test_unsubscribe_trades(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_trades(audusd_sim.id, None, None);

    let topic = get_trades_topic(audusd_sim.id);
    let trade = TradeTick::default();
    msgbus::publish(topic, &trade);
    msgbus::publish(topic, &trade);

    actor.unsubscribe_trades(audusd_sim.id, None, None);

    // Publish more trades
    msgbus::publish(topic, &trade);
    msgbus::publish(topic, &trade);

    // Actor should not receive new trades
    assert_eq!(actor.received_trades.len(), 2);
}

#[rstest]
fn test_subscribe_and_receive_bars(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let bar_type = BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
    actor.subscribe_bars(bar_type, None, None);

    let topic = get_bars_topic(bar_type);
    let bar = Bar::default();
    msgbus::publish(topic, &bar);

    assert_eq!(actor.received_bars.len(), 1);
}

#[rstest]
fn test_unsubscribe_bars(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let bar_type = BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
    actor.subscribe_bars(bar_type, None, None);

    let topic = get_bars_topic(bar_type);
    let bar = Bar::default();
    msgbus::publish(topic, &bar);

    // Unsubscribe
    actor.unsubscribe_bars(bar_type, None, None);

    // Publish more bars
    msgbus::publish(topic, &bar);
    msgbus::publish(topic, &bar);

    // Should still only have one bar
    assert_eq!(actor.received_bars.len(), 1);
}

#[rstest]
fn test_request_instrument(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_instrument(audusd_sim.id, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let instrument = InstrumentAny::CurrencyPair(audusd_sim);
    let data = instrument.clone();
    let ts_init = UnixNanos::default();
    let response = InstrumentResponse::new(
        request_id,
        client_id,
        audusd_sim.id,
        data,
        Some(UnixNanos::from(946_684_800_000_000_000)), // 2000-01-01
        Some(UnixNanos::from(946_771_200_000_000_000)), // 2000-01-02
        ts_init,
        None,
    );

    msgbus::response(&request_id, response.as_any());

    assert_eq!(actor.received_instruments.len(), 1);
    assert_eq!(actor.received_instruments[0], instrument);
}

#[rstest]
fn test_request_instruments(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let venue = Venue::from("SIM");
    let request_id = actor
        .request_instruments(Some(venue), None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let instrument1 = InstrumentAny::CurrencyPair(audusd_sim);
    let instrument2 = InstrumentAny::CurrencyPair(gbpusd_sim);
    let data = vec![instrument1.clone(), instrument2.clone()];
    let ts_init = UnixNanos::default();
    let response = InstrumentsResponse::new(
        request_id,
        client_id,
        venue,
        data,
        Some(UnixNanos::from(946_684_800_000_000_000)), // 2000-01-01
        Some(UnixNanos::from(946_771_200_000_000_000)), // 2000-01-02
        ts_init,
        None,
    );

    msgbus::response(&request_id, response.as_any());

    assert_eq!(actor.received_instruments.len(), 2);
    assert_eq!(actor.received_instruments[0], instrument1);
    assert_eq!(actor.received_instruments[1], instrument2);
}

#[rstest]
fn test_request_quotes(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_quotes(audusd_sim.id, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let quote = QuoteTick::default();
    let data = vec![quote];
    let ts_init = UnixNanos::default();
    let response = QuotesResponse::new(
        request_id,
        client_id,
        audusd_sim.id,
        data,
        Some(UnixNanos::from(1_690_000_000_000_000_000)),
        Some(UnixNanos::from(1_700_000_000_000_000_000)),
        ts_init,
        None,
    );

    msgbus::response(&request_id, response.as_any());

    assert_eq!(actor.received_quotes.len(), 1);
    assert_eq!(actor.received_quotes[0], quote);
}

#[rstest]
fn test_request_trades(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_trades(audusd_sim.id, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let trade = TradeTick::default();
    let data = vec![trade];
    let ts_init = UnixNanos::default();
    let response = TradesResponse::new(
        request_id,
        client_id,
        audusd_sim.id,
        data,
        Some(UnixNanos::from(1_695_000_000_000_000_000)),
        Some(UnixNanos::from(1_699_000_000_000_000_000)),
        ts_init,
        None,
    );

    msgbus::response(&request_id, response.as_any());

    assert_eq!(actor.received_trades.len(), 1);
    assert_eq!(actor.received_trades[0], trade);
}

#[rstest]
fn test_request_bars(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let bar_type = BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
    let request_id = actor
        .request_bars(bar_type, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let bar_type = BarType::from_str("AUDUSD.SIM-1-MINUTE-LAST-EXTERNAL").unwrap();
    let bar = Bar::default();
    let data = vec![bar];
    let ts_init = UnixNanos::default();
    let response = BarsResponse::new(
        request_id,
        client_id,
        bar_type,
        data,
        Some(UnixNanos::from(1_700_000_000_000_000_000)),
        Some(UnixNanos::from(1_705_000_000_000_000_000)),
        ts_init,
        None,
    );

    msgbus::response(&request_id, response.as_any());

    assert_eq!(actor.received_bars.len(), 1);
    assert_eq!(actor.received_bars[0], bar);
}

#[rstest]
fn test_subscribe_and_receive_instruments(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let venue = Venue::from("SIM");
    actor.subscribe_instruments(venue, None, None);

    let topic = get_instruments_topic(venue);
    let inst1 = InstrumentAny::CurrencyPair(audusd_sim);
    msgbus::publish(topic, &inst1);
    let inst2 = InstrumentAny::CurrencyPair(gbpusd_sim);
    msgbus::publish(topic, &inst2);

    assert_eq!(actor.received_instruments.len(), 2);
    assert_eq!(actor.received_instruments[0], inst1);
    assert_eq!(actor.received_instruments[1], inst2);
}

#[rstest]
fn test_subscribe_and_receive_instrument(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_instrument(audusd_sim.id, None, None);

    let topic = get_instrument_topic(audusd_sim.id);
    let inst1 = InstrumentAny::CurrencyPair(audusd_sim);
    let inst2 = InstrumentAny::CurrencyPair(gbpusd_sim);
    msgbus::publish(topic, &inst1);
    msgbus::publish(topic, &inst2);

    assert_eq!(actor.received_instruments.len(), 2);
    assert_eq!(actor.received_instruments[0], inst1);
    assert_eq!(actor.received_instruments[1], inst2);
}

#[rstest]
fn test_subscribe_and_receive_mark_prices(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_mark_prices(audusd_sim.id, None, None);

    let topic = get_mark_price_topic(audusd_sim.id);
    let mp1 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish(topic, &mp1);
    let mp2 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00010"),
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    msgbus::publish(topic, &mp2);

    assert_eq!(actor.received_mark_prices.len(), 2);
    assert_eq!(actor.received_mark_prices[0], mp1);
    assert_eq!(actor.received_mark_prices[1], mp2);
}

#[rstest]
fn test_subscribe_and_receive_index_prices(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_index_prices(audusd_sim.id, None, None);

    let topic = get_index_price_topic(audusd_sim.id);
    let ip = IndexPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish(topic, &ip);

    assert_eq!(actor.received_index_prices.len(), 1);
    assert_eq!(actor.received_index_prices[0], ip);
}

#[rstest]
fn test_subscribe_and_receive_funding_rates(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_funding_rates(audusd_sim.id, None, None);

    let topic = get_funding_rate_topic(audusd_sim.id);
    let fr1 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish(topic, &fr1);
    let fr2 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0002".parse().unwrap(),
        None,
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    msgbus::publish(topic, &fr2);

    assert_eq!(actor.received_funding_rates.len(), 2);
    assert_eq!(actor.received_funding_rates[0], fr1);
    assert_eq!(actor.received_funding_rates[1], fr2);
}

#[rstest]
fn test_subscribe_and_receive_instrument_status(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    stub_instrument_status: InstrumentStatus,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_status.instrument_id;
    actor.subscribe_instrument_status(instrument_id, None, None);

    let topic = get_instrument_status_topic(instrument_id);
    msgbus::publish(topic, &stub_instrument_status);

    assert_eq!(actor.received_status.len(), 1);
    assert_eq!(actor.received_status[0], stub_instrument_status);
}

#[rstest]
fn test_subscribe_and_receive_instrument_close(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    stub_instrument_close: InstrumentClose,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_close.instrument_id;
    actor.subscribe_instrument_close(instrument_id, None, None);

    let topic = get_instrument_close_topic(instrument_id);
    msgbus::publish(topic, &stub_instrument_close);

    assert_eq!(actor.received_closes.len(), 1);
    assert_eq!(actor.received_closes[0], stub_instrument_close);
}

#[rstest]
fn test_unsubscribe_instruments(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let venue = Venue::from("SIM");
    actor.subscribe_instruments(venue, None, None);

    let topic = get_instruments_topic(venue);
    let inst1 = InstrumentAny::CurrencyPair(audusd_sim);
    msgbus::publish(topic, &inst1);
    let inst2 = InstrumentAny::CurrencyPair(gbpusd_sim);
    msgbus::publish(topic, &inst2);

    assert_eq!(actor.received_instruments.len(), 2);

    actor.unsubscribe_instruments(venue, None, None);

    let inst3 = InstrumentAny::CurrencyPair(audusd_sim);
    msgbus::publish(topic, &inst3);
    let inst4 = InstrumentAny::CurrencyPair(gbpusd_sim);
    msgbus::publish(topic, &inst4);

    assert_eq!(actor.received_instruments.len(), 2);
}

#[rstest]
fn test_unsubscribe_instrument(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_instrument(audusd_sim.id, None, None);

    let topic = get_instrument_topic(audusd_sim.id);
    let inst1 = InstrumentAny::CurrencyPair(audusd_sim);
    msgbus::publish(topic, &inst1);
    let inst2 = InstrumentAny::CurrencyPair(gbpusd_sim);
    msgbus::publish(topic, &inst2);

    assert_eq!(actor.received_instruments.len(), 2);

    actor.unsubscribe_instrument(audusd_sim.id, None, None);

    let inst3 = InstrumentAny::CurrencyPair(audusd_sim);
    msgbus::publish(topic, &inst3);
    let inst4 = InstrumentAny::CurrencyPair(gbpusd_sim);
    msgbus::publish(topic, &inst4);

    assert_eq!(actor.received_instruments.len(), 2);
}

#[rstest]
fn test_unsubscribe_mark_prices(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_mark_prices(audusd_sim.id, None, None);

    let topic = get_mark_price_topic(audusd_sim.id);
    let mp1 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish(topic, &mp1);
    let mp2 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00010"),
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    msgbus::publish(topic, &mp2);

    assert_eq!(actor.received_mark_prices.len(), 2);

    actor.unsubscribe_mark_prices(audusd_sim.id, None, None);

    let mp3 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00020"),
        UnixNanos::from(5),
        UnixNanos::from(6),
    );
    msgbus::publish(topic, &mp3);
    let mp4 = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00030"),
        UnixNanos::from(7),
        UnixNanos::from(8),
    );
    msgbus::publish(topic, &mp4);

    assert_eq!(actor.received_mark_prices.len(), 2);
}

#[rstest]
fn test_unsubscribe_index_prices(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_index_prices(audusd_sim.id, None, None);

    let topic = get_index_price_topic(audusd_sim.id);
    let ip1 = IndexPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish(topic, &ip1);

    assert_eq!(actor.received_index_prices.len(), 1);

    actor.unsubscribe_index_prices(audusd_sim.id, None, None);

    let ip2 = IndexPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00010"),
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    msgbus::publish(topic, &ip2);

    assert_eq!(actor.received_index_prices.len(), 1);
}

#[rstest]
fn test_unsubscribe_funding_rates(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_funding_rates(audusd_sim.id, None, None);

    let topic = get_funding_rate_topic(audusd_sim.id);
    let fr1 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish(topic, &fr1);

    assert_eq!(actor.received_funding_rates.len(), 1);

    actor.unsubscribe_funding_rates(audusd_sim.id, None, None);

    let fr2 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0002".parse().unwrap(),
        None,
        UnixNanos::from(3),
        UnixNanos::from(4),
    );
    msgbus::publish(topic, &fr2);

    assert_eq!(actor.received_funding_rates.len(), 1);
}

#[rstest]
fn test_unsubscribe_instrument_status(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    stub_instrument_status: InstrumentStatus,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_status.instrument_id;
    actor.subscribe_instrument_status(instrument_id, None, None);

    let topic = get_instrument_status_topic(instrument_id);
    msgbus::publish(topic, &stub_instrument_status);

    assert_eq!(actor.received_status.len(), 1);

    actor.unsubscribe_instrument_status(instrument_id, None, None);

    let stub2 = stub_instrument_status;
    msgbus::publish(topic, &stub2);

    assert_eq!(actor.received_status.len(), 1);
}

#[rstest]
fn test_unsubscribe_instrument_close(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    stub_instrument_close: InstrumentClose,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_close.instrument_id;
    actor.subscribe_instrument_close(instrument_id, None, None);

    let topic = get_instrument_close_topic(instrument_id);
    msgbus::publish(topic, &stub_instrument_close);

    assert_eq!(actor.received_closes.len(), 1);

    actor.unsubscribe_instrument_close(instrument_id, None, None);

    let stub2 = stub_instrument_close;
    msgbus::publish(topic, &stub2);

    assert_eq!(actor.received_closes.len(), 1);
}

#[rstest]
fn test_request_book_snapshot(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    audusd_sim: CurrencyPair,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    // Request a book snapshot
    let request_id = actor
        .request_book_snapshot(audusd_sim.id, None, None, None)
        .unwrap();

    // Build a dummy book and response
    let client_id = ClientId::new("Client2");
    let book = OrderBook::new(audusd_sim.id, BookType::L2_MBP);

    // Provide ts_init and no params
    let ts_init = UnixNanos::default();
    let response = BookResponse::new(
        request_id,
        client_id,
        audusd_sim.id,
        book.clone(),
        Some(UnixNanos::from(946_684_800_000_000_000)), // 2000-01-01
        Some(UnixNanos::from(946_771_200_000_000_000)), // 2000-01-02
        ts_init,
        None,
    );
    msgbus::response(&request_id, response.as_any());

    // Should trigger on_book and record the book
    assert_eq!(actor.received_books.len(), 1);
    assert_eq!(actor.received_books[0], book);
}

#[rstest]
fn test_request_data(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    test_logging();

    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    // Request custom data
    let data_type = DataType::new("TestData", None);
    let client_id = ClientId::new("TestClient");
    let request_id = actor
        .request_data(data_type.clone(), client_id, None, None, None, None)
        .unwrap();

    // Build a response payload containing a String
    let payload = Arc::new(Bytes::from("Data-001"));
    let ts_init = UnixNanos::default();

    // Create response with payload type String
    let response = CustomDataResponse::new(
        request_id,
        client_id,
        None,
        data_type,
        payload,
        Some(UnixNanos::from(946_684_800_000_000_000)), // 2000-01-01
        Some(UnixNanos::from(946_771_200_000_000_000)), // 2000-01-02
        ts_init,
        None,
    );

    // Publish the response
    msgbus::response(&request_id, response.as_any());

    // Actor should receive the custom data
    assert_eq!(actor.received_data.len(), 1);
    assert_eq!(actor.received_data[0], "Any { .. }");
}

// ------------------------------------------------------------------------------------------------
// DeFi Tests
// ------------------------------------------------------------------------------------------------

#[cfg(feature = "defi")]
#[rstest]
fn test_subscribe_and_receive_blocks(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let blockchain = Blockchain::Ethereum;
    actor.subscribe_blocks(blockchain, None, None);

    let topic = get_defi_blocks_topic(blockchain);
    let block = Block::new(
        "0x123".to_string(),
        "0x456".to_string(),
        1u64,
        "miner".into(),
        1000000u64,
        500000u64,
        UnixNanos::from(1),
        Some(blockchain),
    );
    msgbus::publish(topic, &block);

    assert_eq!(actor.received_blocks.len(), 1);
    assert_eq!(actor.received_blocks[0], block);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_unsubscribe_blocks(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let blockchain = Blockchain::Ethereum;
    actor.subscribe_blocks(blockchain, None, None);

    let topic = get_defi_blocks_topic(blockchain);
    let block1 = Block::new(
        "0x123".to_string(),
        "0x456".to_string(),
        1u64,
        "miner".into(),
        1000000u64,
        500000u64,
        UnixNanos::from(1),
        Some(blockchain),
    );
    msgbus::publish(topic, &block1);

    // Unsubscribe
    actor.unsubscribe_blocks(blockchain, None, None);

    let block2 = Block::new(
        "0x789".to_string(),
        "0xabc".to_string(),
        2u64,
        "miner2".into(),
        1000001u64,
        500001u64,
        UnixNanos::from(2),
        Some(blockchain),
    );
    msgbus::publish(topic, &block2);

    // Should still only have one block
    assert_eq!(actor.received_blocks.len(), 1);
    assert_eq!(actor.received_blocks[0], block1);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_subscribe_and_receive_pools(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    use nautilus_model::defi::{Dex, DexType, Pool, Token, chain::chains, dex::AmmType};

    use crate::defi::switchboard::get_defi_pool_topic;

    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    );
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x12; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let pool = Pool::new(
        chain,
        Arc::new(dex),
        Address::from([0x12; 20]),
        1000000,
        token0,
        token1,
        Some(3000),
        Some(60),
        UnixNanos::from(1),
    );

    let instrument_id = pool.instrument_id;
    actor.subscribe_pool(instrument_id, None, None);

    let topic = get_defi_pool_topic(instrument_id);

    msgbus::publish(topic, &pool);

    assert_eq!(actor.received_pools.len(), 1);
    assert_eq!(actor.received_pools[0], pool);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_subscribe_and_receive_pool_swaps(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    use alloy_primitives::{I256, U160};
    use nautilus_model::{
        defi::{AmmType, Dex, DexType, chain::chains},
        identifiers::InstrumentId,
    };

    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    );

    let pool_address = Address::from_str("0xC31E54c7A869B9fCbECC14363CF510d1C41Fa443").unwrap();
    let instrument_id =
        InstrumentId::from("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:UniswapV3");

    let swap = PoolSwap::new(
        chain,
        Arc::new(dex),
        instrument_id,
        pool_address,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        Address::from([0x12; 20]),
        Address::from([0x12; 20]),
        I256::from_str("1000000000000000000").unwrap(),
        I256::from_str("400000000000000").unwrap(),
        U160::from(59000000000000u128),
        1000000,
        100,
        Some(OrderSide::Buy),
        Some(Quantity::from("1000")),
        Some(Price::from("500")),
    );

    actor.subscribe_pool_swaps(instrument_id, None, None);

    let topic = get_defi_pool_swaps_topic(instrument_id);

    msgbus::publish(topic, &swap);

    assert_eq!(actor.received_pool_swaps.len(), 1);
    assert_eq!(actor.received_pool_swaps[0], swap);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_unsubscribe_pool_swaps(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    use alloy_primitives::{I256, U160};
    use nautilus_model::defi::{Dex, DexType, Pool, chain::chains, dex::AmmType};

    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    );
    let pool_address = Address::from_str("0xC31E54c7A869B9fCbECC14363CF510d1C41Fa443").unwrap();
    let instrument_id = Pool::create_instrument_id(chain.name, &dex, &pool_address);

    actor.subscribe_pool_swaps(instrument_id, None, None);

    let topic = get_defi_pool_swaps_topic(instrument_id);

    let swap1 = PoolSwap::new(
        chain.clone(),
        Arc::new(dex.clone()),
        instrument_id,
        pool_address,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        Address::from([0x12; 20]),
        Address::from([0x12; 20]),
        I256::from_str("1000000000000000000").unwrap(),
        I256::from_str("400000000000000").unwrap(),
        U160::from(59000000000000u128),
        1000000,
        100,
        Some(OrderSide::Buy),
        Some(Quantity::from("1000")),
        Some(Price::from("500")),
    );
    msgbus::publish(topic, &swap1);

    // Unsubscribe
    actor.unsubscribe_pool_swaps(instrument_id, None, None);

    let swap2 = PoolSwap::new(
        chain,
        Arc::new(dex),
        instrument_id,
        pool_address,
        2000u64,
        "0x456".to_string(),
        0,
        0,
        None,
        Address::from([0x12; 20]),
        Address::from([0x12; 20]),
        I256::from_str("1000000000000000000").unwrap(),
        I256::from_str("400000000000000").unwrap(),
        U160::from(59000000000000u128),
        1000000,
        100,
        Some(OrderSide::Sell),
        Some(Quantity::from("2000")),
        Some(Price::from("1000")),
    );
    msgbus::publish(topic, &swap2);

    // Should still only have one swap
    assert_eq!(actor.received_pool_swaps.len(), 1);
    assert_eq!(actor.received_pool_swaps[0], swap1);
}

#[rstest]
fn test_duplicate_subscribe_custom_data(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    // Register actor
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    // Subscribe twice to the same DataType
    let data_type = DataType::new(stringify!(String), None);
    actor.subscribe_data(data_type.clone(), None, None);
    actor.subscribe_data(data_type.clone(), None, None);

    // Publish a single message
    let topic = get_custom_topic(&data_type);
    let payload = String::from("Custom-XYZ");
    msgbus::publish(topic, &payload);

    // Only a single handler should be active despite duplicate subscribe attempt
    assert_eq!(actor.received_data.len(), 1);
}

#[rstest]
fn test_unsubscribe_before_subscribe_custom_data(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let actor_id = register_data_actor(clock, cache, trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let data_type = DataType::new(stringify!(String), None);

    // Unsubscribe without prior subscription: should not panic and no data received
    actor.unsubscribe_data(data_type.clone(), None, None);

    let topic = get_custom_topic(&data_type);
    let payload = String::from("Custom-ABC");
    msgbus::publish(topic, &payload);

    assert!(actor.received_data.is_empty());
}

// ---------------------------------------------------------------------------------------------
// save / load round-trip
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
struct SaveLoadActor {
    core: DataActorCore,
    loaded_state: Option<IndexMap<String, Vec<u8>>>,
}

impl SaveLoadActor {
    fn new(config: DataActorConfig) -> Self {
        Self {
            core: DataActorCore::new(config),
            loaded_state: None,
        }
    }
}

impl Deref for SaveLoadActor {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for SaveLoadActor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl DataActor for SaveLoadActor {
    fn on_save(&self) -> anyhow::Result<IndexMap<String, Vec<u8>>> {
        let mut map = IndexMap::new();
        map.insert("answer".to_string(), vec![4, 2]);
        Ok(map)
    }

    fn on_load(&mut self, state: IndexMap<String, Vec<u8>>) -> anyhow::Result<()> {
        self.loaded_state = Some(state);
        Ok(())
    }
}

#[rstest]
fn test_on_save_and_on_load(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
) {
    let config = DataActorConfig::default();

    // Prepare actor & register
    let mut actor = SaveLoadActor::new(config);
    actor.register(trader_id, clock, cache).unwrap();
    let actor_id = actor.actor_id();
    register_actor(actor);

    // Fetch back to mutate
    let actor_key = actor_id.inner();
    let actor_ref = get_actor_unchecked::<SaveLoadActor>(&actor_key);

    // Invoke on_save – emulate persistence snapshot
    let snapshot = actor_ref.on_save().unwrap();
    assert!(snapshot.contains_key("answer"));

    // Invoke on_load with snapshot
    actor_ref.on_load(snapshot.clone()).unwrap();

    // Verify state stored
    assert_eq!(actor_ref.loaded_state.as_ref(), Some(&snapshot));
}
