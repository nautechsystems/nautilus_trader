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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

use std::{
    any::{Any, TypeId},
    cell::{RefCell, UnsafeCell},
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::Arc,
};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{
        Bar, BarType, DataType, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate,
        OrderBookDeltas, QuoteTick, TradeTick,
    },
    identifiers::{ClientId, ComponentId, InstrumentId, TraderId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use ustr::Ustr;
use uuid::Uuid;

use super::{
    Actor, executor::ActorExecutor, indicators::Indicators, registry::get_actor_unchecked,
};
use crate::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, RECV, SENT},
    messages::data::{
        DataCommand, DataRequest, DataResponse, RequestBars, RequestInstrument, RequestInstruments,
        RequestOrderBookSnapshot, RequestQuoteTicks, RequestTradeTicks, SubscribeBars,
        SubscribeCommand, SubscribeData, SubscribeIndexPrices, SubscribeInstrument,
        SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments,
        SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades, UnsubscribeBars, UnsubscribeData,
        UnsubscribeIndexPrices, UnsubscribeInstrument, UnsubscribeInstrumentStatus,
        UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeQuotes,
    },
    msgbus::{
        self, get_message_bus,
        handler::{MessageHandler, ShareableMessageHandler, TypedMessageHandler},
        switchboard::{
            self, MessagingSwitchboard, get_custom_topic, get_instrument_topic,
            get_instruments_topic, get_trades_topic,
        },
    },
};

/// Configuration for Actor components.
#[derive(Debug, Clone)]
pub struct DataActorConfig {
    /// The custom identifier for the Actor.
    pub actor_id: Option<ComponentId>, // TODO: Define ActorId
    /// Whether to log events.
    pub log_events: bool,
    /// Whether to log commands.
    pub log_commands: bool,
}

impl Default for DataActorConfig {
    fn default() -> Self {
        Self {
            actor_id: None,
            log_events: true,
            log_commands: true,
        }
    }
}

type RequestCallback = Box<dyn Fn(UUID4) + Send + Sync>; // TODO: TBD

impl Actor for DataActorCore {
    fn id(&self) -> Ustr {
        self.actor_id.inner()
    }

    fn handle(&mut self, msg: &dyn Any) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub trait DataActor: Actor {
    /// Actions to be performed when the actor state is saved.
    fn on_save(&self) -> HashMap<String, Vec<u8>> {
        HashMap::new()
    }
    /// Actions to be performed when the actor state is loaded.
    fn on_load(&mut self, state: HashMap<String, Vec<u8>>) {}
    /// Actions to be performed on start.
    fn on_start(&mut self) {}
    /// Actions to be performed on stop.
    fn on_stop(&mut self) {}
    /// Actions to be performed on resume.
    fn on_resume(&mut self) {}
    /// Actions to be performed on reset.
    fn on_reset(&mut self) {}
    /// Actions to be performed on dispose.
    fn on_dispose(&mut self) {}
    /// Actions to be performed on degrade.
    fn on_degrade(&mut self) {}
    /// Actions to be performed on fault.
    fn on_fault(&mut self) {}
    // Actions to be performed when receiving an event.
    // pub fn on_event(&mut self, event: &i Event) {  // TODO: TBD
    //     // Default empty implementation
    // }
    fn on_data(&mut self, data: &dyn Any);
    /// Actions to be performed when receiving an instrument status update.
    fn on_instrument_status(&mut self, data: &InstrumentStatus) {}
    // Actions to be performed when receiving an instrument close update.  // TODO: Implement data
    // pub fn on_instrument_close(&mut self, update: InstrumentClose) {}
    /// Actions to be performed when receiving an instrument.
    fn on_instrument(&mut self, instrument: &InstrumentAny) {}
    /// Actions to be performed when receiving an order book.
    fn on_book(&mut self, order_book: &OrderBook) {}
    /// Actions to be performed when receiving order book deltas.
    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) {}
    /// Actions to be performed when receiving a quote.
    fn on_quote(&mut self, quote: &QuoteTick);
    /// Actions to be performed when receiving a trade.
    fn on_trade(&mut self, tick: &TradeTick) {}
    /// Actions to be performed when receiving a mark price update.
    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) {}
    /// Actions to be performed when receiving an index price update.
    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) {}
    /// Actions to be performed when receiving a bar.
    fn on_bar(&mut self, bar: &Bar) {}
    // Actions to be performed when receiving a signal. // TODO: TBD
    // pub fn on_signal(&mut self, signal: &impl Data) {}
    // Actions to be performed when receiving historical data.
    fn on_historical_data(&mut self, data: &dyn Any) {} // TODO: Probably break this down further
}

/// Core functionality for all actors.
pub struct DataActorCore {
    /// The component ID for the actor.
    pub actor_id: ComponentId, // TODO: Probably just add an ActorId now?
    /// The actors configuration.
    pub config: DataActorConfig,
    /// The actors clock.
    pub clock: Rc<RefCell<dyn Clock>>,
    /// The read-only cache for the actor.
    pub cache: Rc<RefCell<Cache>>,
    trader_id: Option<TraderId>,
    executor: Option<Arc<dyn ActorExecutor>>, // TODO: TBD
    warning_events: HashSet<String>,          // TODO: TBD
    pending_requests: HashMap<UUID4, Option<RequestCallback>>,
    signal_classes: HashMap<String, String>,
    #[cfg(feature = "indicators")]
    indicators: Indicators,
}

impl DataActor for DataActorCore {
    fn on_data(&mut self, data: &dyn Any) {}
    fn on_quote(&mut self, quote: &QuoteTick) {}
}

impl DataActorCore {
    /// Creates a new [`Actor`] instance.
    pub fn new(
        config: DataActorConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
        switchboard: Arc<MessagingSwitchboard>,
    ) -> Self {
        let actor_id = config.actor_id.unwrap_or(ComponentId::new("Actor")); // TODO: Determine default ID

        Self {
            actor_id,
            config,
            clock,
            cache,
            trader_id: None, // None until registered
            executor: None,
            warning_events: HashSet::new(),
            pending_requests: HashMap::new(),
            signal_classes: HashMap::new(),
            #[cfg(feature = "indicators")]
            indicators: Indicators::default(),
        }
    }

    /// Returns the trader ID this actor is registered to.
    pub fn trader_id(&self) -> Option<TraderId> {
        self.trader_id
    }

    /// Register an executor for the actor.
    pub fn register_executor(&mut self, executor: Arc<dyn ActorExecutor>) {
        self.executor = Some(executor);
        // TODO: Log registration
    }

    /// Register an event type for warning log levels.
    pub fn register_warning_event(&mut self, event_type: &str) {
        self.warning_events.insert(event_type.to_string());
    }

    /// Deregister an event type from warning log levels.
    pub fn deregister_warning_event(&mut self, event_type: &str) {
        self.warning_events.remove(event_type);
        // TODO: Log deregistration
    }

    fn check_registered(&self) {
        assert!(
            self.trader_id.is_some(),
            "Actor has not been registered with a Trader"
        );
    }

    fn generate_ts_init(&self) -> UnixNanos {
        self.clock.borrow().timestamp_ns()
    }

    fn send_data_cmd(&self, command: DataCommand) {
        if self.config.log_commands {
            log::info!("{CMD}{SENT} {command:?}");
        }

        let endpoint = MessagingSwitchboard::data_engine_execute();
        msgbus::send(&endpoint, command.as_any())
    }

    /// Subscribe to data of the given data type.
    pub fn subscribe_data(
        &self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::with_any(
            move |data: &dyn Any| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle(data);
            },
        )));

        let topic = get_custom_topic(&data_type);
        msgbus::subscribe(topic, handler, None);

        if client_id.is_none() {
            // If no client ID specified, just subscribe to the topic
            return;
        }

        let command = SubscribeCommand::Data(SubscribeData {
            data_type,
            client_id,
            venue: None,
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to instrument data for the given venue.
    pub fn subscribe_instruments(
        &self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |instruments: &Vec<InstrumentAny>| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle_instruments(instruments);
            },
        )));

        let topic = get_instruments_topic(venue);
        msgbus::subscribe(topic, handler, None);

        let command = SubscribeCommand::Instruments(SubscribeInstruments {
            client_id,
            venue,
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to instrument data for the given instrument ID.
    pub fn subscribe_instrument(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |instrument: &InstrumentAny| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle_instrument(instrument);
            },
        )));

        let topic = get_instrument_topic(instrument_id);
        msgbus::subscribe(topic, handler, None);

        let command = SubscribeCommand::Instrument(SubscribeInstrument {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to quotes for the given instrument ID.
    pub fn subscribe_quotes(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |quote: &QuoteTick| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle_quote(quote);
            },
        )));

        let topic = get_trades_topic(instrument_id);
        msgbus::subscribe(topic, handler, None);

        let command = SubscribeCommand::Quotes(SubscribeQuotes {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to trades for the given instrument ID.
    pub fn subscribe_trades(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |trade: &TradeTick| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle_trade(trade);
            },
        )));

        let topic = get_trades_topic(instrument_id);
        msgbus::subscribe(topic, handler, None);

        let command = SubscribeCommand::Trades(SubscribeTrades {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Handles a received custom/generic data point.
    pub fn handle_data(&mut self, data: &dyn Any) {
        log_received(&data);
        // TODO: Check component state is running

        self.on_data(data)
    }

    /// Handles a received instrument.
    pub(crate) fn handle_instrument(&mut self, instrument: &InstrumentAny) {
        log_received(&instrument);
        // TODO: Check component state is running

        self.on_instrument(instrument);
    }

    /// Handles multiple received instruments.
    pub(crate) fn handle_instruments(&mut self, instruments: &Vec<InstrumentAny>) {
        // TODO: Check component state is running

        for instrument in instruments {
            self.handle_instrument(instrument);
        }
    }

    /// Handle a received order book reference.
    pub(crate) fn handle_book(&mut self, book: &OrderBook) {
        log_received(&book);
        // TODO: Check component state is running

        self.on_book(book);
    }

    /// Handles received order book deltas.
    pub(crate) fn handle_book_deltas(&mut self, deltas: &OrderBookDeltas) {
        log_received(&deltas);
        // TODO: Check component state is running

        self.on_book_deltas(deltas);
    }

    /// Handles a received quote.
    pub(crate) fn handle_quote(&mut self, quote: &QuoteTick) {
        log_received(&quote);
        // TODO: Check component state is running

        self.on_quote(quote);
    }

    /// Handles a received trade.
    pub(crate) fn handle_trade(&mut self, trade: &TradeTick) {
        log_received(&trade);
        // TODO: Check component state is running

        self.on_trade(trade);
    }

    /// Handles a received mark price update.
    pub(crate) fn handle_mark_price(&mut self, mark_price: &MarkPriceUpdate) {
        log_received(&mark_price);
        // TODO: Check component state is running

        self.on_mark_price(mark_price);
    }

    /// Handles a received index price update.
    pub(crate) fn handle_index_price(&mut self, index_price: &IndexPriceUpdate) {
        log_received(&index_price);
        // TODO: Check component state is running

        self.on_index_price(index_price);
    }

    /// Handles a received instrument status.
    pub(crate) fn handle_instrument_status(&mut self, status: &InstrumentStatus) {
        log_received(&status);
        // TODO: Check component state is running

        self.on_instrument_status(status);
    }

    /// Handles a receiving bar.
    pub(crate) fn handle_bar(&mut self, bar: &Bar) {
        log_received(&bar);
        // TODO: Check component state is running

        self.on_bar(bar);
    }
}

fn log_received<T>(msg: &T)
where
    T: std::fmt::Debug,
{
    log::debug!("{} {:?}", RECV, msg);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{
        any::Any,
        cell::{RefCell, UnsafeCell},
        ops::{Deref, DerefMut},
        rc::Rc,
        sync::Arc,
    };

    use nautilus_model::{
        data::{QuoteTick, TradeTick},
        identifiers::ComponentId,
        instruments::CurrencyPair,
        orderbook::OrderBook,
    };
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::{Actor, DataActor, DataActorConfig, DataActorCore};
    use crate::{
        actor::registry::{get_actor_unchecked, register_actor},
        cache::Cache,
        clock::{Clock, TestClock},
        msgbus::{
            self,
            switchboard::{MessagingSwitchboard, get_trades_topic},
        },
    };

    struct TestDataActor {
        core: DataActorCore,
        pub trades_received: RefCell<usize>,
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

    // Implement DataActor trait overriding handlers are required
    impl DataActor for TestDataActor {
        fn on_data(&mut self, data: &dyn Any) {
            println!("Received generic data");
        }

        fn on_book(&mut self, book: &OrderBook) {
            println!("Received a book {book}");
        }

        fn on_quote(&mut self, quote: &QuoteTick) {
            println!("Received a quote {quote}");
        }

        fn on_trade(&mut self, trade: &TradeTick) {
            *self.trades_received.borrow_mut() += 1;
            println!("Received a trade {trade}");
        }
    }

    // Custom functionality as required
    impl TestDataActor {
        pub fn new(
            config: DataActorConfig,
            cache: Rc<RefCell<Cache>>,
            clock: Rc<RefCell<dyn Clock>>,
            switchboard: Arc<MessagingSwitchboard>,
        ) -> Self {
            Self {
                core: DataActorCore::new(config, cache, clock, switchboard),
                trades_received: RefCell::new(0),
            }
        }
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
    pub fn switchboard() -> Arc<MessagingSwitchboard> {
        Arc::new(MessagingSwitchboard::default())
    }

    fn register_data_actor(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        switchboard: Arc<MessagingSwitchboard>,
    ) {
        let config = DataActorConfig::default();
        let actor = TestDataActor::new(config, cache, clock, switchboard);
        let actor_rc = Rc::new(UnsafeCell::new(actor));
        register_actor(actor_rc);
    }

    fn test_subscribe_and_receive_trades(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        switchboard: Arc<MessagingSwitchboard>,
        audusd_sim: CurrencyPair,
    ) {
        register_data_actor(clock.clone(), cache.clone(), switchboard.clone());

        let actor_id = ComponentId::new("Actor").inner(); // TODO: Determine default ID
        let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
        actor.subscribe_trades(audusd_sim.id, None, None);

        let topic = get_trades_topic(audusd_sim.id);
        let trade = TradeTick::default();

        msgbus::publish(&topic, &trade);
        assert_eq!(*actor.trades_received.borrow(), 1);

        msgbus::publish(&topic, &trade);
        assert_eq!(*actor.trades_received.borrow(), 2);
    }
}
