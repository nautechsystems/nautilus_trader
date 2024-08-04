// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

//! Provides a generic `DataEngine` for all environments.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

pub mod runner;

use std::{
    any::Any,
    cell::RefCell,
    collections::{HashMap, HashSet},
    ops::Deref,
    rc::Rc,
    sync::Arc,
};

use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{RECV, RES},
    messages::data::{DataRequest, DataResponse, SubscriptionCommand},
    msgbus::{handler::MessageHandler, MessageBus},
};
use nautilus_model::{
    data::{
        bar::{Bar, BarType},
        delta::OrderBookDelta,
        deltas::OrderBookDeltas,
        depth::OrderBookDepth10,
        quote::QuoteTick,
        trade::TradeTick,
        Data, DataType,
    },
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{any::InstrumentAny, synthetic::SyntheticInstrument},
};
use ustr::Ustr;

use crate::{aggregation::BarAggregator, client::DataClientAdapter};

pub struct DataEngineConfig {
    pub time_bars_build_with_no_updates: bool,
    pub time_bars_timestamp_on_close: bool,
    pub time_bars_interval_type: String, // Make this an enum `BarIntervalType`
    pub validate_data_sequence: bool,
    pub buffer_deltas: bool,
    pub external_clients: Option<Vec<ClientId>>,
    pub debug: bool,
}

impl Default for DataEngineConfig {
    fn default() -> Self {
        Self {
            time_bars_build_with_no_updates: true,
            time_bars_timestamp_on_close: true,
            time_bars_interval_type: "left_open".to_string(), // Make this an enum `BarIntervalType`
            validate_data_sequence: false,
            buffer_deltas: false,
            external_clients: None,
            debug: false,
        }
    }
}

pub struct DataEngine {
    clock: Box<dyn Clock>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    clients: IndexMap<ClientId, DataClientAdapter>,
    default_client: Option<DataClientAdapter>,
    routing_map: IndexMap<Venue, ClientId>,
    // order_book_intervals: HashMap<(InstrumentId, usize), Vec<fn(&OrderBook)>>,  // TODO
    bar_aggregators: Vec<Box<dyn BarAggregator>>, // TODO: dyn for now
    synthetic_quote_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    synthetic_trade_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    buffered_deltas_map: HashMap<InstrumentId, Vec<OrderBookDelta>>,
    config: DataEngineConfig,
}

impl DataEngine {
    #[must_use]
    pub fn new(
        clock: Box<dyn Clock>,
        cache: Rc<RefCell<Cache>>,
        msgbus: Rc<RefCell<MessageBus>>,
        config: Option<DataEngineConfig>,
    ) -> Self {
        Self {
            clock,
            cache,
            msgbus,
            clients: IndexMap::new(),
            routing_map: IndexMap::new(),
            default_client: None,
            bar_aggregators: Vec::new(),
            synthetic_quote_feeds: HashMap::new(),
            synthetic_trade_feeds: HashMap::new(),
            buffered_deltas_map: HashMap::new(),
            config: config.unwrap_or_default(),
        }
    }
}

impl DataEngine {
    // pub fn register_catalog(&mut self, catalog: ParquetDataCatalog) {}  TODO: Implement catalog

    /// Register the given data `client` with the engine as the default routing client.
    ///
    /// When a specific venue routing cannot be found, this client will receive messages.
    ///
    /// # Warnings
    ///
    /// Any existing default routing client will be overwritten.
    /// TODO: change this to suit message bus behaviour
    pub fn register_default_client(&mut self, client: DataClientAdapter) {
        log::info!("Registered default client {}", client.client_id());
        self.default_client = Some(client);
    }

    pub fn start(self) {
        self.clients.values().for_each(|client| client.start());
    }

    pub fn stop(self) {
        self.clients.values().for_each(|client| client.stop());
    }

    pub fn reset(self) {
        self.clients.values().for_each(|client| client.reset());
    }

    pub fn dispose(mut self) {
        self.clients.values().for_each(|client| client.dispose());
        self.clock.cancel_timers();
    }

    pub fn connect(&self) {
        todo!() //  Implement actual client connections for a live/sandbox context
    }

    pub fn disconnect(&self) {
        todo!() // Implement actual client connections for a live/sandbox context
    }

    #[must_use]
    pub fn check_connected(&self) -> bool {
        self.clients.values().all(|client| client.is_connected())
    }

    #[must_use]
    pub fn check_disconnected(&self) -> bool {
        self.clients.values().all(|client| !client.is_connected())
    }

    #[must_use]
    pub fn registed_clients(&self) -> Vec<ClientId> {
        self.clients.keys().copied().collect()
    }

    // -- SUBSCRIPTIONS ---------------------------------------------------------------------------

    fn collect_subscriptions<F, T>(&self, get_subs: F) -> Vec<T>
    where
        F: Fn(&DataClientAdapter) -> &HashSet<T>,
        T: Clone,
    {
        let mut subs = Vec::new();
        for client in self.clients.values() {
            subs.extend(get_subs(client).iter().cloned());
        }
        subs
    }

    #[must_use]
    pub fn subscribed_custom_data(&self) -> Vec<DataType> {
        self.collect_subscriptions(|client| &client.subscriptions_generic)
    }

    #[must_use]
    pub fn subscribed_instruments(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_instrument)
    }

    #[must_use]
    pub fn subscribed_order_book_deltas(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_order_book_delta)
    }

    #[must_use]
    pub fn subscribed_order_book_snapshots(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_order_book_snapshot)
    }

    #[must_use]
    pub fn subscribed_quote_ticks(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_quote_tick)
    }

    #[must_use]
    pub fn subscribed_trade_ticks(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_trade_tick)
    }

    #[must_use]
    pub fn subscribed_bars(&self) -> Vec<BarType> {
        self.collect_subscriptions(|client| &client.subscriptions_bar)
    }

    #[must_use]
    pub fn subscribed_instrument_status(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_instrument_status)
    }

    #[must_use]
    pub fn subscribed_instrument_close(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_instrument_close)
    }

    pub fn on_start(self) {
        todo!()
    }

    pub fn on_stop(self) {
        todo!()
    }

    /// Registers a new [`DataClientAdapter`]
    pub fn register_client(&mut self, client: DataClientAdapter, routing: Option<Venue>) {
        if let Some(routing) = routing {
            self.routing_map.insert(routing, client.client_id());
            log::info!("Set client {} routing for {routing}", client.client_id());
        }

        log::info!("Registered client {}", client.client_id());
        self.clients.insert(client.client_id, client);
    }

    /// Deregisters a [`DataClientAdapter`]
    pub fn deregister_client(&mut self, client_id: &ClientId) {
        // TODO: We could return a `Result` but then this is part of system wiring and instead of
        // propagating results all over the place it may be cleaner to just immediately fail
        // for these sorts of design-time errors?
        // correctness::check_key_in_map(&client_id, &self.clients, "client_id", "clients").unwrap();

        self.clients.shift_remove(client_id);
        log::info!("Deregistered client {client_id}");
    }

    fn get_client(&self, client_id: &ClientId, venue: &Venue) -> Option<&DataClientAdapter> {
        match self.clients.get(client_id) {
            Some(client) => Some(client),
            None => self
                .routing_map
                .get(venue)
                .and_then(|client_id: &ClientId| self.clients.get(client_id)),
        }
    }

    /// Send a [`DataRequest`] to an endpoint that must be a data client implementation.
    pub fn execute(&mut self, msg: &dyn Any) {
        // TODO: log error
        if let Some(cmd) = msg.downcast_ref::<SubscriptionCommand>() {
            if let Some(client) = self.clients.get_mut(&cmd.client_id) {
                client.execute(cmd.clone())
            } else {
                log::error!(
                    "Cannot handle command: no client found for {}",
                    cmd.client_id
                );
            }
        }
    }

    pub fn request(&self, req: DataRequest) {
        if let Some(client) = self.clients.get(&req.client_id) {
            // TODO: We don't immediately need the response
            let _ = client.request(req);
        } else {
            log::error!(
                "Cannot handle request: no client found for {}",
                req.client_id
            );
        }
    }

    /// TODO: Probably not required
    /// Send a [`SubscriptionCommand`] to an endpoint that must be a data client implementation.
    pub fn send_subscription_command(&self, message: SubscriptionCommand) {
        if let Some(client) = self.get_client(&message.client_id, &message.venue) {
            client.through_execute(message);
        }
    }

    pub fn process(&self, data: Data) {
        match data {
            Data::Delta(delta) => self.handle_delta(delta),
            Data::Deltas(deltas) => self.handle_deltas(deltas.deref().clone()), // TODO: Optimize
            Data::Depth10(depth) => self.handle_depth10(depth),
            Data::Quote(quote) => self.handle_quote(quote),
            Data::Trade(trade) => self.handle_trade(trade),
            Data::Bar(bar) => self.handle_bar(bar),
        }
    }

    pub fn response(&self, resp: DataResponse) {
        log::debug!("{}", format!("{RECV}{RES} response")); // TODO: Display for response

        match resp.data_type.type_name() {
            stringify!(InstrumentAny) => {
                let instruments = Arc::downcast::<Vec<InstrumentAny>>(resp.data.clone())
                    .expect("Invalid response data");
                self.handle_instruments(instruments);
            }
            stringify!(QuoteTick) => {
                let quotes = Arc::downcast::<Vec<QuoteTick>>(resp.data.clone())
                    .expect("Invalid response data");
                self.handle_quotes(quotes);
            }
            stringify!(TradeTick) => {
                let trades = Arc::downcast::<Vec<TradeTick>>(resp.data.clone())
                    .expect("Invalid response data");
                self.handle_trades(trades);
            }
            stringify!(Bar) => {
                let bars =
                    Arc::downcast::<Vec<Bar>>(resp.data.clone()).expect("Invalid response data");
                self.handle_bars(bars);
            }
            _ => {} // Nothing else to handle
        }

        self.msgbus.as_ref().borrow().send_response(resp)
    }

    // -- DATA HANDLERS ---------------------------------------------------------------------------

    // TODO: Fix all handlers to not use msgbus
    fn handle_instrument(&self, instrument: InstrumentAny) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_instrument(instrument.clone())
        {
            log::error!("Error on cache insert: {e}");
        }

        let topic = get_instrument_publish_topic(&instrument);
        self.msgbus
            .as_ref()
            .borrow()
            .publish(&topic, &instrument as &dyn Any); // TODO: Optimize
    }

    fn handle_delta(&self, delta: OrderBookDelta) {
        // TODO: Manage buffered deltas
        // TODO: Manage book

        let topic = get_delta_publish_topic(&delta);
        self.msgbus
            .as_ref()
            .borrow()
            .publish(&topic, &delta as &dyn Any); // TODO: Optimize
    }

    fn handle_deltas(&self, deltas: OrderBookDeltas) {
        // TODO: Manage book

        let topic = get_deltas_publish_topic(&deltas);
        self.msgbus
            .as_ref()
            .borrow()
            .publish(&topic, &deltas as &dyn Any); // TODO: Optimize
    }

    fn handle_depth10(&self, depth: OrderBookDepth10) {
        // TODO: Manage book

        let topic = get_depth_publish_topic(&depth);
        self.msgbus
            .as_ref()
            .borrow()
            .publish(&topic, &depth as &dyn Any); // TODO: Optimize
    }

    fn handle_quote(&self, quote: QuoteTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_quote(quote) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle synthetics

        let topic = get_quote_publish_topic(&quote);
        self.msgbus
            .as_ref()
            .borrow()
            .publish(&topic, &quote as &dyn Any); // TODO: Optimize
    }

    fn handle_trade(&self, trade: TradeTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_trade(trade) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle synthetics

        let topic = get_trade_publish_topic(&trade);
        self.msgbus
            .as_ref()
            .borrow()
            .publish(&topic, &trade as &dyn Any); // TODO: Optimize
    }

    fn handle_bar(&self, bar: Bar) {
        // TODO: Handle additional bar logic
        if self.config.validate_data_sequence {
            if let Some(last_bar) = self.cache.as_ref().borrow().bar(&bar.bar_type) {
                if bar.ts_event < last_bar.ts_event {
                    log::warn!(
                        "Bar {bar} was prior to last bar `ts_event` {}",
                        last_bar.ts_event
                    );
                    return; // `bar` is out of sequence
                }
                if bar.ts_init < last_bar.ts_init {
                    log::warn!(
                        "Bar {bar} was prior to last bar `ts_init` {}",
                        last_bar.ts_init
                    );
                    return; // `bar` is out of sequence
                }
                // TODO: Implement `bar.is_revision` logic
            }
        }

        if let Err(e) = self.cache.as_ref().borrow_mut().add_bar(bar) {
            log::error!("Error on cache insert: {e}");
        }

        let topic = get_bar_publish_topic(&bar);
        self.msgbus
            .as_ref()
            .borrow()
            .publish(&topic, &bar as &dyn Any); // TODO: Optimize
    }

    // -- RESPONSE HANDLERS -----------------------------------------------------------------------

    fn handle_instruments(&self, instruments: Arc<Vec<InstrumentAny>>) {
        // TODO improve by adding bulk update methods to cache and database
        let mut cache = self.cache.as_ref().borrow_mut();
        for instrument in instruments.iter() {
            if let Err(e) = cache.add_instrument(instrument.clone()) {
                log::error!("Error on cache insert: {e}");
            }
        }
    }

    fn handle_quotes(&self, quotes: Arc<Vec<QuoteTick>>) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_quotes(&quotes) {
            log::error!("Error on cache insert: {e}");
        }
    }

    fn handle_trades(&self, trades: Arc<Vec<TradeTick>>) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_trades(&trades) {
            log::error!("Error on cache insert: {e}");
        }
    }

    fn handle_bars(&self, bars: Arc<Vec<Bar>>) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_bars(&bars) {
            log::error!("Error on cache insert: {e}");
        }
    }

    // -- INTERNAL --------------------------------------------------------------------------------

    fn update_order_book(&self, data: &Data) {
        // Only apply data if there is a book being managed,
        // as it may be being managed manually.
        if let Some(book) = self
            .cache
            .as_ref()
            .borrow_mut()
            .order_book(data.instrument_id())
        {
            match data {
                Data::Delta(delta) => book.apply_delta(delta),
                Data::Deltas(deltas) => book.apply_deltas(deltas),
                Data::Depth10(depth) => book.apply_depth(depth),
                _ => log::error!("Invalid data type for book update"),
            }
        }
    }
}

// TODO: Potentially move these
pub fn get_instrument_publish_topic(instrument: &InstrumentAny) -> String {
    let instrument_id = instrument.id();
    format!(
        "data.instrument.{}.{}",
        instrument_id.venue, instrument_id.symbol
    )
}

pub fn get_delta_publish_topic(delta: &OrderBookDelta) -> String {
    format!(
        "data.book.delta.{}.{}",
        delta.instrument_id.venue, delta.instrument_id.symbol
    )
}

pub fn get_deltas_publish_topic(delta: &OrderBookDeltas) -> String {
    format!(
        "data.book.snapshots.{}.{}",
        delta.instrument_id.venue, delta.instrument_id.symbol
    )
}

pub fn get_depth_publish_topic(depth: &OrderBookDepth10) -> String {
    format!(
        "data.book.depth.{}.{}",
        depth.instrument_id.venue, depth.instrument_id.symbol
    )
}

pub fn get_quote_publish_topic(quote: &QuoteTick) -> String {
    format!(
        "data.quotes.{}.{}",
        quote.instrument_id.venue, quote.instrument_id.symbol
    )
}

pub fn get_trade_publish_topic(trade: &TradeTick) -> String {
    format!(
        "data.trades.{}.{}",
        trade.instrument_id.venue, trade.instrument_id.symbol
    )
}

pub fn get_bar_publish_topic(bar: &Bar) -> String {
    format!("data.bars.{}", bar.bar_type)
}

pub struct SubscriptionCommandHandler {
    id: Ustr,
    data_engine: Rc<RefCell<DataEngine>>,
}

impl MessageHandler for SubscriptionCommandHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        self.data_engine.borrow_mut().execute(message)
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _resp: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use indexmap::indexmap;
    use nautilus_common::{
        clock::TestClock, messages::data::Action, msgbus::handler::ShareableMessageHandler,
    };
    use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
    use nautilus_model::{
        identifiers::TraderId,
        instruments::{currency_pair::CurrencyPair, stubs::audusd_sim},
    };
    use rstest::rstest;

    use super::*;
    use crate::mocks::MockDataClient;

    #[rstest]
    fn test_execute_subscribe_instruments(audusd_sim: CurrencyPair) {
        // TODO: Cleanup test and provide more stubs
        let trader_id = TraderId::from("TESTER-001");
        let clock = Box::new(TestClock::new());
        let cache = Rc::new(RefCell::new(Cache::default()));
        let msgbus = Rc::new(RefCell::new(MessageBus::new(
            trader_id,
            UUID4::new(),
            None,
            None,
        )));
        let switchboard = msgbus.borrow().switchboard.clone();
        let data_engine = DataEngine::new(clock, cache.clone(), msgbus.clone(), None);
        let data_engine = Rc::new(RefCell::new(data_engine));

        let client_id = ClientId::from("SIM");
        let venue = Venue::from("SIM");
        let client = Box::new(MockDataClient::new(
            cache.clone(),
            msgbus.clone(),
            client_id,
            venue,
        ));

        let client = DataClientAdapter::new(client_id, venue, client, Box::new(TestClock::new()));
        data_engine.borrow_mut().register_client(client, None);

        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id.to_string(),
        };
        let data_type = DataType::new(stringify!(QuoteTick), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: switchboard.data_engine_process,
            data_engine: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint.as_str(), handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        assert!(data_engine
            .borrow()
            .subscribed_quote_ticks()
            .contains(&audusd_sim.id));
    }
}
