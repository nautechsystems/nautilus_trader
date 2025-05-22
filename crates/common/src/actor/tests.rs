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
use log::LevelFilter;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, DataType, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate,
        OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
        close::InstrumentClose,
        stubs::{stub_instrument_close, stub_instrument_status},
    },
    enums::{BookAction, BookType, OrderSide},
    identifiers::{ClientId, TraderId, Venue},
    instruments::{
        CurrencyPair, InstrumentAny,
        stubs::{audusd_sim, gbpusd_sim},
    },
    orderbook::OrderBook,
    types::{Price, Quantity},
};
use rstest::{fixture, rstest};
use ustr::Ustr;

use super::{Actor, DataActor, DataActorCore, data_actor::DataActorConfig};
use crate::{
    actor::registry::{get_actor, get_actor_unchecked, register_actor},
    cache::Cache,
    clock::{Clock, TestClock},
    enums::ComponentState,
    logging::{logger::LogGuard, logging_is_initialized},
    messages::data::{
        BarsResponse, BookResponse, CustomDataResponse, InstrumentResponse, InstrumentsResponse,
        QuotesResponse, TradesResponse,
    },
    msgbus::{
        self, MessageBus, get_message_bus,
        switchboard::{
            MessagingSwitchboard, get_bars_topic, get_book_deltas_topic, get_book_snapshots_topic,
            get_custom_topic, get_index_price_topic, get_instrument_close_topic,
            get_instrument_status_topic, get_instrument_topic, get_instruments_topic,
            get_mark_price_topic, get_quotes_topic, get_trades_topic,
        },
    },
    testing::init_logger_for_testing,
    timer::TimeEvent,
};

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
    pub received_status: Vec<InstrumentStatus>,
    pub received_closes: Vec<InstrumentClose>,
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
        self.received_data.push(format!("{:?}", data));
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

    fn on_instrument_status(&mut self, status: &InstrumentStatus) -> anyhow::Result<()> {
        self.received_status.push(*status);
        Ok(())
    }

    fn on_instrument_close(&mut self, close: &InstrumentClose) -> anyhow::Result<()> {
        self.received_closes.push(*close);
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
            received_status: Vec::new(),
            received_closes: Vec::new(),
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
    // Avoid reinitializing logger if already set
    if logging_is_initialized() {
        return None;
    }

    Some(init_logger_for_testing(Some(LevelFilter::Trace)).unwrap())
}

/// A simple Actor implementation for testing.
struct DummyActor {
    id_str: Ustr,
    count: usize,
}
impl DummyActor {
    fn new<S: AsRef<str>>(s: S) -> Self {
        DummyActor {
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
    let config = DataActorConfig::default();
    // Ensure clean message bus state for this actor's subscriptions
    let bus = get_message_bus();
    *bus.borrow_mut() = MessageBus::default();
    let mut actor = TestDataActor::new(config, cache, clock);
    let actor_id = actor.actor_id;
    actor.set_trader_id(trader_id);
    actor.initialize().unwrap();

    let actor_rc = Rc::new(UnsafeCell::new(actor));
    register_actor(actor_rc);
    actor_id.inner()
}

/// Helper to register a dummy actor and return its Rc.
fn register_dummy(name: &str) -> Rc<UnsafeCell<dyn Actor>> {
    let actor = DummyActor::new(name);
    let rc: Rc<UnsafeCell<dyn Actor>> = Rc::new(UnsafeCell::new(actor));
    register_actor(rc.clone());
    rc
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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let data_type = DataType::new(stringify!(String), None);
    actor.subscribe_data::<TestDataActor>(data_type.clone(), None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let data_type = DataType::new(stringify!(String), None);
    actor.subscribe_data::<TestDataActor>(data_type.clone(), None, None);

    let topic = get_custom_topic(&data_type);
    let data = String::from("CustomData-01");
    msgbus::publish(topic, &data);
    let data = String::from("CustomData-02");
    msgbus::publish(topic, &data);

    actor.unsubscribe_data::<TestDataActor>(data_type, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_book_deltas::<TestDataActor>(
        audusd_sim.id,
        BookType::L2_MBP,
        None,
        None,
        false,
        None,
    );

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_book_deltas::<TestDataActor>(
        audusd_sim.id,
        BookType::L2_MBP,
        None,
        None,
        false,
        None,
    );

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
    actor.unsubscribe_book_deltas::<TestDataActor>(audusd_sim.id, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let book_type = BookType::L2_MBP;
    let interval_ms = NonZeroUsize::new(1_000).unwrap();

    actor.subscribe_book_at_interval::<TestDataActor>(
        audusd_sim.id,
        book_type,
        None,
        interval_ms,
        None,
        None,
    );

    let topic = get_book_snapshots_topic(audusd_sim.id);
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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let book_type = BookType::L2_MBP;
    let interval_ms = NonZeroUsize::new(1_000).unwrap();

    actor.subscribe_book_at_interval::<TestDataActor>(
        audusd_sim.id,
        book_type,
        None,
        interval_ms,
        None,
        None,
    );

    let topic = get_book_snapshots_topic(audusd_sim.id);
    let book = OrderBook::new(audusd_sim.id, book_type);

    msgbus::publish(topic, &book);

    assert_eq!(actor.received_books.len(), 1);

    actor.unsubscribe_book_at_interval::<TestDataActor>(audusd_sim.id, interval_ms, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_quotes::<TestDataActor>(audusd_sim.id, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_quotes::<TestDataActor>(audusd_sim.id, None, None);

    let topic = get_quotes_topic(audusd_sim.id);
    let quote = QuoteTick::default();
    msgbus::publish(topic, &quote);
    msgbus::publish(topic, &quote);

    actor.unsubscribe_quotes::<TestDataActor>(audusd_sim.id, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_trades::<TestDataActor>(audusd_sim.id, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_trades::<TestDataActor>(audusd_sim.id, None, None);

    let topic = get_trades_topic(audusd_sim.id);
    let trade = TradeTick::default();
    msgbus::publish(topic, &trade);
    msgbus::publish(topic, &trade);

    actor.unsubscribe_trades::<TestDataActor>(audusd_sim.id, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let bar_type = BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
    actor.subscribe_bars::<TestDataActor>(bar_type, None, false, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let bar_type = BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
    actor.subscribe_bars::<TestDataActor>(bar_type, None, false, None);

    let topic = get_bars_topic(bar_type);
    let bar = Bar::default();
    msgbus::publish(topic, &bar);

    // Unsubscribe
    actor.unsubscribe_bars::<TestDataActor>(bar_type, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_instrument::<TestDataActor>(audusd_sim.id, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let instrument = InstrumentAny::CurrencyPair(audusd_sim);
    let data = instrument.clone();
    let ts_init = UnixNanos::default();
    let response =
        InstrumentResponse::new(request_id, client_id, audusd_sim.id, data, ts_init, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let venue = Venue::from("SIM");
    let request_id = actor
        .request_instruments::<TestDataActor>(Some(venue), None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let instrument1 = InstrumentAny::CurrencyPair(audusd_sim);
    let instrument2 = InstrumentAny::CurrencyPair(gbpusd_sim);
    let data = vec![instrument1.clone(), instrument2.clone()];
    let ts_init = UnixNanos::default();
    let response = InstrumentsResponse::new(request_id, client_id, venue, data, ts_init, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_quotes::<TestDataActor>(audusd_sim.id, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let quote = QuoteTick::default();
    let data = vec![quote];
    let ts_init = UnixNanos::default();
    let response = QuotesResponse::new(request_id, client_id, audusd_sim.id, data, ts_init, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let request_id = actor
        .request_trades::<TestDataActor>(audusd_sim.id, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let trade = TradeTick::default();
    let data = vec![trade];
    let ts_init = UnixNanos::default();
    let response = TradesResponse::new(request_id, client_id, audusd_sim.id, data, ts_init, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let bar_type = BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
    let request_id = actor
        .request_bars::<TestDataActor>(bar_type, None, None, None, None, None)
        .unwrap();

    let client_id = ClientId::new("TestClient");
    let bar_type = BarType::from_str("AUDUSD.SIM-1-MINUTE-LAST-EXTERNAL").unwrap();
    let bar = Bar::default();
    let data = vec![bar];
    let ts_init = UnixNanos::default();
    let response = BarsResponse::new(request_id, client_id, bar_type, data, ts_init, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let venue = Venue::from("SIM");
    actor.subscribe_instruments::<TestDataActor>(venue, None, None);

    let topic = get_instruments_topic(venue);
    let inst1 = InstrumentAny::CurrencyPair(audusd_sim.clone());
    msgbus::publish(topic, &inst1);
    let inst2 = InstrumentAny::CurrencyPair(gbpusd_sim.clone());
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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_instrument::<TestDataActor>(audusd_sim.id, None, None);

    let topic = get_instrument_topic(audusd_sim.id);
    let inst1 = InstrumentAny::CurrencyPair(audusd_sim.clone());
    let inst2 = InstrumentAny::CurrencyPair(gbpusd_sim.clone());
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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_mark_prices::<TestDataActor>(audusd_sim.id, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_index_prices::<TestDataActor>(audusd_sim.id, None, None);

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
fn test_subscribe_and_receive_instrument_status(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    stub_instrument_status: InstrumentStatus,
) {
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_status.instrument_id;
    actor.subscribe_instrument_status::<TestDataActor>(instrument_id, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_close.instrument_id;
    actor.subscribe_instrument_close::<TestDataActor>(instrument_id, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let venue = Venue::from("SIM");
    actor.subscribe_instruments::<TestDataActor>(venue, None, None);

    let topic = get_instruments_topic(venue);
    let inst1 = InstrumentAny::CurrencyPair(audusd_sim.clone());
    msgbus::publish(topic, &inst1);
    let inst2 = InstrumentAny::CurrencyPair(gbpusd_sim.clone());
    msgbus::publish(topic, &inst2);

    assert_eq!(actor.received_instruments.len(), 2);

    actor.unsubscribe_instruments::<TestDataActor>(venue, None, None);

    let inst3 = InstrumentAny::CurrencyPair(audusd_sim.clone());
    msgbus::publish(topic, &inst3);
    let inst4 = InstrumentAny::CurrencyPair(gbpusd_sim.clone());
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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_instrument::<TestDataActor>(audusd_sim.id, None, None);

    let topic = get_instrument_topic(audusd_sim.id);
    let inst1 = InstrumentAny::CurrencyPair(audusd_sim.clone());
    msgbus::publish(topic, &inst1);
    let inst2 = InstrumentAny::CurrencyPair(gbpusd_sim.clone());
    msgbus::publish(topic, &inst2);

    assert_eq!(actor.received_instruments.len(), 2);

    actor.unsubscribe_instrument::<TestDataActor>(audusd_sim.id, None, None);

    let inst3 = InstrumentAny::CurrencyPair(audusd_sim.clone());
    msgbus::publish(topic, &inst3);
    let inst4 = InstrumentAny::CurrencyPair(gbpusd_sim.clone());
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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_mark_prices::<TestDataActor>(audusd_sim.id, None, None);

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

    actor.unsubscribe_mark_prices::<TestDataActor>(audusd_sim.id, None, None);

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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    actor.subscribe_index_prices::<TestDataActor>(audusd_sim.id, None, None);

    let topic = get_index_price_topic(audusd_sim.id);
    let ip1 = IndexPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    msgbus::publish(topic, &ip1);

    assert_eq!(actor.received_index_prices.len(), 1);

    actor.unsubscribe_index_prices::<TestDataActor>(audusd_sim.id, None, None);

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
fn test_unsubscribe_instrument_status(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    trader_id: TraderId,
    stub_instrument_status: InstrumentStatus,
) {
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_status.instrument_id;
    actor.subscribe_instrument_status::<TestDataActor>(instrument_id, None, None);

    let topic = get_instrument_status_topic(instrument_id);
    msgbus::publish(topic, &stub_instrument_status);

    assert_eq!(actor.received_status.len(), 1);

    actor.unsubscribe_instrument_status::<TestDataActor>(instrument_id, None, None);

    let stub2 = stub_instrument_status.clone();
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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    let instrument_id = stub_instrument_close.instrument_id;
    actor.subscribe_instrument_close::<TestDataActor>(instrument_id, None, None);

    let topic = get_instrument_close_topic(instrument_id);
    msgbus::publish(topic, &stub_instrument_close);

    assert_eq!(actor.received_closes.len(), 1);

    actor.unsubscribe_instrument_close::<TestDataActor>(instrument_id, None, None);

    let stub2 = stub_instrument_close.clone();
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
    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    // Request a book snapshot
    let request_id = actor
        .request_book_snapshot::<TestDataActor>(audusd_sim.id, None, None, None)
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

    let actor_id = register_data_actor(clock.clone(), cache.clone(), trader_id);
    let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
    actor.start().unwrap();

    // Request custom data
    let data_type = DataType::new("TestData", None);
    let client_id = ClientId::new("TestClient");
    let request_id = actor
        .request_data::<TestDataActor>(data_type.clone(), client_id.clone(), None, None, None, None)
        .unwrap();

    // Build a response payload containing a String
    let payload = Arc::new(Bytes::from("Data-001"));
    let ts_init = UnixNanos::default();

    // Create response with payload type String
    let response = CustomDataResponse::new(
        request_id,
        client_id.clone(),
        None,
        data_type.clone(),
        payload,
        ts_init,
        None,
    );

    // Publish the response
    msgbus::response(&request_id, response.as_any());

    // Actor should receive the custom data
    assert_eq!(actor.received_data.len(), 1);
    assert_eq!(actor.received_data[0], "Any { .. }");
}
