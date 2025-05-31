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
    collections::HashSet,
    fmt::Debug,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::Arc,
};

use ahash::{AHashMap, AHashSet};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos, correctness::check_predicate_true};
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

#[cfg(feature = "indicators")]
use super::indicators::Indicators;
use super::{Actor, registry::get_actor_unchecked};
use crate::{
    cache::Cache,
    clock::Clock,
    enums::{ComponentState, ComponentTrigger},
    logging::{CMD, RECV, REQ, SEND},
    messages::{
        data::{
            BarsResponse, BookResponse, CustomDataResponse, DataCommand, InstrumentResponse,
            InstrumentsResponse, QuotesResponse, RequestBars, RequestBookSnapshot, RequestCommand,
            RequestCustomData, RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades,
            SubscribeBars, SubscribeBookDeltas, SubscribeBookSnapshots, SubscribeCommand,
            SubscribeCustomData, SubscribeIndexPrices, SubscribeInstrument,
            SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments,
            SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeBookSnapshots, UnsubscribeCommand,
            UnsubscribeCustomData, UnsubscribeIndexPrices, UnsubscribeInstrument,
            UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus, UnsubscribeInstruments,
            UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
        system::ShutdownSystem,
    },
    msgbus::{
        self, MStr, Pattern, Topic, get_message_bus,
        handler::{MessageHandler, ShareableMessageHandler, TypedMessageHandler},
        switchboard::{
            self, MessagingSwitchboard, get_bars_topic, get_book_deltas_topic,
            get_book_snapshots_topic, get_custom_topic, get_index_price_topic,
            get_instrument_close_topic, get_instrument_status_topic, get_instrument_topic,
            get_instruments_topic, get_mark_price_topic, get_quotes_topic, get_trades_topic,
        },
    },
    signal::Signal,
    timer::TimeEvent,
};

/// Common configuration for [`DataActor`] based components.
#[derive(Debug, Clone)]
pub struct DataActorConfig {
    /// The custom identifier for the Actor.
    pub actor_id: Option<ActorId>,
    /// If events should be logged.
    pub log_events: bool,
    /// If commands should be logged.
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
    /// Returns the [`ComponentState`] of the actor.
    fn state(&self) -> ComponentState;

    /// Returns `true` if the actor is in a `Ready` state.
    fn is_ready(&self) -> bool {
        self.state() == ComponentState::Ready
    }

    /// Returns `true` if the actor is in a `Running` state.
    fn is_running(&self) -> bool {
        self.state() == ComponentState::Running
    }

    /// Returns `true` if the actor is in a `Stopped` state.
    fn is_stopped(&self) -> bool {
        self.state() == ComponentState::Stopped
    }

    /// Returns `true` if the actor is in a `Disposed` state.
    fn is_disposed(&self) -> bool {
        self.state() == ComponentState::Disposed
    }

    /// Returns `true` if the actor is in a `Degraded` state.
    fn is_degraded(&self) -> bool {
        self.state() == ComponentState::Degraded
    }

    /// Returns `true` if the actor is in a `Faulted` state.
    fn is_faulted(&self) -> bool {
        self.state() == ComponentState::Faulted
    }

    /// Actions to be performed when the actor state is saved.
    ///
    /// # Errors
    ///
    /// Returns an error if saving the actor state fails.
    fn on_save(&self) -> anyhow::Result<IndexMap<String, Vec<u8>>> {
        Ok(IndexMap::new())
    }

    /// Actions to be performed when the actor state is loaded.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the actor state fails.
    fn on_load(&mut self, state: IndexMap<String, Vec<u8>>) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on start.
    ///
    /// # Errors
    ///
    /// Returns an error if starting the actor fails.
    fn on_start(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_start` handler was called when not overridden, \
            it's expected that any actions required when starting the actor \
            occur here, such as subscribing/requesting data"
        );
        Ok(())
    }

    /// Actions to be performed on stop.
    ///
    /// # Errors
    ///
    /// Returns an error if stopping the actor fails.
    fn on_stop(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_stop` handler was called when not overridden, \
            it's expected that any actions required when stopping the actor \
            occur here, such as unsubscribing from data",
        );
        Ok(())
    }

    /// Actions to be performed on resume.
    ///
    /// # Errors
    ///
    /// Returns an error if resuming the actor fails.
    fn on_resume(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_resume` handler was called when not overridden, \
            it's expected that any actions required when resuming the actor \
            following a stop occur here"
        );
        Ok(())
    }

    /// Actions to be performed on reset.
    ///
    /// # Errors
    ///
    /// Returns an error if resetting the actor fails.
    fn on_reset(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_reset` handler was called when not overridden, \
            it's expected that any actions required when resetting the actor \
            occur here, such as resetting indicators and other state"
        );
        Ok(())
    }

    /// Actions to be performed on dispose.
    ///
    /// # Errors
    ///
    /// Returns an error if disposing the actor fails.
    fn on_dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on degrade.
    ///
    /// # Errors
    ///
    /// Returns an error if degrading the actor fails.
    fn on_degrade(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on fault.
    ///
    /// # Errors
    ///
    /// Returns an error if faulting the actor fails.
    fn on_fault(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an event.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the event fails.
    fn on_event(&mut self, event: &dyn Any) -> anyhow::Result<()> {
        // TODO: Implement `Event` enum?
        Ok(())
    }

    /// Actions to be performed when receiving a time event.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the time event fails.
    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving custom data.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the data fails.
    fn on_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a signal.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the signal fails.
    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the instrument fails.
    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving order book deltas.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the book deltas fails.
    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an order book.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the book fails.
    fn on_book(&mut self, order_book: &OrderBook) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a quote.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the quote fails.
    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a trade.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the trade fails.
    fn on_trade(&mut self, tick: &TradeTick) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a bar.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the bar fails.
    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a mark price update.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the mark price update fails.
    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an index price update.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the index price update fails.
    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an instrument status update.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the instrument status update fails.
    fn on_instrument_status(&mut self, data: &InstrumentStatus) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an instrument close update.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the instrument close update fails.
    fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical data.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical data fails.
    fn on_historical_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical quotes.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical quotes fails.
    fn on_historical_quotes(&mut self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical trades.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical trades fails.
    fn on_historical_trades(&mut self, trades: &[TradeTick]) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical bars.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical bars fails.
    fn on_historical_bars(&mut self, bars: &[Bar]) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical mark prices.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical mark prices fails.
    fn on_historical_mark_prices(&mut self, mark_prices: &[MarkPriceUpdate]) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical index prices.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical index prices fails.
    fn on_historical_index_prices(
        &mut self,
        index_prices: &[IndexPriceUpdate],
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Handles a received custom data point.
    fn handle_data(&mut self, data: &dyn Any) {
        log_received(&data);

        if !self.is_running() {
            log_not_running(&data);
            return;
        }

        if let Err(e) = self.on_data(data) {
            log_error(&e);
        }
    }

    /// Handles a received signal.
    fn handle_signal(&mut self, signal: &Signal) {
        log_received(&signal);

        if !self.is_running() {
            log_not_running(&signal);
            return;
        }

        if let Err(e) = self.on_signal(signal) {
            log_error(&e);
        }
    }

    /// Handles a received instrument.
    fn handle_instrument(&mut self, instrument: &InstrumentAny) {
        log_received(&instrument);

        if !self.is_running() {
            log_not_running(&instrument);
            return;
        }

        if let Err(e) = self.on_instrument(instrument) {
            log_error(&e);
        }
    }

    /// Handles received order book deltas.
    fn handle_book_deltas(&mut self, deltas: &OrderBookDeltas) {
        log_received(&deltas);

        if !self.is_running() {
            log_not_running(&deltas);
            return;
        }

        if let Err(e) = self.on_book_deltas(deltas) {
            log_error(&e);
        }
    }

    /// Handles a received order book reference.
    fn handle_book(&mut self, book: &OrderBook) {
        log_received(&book);

        if !self.is_running() {
            log_not_running(&book);
            return;
        }

        if let Err(e) = self.on_book(book) {
            log_error(&e);
        };
    }

    /// Handles a received quote.
    fn handle_quote(&mut self, quote: &QuoteTick) {
        log_received(&quote);

        if !self.is_running() {
            log_not_running(&quote);
            return;
        }

        if let Err(e) = self.on_quote(quote) {
            log_error(&e);
        }
    }

    /// Handles a received trade.
    fn handle_trade(&mut self, trade: &TradeTick) {
        log_received(&trade);

        if !self.is_running() {
            log_not_running(&trade);
            return;
        }

        if let Err(e) = self.on_trade(trade) {
            log_error(&e);
        }
    }

    /// Handles a receiving bar.
    fn handle_bar(&mut self, bar: &Bar) {
        log_received(&bar);

        if !self.is_running() {
            log_not_running(&bar);
            return;
        }

        if let Err(e) = self.on_bar(bar) {
            log_error(&e);
        }
    }

    /// Handles a received mark price update.
    fn handle_mark_price(&mut self, mark_price: &MarkPriceUpdate) {
        log_received(&mark_price);

        if !self.is_running() {
            log_not_running(&mark_price);
            return;
        }

        if let Err(e) = self.on_mark_price(mark_price) {
            log_error(&e);
        }
    }

    /// Handles a received index price update.
    fn handle_index_price(&mut self, index_price: &IndexPriceUpdate) {
        log_received(&index_price);

        if !self.is_running() {
            log_not_running(&index_price);
            return;
        }

        if let Err(e) = self.on_index_price(index_price) {
            log_error(&e);
        }
    }

    /// Handles a received instrument status.
    fn handle_instrument_status(&mut self, status: &InstrumentStatus) {
        log_received(&status);

        if !self.is_running() {
            log_not_running(&status);
            return;
        }

        if let Err(e) = self.on_instrument_status(status) {
            log_error(&e);
        }
    }

    /// Handles a received instrument close.
    fn handle_instrument_close(&mut self, close: &InstrumentClose) {
        log_received(&close);

        if !self.is_running() {
            log_not_running(&close);
            return;
        }

        if let Err(e) = self.on_instrument_close(close) {
            log_error(&e);
        }
    }

    /// Handles received historical data.
    fn handle_historical_data(&mut self, data: &dyn Any) {
        log_received(&data);

        if let Err(e) = self.on_historical_data(data) {
            log_error(&e);
        }
    }

    /// Handles a received time event.
    fn handle_time_event(&mut self, event: &TimeEvent) {
        log_received(&event);

        if let Err(e) = self.on_time_event(event) {
            log_error(&e);
        }
    }

    /// Handles a received event.
    fn handle_event(&mut self, event: &dyn Any) {
        log_received(&event);

        if let Err(e) = self.on_event(event) {
            log_error(&e);
        }
    }

    /// Handles a data response.
    fn handle_data_response(&mut self, response: &CustomDataResponse) {
        log_received(&response);

        if let Err(e) = self.on_historical_data(response.data.as_ref()) {
            log_error(&e);
        }
    }

    /// Handles an instrument response.
    fn handle_instrument_response(&mut self, response: &InstrumentResponse) {
        log_received(&response);

        if let Err(e) = self.on_instrument(&response.data) {
            log_error(&e);
        }
    }

    /// Handles an instruments response.
    fn handle_instruments_response(&mut self, response: &InstrumentsResponse) {
        log_received(&response);

        for inst in &response.data {
            if let Err(e) = self.on_instrument(inst) {
                log_error(&e);
            }
        }
    }

    /// Handles a book response.
    fn handle_book_response(&mut self, response: &BookResponse) {
        log_received(&response);

        if let Err(e) = self.on_book(&response.data) {
            log_error(&e);
        }
    }

    /// Handles a quotes response.
    fn handle_quotes_response(&mut self, response: &QuotesResponse) {
        log_received(&response);

        if let Err(e) = self.on_historical_quotes(&response.data) {
            log_error(&e);
        }
    }

    /// Handles a trades response.
    fn handle_trades_response(&mut self, response: &TradesResponse) {
        log_received(&response);

        if let Err(e) = self.on_historical_trades(&response.data) {
            log_error(&e);
        }
    }

    /// Handles a bars response.
    fn handle_bars_response(&mut self, response: &BarsResponse) {
        log_received(&response);

        if let Err(e) = self.on_historical_bars(&response.data) {
            log_error(&e);
        }
    }
}

/// Core functionality for all actors.
pub struct DataActorCore {
    /// The actor identifier.
    pub actor_id: ActorId,
    /// The actors configuration.
    pub config: DataActorConfig,
    /// The actors clock.
    pub clock: Rc<RefCell<dyn Clock>>,
    /// The cache for the actor.
    pub cache: Rc<RefCell<Cache>>,
    state: ComponentState,
    trader_id: Option<TraderId>,
    warning_events: AHashSet<String>, // TODO: TBD
    pending_requests: AHashMap<UUID4, Option<RequestCallback>>,
    signal_classes: AHashMap<String, String>,
    #[cfg(feature = "indicators")]
    indicators: Indicators,
    topic_handlers: AHashMap<MStr<Topic>, ShareableMessageHandler>,
}

impl Debug for DataActorCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DataActorCore))
            .field("actor_id", &self.actor_id)
            .field("config", &self.config)
            .field("state", &self.state)
            .field("trader_id", &self.trader_id)
            .finish()
    }
}

impl DataActor for DataActorCore {
    fn state(&self) -> ComponentState {
        self.state
    }
}

impl DataActorCore {
    /// Creates a new [`DataActorCore`] instance.
    pub fn new(
        config: DataActorConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> Self {
        let actor_id = config
            .actor_id
            .unwrap_or_else(|| Self::default_actor_id(&config));

        Self {
            actor_id,
            config,
            clock,
            cache,
            state: ComponentState::default(),
            trader_id: None, // None until registered
            warning_events: AHashSet::new(),
            pending_requests: AHashMap::new(),
            signal_classes: AHashMap::new(),
            #[cfg(feature = "indicators")]
            indicators: Indicators::default(),
            topic_handlers: AHashMap::new(),
        }
    }

    fn default_actor_id(config: &DataActorConfig) -> ActorId {
        let memory_address = std::ptr::from_ref(config) as *const _ as usize;
        ActorId::from(format!("{}-{memory_address}", stringify!(DataActor)))
    }

    fn transition_state(&mut self, trigger: ComponentTrigger) -> anyhow::Result<()> {
        self.state = self.state.transition(&trigger)?;
        log::info!("{}", self.state);
        Ok(())
    }

    // TODO: TBD initialization flow

    /// Initializes the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if the initialization state transition fails.
    pub fn initialize(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Initialize)
    }

    /// Returns the trader ID this actor is registered to.
    pub fn trader_id(&self) -> Option<TraderId> {
        self.trader_id
    }

    // TODO: Extract this common state logic and handling out to some component module
    pub fn state(&self) -> ComponentState {
        self.state
    }

    // -- REGISTRATION ----------------------------------------------------------------------------

    /// Register an event type for warning log levels.
    pub fn register_warning_event(&mut self, event_type: &str) {
        self.warning_events.insert(event_type.to_string());
    }

    /// Deregister an event type from warning log levels.
    pub fn deregister_warning_event(&mut self, event_type: &str) {
        self.warning_events.remove(event_type);
        // TODO: Log deregistration
    }

    /// Sets the trader ID for the actor.
    ///
    /// # Panics
    ///
    /// Panics if a trader ID has already been set.
    pub(crate) fn set_trader_id(&mut self, trader_id: TraderId) {
        if let Some(existing_trader_id) = self.trader_id {
            panic!("trader_id {existing_trader_id} already set");
        }

        self.trader_id = Some(trader_id)
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
            log::info!("{CMD}{SEND} {command:?}");
        }

        let endpoint = MessagingSwitchboard::data_engine_execute();
        msgbus::send(endpoint, command.as_any())
    }

    fn send_data_req<A: DataActor>(&self, request: RequestCommand) {
        if self.config.log_commands {
            log::info!("{REQ}{SEND} {request:?}");
        }

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |response: &CustomDataResponse| {
                get_actor_unchecked::<A>(&actor_id).handle_data_response(response);
            },
        )));

        let msgbus = get_message_bus()
            .borrow_mut()
            .register_response_handler(request.request_id(), handler);

        let endpoint = MessagingSwitchboard::data_engine_execute();
        msgbus::send(endpoint, request.as_any())
    }

    /// Starts the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if starting the actor fails.
    pub fn start(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Start)?; // -> Starting

        if let Err(e) = self.on_start() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::StartCompleted)?;

        Ok(())
    }

    /// Stops the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if stopping the actor fails.
    pub fn stop(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Stop)?; // -> Stopping

        if let Err(e) = self.on_stop() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::StopCompleted)?;

        Ok(())
    }

    /// Resumes the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if resuming the actor fails.
    pub fn resume(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Resume)?; // -> Resuming

        if let Err(e) = self.on_stop() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::ResumeCompleted)?;

        Ok(())
    }

    /// Resets the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if resetting the actor fails.
    pub fn reset(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Reset)?; // -> Resetting

        if let Err(e) = self.on_reset() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::ResetCompleted)?;

        Ok(())
    }

    /// Disposes the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if disposing the actor fails.
    pub fn dispose(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Dispose)?; // -> Disposing

        if let Err(e) = self.on_dispose() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::DisposeCompleted)?;

        Ok(())
    }

    /// Degrades the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if degrading the actor fails.
    pub fn degrade(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Degrade)?; // -> Degrading

        if let Err(e) = self.on_degrade() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::DegradeCompleted)?;

        Ok(())
    }

    /// Faults the actor.
    ///
    /// # Errors
    ///
    /// Returns an error if faulting the actor fails.
    pub fn fault(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Fault)?; // -> Faulting

        if let Err(e) = self.on_fault() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::FaultCompleted)?;

        Ok(())
    }

    /// Sends a shutdown command to the system with an optional reason.
    ///
    /// # Panics
    ///
    /// Panics if the actor is not registered or has no trader ID.
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

        let endpoint = "command.system.shutdown".into();
        msgbus::send(endpoint, command.as_any());
    }

    // -- SUBSCRIPTIONS ---------------------------------------------------------------------------

    fn get_or_create_handler_for_topic<F>(
        &mut self,
        topic: MStr<Topic>,
        create_handler: F,
    ) -> ShareableMessageHandler
    where
        F: FnOnce() -> ShareableMessageHandler,
    {
        if let Some(existing_handler) = self.topic_handlers.get(&topic) {
            existing_handler.clone()
        } else {
            let new_handler = create_handler();
            self.topic_handlers.insert(topic, new_handler.clone());
            new_handler
        }
    }

    fn get_handler_for_topic(&self, topic: MStr<Topic>) -> Option<ShareableMessageHandler> {
        self.topic_handlers.get(&topic).cloned()
    }

    /// Subscribe to streaming `data_type` data.
    pub fn subscribe_data<A: DataActor>(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_custom_topic(&data_type);
        let actor_id = self.actor_id.inner();
        let handler = if let Some(existing_handler) = self.topic_handlers.get(&topic) {
            existing_handler.clone()
        } else {
            let new_handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::with_any(
                move |data: &dyn Any| {
                    get_actor_unchecked::<A>(&actor_id).handle_data(data);
                },
            )));

            self.topic_handlers.insert(topic, new_handler.clone());
            new_handler
        };

        msgbus::subscribe_topic(topic, handler, None);

        if client_id.is_none() {
            // If no client ID specified, just subscribe to the topic
            return;
        }

        let command = SubscribeCommand::Data(SubscribeCustomData {
            data_type,
            client_id,
            venue: None,
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to streaming [`InstrumentAny`] data for the `venue`.
    pub fn subscribe_instruments<A: DataActor>(
        &mut self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instruments_topic(venue);
        let actor_id = self.actor_id.inner();
        let handler = self.get_or_create_handler_for_topic(topic, || {
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |instrument: &InstrumentAny| {
                    get_actor_unchecked::<A>(&actor_id).handle_instrument(instrument);
                },
            )))
        });

        msgbus::subscribe_topic(topic, handler, None);

        let command = SubscribeCommand::Instruments(SubscribeInstruments {
            client_id,
            venue,
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Subscribe to streaming [`InstrumentAny`] data for the `instrument_id`.
    pub fn subscribe_instrument<A: DataActor>(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_topic(instrument_id);
        let actor_id = self.actor_id.inner();
        let handler = self.get_or_create_handler_for_topic(topic, || {
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |instrument: &InstrumentAny| {
                    get_actor_unchecked::<A>(&actor_id).handle_instrument(instrument);
                },
            )))
        });

        msgbus::subscribe_topic(topic, handler, None);

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

    /// Subscribe to streaming [`OrderBookDeltas`] data for the `instrument_id`.
    ///
    /// Once subscribed, any matching order book deltas published on the message bus are forwarded
    /// to the `on_book_deltas` handler.
    pub fn subscribe_book_deltas<A: DataActor>(
        &mut self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        managed: bool,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_book_deltas_topic(instrument_id);
        let actor_id = self.actor_id.inner();
        let handler = self.get_or_create_handler_for_topic(topic, || {
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |deltas: &OrderBookDeltas| {
                    get_actor_unchecked::<A>(&actor_id).handle_book_deltas(deltas);
                },
            )))
        });

        msgbus::subscribe_topic(topic, handler, None);

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

    /// Subscribe to [`OrderBook`] snapshots at a specified interval for the `instrument_id`.
    ///
    /// Once subscribed, any matching order book snapshots published on the message bus are forwarded
    /// to the `on_book` handler.
    ///
    /// # Warnings
    ///
    /// Consider subscribing to order book deltas if you need intervals less than 100 milliseconds.
    pub fn subscribe_book_at_interval<A: DataActor>(
        &mut self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<NonZeroUsize>,
        interval_ms: NonZeroUsize,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        if book_type == BookType::L1_MBP && depth.is_some_and(|d| d.get() > 1) {
            log::error!(
                "Cannot subscribe to order book snapshots: L1 MBP book subscription depth > 1, was {:?}",
                depth,
            );
            return;
        }

        let topic = get_book_snapshots_topic(instrument_id);
        let actor_id = self.actor_id.inner();
        let handler = self.get_or_create_handler_for_topic(topic, || {
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |book: &OrderBook| {
                    get_actor_unchecked::<A>(&actor_id).handle_book(book);
                },
            )))
        });

        msgbus::subscribe_topic(topic, handler, None);

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

    /// Subscribe to streaming [`QuoteTick`] data for the `instrument_id`.
    pub fn subscribe_quotes<A: DataActor>(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_quotes_topic(instrument_id);
        let actor_id = self.actor_id.inner();
        let handler = self.get_or_create_handler_for_topic(topic, || {
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |quote: &QuoteTick| {
                    get_actor_unchecked::<A>(&actor_id).handle_quote(quote);
                },
            )))
        });

        msgbus::subscribe_topic(topic, handler, None);

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

    /// Subscribe to streaming [`TradeTick`] data for the `instrument_id`.
    pub fn subscribe_trades<A: DataActor>(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_trades_topic(instrument_id);
        let actor_id = self.actor_id.inner();
        let handler = self.get_or_create_handler_for_topic(topic, || {
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |trade: &TradeTick| {
                    get_actor_unchecked::<A>(&actor_id).handle_trade(trade);
                },
            )))
        });

        msgbus::subscribe_topic(topic, handler, None);

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

    /// Subscribe to streaming [`Bar`] data for the `bar_type`.
    ///
    /// Once subscribed, any matching bar data published on the message bus is forwarded
    /// to the `on_bar` handler.
    pub fn subscribe_bars<A: DataActor>(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        await_partial: bool,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_bars_topic(bar_type);
        let actor_id = self.actor_id.inner();
        let handler = self.get_or_create_handler_for_topic(topic, || {
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(move |bar: &Bar| {
                get_actor_unchecked::<A>(&actor_id).handle_bar(bar);
            })))
        });

        msgbus::subscribe_topic(topic, handler, None);

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

    /// Subscribe to streaming [`MarkPriceUpdate`] data for the `instrument_id`.
    ///
    /// Once subscribed, any matching mark price updates published on the message bus are forwarded
    /// to the `on_mark_price` handler.
    pub fn subscribe_mark_prices<A: DataActor>(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_mark_price_topic(instrument_id);
        let actor_id = self.actor_id.inner();
        let handler = self.get_or_create_handler_for_topic(topic, || {
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |mark_price: &MarkPriceUpdate| {
                    get_actor_unchecked::<A>(&actor_id).handle_mark_price(mark_price);
                },
            )))
        });

        msgbus::subscribe_topic(topic, handler, None);

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

    /// Subscribe to streaming [`IndexPriceUpdate`] data for the `instrument_id`.
    ///
    /// Once subscribed, any matching index price updates published on the message bus are forwarded
    /// to the `on_index_price` handler.
    pub fn subscribe_index_prices<A: DataActor>(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_index_price_topic(instrument_id);
        let actor_id = self.actor_id.inner();
        let handler = self.get_or_create_handler_for_topic(topic, || {
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |index_price: &IndexPriceUpdate| {
                    get_actor_unchecked::<A>(&actor_id).handle_index_price(index_price);
                },
            )))
        });

        msgbus::subscribe_topic(topic, handler, None);

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

    /// Subscribe to streaming [`InstrumentStatus`] data for the `instrument_id`.
    ///
    /// Once subscribed, any matching bar data published on the message bus is forwarded
    /// to the `on_bar` handler.
    pub fn subscribe_instrument_status<A: DataActor>(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_status_topic(instrument_id);
        let actor_id = self.actor_id.inner();
        let handler = self.get_or_create_handler_for_topic(topic, || {
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |status: &InstrumentStatus| {
                    get_actor_unchecked::<A>(&actor_id).handle_instrument_status(status);
                },
            )))
        });

        msgbus::subscribe_topic(topic, handler, None);

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

    /// Subscribe to streaming [`InstrumentClose`] data for the `instrument_id`.
    ///
    /// Once subscribed, any matching instrument close data published on the message bus is forwarded
    /// to the `on_instrument_close` handler.
    pub fn subscribe_instrument_close<A: DataActor>(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_close_topic(instrument_id);
        let actor_id = self.actor_id.inner();
        let handler = self.get_or_create_handler_for_topic(topic, || {
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
                move |close: &InstrumentClose| {
                    get_actor_unchecked::<A>(&actor_id).handle_instrument_close(close);
                },
            )))
        });

        msgbus::subscribe_topic(topic, handler, None);

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

    /// Unsubscribe from streaming `data_type` data.
    pub fn unsubscribe_data<A: DataActor>(
        &self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_custom_topic(&data_type);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

        if client_id.is_none() {
            return;
        }

        let command = UnsubscribeCommand::Data(UnsubscribeCustomData {
            data_type,
            client_id,
            venue: None,
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Unsubscribe from streaming [`Instrument`] data for the `venue`.
    pub fn unsubscribe_instruments<A: DataActor>(
        &self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instruments_topic(venue);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

        let command = UnsubscribeCommand::Instruments(UnsubscribeInstruments {
            client_id,
            venue,
            command_id: UUID4::new(),
            ts_init: self.generate_ts_init(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Unsubscribe from streaming [`Instrument`] definitions for the `instrument_id`.
    pub fn unsubscribe_instrument<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_topic(instrument_id);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

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

    /// Unsubscribe from streaming [`OrderBookDeltas`] for the `instrument_id`.
    pub fn unsubscribe_book_deltas<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_book_deltas_topic(instrument_id);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

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

    /// Unsubscribe from [`OrderBook`] snapshots at a specified interval for the `instrument_id`.
    ///
    /// The `interval_ms` must match a previously subscribed interval.
    pub fn unsubscribe_book_at_interval<A: DataActor>(
        &mut self,
        instrument_id: InstrumentId,
        interval_ms: NonZeroUsize,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_book_snapshots_topic(instrument_id);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

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

    /// Unsubscribe from streaming [`QuoteTick`] data for the `instrument_id`.
    pub fn unsubscribe_quotes<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_quotes_topic(instrument_id);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

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

    /// Unsubscribe from streaming [`TradeTick`] data for the `instrument_id`.
    pub fn unsubscribe_trades<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_trades_topic(instrument_id);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

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

    /// Unsubscribe from streaming [`Bar`] data for the `bar_type`.
    pub fn unsubscribe_bars<A: DataActor>(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_bars_topic(bar_type);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

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

    /// Unsubscribe from streaming [`MarkPriceUpdate`] data for the `instrument_id`.
    pub fn unsubscribe_mark_prices<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_mark_price_topic(instrument_id);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

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

    /// Unsubscribe from streaming [`IndexPriceUpdate`] data for the `instrument_id`.
    pub fn unsubscribe_index_prices<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_index_price_topic(instrument_id);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

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

    /// Unsubscribe from streaming [`InstrumentStatus`] data for the `instrument_id`.
    pub fn unsubscribe_instrument_status<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_status_topic(instrument_id);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

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

    /// Unsubscribe from streaming [`InstrumentClose`] data for the `instrument_id`.
    pub fn unsubscribe_instrument_close<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_close_topic(instrument_id);
        if let Some(handler) = self.topic_handlers.get(&topic) {
            msgbus::unsubscribe_topic(topic, handler.clone());
        };

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

    // -- REQUESTS --------------------------------------------------------------------------------

    /// Request historical custom data of the given `data_type`.
    ///
    /// Returns a unique request ID to correlate subsequent [`CustomDataResponse`].
    ///
    /// # Errors
    ///
    /// Returns an error if the provided time range is invalid.
    pub fn request_data<A: DataActor>(
        &self,
        data_type: DataType,
        client_id: ClientId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock.borrow().utc_now();
        check_timestamps(now, start, end)?;

        let request_id = UUID4::new();
        let command = RequestCommand::Data(RequestCustomData {
            client_id,
            data_type,
            request_id,
            ts_init: self.generate_ts_init(),
            params,
        });

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |response: &CustomDataResponse| {
                get_actor_unchecked::<A>(&actor_id).handle_data_response(response);
            },
        )));

        let msgbus = get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler);

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Request historical [`InstrumentResponse`] data for the given `instrument_id`.
    ///
    /// Returns a unique request ID to correlate subsequent [`InstrumentResponse`].
    ///
    /// # Errors
    ///
    /// Returns an error if the provided time range is invalid.
    pub fn request_instrument<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock.borrow().utc_now();
        check_timestamps(now, start, end)?;

        let request_id = UUID4::new();
        let command = RequestCommand::Instrument(RequestInstrument {
            instrument_id,
            start,
            end,
            client_id,
            request_id,
            ts_init: now.into(),
            params,
        });

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |response: &InstrumentResponse| {
                get_actor_unchecked::<A>(&actor_id).handle_instrument_response(response);
            },
        )));

        let msgbus = get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler);

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Request historical [`InstrumentsResponse`] definitions for the optional `venue`.
    ///
    /// Returns a unique request ID to correlate subsequent [`InstrumentsResponse`].
    ///
    /// # Errors
    ///
    /// Returns an error if the provided time range is invalid.
    pub fn request_instruments<A: DataActor>(
        &self,
        venue: Option<Venue>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock.borrow().utc_now();
        check_timestamps(now, start, end)?;

        let request_id = UUID4::new();
        let command = RequestCommand::Instruments(RequestInstruments {
            venue,
            start,
            end,
            client_id,
            request_id,
            ts_init: now.into(),
            params,
        });

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |response: &InstrumentsResponse| {
                get_actor_unchecked::<A>(&actor_id).handle_instruments_response(response);
            },
        )));

        let msgbus = get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler);

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Request an [`OrderBook`] snapshot for the given `instrument_id`.
    ///
    /// Returns a unique request ID to correlate subsequent [`BookResponse`].
    ///
    /// # Errors
    ///
    /// This function never returns an error.
    pub fn request_book_snapshot<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        depth: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let request_id = UUID4::new();
        let command = RequestCommand::BookSnapshot(RequestBookSnapshot {
            instrument_id,
            depth,
            client_id,
            request_id,
            ts_init: self.generate_ts_init(),
            params,
        });

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |response: &BookResponse| {
                get_actor_unchecked::<A>(&actor_id).handle_book_response(response);
            },
        )));

        let msgbus = get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler);

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Request historical [`QuoteTick`] data for the given `instrument_id`.
    ///
    /// Returns a unique request ID to correlate subsequent [`QuotesResponse`].
    ///
    /// # Errors
    ///
    /// Returns an error if the provided time range is invalid.
    pub fn request_quotes<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock.borrow().utc_now();
        check_timestamps(now, start, end)?;

        let request_id = UUID4::new();
        let command = RequestCommand::Quotes(RequestQuotes {
            instrument_id,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init: now.into(),
            params,
        });

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |response: &QuotesResponse| {
                get_actor_unchecked::<A>(&actor_id).handle_quotes_response(response);
            },
        )));

        let msgbus = get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler);

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Request historical [`TradeTick`] data for the given `instrument_id`.
    ///
    /// Returns a unique request ID to correlate subsequent [`TradesResponse`].
    ///
    /// # Errors
    ///
    /// Returns an error if the provided time range is invalid.
    pub fn request_trades<A: DataActor>(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock.borrow().utc_now();
        check_timestamps(now, start, end)?;

        let request_id = UUID4::new();
        let command = RequestCommand::Trades(RequestTrades {
            instrument_id,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init: now.into(),
            params,
        });

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |response: &TradesResponse| {
                get_actor_unchecked::<A>(&actor_id).handle_trades_response(response);
            },
        )));

        let msgbus = get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler);

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Request historical [`Bar`] data for the given `bar_type`.
    ///
    /// Returns a unique request ID to correlate subsequent [`BarsResponse`].
    ///
    /// # Errors
    ///
    /// Returns an error if the provided time range is invalid.
    pub fn request_bars<A: DataActor>(
        &self,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock.borrow().utc_now();
        check_timestamps(now, start, end)?;

        let request_id = UUID4::new();
        let command = RequestCommand::Bars(RequestBars {
            bar_type,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init: now.into(),
            params,
        });

        let actor_id = self.actor_id.inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |response: &BarsResponse| {
                get_actor_unchecked::<A>(&actor_id).handle_bars_response(response);
            },
        )));

        let msgbus = get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler);

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }
}

fn check_timestamps(
    now: DateTime<Utc>,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> anyhow::Result<()> {
    if let Some(start) = start {
        check_predicate_true(start <= now, "start was > now")?
    }
    if let Some(end) = end {
        check_predicate_true(end <= now, "end was > now")?
    }

    if let (Some(start), Some(end)) = (start, end) {
        check_predicate_true(start < end, "start was >= end")?
    }

    Ok(())
}

fn log_error(e: &anyhow::Error) {
    log::error!("{e}");
}

fn log_not_running<T>(msg: &T)
where
    T: Debug,
{
    // TODO: Potentially temporary for development? drop level at some stage
    log::warn!("Received message when not running - skipping {msg:?}");
}

fn log_received<T>(msg: &T)
where
    T: Debug,
{
    log::debug!("{RECV} {msg:?}");
}
