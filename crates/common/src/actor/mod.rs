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

pub mod config;
pub mod executor;
#[cfg(feature = "indicators")]
pub(crate) mod indicators;
pub mod registry;
pub mod registry_v2;

use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

use config::ActorConfig;
use executor::ActorExecutor;
use indicators::Indicators;
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
use registry_v2::{Actor, get_actor, get_actor_unchecked, register_actor};
use ustr::Ustr;
use uuid::Uuid;

use crate::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, SENT},
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
            get_instruments_topic,
        },
    },
};

type RequestCallback = Box<dyn Fn(UUID4) + Send + Sync>; // TODO: TBD

impl Actor for DataActorCore {
    fn id(&self) -> ComponentId {
        self.actor_id
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
    pub config: ActorConfig,
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
        config: ActorConfig,
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
                get_actor_unchecked(&actor_id).borrow_mut().handle(data);
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
                get_actor_unchecked(&actor_id)
                    .borrow_mut()
                    .handle(instruments);
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
                get_actor_unchecked(&actor_id)
                    .borrow_mut()
                    .handle(instrument);
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

    pub fn handle(&mut self, msg: &dyn Any) {
        // TODO: Optimize
        let type_id = msg.type_id();

        if type_id == TypeId::of::<InstrumentAny>() {
            self.handle_instrument(msg.downcast_ref::<InstrumentAny>().unwrap())
        } else if type_id == TypeId::of::<Vec<InstrumentAny>>() {
            self.handle_instruments(msg.downcast_ref::<Vec<InstrumentAny>>().unwrap())
        } else if type_id == TypeId::of::<OrderBook>() {
            self.handle_book(msg.downcast_ref::<OrderBook>().unwrap())
        } else if type_id == TypeId::of::<OrderBookDeltas>() {
            self.handle_book_deltas(msg.downcast_ref::<OrderBookDeltas>().unwrap())
        } else if type_id == TypeId::of::<QuoteTick>() {
            self.handle_quote(msg.downcast_ref::<QuoteTick>().unwrap())
        } else if type_id == TypeId::of::<TradeTick>() {
            self.handle_trade(msg.downcast_ref::<TradeTick>().unwrap())
        } else if type_id == TypeId::of::<MarkPriceUpdate>() {
            self.handle_mark_price(msg.downcast_ref::<MarkPriceUpdate>().unwrap())
        } else if type_id == TypeId::of::<IndexPriceUpdate>() {
            self.handle_index_price(msg.downcast_ref::<IndexPriceUpdate>().unwrap())
        } else if type_id == TypeId::of::<InstrumentStatus>() {
            self.handle_instrument_status(msg.downcast_ref::<InstrumentStatus>().unwrap())
        } else if type_id == TypeId::of::<Bar>() {
            self.handle_bar(msg.downcast_ref::<Bar>().unwrap())
        } else {
            self.handle_data(msg)
        }
    }

    // Handler methods for data processing
    /// Handle a received instrument
    pub fn handle_data(&mut self, data: &dyn Any) {
        // TODO: Log receipt
        // TODO: Check component state is running
        self.on_data(data)
    }

    // Handler methods for data processing
    /// Handle a received instrument
    pub(crate) fn handle_instrument(&mut self, instrument: &InstrumentAny) {
        // TODO: Log receipt
        // TODO: Check component state is running

        self.on_instrument(instrument);
    }

    /// Handle multiple received instruments
    pub(crate) fn handle_instruments(&mut self, instruments: &Vec<InstrumentAny>) {
        // TODO: Log receipt
        // TODO: Check component state is running

        for instrument in instruments {
            self.handle_instrument(instrument);
        }
    }

    /// Handle receiving order book deltas.
    pub(crate) fn handle_book(&mut self, book: &OrderBook) {
        // TODO: Log receipt
        // TODO: Check component state is running
        self.on_book(book);
    }

    /// Handle receiving an order book.
    pub(crate) fn handle_book_deltas(&mut self, deltas: &OrderBookDeltas) {
        // TODO: Log receipt
        // TODO: Check component state is running
        self.on_book_deltas(deltas);
    }

    /// Handle receiving a quote.
    pub(crate) fn handle_quote(&mut self, quote: &QuoteTick) {
        // TODO: Log receipt
        // TODO: Check component state is running
        self.on_quote(quote);
    }

    /// Handle receiving a trade.
    pub(crate) fn handle_trade(&mut self, trade: &TradeTick) {
        // TODO: Log receipt
        // TODO: Check component state is running
        self.on_trade(trade);
    }

    /// Handle receiving a mark price update.
    pub(crate) fn handle_mark_price(&mut self, mark_price: &MarkPriceUpdate) {
        // TODO: Log receipt
        // TODO: Check component state is running
        self.on_mark_price(mark_price);
    }

    /// Handle receiving a mark price update.
    pub(crate) fn handle_index_price(&mut self, index_price: &IndexPriceUpdate) {
        // TODO: Log receipt
        // TODO: Check component state is running
        self.on_index_price(index_price);
    }

    /// Handle receiving a mark price update.
    pub(crate) fn handle_instrument_status(&mut self, instrument_status: &InstrumentStatus) {
        // TODO: Log receipt
        // TODO: Check component state is running
        self.on_instrument_status(instrument_status);
    }

    /// Handle receiving a bar.
    pub(crate) fn handle_bar(&mut self, bar: &Bar) {
        // TODO: Log receipt
        // TODO: Check component state is running
        self.on_bar(bar);
    }
}

// TODO: Scratch implementation for development

struct MyDataActor {
    core: DataActorCore,
}

impl Actor for MyDataActor {
    fn id(&self) -> ComponentId {
        self.core.actor_id
    }

    fn handle(&mut self, msg: &dyn Any) {
        // Let the core handle message routing
        self.core.handle(msg);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// User implements DataActor trait overriding handlers are required
impl DataActor for MyDataActor {
    fn on_data(&mut self, data: &dyn Any) {
        println!("Received generic data");
    }

    fn on_book(&mut self, book: &OrderBook) {
        println!("Received a book {book}");
    }

    fn on_quote(&mut self, quote: &QuoteTick) {
        println!("Received a quote {quote}");
    }
}

// Custom functionality as required
impl MyDataActor {
    pub fn custom_function(&mut self) {}
}
