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
pub mod handlers;
#[cfg(feature = "indicators")]
pub(crate) mod indicators;
pub mod registry;
pub mod registry_v2;

use std::{
    any::Any,
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

use config::ActorConfig;
use executor::ActorExecutor;
use handlers::{HandleData, HandleInstrument, HandleInstruments};
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
use registry_v2::{get_actor, get_actor_unchecked, register_actor};
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
        handler::ShareableMessageHandler,
        switchboard::{
            self, MessagingSwitchboard, get_custom_topic, get_instrument_topic,
            get_instruments_topic,
        },
    },
};

type RequestCallback = Box<dyn Fn(UUID4) + Send + Sync>; // TODO: TBD

/// Core functionality for all actors.
pub struct Actor {
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

impl Actor {
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

        let actor_id = self.actor_id;
        let handler =
            ShareableMessageHandler(Rc::new(HandleData::new(Box::new(move |data: &dyn Any| {
                get_actor_unchecked(&actor_id)
                    .borrow_mut()
                    .handle_data(data);
            }))));

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

        let actor_id = self.actor_id;
        let handler = ShareableMessageHandler(Rc::new(HandleInstruments::new(Box::new(
            move |instruments: &Vec<InstrumentAny>| {
                get_actor_unchecked(&actor_id)
                    .borrow_mut()
                    .handle_instruments(instruments);
            },
        ))));

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

        let actor_id = self.actor_id;
        let handler = ShareableMessageHandler(Rc::new(HandleInstrument::new(Box::new(Box::new(
            move |instrument: &InstrumentAny| {
                get_actor_unchecked(&actor_id)
                    .borrow_mut()
                    .handle_instrument(instrument);
            },
        )))));

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

    /// Actions to be performed when the actor state is saved.
    pub fn on_save(&self) -> HashMap<String, Vec<u8>> {
        // Default implementation returns empty state
        HashMap::new()
    }

    /// Actions to be performed when the actor state is loaded.
    pub fn on_load(&mut self, state: HashMap<String, Vec<u8>>) {
        // Default empty implementation
    }

    /// Actions to be performed on start.
    pub fn on_start(&mut self) {
        // Default implementation - warning will be logged by implementations
    }

    /// Actions to be performed on stop.
    pub fn on_stop(&mut self) {
        // Default implementation - warning will be logged by implementations
    }

    /// Actions to be performed on resume.
    pub fn on_resume(&mut self) {
        // Default implementation - warning will be logged by implementations
    }

    /// Actions to be performed on reset.
    pub fn on_reset(&mut self) {
        // Default implementation - warning will be logged by implementations
    }

    /// Actions to be performed on dispose.
    pub fn on_dispose(&mut self) {
        // Default empty implementation
    }

    /// Actions to be performed on degrade.
    pub fn on_degrade(&mut self) {
        // Default empty implementation
    }

    /// Actions to be performed on fault.
    pub fn on_fault(&mut self) {
        // Default empty implementation
    }

    /// Actions to be performed when receiving an instrument status update.
    pub fn on_instrument_status(&mut self, data: InstrumentStatus) {
        // Default empty implementation
    }

    // Actions to be performed when receiving an instrument close update.  // TODO: Implement data
    // pub fn on_instrument_close(&mut self, update: InstrumentClose) {
    //     // Default empty implementation
    // }

    /// Actions to be performed when receiving an instrument.
    pub fn on_instrument(&mut self, instrument: &InstrumentAny) {
        // Default empty implementation
    }

    /// Actions to be performed when receiving an order book.
    pub fn on_order_book(&mut self, order_book: &OrderBook) {
        // Default empty implementation
    }

    /// Actions to be performed when receiving order book deltas.
    pub fn on_order_book_deltas(&mut self, deltas: &OrderBookDeltas) {
        // Default empty implementation
    }

    /// Actions to be performed when receiving a quote tick.
    pub fn on_quote_tick(&mut self, tick: &QuoteTick) {
        // Default empty implementation
    }

    /// Actions to be performed when receiving a trade tick.
    pub fn on_trade_tick(&mut self, tick: &TradeTick) {
        // Default empty implementation
    }

    /// Actions to be performed when receiving a mark price update.
    pub fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) {
        // Default empty implementation
    }

    /// Actions to be performed when receiving an index price update.
    pub fn on_index_price(&mut self, index_price: &IndexPriceUpdate) {
        // Default empty implementation
    }

    /// Actions to be performed when receiving a bar.
    pub fn on_bar(&mut self, bar: &Bar) {
        // Default empty implementation
    }

    /// Actions to be performed when receiving data.
    pub fn on_data(&mut self, data: &dyn Any) {
        // Default empty implementation
    }

    // Actions to be performed when receiving a signal. // TODO: TBD
    // pub fn on_signal(&mut self, signal: &impl Data) {
    //     // Default empty implementation
    // }

    // Actions to be performed when receiving historical data.
    pub fn on_historical_data(&mut self, data: &dyn Any) { // TODO: Probably break this down
        // Default empty implementation
    }

    // Actions to be performed when receiving an event.
    // pub fn on_event(&mut self, event: &i Event) {  // TODO: TBD
    //     // Default empty implementation
    // }

    // Handler methods for data processing
    /// Handle a received instrument
    pub fn handle_data(&mut self, data: &dyn Any) {
        // TODO: Check component state is running
        // TODO: Try to call on_instrument and handle any errors
    }

    // Handler methods for data processing
    /// Handle a received instrument
    pub(crate) fn handle_instrument(&mut self, instrument: &InstrumentAny) {
        // TODO: Check component state is running
        // TODO: Try to call on_instrument and handle any errors
    }

    /// Handle multiple received instruments
    pub(crate) fn handle_instruments(&mut self, instruments: &Vec<InstrumentAny>) {
        // TODO: Log receipt of instruments

        for instrument in instruments {
            self.handle_instrument(instrument);
        }
    }

    // TBC
}
