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
    cell::RefCell,
    collections::{HashMap, HashSet},
    marker::PhantomData,
    ops::Deref,
    rc::Rc,
    sync::Arc,
};

use log;
use nautilus_common::{
    cache::Cache,
    client::DataClientAdapter,
    clock::Clock,
    component::{Disposed, PreInitialized, Ready, Running, Starting, State, Stopped, Stopping},
    enums::ComponentState,
    logging::{RECV, RES},
    messages::data::DataResponse,
    msgbus::MessageBus,
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
    identifiers::{ClientId, InstrumentId},
    instruments::{any::InstrumentAny, synthetic::SyntheticInstrument},
};

pub struct DataEngineConfig {
    pub time_bars_build_with_no_updates: bool,
    pub time_bars_timestamp_on_close: bool,
    pub time_bars_interval_type: String, // Make this an enum `BarIntervalType`
    pub validate_data_sequence: bool,
    pub buffer_deltas: bool,
    pub external_clients: Vec<ClientId>,
    pub debug: bool,
}

pub struct DataEngine<State = PreInitialized> {
    state: PhantomData<State>,
    clock: Box<dyn Clock>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    default_client: Option<DataClientAdapter>,
    // order_book_intervals: HashMap<(InstrumentId, usize), Vec<fn(&OrderBook)>>,  // TODO
    // bar_aggregators:  // TODO
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
        config: DataEngineConfig,
    ) -> Self {
        Self {
            state: PhantomData::<PreInitialized>,
            clock,
            cache,
            default_client: None,
            synthetic_quote_feeds: HashMap::new(),
            synthetic_trade_feeds: HashMap::new(),
            buffered_deltas_map: HashMap::new(),
            config,
            msgbus,
        }
    }
}

impl<S: State> DataEngine<S> {
    fn transition<NewState>(self) -> DataEngine<NewState> {
        DataEngine {
            state: PhantomData,
            clock: self.clock,
            cache: self.cache,
            default_client: self.default_client,
            synthetic_quote_feeds: self.synthetic_quote_feeds,
            synthetic_trade_feeds: self.synthetic_trade_feeds,
            buffered_deltas_map: self.buffered_deltas_map,
            config: self.config,
            msgbus: self.msgbus,
        }
    }

    #[must_use]
    pub fn state(&self) -> ComponentState {
        S::state()
    }

    #[must_use]
    pub fn check_connected(&self) -> bool {
        self.msgbus
            .borrow()
            .clients
            .values()
            .all(|client| client.is_connected())
    }

    #[must_use]
    pub fn check_disconnected(&self) -> bool {
        self.msgbus
            .borrow()
            .clients
            .values()
            .all(|client| !client.is_connected())
    }

    #[must_use]
    pub fn registed_clients(&self) -> Vec<ClientId> {
        self.msgbus.borrow().clients.keys().copied().collect()
    }

    // -- SUBSCRIPTIONS ---------------------------------------------------------------------------

    fn collect_subscriptions<F, T>(&self, get_subs: F) -> Vec<T>
    where
        F: Fn(&DataClientAdapter) -> &HashSet<T>,
        T: Clone,
    {
        let mut subs = Vec::new();
        for client in self.msgbus.borrow().clients.values() {
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
}

impl DataEngine<PreInitialized> {
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

    fn initialize(self) -> DataEngine<Ready> {
        self.transition()
    }
}

impl DataEngine<Ready> {
    #[must_use]
    pub fn start(self) -> DataEngine<Starting> {
        self.msgbus
            .borrow()
            .clients
            .values()
            .for_each(|client| client.start());
        self.transition()
    }

    #[must_use]
    pub fn stop(self) -> DataEngine<Stopping> {
        self.msgbus
            .borrow()
            .clients
            .values()
            .for_each(|client| client.stop());
        self.transition()
    }

    #[must_use]
    pub fn reset(self) -> Self {
        self.msgbus
            .borrow()
            .clients
            .values()
            .for_each(|client| client.reset());
        self.transition()
    }

    #[must_use]
    pub fn dispose(mut self) -> DataEngine<Disposed> {
        self.msgbus
            .borrow()
            .clients
            .values()
            .for_each(|client| client.dispose());
        self.clock.cancel_timers();
        self.transition()
    }
}

impl DataEngine<Starting> {
    #[must_use]
    pub fn on_start(self) -> DataEngine<Running> {
        self.transition()
    }
}

impl DataEngine<Running> {
    pub fn connect(&self) {
        todo!() //  Implement actual client connections for a live/sandbox context
    }

    pub fn disconnect(&self) {
        todo!() // Implement actual client connections for a live/sandbox context
    }

    #[must_use]
    pub fn stop(self) -> DataEngine<Stopping> {
        self.transition()
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

    pub fn response(&self, response: DataResponse) {
        log::debug!("{}", format!("{RECV}{RES} response")); // TODO: Display for response

        match response.data_type.type_name() {
            stringify!(InstrumentAny) => {
                let instruments = Arc::downcast::<Vec<InstrumentAny>>(response.data.clone())
                    .expect("Invalid response data");
                self.handle_instruments(instruments);
            }
            stringify!(QuoteTick) => {
                let quotes = Arc::downcast::<Vec<QuoteTick>>(response.data.clone())
                    .expect("Invalid response data");
                self.handle_quotes(quotes);
            }
            stringify!(TradeTick) => {
                let trades = Arc::downcast::<Vec<TradeTick>>(response.data.clone())
                    .expect("Invalid response data");
                self.handle_trades(trades);
            }
            stringify!(Bar) => {
                let bars = Arc::downcast::<Vec<Bar>>(response.data.clone())
                    .expect("Invalid response data");
                self.handle_bars(bars);
            }
            _ => {} // Nothing else to handle
        }

        // self.msgbus.response()  // TODO: Send response to registered handler
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

        let instrument_id = instrument.id();
        let topic = format!(
            "data.instrument.{}.{}",
            instrument_id.venue, instrument_id.symbol
        );
    }

    fn handle_delta(&self, delta: OrderBookDelta) {
        // TODO: Manage buffered deltas
        // TODO: Manage book

        let topic = format!(
            "data.book.deltas.{}.{}",
            delta.instrument_id.venue, delta.instrument_id.symbol
        );
    }

    fn handle_deltas(&self, deltas: OrderBookDeltas) {
        // TODO: Manage book

        let topic = format!(
            "data.book.snapshots.{}.{}", // TODO: Revise snapshots topic component
            deltas.instrument_id.venue, deltas.instrument_id.symbol
        );
    }

    fn handle_depth10(&self, depth: OrderBookDepth10) {
        // TODO: Manage book

        let topic = format!(
            "data.book.depth.{}.{}",
            depth.instrument_id.venue, depth.instrument_id.symbol
        );
    }

    fn handle_quote(&self, quote: QuoteTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_quote(quote) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle synthetics

        let topic = format!(
            "data.quotes.{}.{}",
            quote.instrument_id.venue, quote.instrument_id.symbol
        );
    }

    fn handle_trade(&self, trade: TradeTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_trade(trade) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle synthetics

        let topic = format!(
            "data.trades.{}.{}",
            trade.instrument_id.venue, trade.instrument_id.symbol
        );
    }

    fn handle_bar(&self, bar: Bar) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_bar(bar) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle additional bar logic

        let topic = format!("data.bars.{}", bar.bar_type);
    }

    // -- RESPONSE HANDLERS -----------------------------------------------------------------------

    fn handle_instruments(&self, instruments: Arc<Vec<InstrumentAny>>) {
        // TODO improve by adding bulk update methods to cache and database
        for instrument in instruments.iter() {
            if let Err(e) = self
                .cache
                .as_ref()
                .borrow_mut()
                .add_instrument(instrument.clone())
            {
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

impl DataEngine<Stopping> {
    #[must_use]
    pub fn on_stop(self) -> DataEngine<Stopped> {
        self.transition()
    }
}

impl DataEngine<Stopped> {
    #[must_use]
    pub fn reset(self) -> DataEngine<Ready> {
        self.transition()
    }

    #[must_use]
    pub fn dispose(self) -> DataEngine<Disposed> {
        self.transition()
    }
}
