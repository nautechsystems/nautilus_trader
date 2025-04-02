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
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::Arc,
};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{
        Bar, BarType, DataType, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate,
        OrderBookDeltas, QuoteTick, TradeTick, close::InstrumentClose,
    },
    enums::BookType,
    identifiers::{ActorId, ClientId, InstrumentId, TraderId, Venue},
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
    enums::{ComponentState, ComponentTrigger},
    logging::{CMD, RECV, SENT},
    messages::{
        data::{
            DataCommand, DataRequest, DataResponse, RequestBars, RequestInstrument,
            RequestInstruments, RequestOrderBookSnapshot, RequestQuoteTicks, RequestTradeTicks,
            SubscribeBars, SubscribeBookDeltas, SubscribeBookSnapshots, SubscribeCommand,
            SubscribeData, SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose,
            SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices, SubscribeQuotes,
            SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookSnapshots,
            UnsubscribeCommand, UnsubscribeData, UnsubscribeIndexPrices, UnsubscribeInstrument,
            UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus, UnsubscribeInstruments,
            UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
        system::ShutdownSystem,
    },
    msgbus::{
        self, get_message_bus,
        handler::{MessageHandler, ShareableMessageHandler, TypedMessageHandler},
        switchboard::{
            self, MessagingSwitchboard, get_bars_topic, get_book_deltas_topic,
            get_book_snapshots_topic, get_custom_topic, get_index_price_topic,
            get_instrument_close_topic, get_instrument_status_topic, get_instrument_topic,
            get_instruments_topic, get_mark_price_topic, get_quotes_topic, get_trades_topic,
        },
    },
    signal::Signal,
};

/// Configuration for Actor components.
#[derive(Debug, Clone)]
pub struct DataActorConfig {
    /// The custom identifier for the Actor.
    pub actor_id: Option<ActorId>,
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
    fn on_save(&self) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        Ok(HashMap::new())
    }

    /// Actions to be performed when the actor state is loaded.
    fn on_load(&mut self, state: HashMap<String, Vec<u8>>) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on start.
    fn on_start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on stop.
    fn on_stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on resume.
    fn on_resume(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on reset.
    fn on_reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on dispose.
    fn on_dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on degrade.
    fn on_degrade(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on fault.
    fn on_fault(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    // Actions to be performed when receiving an event.
    // pub fn on_event(&mut self, event: &i Event) {  // TODO: TBD
    //     // Default empty implementation
    // }
    //
    fn on_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a signal.
    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an instrument.
    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving order book deltas.
    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an order book.
    fn on_book(&mut self, order_book: &OrderBook) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a quote.
    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a trade.
    fn on_trade(&mut self, tick: &TradeTick) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a bar.
    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a mark price update.
    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an index price update.
    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an instrument status update.
    fn on_instrument_status(&mut self, data: &InstrumentStatus) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an instrument close update.
    fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical data.
    fn on_historical_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        // TODO: Probably break this down into more granular methods
        Ok(())
    }
}

/// Core functionality for all actors.
pub struct DataActorCore {
    /// The component ID for the actor.
    pub actor_id: ActorId,
    /// The actors configuration.
    pub config: DataActorConfig,
    /// The actors clock.
    pub clock: Rc<RefCell<dyn Clock>>,
    /// The read-only cache for the actor.
    pub cache: Rc<RefCell<Cache>>,
    state: ComponentState,
    trader_id: Option<TraderId>,
    executor: Option<Arc<dyn ActorExecutor>>, // TODO: TBD
    warning_events: HashSet<String>,          // TODO: TBD
    pending_requests: HashMap<UUID4, Option<RequestCallback>>,
    signal_classes: HashMap<String, String>,
    #[cfg(feature = "indicators")]
    indicators: Indicators,
}

impl DataActor for DataActorCore {}

impl DataActorCore {
    /// Creates a new [`Actor`] instance.
    pub fn new(
        config: DataActorConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
        switchboard: Arc<MessagingSwitchboard>,
    ) -> Self {
        let actor_id = config.actor_id.unwrap_or(ActorId::new("DataActor")); // TODO: Determine default ID

        Self {
            actor_id,
            config,
            clock,
            cache,
            state: ComponentState::default(),
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

    // TODO: Extract this common state logic and handling out to some component module
    pub fn state(&self) -> ComponentState {
        self.state
    }

    pub fn is_ready(&self) -> bool {
        self.state == ComponentState::Ready
    }

    pub fn is_running(&self) -> bool {
        self.state == ComponentState::Running
    }

    pub fn is_stopped(&self) -> bool {
        self.state == ComponentState::Stopped
    }

    pub fn is_disposed(&self) -> bool {
        self.state == ComponentState::Disposed
    }

    pub fn is_degraded(&self) -> bool {
        self.state == ComponentState::Degraded
    }

    pub fn is_faulting(&self) -> bool {
        self.state == ComponentState::Faulted
    }

    // -- REGISTRATION ----------------------------------------------------------------------------

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

    pub fn start(&mut self) -> anyhow::Result<()> {
        self.state.transition(&ComponentTrigger::Start)?; // -> Starting

        if let Err(e) = self.on_start() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.state.transition(&ComponentTrigger::StartCompleted)?;

        Ok(())
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        self.state.transition(&ComponentTrigger::Stop)?; // -> Stopping

        if let Err(e) = self.on_stop() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.state.transition(&ComponentTrigger::StopCompleted)?;

        Ok(())
    }

    pub fn resume(&mut self) -> anyhow::Result<()> {
        self.state.transition(&ComponentTrigger::Resume)?; // -> Resuming

        if let Err(e) = self.on_stop() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.state.transition(&ComponentTrigger::ResumeCompleted)?;

        Ok(())
    }

    pub fn reset(&mut self) -> anyhow::Result<()> {
        self.state.transition(&ComponentTrigger::Reset)?; // -> Resetting

        if let Err(e) = self.on_reset() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.state.transition(&ComponentTrigger::ResetCompleted)?;

        Ok(())
    }

    pub fn dispose(&mut self) -> anyhow::Result<()> {
        self.state.transition(&ComponentTrigger::Dispose)?; // -> Disposing

        if let Err(e) = self.on_dispose() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.state.transition(&ComponentTrigger::DisposeCompleted)?;

        Ok(())
    }

    pub fn degrade(&mut self) -> anyhow::Result<()> {
        self.state.transition(&ComponentTrigger::Degrade)?; // -> Degrading

        if let Err(e) = self.on_degrade() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.state.transition(&ComponentTrigger::DegradeCompleted)?;

        Ok(())
    }

    pub fn fault(&mut self) -> anyhow::Result<()> {
        self.state.transition(&ComponentTrigger::Fault)?; // -> Faulting

        if let Err(e) = self.on_fault() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.state.transition(&ComponentTrigger::FaultCompleted)?;

        Ok(())
    }

    pub fn shutdown_system(&self, reason: Option<String>) {
        self.check_registered();

        // SAFETY: Checked registered before unwrapping trader ID
        let command = ShutdownSystem::new(
            self.trader_id().unwrap(),
            self.actor_id.inner(),
            reason,
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
        );

        let topic = Ustr::from("command.system.shutdown");
        msgbus::send(&topic, command.as_any());
    }

    // -- SUBSCRIPTIONS ---------------------------------------------------------------------------

    /// Subscribe to streaming data of the given data type.
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

    /// Subscribe to streaming [`Instrument`] data for the given venue.
    pub fn subscribe_instruments(
        &self,
        venue: Venue,
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

    /// Subscribe to streaming [`InstrumentAny`] data for the given instrument ID.
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

    /// Subscribe to streaming [`OrderBookDeltas`] data for the given instrument ID.
    ///
    /// Once subscribed, any matching order book deltas published on the message bus is forwarded
    /// to the `on_book_deltas` handler.
    pub fn subscribe_book_deltas(
        &self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        managed: bool,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |deltas: &OrderBookDeltas| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle_book_deltas(deltas);
            },
        )));

        let topic = get_book_deltas_topic(instrument_id);
        msgbus::subscribe(topic, handler, None);

        let command = SubscribeCommand::BookDeltas(SubscribeBookDeltas {
            instrument_id,
            book_type,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            depth,
            managed,
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to [`OrderBook`] snapshots at a specified interval for the given instrument ID.
    ///
    /// Once subscribed, any matching order book snapshots published on the message bus are forwarded
    /// to the `on_book` handler.
    ///
    /// # Warnings
    ///
    /// Consider subscribing to order book deltas if you need intervals less than 100 milliseconds.
    pub fn subscribe_book_snapshots(
        &self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<NonZeroUsize>,
        interval_ms: NonZeroUsize,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        if book_type == BookType::L1_MBP && depth.is_some_and(|d| d.get() > 1) {
            log::error!(
                "Cannot subscribe to order book snapshots: L1 MBP book subscription depth > 1, was {:?}",
                depth,
            );
            return;
        }

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |book: &OrderBook| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle_book(book);
            },
        )));

        let topic = get_book_snapshots_topic(instrument_id);
        msgbus::subscribe(topic, handler, None);

        let command = SubscribeCommand::BookSnapshots(SubscribeBookSnapshots {
            instrument_id,
            book_type,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            depth,
            interval_ms,
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to streaming [`QuoteTick`] data for the given instrument ID.
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

    /// Subscribe to streaming [`TradeTick`] data for the given instrument ID.
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

    /// Subscribe to streaming [`Bar`] data for the given bar type.
    ///
    /// Once subscribed, any matching bar data published on the message bus is forwarded
    /// to the `on_bar` handler.
    pub fn subscribe_bars(
        &self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        await_partial: bool,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let actor_id = self.actor_id.inner();
        let handler =
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(move |bar: &Bar| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle_bar(bar);
            })));

        let topic = get_bars_topic(bar_type);
        msgbus::subscribe(topic, handler, None);

        let command = SubscribeCommand::Bars(SubscribeBars {
            bar_type,
            client_id,
            venue: Some(bar_type.instrument_id().venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            await_partial,
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to streaming [`MarkPriceUpdate`] data for the given instrument ID.
    ///
    /// Once subscribed, any matching mark price updates published on the message bus are forwarded
    /// to the `on_mark_price` handler.
    pub fn subscribe_mark_prices(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |mark_price: &MarkPriceUpdate| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle_mark_price(mark_price);
            },
        )));

        let topic = get_mark_price_topic(instrument_id);
        msgbus::subscribe(topic, handler, None);

        let command = SubscribeCommand::MarkPrices(SubscribeMarkPrices {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to streaming [`IndexPriceUpdate`] data for the given instrument ID.
    ///
    /// Once subscribed, any matching index price updates published on the message bus are forwarded
    /// to the `on_index_price` handler.
    pub fn subscribe_index_prices(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |index_price: &IndexPriceUpdate| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle_index_price(index_price);
            },
        )));

        let topic = get_index_price_topic(instrument_id);
        msgbus::subscribe(topic, handler, None);

        let command = SubscribeCommand::IndexPrices(SubscribeIndexPrices {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to streaming [`InstrumentStatus`] data for the given instrument ID.
    ///
    /// Once subscribed, any matching bar data published on the message bus is forwarded
    /// to the `on_bar` handler.
    pub fn subscribe_instrument_status(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |status: &InstrumentStatus| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle_instrument_status(status);
            },
        )));

        let topic = get_instrument_status_topic(instrument_id);
        msgbus::subscribe(topic, handler, None);

        let command = SubscribeCommand::InstrumentStatus(SubscribeInstrumentStatus {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to streaming [`InstrumentClose`] data for the given instrument ID.
    ///
    /// Once subscribed, any matching instrument close data published on the message bus is forwarded
    /// to the `on_instrument_close` handler.
    pub fn subscribe_instrument_close(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |close: &InstrumentClose| {
                get_actor_unchecked::<DataActorCore>(&actor_id).handle_instrument_close(close);
            },
        )));

        // Topic may need to be adjusted to match Python implementation
        let topic = get_instrument_close_topic(instrument_id);
        msgbus::subscribe(topic, handler, None);

        let command = SubscribeCommand::InstrumentClose(SubscribeInstrumentClose {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Unsubscribe from data of the given data type.
    pub fn unsubscribe_data(
        &self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_custom_topic(&data_type);
        // msgbus::unsubscribe(&topic, self.handle_data);  // TODO

        if client_id.is_none() {
            return;
        }

        let command = UnsubscribeCommand::Data(UnsubscribeData {
            data_type,
            client_id,
            venue: None,
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Unsubscribe from update `Instrument` data for the given venue.
    pub fn unsubscribe_instruments(
        &self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instruments_topic(venue);
        // msgbus::unsubscribe(&topic, self.handle_instruments);  // TODO!

        let command = UnsubscribeCommand::Instruments(UnsubscribeInstruments {
            client_id,
            venue,
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    pub fn unsubscribe_instrument(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_topic(instrument_id);
        // msgbus::unsubscribe(&topic, self.handle_instrument);  // TODO

        let command = UnsubscribeCommand::Instrument(UnsubscribeInstrument {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    pub fn unsubscribe_book_deltas(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_book_deltas_topic(instrument_id);
        // msgbus::unsubscribe(&topic, self.handle_book_deltas);

        let command = UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Unsubscribe from order book snapshots for the given instrument ID.
    pub fn unsubscribe_book_snapshots(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_book_snapshots_topic(instrument_id);
        // msgbus::unsubscribe(&topic, self.handle_book);  // TODO

        let command = UnsubscribeCommand::BookSnapshots(UnsubscribeBookSnapshots {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Unsubscribe from streaming `QuoteTick` data for the given instrument ID.
    pub fn unsubscribe_quote_ticks(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_quotes_topic(instrument_id);
        // msgbus::unsubscribe(&topic, self.handle_quote);  // TODO

        let command = UnsubscribeCommand::Quotes(UnsubscribeQuotes {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Unsubscribe from streaming `TradeTick` data for the given instrument ID.
    pub fn unsubscribe_trade_ticks(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_trades_topic(instrument_id);
        // msgbus::unsubscribe(&topic, self.handle_trade);  // TODO

        let command = UnsubscribeCommand::Trades(UnsubscribeTrades {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Unsubscribe from streaming `Bar` data for the given bar type.
    pub fn unsubscribe_bars(
        &self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_bars_topic(bar_type);
        // msgbus::unsubscribe(&topic, self.handle_bar);  // TODO

        let command = UnsubscribeCommand::Bars(UnsubscribeBars {
            bar_type,
            client_id,
            venue: Some(bar_type.instrument_id().venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Unsubscribe from streaming `MarkPriceUpdate` data for the given instrument ID.
    pub fn unsubscribe_mark_prices(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_mark_price_topic(instrument_id);
        // msgbus::unsubscribe(&topic, self.handle_mark_price);  // TODO

        let command = UnsubscribeCommand::MarkPrices(UnsubscribeMarkPrices {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Unsubscribe from streaming `IndexPriceUpdate` data for the given instrument ID.
    pub fn unsubscribe_index_prices(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_index_price_topic(instrument_id);
        // msgbus::unsubscribe(&topic, self.handle_index_price);  // TODO

        let command = UnsubscribeCommand::IndexPrices(UnsubscribeIndexPrices {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Unsubscribe from instrument status updates for the given instrument ID.
    pub fn unsubscribe_instrument_status(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_status_topic(instrument_id);
        // msgbus::unsubscribe(&topic, self.handle_instrument_status);  // TODO

        let command = UnsubscribeCommand::InstrumentStatus(UnsubscribeInstrumentStatus {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Unsubscribe from instrument close updates for the given instrument ID.
    pub fn unsubscribe_instrument_close(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<HashMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_close_topic(instrument_id);
        // msgbus::unsubscribe(&topic, self.handle_instrument_close);  // TODO

        let command = UnsubscribeCommand::InstrumentClose(UnsubscribeInstrumentClose {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    // -- HANDLERS --------------------------------------------------------------------------------

    /// Handles a received custom/generic data point.
    pub fn handle_data(&mut self, data: &dyn Any) {
        log_received(&data);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_data(data) {
            log_error(&e);
        }
    }

    /// Handles a received instrument.
    pub(crate) fn handle_instrument(&mut self, instrument: &InstrumentAny) {
        log_received(&instrument);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_instrument(instrument) {
            log_error(&e);
        }
    }

    /// Handles received order book deltas.
    pub(crate) fn handle_book_deltas(&mut self, deltas: &OrderBookDeltas) {
        log_received(&deltas);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_book_deltas(deltas) {
            log_error(&e);
        }
    }

    /// Handle a received order book reference.
    pub(crate) fn handle_book(&mut self, book: &OrderBook) {
        log_received(&book);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_book(book) {
            log_error(&e);
        };
    }

    /// Handles a received quote.
    pub(crate) fn handle_quote(&mut self, quote: &QuoteTick) {
        log_received(&quote);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_quote(quote) {
            log_error(&e);
        }
    }

    /// Handles a received trade.
    pub(crate) fn handle_trade(&mut self, trade: &TradeTick) {
        log_received(&trade);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_trade(trade) {
            log_error(&e);
        }
    }

    /// Handles a receiving bar.
    pub(crate) fn handle_bar(&mut self, bar: &Bar) {
        log_received(&bar);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_bar(bar) {
            log_error(&e);
        }
    }

    /// Handles a received mark price update.
    pub(crate) fn handle_mark_price(&mut self, mark_price: &MarkPriceUpdate) {
        log_received(&mark_price);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_mark_price(mark_price) {
            log_error(&e);
        }
    }

    /// Handles a received index price update.
    pub(crate) fn handle_index_price(&mut self, index_price: &IndexPriceUpdate) {
        log_received(&index_price);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_index_price(index_price) {
            log_error(&e);
        }
    }

    /// Handles a received instrument status.
    pub(crate) fn handle_instrument_status(&mut self, status: &InstrumentStatus) {
        log_received(&status);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_instrument_status(status) {
            log_error(&e);
        }
    }

    /// Handles a received instrument close.
    pub(crate) fn handle_instrument_close(&mut self, close: &InstrumentClose) {
        log_received(&close);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_instrument_close(close) {
            log_error(&e);
        }
    }

    /// Handles multiple received instruments.
    pub(crate) fn handle_instruments(&mut self, instruments: &Vec<InstrumentAny>) {
        for instrument in instruments {
            self.handle_instrument(instrument);
        }
    }

    /// Handles multiple received quote ticks.
    pub(crate) fn handle_quotes(&mut self, quotes: &Vec<QuoteTick>) {
        for quote in quotes {
            self.handle_quote(quote);
        }
    }

    /// Handles multiple received trade ticks.
    pub(crate) fn handle_trades(&mut self, trades: &Vec<TradeTick>) {
        for trade in trades {
            self.handle_trade(trade);
        }
    }

    /// Handles multiple received bars.
    pub(crate) fn handle_bars(&mut self, bars: &Vec<Bar>) {
        for bar in bars {
            self.handle_bar(bar);
        }
    }

    /// Handles a received historical data.
    pub(crate) fn handle_historical_data(&mut self, data: &dyn Any) {
        log_received(&data);

        if let Err(e) = self.on_historical_data(data) {
            log_error(&e);
        }
    }

    /// Handles a received signal.
    pub(crate) fn handle_signal(&mut self, signal: &Signal) {
        log_received(&signal);

        if !self.is_running() {
            return;
        }

        if let Err(e) = self.on_signal(signal) {
            log_error(&e);
        }
    }
}

fn log_error(e: &anyhow::Error) {
    log::error!("{e}");
}

fn log_received<T>(msg: &T)
where
    T: std::fmt::Debug,
{
    log::debug!("{} {:?}", RECV, msg);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// Tests
///////////////////////////////////////////////////////////////////////////////////////////////////
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
        identifiers::ActorId,
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
            switchboard::{MessagingSwitchboard, get_quotes_topic, get_trades_topic},
        },
    };

    struct TestDataActor {
        core: DataActorCore,
        pub received_quotes: Vec<TradeTick>,
        pub received_trades: Vec<TradeTick>,
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
        fn on_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
            Ok(())
        }

        fn on_book(&mut self, book: &OrderBook) -> anyhow::Result<()> {
            Ok(())
        }

        fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
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
            switchboard: Arc<MessagingSwitchboard>,
        ) -> Self {
            Self {
                core: DataActorCore::new(config, cache, clock, switchboard),
                received_quotes: Vec::new(),
                received_trades: Vec::new(),
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

    fn test_subscribe_and_receive_quotes(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        switchboard: Arc<MessagingSwitchboard>,
        audusd_sim: CurrencyPair,
    ) {
        register_data_actor(clock.clone(), cache.clone(), switchboard.clone());

        let actor_id = ActorId::new("DataActor").inner(); // TODO: Determine default ID
        let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
        actor.subscribe_quotes(audusd_sim.id, None, None);

        let topic = get_quotes_topic(audusd_sim.id);
        let trade = QuoteTick::default();
        msgbus::publish(&topic, &trade);
        msgbus::publish(&topic, &trade);

        assert_eq!(actor.received_quotes.len(), 2);
    }

    fn test_subscribe_and_receive_trades(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        switchboard: Arc<MessagingSwitchboard>,
        audusd_sim: CurrencyPair,
    ) {
        register_data_actor(clock.clone(), cache.clone(), switchboard.clone());

        let actor_id = ActorId::new("DataActor").inner(); // TODO: Determine default ID
        let actor = get_actor_unchecked::<TestDataActor>(&actor_id);
        actor.subscribe_trades(audusd_sim.id, None, None);

        let topic = get_trades_topic(audusd_sim.id);
        let trade = TradeTick::default();
        msgbus::publish(&topic, &trade);
        msgbus::publish(&topic, &trade);

        assert_eq!(actor.received_trades.len(), 2);
    }
}
