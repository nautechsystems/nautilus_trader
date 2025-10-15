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
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    fmt::Debug,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use ahash::{AHashMap, AHashSet};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos, correctness::check_predicate_true};
#[cfg(feature = "defi")]
use nautilus_model::defi::{
    Block, Blockchain, Pool, PoolLiquidityUpdate, PoolSwap, data::PoolFeeCollect, data::PoolFlash,
};
use nautilus_model::{
    data::{
        Bar, BarType, DataType, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick, close::InstrumentClose,
    },
    enums::BookType,
    events::order::filled::OrderFilled,
    identifiers::{ActorId, ClientId, ComponentId, InstrumentId, TraderId, Venue},
    instruments::InstrumentAny,
    orderbook::OrderBook,
};
use ustr::Ustr;

#[cfg(feature = "indicators")]
use super::indicators::Indicators;
use super::{
    Actor,
    registry::{get_actor_unchecked, try_get_actor_unchecked},
};
#[cfg(feature = "defi")]
use crate::defi;
#[cfg(feature = "defi")]
#[allow(unused_imports)]
use crate::defi::data_actor as _; // Brings DeFi impl blocks into scope
use crate::{
    cache::Cache,
    clock::Clock,
    component::Component,
    enums::{ComponentState, ComponentTrigger},
    logging::{CMD, RECV, REQ, SEND},
    messages::{
        data::{
            BarsResponse, BookResponse, CustomDataResponse, DataCommand, InstrumentResponse,
            InstrumentsResponse, QuotesResponse, RequestBars, RequestBookSnapshot, RequestCommand,
            RequestCustomData, RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades,
            SubscribeBars, SubscribeBookDeltas, SubscribeBookSnapshots, SubscribeCommand,
            SubscribeCustomData, SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
            SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments,
            SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeBookSnapshots, UnsubscribeCommand,
            UnsubscribeCustomData, UnsubscribeFundingRates, UnsubscribeIndexPrices,
            UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
            UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
        system::ShutdownSystem,
    },
    msgbus::{
        self, MStr, Topic, get_message_bus,
        handler::{ShareableMessageHandler, TypedMessageHandler},
        switchboard::{
            MessagingSwitchboard, get_bars_topic, get_book_deltas_topic, get_book_snapshots_topic,
            get_custom_topic, get_funding_rate_topic, get_index_price_topic,
            get_instrument_close_topic, get_instrument_status_topic, get_instrument_topic,
            get_instruments_topic, get_mark_price_topic, get_order_fills_topic, get_quotes_topic,
            get_trades_topic,
        },
    },
    signal::Signal,
    timer::{TimeEvent, TimeEventCallback},
};

/// Common configuration for [`DataActor`] based components.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common", subclass)
)]
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

/// Configuration for creating actors from importable paths.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
pub struct ImportableActorConfig {
    /// The fully qualified name of the Actor class.
    pub actor_path: String,
    /// The fully qualified name of the Actor config class.
    pub config_path: String,
    /// The actor configuration as a dictionary.
    pub config: HashMap<String, serde_json::Value>,
}

type RequestCallback = Box<dyn Fn(UUID4) + Send + Sync>; // TODO: TBD

pub trait DataActor:
    Component + Deref<Target = DataActorCore> + DerefMut<Target = DataActorCore>
{
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
    #[allow(unused_variables)]
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

    /// Actions to be performed when receiving a time event.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the time event fails.
    #[allow(unused_variables)]
    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving custom data.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the data fails.
    #[allow(unused_variables)]
    fn on_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a signal.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the signal fails.
    #[allow(unused_variables)]
    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the instrument fails.
    #[allow(unused_variables)]
    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving order book deltas.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the book deltas fails.
    #[allow(unused_variables)]
    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an order book.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the book fails.
    #[allow(unused_variables)]
    fn on_book(&mut self, order_book: &OrderBook) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a quote.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the quote fails.
    #[allow(unused_variables)]
    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a trade.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the trade fails.
    #[allow(unused_variables)]
    fn on_trade(&mut self, tick: &TradeTick) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a bar.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the bar fails.
    #[allow(unused_variables)]
    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a mark price update.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the mark price update fails.
    #[allow(unused_variables)]
    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an index price update.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the index price update fails.
    #[allow(unused_variables)]
    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving a funding rate update.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the funding rate update fails.
    #[allow(unused_variables)]
    fn on_funding_rate(&mut self, funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an instrument status update.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the instrument status update fails.
    #[allow(unused_variables)]
    fn on_instrument_status(&mut self, data: &InstrumentStatus) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an instrument close update.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the instrument close update fails.
    #[allow(unused_variables)]
    fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving an order filled event.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the order filled event fails.
    #[allow(unused_variables)]
    fn on_order_filled(&mut self, event: &OrderFilled) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Actions to be performed when receiving a block.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the block fails.
    #[allow(unused_variables)]
    fn on_block(&mut self, block: &Block) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Actions to be performed when receiving a pool.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the pool fails.
    #[allow(unused_variables)]
    fn on_pool(&mut self, pool: &Pool) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Actions to be performed when receiving a pool swap.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the pool swap fails.
    #[allow(unused_variables)]
    fn on_pool_swap(&mut self, swap: &PoolSwap) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Actions to be performed when receiving a pool liquidity update.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the pool liquidity update fails.
    #[allow(unused_variables)]
    fn on_pool_liquidity_update(&mut self, update: &PoolLiquidityUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Actions to be performed when receiving a pool fee collect event.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the pool fee collect fails.
    #[allow(unused_variables)]
    fn on_pool_fee_collect(&mut self, collect: &PoolFeeCollect) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "defi")]
    /// Actions to be performed when receiving a pool flash event.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the pool flash fails.
    #[allow(unused_variables)]
    fn on_pool_flash(&mut self, flash: &PoolFlash) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical data.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical data fails.
    #[allow(unused_variables)]
    fn on_historical_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical quotes.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical quotes fails.
    #[allow(unused_variables)]
    fn on_historical_quotes(&mut self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical trades.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical trades fails.
    #[allow(unused_variables)]
    fn on_historical_trades(&mut self, trades: &[TradeTick]) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical bars.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical bars fails.
    #[allow(unused_variables)]
    fn on_historical_bars(&mut self, bars: &[Bar]) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical mark prices.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical mark prices fails.
    #[allow(unused_variables)]
    fn on_historical_mark_prices(&mut self, mark_prices: &[MarkPriceUpdate]) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed when receiving historical index prices.
    ///
    /// # Errors
    ///
    /// Returns an error if handling the historical index prices fails.
    #[allow(unused_variables)]
    fn on_historical_index_prices(
        &mut self,
        index_prices: &[IndexPriceUpdate],
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Handles a received time event.
    fn handle_time_event(&mut self, event: &TimeEvent) {
        log_received(&event);

        if let Err(e) = DataActor::on_time_event(self, event) {
            log_error(&e);
        }
    }

    /// Handles a received custom data point.
    fn handle_data(&mut self, data: &dyn Any) {
        log_received(&data);

        if self.not_running() {
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

        if self.not_running() {
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

        if self.not_running() {
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

        if self.not_running() {
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

        if self.not_running() {
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

        if self.not_running() {
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

        if self.not_running() {
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

        if self.not_running() {
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

        if self.not_running() {
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

        if self.not_running() {
            log_not_running(&index_price);
            return;
        }

        if let Err(e) = self.on_index_price(index_price) {
            log_error(&e);
        }
    }

    /// Handles a received funding rate update.
    fn handle_funding_rate(&mut self, funding_rate: &FundingRateUpdate) {
        log_received(&funding_rate);

        if self.not_running() {
            log_not_running(&funding_rate);
            return;
        }

        if let Err(e) = self.on_funding_rate(funding_rate) {
            log_error(&e);
        }
    }

    /// Handles a received instrument status.
    fn handle_instrument_status(&mut self, status: &InstrumentStatus) {
        log_received(&status);

        if self.not_running() {
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

        if self.not_running() {
            log_not_running(&close);
            return;
        }

        if let Err(e) = self.on_instrument_close(close) {
            log_error(&e);
        }
    }

    /// Handles a received order filled event.
    fn handle_order_filled(&mut self, event: &OrderFilled) {
        log_received(&event);

        // Check for double-handling: if the event's strategy_id matches this actor's id,
        // it means a Strategy is receiving its own fill event through both automatic
        // subscription and manual subscribe_order_fills, so skip the manual handler.
        if event.strategy_id.inner() == self.actor_id().inner() {
            return;
        }

        if self.not_running() {
            log_not_running(&event);
            return;
        }

        if let Err(e) = self.on_order_filled(event) {
            log_error(&e);
        }
    }

    #[cfg(feature = "defi")]
    /// Handles a received block.
    fn handle_block(&mut self, block: &Block) {
        log_received(&block);

        if self.not_running() {
            log_not_running(&block);
            return;
        }

        if let Err(e) = self.on_block(block) {
            log_error(&e);
        }
    }

    #[cfg(feature = "defi")]
    /// Handles a received pool definition update.
    fn handle_pool(&mut self, pool: &Pool) {
        log_received(&pool);

        if self.not_running() {
            log_not_running(&pool);
            return;
        }

        if let Err(e) = self.on_pool(pool) {
            log_error(&e);
        }
    }

    #[cfg(feature = "defi")]
    /// Handles a received pool swap.
    fn handle_pool_swap(&mut self, swap: &PoolSwap) {
        log_received(&swap);

        if self.not_running() {
            log_not_running(&swap);
            return;
        }

        if let Err(e) = self.on_pool_swap(swap) {
            log_error(&e);
        }
    }

    #[cfg(feature = "defi")]
    /// Handles a received pool liquidity update.
    fn handle_pool_liquidity_update(&mut self, update: &PoolLiquidityUpdate) {
        log_received(&update);

        if self.not_running() {
            log_not_running(&update);
            return;
        }

        if let Err(e) = self.on_pool_liquidity_update(update) {
            log_error(&e);
        }
    }

    #[cfg(feature = "defi")]
    /// Handles a received pool fee collect.
    fn handle_pool_fee_collect(&mut self, collect: &PoolFeeCollect) {
        log_received(&collect);

        if self.not_running() {
            log_not_running(&collect);
            return;
        }

        if let Err(e) = self.on_pool_fee_collect(collect) {
            log_error(&e);
        }
    }

    #[cfg(feature = "defi")]
    /// Handles a received pool flash event.
    fn handle_pool_flash(&mut self, flash: &PoolFlash) {
        log_received(&flash);

        if self.not_running() {
            log_not_running(&flash);
            return;
        }

        if let Err(e) = self.on_pool_flash(flash) {
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

    /// Handles a data response.
    fn handle_data_response(&mut self, resp: &CustomDataResponse) {
        log_received(&resp);

        if let Err(e) = self.on_historical_data(resp.data.as_ref()) {
            log_error(&e);
        }
    }

    /// Handles an instrument response.
    fn handle_instrument_response(&mut self, resp: &InstrumentResponse) {
        log_received(&resp);

        if let Err(e) = self.on_instrument(&resp.data) {
            log_error(&e);
        }
    }

    /// Handles an instruments response.
    fn handle_instruments_response(&mut self, resp: &InstrumentsResponse) {
        log_received(&resp);

        for inst in &resp.data {
            if let Err(e) = self.on_instrument(inst) {
                log_error(&e);
            }
        }
    }

    /// Handles a book response.
    fn handle_book_response(&mut self, resp: &BookResponse) {
        log_received(&resp);

        if let Err(e) = self.on_book(&resp.data) {
            log_error(&e);
        }
    }

    /// Handles a quotes response.
    fn handle_quotes_response(&mut self, resp: &QuotesResponse) {
        log_received(&resp);

        if let Err(e) = self.on_historical_quotes(&resp.data) {
            log_error(&e);
        }
    }

    /// Handles a trades response.
    fn handle_trades_response(&mut self, resp: &TradesResponse) {
        log_received(&resp);

        if let Err(e) = self.on_historical_trades(&resp.data) {
            log_error(&e);
        }
    }

    /// Handles a bars response.
    fn handle_bars_response(&mut self, resp: &BarsResponse) {
        log_received(&resp);

        if let Err(e) = self.on_historical_bars(&resp.data) {
            log_error(&e);
        }
    }

    /// Subscribe to streaming `data_type` data.
    fn subscribe_data(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::with_any(
            move |data: &dyn Any| {
                get_actor_unchecked::<Self>(&actor_id).handle_data(data);
            },
        )));

        DataActorCore::subscribe_data(self, handler, data_type, client_id, params);
    }

    /// Subscribe to streaming [`QuoteTick`] data for the `instrument_id`.
    fn subscribe_quotes(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_quotes_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |quote: &QuoteTick| {
                if let Some(actor) = try_get_actor_unchecked::<Self>(&actor_id) {
                    actor.handle_quote(quote);
                } else {
                    log::error!("Actor {actor_id} not found for quote handling");
                }
            },
        )));

        DataActorCore::subscribe_quotes(self, topic, handler, instrument_id, client_id, params);
    }

    /// Subscribe to streaming [`InstrumentAny`] data for the `venue`.
    fn subscribe_instruments(
        &mut self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_instruments_topic(venue);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |instrument: &InstrumentAny| {
                if let Some(actor) = try_get_actor_unchecked::<Self>(&actor_id) {
                    actor.handle_instrument(instrument);
                } else {
                    log::error!("Actor {actor_id} not found for instruments handling");
                }
            },
        )));

        DataActorCore::subscribe_instruments(self, topic, handler, venue, client_id, params);
    }

    /// Subscribe to streaming [`InstrumentAny`] data for the `instrument_id`.
    fn subscribe_instrument(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_instrument_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |instrument: &InstrumentAny| {
                if let Some(actor) = try_get_actor_unchecked::<Self>(&actor_id) {
                    actor.handle_instrument(instrument);
                } else {
                    log::error!("Actor {actor_id} not found for instrument handling");
                }
            },
        )));

        DataActorCore::subscribe_instrument(self, topic, handler, instrument_id, client_id, params);
    }

    /// Subscribe to streaming [`OrderBookDeltas`] data for the `instrument_id`.
    fn subscribe_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        managed: bool,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_book_deltas_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |deltas: &OrderBookDeltas| {
                get_actor_unchecked::<Self>(&actor_id).handle_book_deltas(deltas);
            },
        )));

        DataActorCore::subscribe_book_deltas(
            self,
            topic,
            handler,
            instrument_id,
            book_type,
            depth,
            client_id,
            managed,
            params,
        );
    }

    /// Subscribe to [`OrderBook`] snapshots at a specified interval for the `instrument_id`.
    fn subscribe_book_at_interval(
        &mut self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<NonZeroUsize>,
        interval_ms: NonZeroUsize,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_book_snapshots_topic(instrument_id, interval_ms);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |book: &OrderBook| {
                get_actor_unchecked::<Self>(&actor_id).handle_book(book);
            },
        )));

        DataActorCore::subscribe_book_at_interval(
            self,
            topic,
            handler,
            instrument_id,
            book_type,
            depth,
            interval_ms,
            client_id,
            params,
        );
    }

    /// Subscribe to streaming [`TradeTick`] data for the `instrument_id`.
    fn subscribe_trades(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_trades_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |trade: &TradeTick| {
                get_actor_unchecked::<Self>(&actor_id).handle_trade(trade);
            },
        )));

        DataActorCore::subscribe_trades(self, topic, handler, instrument_id, client_id, params);
    }

    /// Subscribe to streaming [`Bar`] data for the `bar_type`.
    fn subscribe_bars(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_bars_topic(bar_type);

        let handler =
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(move |bar: &Bar| {
                get_actor_unchecked::<Self>(&actor_id).handle_bar(bar);
            })));

        DataActorCore::subscribe_bars(self, topic, handler, bar_type, client_id, params);
    }

    /// Subscribe to streaming [`MarkPriceUpdate`] data for the `instrument_id`.
    fn subscribe_mark_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_mark_price_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |mark_price: &MarkPriceUpdate| {
                get_actor_unchecked::<Self>(&actor_id).handle_mark_price(mark_price);
            },
        )));

        DataActorCore::subscribe_mark_prices(
            self,
            topic,
            handler,
            instrument_id,
            client_id,
            params,
        );
    }

    /// Subscribe to streaming [`IndexPriceUpdate`] data for the `instrument_id`.
    fn subscribe_index_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_index_price_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |index_price: &IndexPriceUpdate| {
                get_actor_unchecked::<Self>(&actor_id).handle_index_price(index_price);
            },
        )));

        DataActorCore::subscribe_index_prices(
            self,
            topic,
            handler,
            instrument_id,
            client_id,
            params,
        );
    }

    /// Subscribe to streaming [`FundingRateUpdate`] data for the `instrument_id`.
    fn subscribe_funding_rates(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_funding_rate_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |funding_rate: &FundingRateUpdate| {
                get_actor_unchecked::<Self>(&actor_id).handle_funding_rate(funding_rate);
            },
        )));

        DataActorCore::subscribe_funding_rates(
            self,
            topic,
            handler,
            instrument_id,
            client_id,
            params,
        );
    }

    /// Subscribe to streaming [`InstrumentStatus`] data for the `instrument_id`.
    fn subscribe_instrument_status(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_instrument_status_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |status: &InstrumentStatus| {
                get_actor_unchecked::<Self>(&actor_id).handle_instrument_status(status);
            },
        )));

        DataActorCore::subscribe_instrument_status(
            self,
            topic,
            handler,
            instrument_id,
            client_id,
            params,
        );
    }

    /// Subscribe to streaming [`InstrumentClose`] data for the `instrument_id`.
    fn subscribe_instrument_close(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_instrument_close_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |close: &InstrumentClose| {
                get_actor_unchecked::<Self>(&actor_id).handle_instrument_close(close);
            },
        )));

        DataActorCore::subscribe_instrument_close(
            self,
            topic,
            handler,
            instrument_id,
            client_id,
            params,
        );
    }

    /// Subscribe to [`OrderFilled`] events for the `instrument_id`.
    fn subscribe_order_fills(&mut self, instrument_id: InstrumentId)
    where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = get_order_fills_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |event: &OrderFilled| {
                get_actor_unchecked::<Self>(&actor_id).handle_order_filled(event);
            },
        )));

        DataActorCore::subscribe_order_fills(self, topic, handler);
    }

    #[cfg(feature = "defi")]
    /// Subscribe to streaming [`Block`] data for the `chain`.
    fn subscribe_blocks(
        &mut self,
        chain: Blockchain,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = defi::switchboard::get_defi_blocks_topic(chain);

        let handler =
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(move |block: &Block| {
                get_actor_unchecked::<Self>(&actor_id).handle_block(block);
            })));

        DataActorCore::subscribe_blocks(self, topic, handler, chain, client_id, params);
    }

    #[cfg(feature = "defi")]
    /// Subscribe to streaming [`Pool`] definition updates for the AMM pool at the `instrument_id`.
    fn subscribe_pool(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = defi::switchboard::get_defi_pool_topic(instrument_id);

        let handler =
            ShareableMessageHandler(Rc::new(TypedMessageHandler::from(move |pool: &Pool| {
                get_actor_unchecked::<Self>(&actor_id).handle_pool(pool);
            })));

        DataActorCore::subscribe_pool(self, topic, handler, instrument_id, client_id, params);
    }

    #[cfg(feature = "defi")]
    /// Subscribe to streaming [`PoolSwap`] data for the `instrument_id`.
    fn subscribe_pool_swaps(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = defi::switchboard::get_defi_pool_swaps_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |swap: &PoolSwap| {
                get_actor_unchecked::<Self>(&actor_id).handle_pool_swap(swap);
            },
        )));

        DataActorCore::subscribe_pool_swaps(self, topic, handler, instrument_id, client_id, params);
    }

    #[cfg(feature = "defi")]
    /// Subscribe to streaming [`PoolLiquidityUpdate`] data for the `instrument_id`.
    fn subscribe_pool_liquidity_updates(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = defi::switchboard::get_defi_liquidity_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |update: &PoolLiquidityUpdate| {
                get_actor_unchecked::<Self>(&actor_id).handle_pool_liquidity_update(update);
            },
        )));

        DataActorCore::subscribe_pool_liquidity_updates(
            self,
            topic,
            handler,
            instrument_id,
            client_id,
            params,
        );
    }

    #[cfg(feature = "defi")]
    /// Subscribe to streaming [`PoolFeeCollect`] data for the `instrument_id`.
    fn subscribe_pool_fee_collects(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = defi::switchboard::get_defi_collect_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |collect: &PoolFeeCollect| {
                get_actor_unchecked::<Self>(&actor_id).handle_pool_fee_collect(collect);
            },
        )));

        DataActorCore::subscribe_pool_fee_collects(
            self,
            topic,
            handler,
            instrument_id,
            client_id,
            params,
        );
    }

    #[cfg(feature = "defi")]
    /// Subscribe to streaming [`PoolFlash`] events for the given `instrument_id`.
    fn subscribe_pool_flash_events(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let topic = defi::switchboard::get_defi_flash_topic(instrument_id);

        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |flash: &PoolFlash| {
                get_actor_unchecked::<Self>(&actor_id).handle_pool_flash(flash);
            },
        )));

        DataActorCore::subscribe_pool_flash_events(
            self,
            topic,
            handler,
            instrument_id,
            client_id,
            params,
        );
    }

    /// Unsubscribe from streaming `data_type` data.
    fn unsubscribe_data(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_data(self, data_type, client_id, params);
    }

    /// Unsubscribe from streaming [`InstrumentAny`] data for the `venue`.
    fn unsubscribe_instruments(
        &mut self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_instruments(self, venue, client_id, params);
    }

    /// Unsubscribe from streaming [`InstrumentAny`] data for the `instrument_id`.
    fn unsubscribe_instrument(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_instrument(self, instrument_id, client_id, params);
    }

    /// Unsubscribe from streaming [`OrderBookDeltas`] data for the `instrument_id`.
    fn unsubscribe_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_book_deltas(self, instrument_id, client_id, params);
    }

    /// Unsubscribe from [`OrderBook`] snapshots at a specified interval for the `instrument_id`.
    fn unsubscribe_book_at_interval(
        &mut self,
        instrument_id: InstrumentId,
        interval_ms: NonZeroUsize,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_book_at_interval(
            self,
            instrument_id,
            interval_ms,
            client_id,
            params,
        );
    }

    /// Unsubscribe from streaming [`QuoteTick`] data for the `instrument_id`.
    fn unsubscribe_quotes(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_quotes(self, instrument_id, client_id, params);
    }

    /// Unsubscribe from streaming [`TradeTick`] data for the `instrument_id`.
    fn unsubscribe_trades(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_trades(self, instrument_id, client_id, params);
    }

    /// Unsubscribe from streaming [`Bar`] data for the `bar_type`.
    fn unsubscribe_bars(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_bars(self, bar_type, client_id, params);
    }

    /// Unsubscribe from streaming [`MarkPriceUpdate`] data for the `instrument_id`.
    fn unsubscribe_mark_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_mark_prices(self, instrument_id, client_id, params);
    }

    /// Unsubscribe from streaming [`IndexPriceUpdate`] data for the `instrument_id`.
    fn unsubscribe_index_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_index_prices(self, instrument_id, client_id, params);
    }

    /// Unsubscribe from streaming [`FundingRateUpdate`] data for the `instrument_id`.
    fn unsubscribe_funding_rates(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_funding_rates(self, instrument_id, client_id, params);
    }

    /// Unsubscribe from streaming [`InstrumentStatus`] data for the `instrument_id`.
    fn unsubscribe_instrument_status(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_instrument_status(self, instrument_id, client_id, params);
    }

    /// Unsubscribe from streaming [`InstrumentClose`] data for the `instrument_id`.
    fn unsubscribe_instrument_close(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_instrument_close(self, instrument_id, client_id, params);
    }

    /// Unsubscribe from [`OrderFilled`] events for the `instrument_id`.
    fn unsubscribe_order_fills(&mut self, instrument_id: InstrumentId)
    where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_order_fills(self, instrument_id);
    }

    #[cfg(feature = "defi")]
    /// Unsubscribe from streaming [`Block`] data for the `chain`.
    fn unsubscribe_blocks(
        &mut self,
        chain: Blockchain,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_blocks(self, chain, client_id, params);
    }

    #[cfg(feature = "defi")]
    /// Unsubscribe from streaming [`Pool`] definition updates for the AMM pool at the `instrument_id`.
    fn unsubscribe_pool(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_pool(self, instrument_id, client_id, params);
    }

    #[cfg(feature = "defi")]
    /// Unsubscribe from streaming [`PoolSwap`] data for the `instrument_id`.
    fn unsubscribe_pool_swaps(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_pool_swaps(self, instrument_id, client_id, params);
    }

    #[cfg(feature = "defi")]
    /// Unsubscribe from streaming [`PoolLiquidityUpdate`] data for the `instrument_id`.
    fn unsubscribe_pool_liquidity_updates(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_pool_liquidity_updates(self, instrument_id, client_id, params);
    }

    #[cfg(feature = "defi")]
    /// Unsubscribe from streaming [`PoolFeeCollect`] data for the `instrument_id`.
    fn unsubscribe_pool_fee_collects(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_pool_fee_collects(self, instrument_id, client_id, params);
    }

    #[cfg(feature = "defi")]
    /// Unsubscribe from streaming [`PoolFlash`] events for the given `instrument_id`.
    fn unsubscribe_pool_flash_events(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) where
        Self: 'static + Debug + Sized,
    {
        DataActorCore::unsubscribe_pool_flash_events(self, instrument_id, client_id, params);
    }

    /// Request historical custom data of the given `data_type`.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    fn request_data(
        &mut self,
        data_type: DataType,
        client_id: ClientId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4>
    where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |resp: &CustomDataResponse| {
                get_actor_unchecked::<Self>(&actor_id).handle_data_response(resp);
            },
        )));

        DataActorCore::request_data(
            self, data_type, client_id, start, end, limit, params, handler,
        )
    }

    /// Request historical [`InstrumentResponse`] data for the given `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    fn request_instrument(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4>
    where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |resp: &InstrumentResponse| {
                get_actor_unchecked::<Self>(&actor_id).handle_instrument_response(resp);
            },
        )));

        DataActorCore::request_instrument(
            self,
            instrument_id,
            start,
            end,
            client_id,
            params,
            handler,
        )
    }

    /// Request historical [`InstrumentsResponse`] definitions for the optional `venue`.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    fn request_instruments(
        &mut self,
        venue: Option<Venue>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4>
    where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |resp: &InstrumentsResponse| {
                get_actor_unchecked::<Self>(&actor_id).handle_instruments_response(resp);
            },
        )));

        DataActorCore::request_instruments(self, venue, start, end, client_id, params, handler)
    }

    /// Request an [`OrderBook`] snapshot for the given `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    fn request_book_snapshot(
        &mut self,
        instrument_id: InstrumentId,
        depth: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4>
    where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |resp: &BookResponse| {
                get_actor_unchecked::<Self>(&actor_id).handle_book_response(resp);
            },
        )));

        DataActorCore::request_book_snapshot(self, instrument_id, depth, client_id, params, handler)
    }

    /// Request historical [`QuoteTick`] data for the given `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    fn request_quotes(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4>
    where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |resp: &QuotesResponse| {
                get_actor_unchecked::<Self>(&actor_id).handle_quotes_response(resp);
            },
        )));

        DataActorCore::request_quotes(
            self,
            instrument_id,
            start,
            end,
            limit,
            client_id,
            params,
            handler,
        )
    }

    /// Request historical [`TradeTick`] data for the given `instrument_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    fn request_trades(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4>
    where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |resp: &TradesResponse| {
                get_actor_unchecked::<Self>(&actor_id).handle_trades_response(resp);
            },
        )));

        DataActorCore::request_trades(
            self,
            instrument_id,
            start,
            end,
            limit,
            client_id,
            params,
            handler,
        )
    }

    /// Request historical [`Bar`] data for the given `bar_type`.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    fn request_bars(
        &mut self,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> anyhow::Result<UUID4>
    where
        Self: 'static + Debug + Sized,
    {
        let actor_id = self.actor_id().inner();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |resp: &BarsResponse| {
                get_actor_unchecked::<Self>(&actor_id).handle_bars_response(resp);
            },
        )));

        DataActorCore::request_bars(
            self, bar_type, start, end, limit, client_id, params, handler,
        )
    }
}

// Blanket implementation: any DataActor automatically implements Actor
impl<T> Actor for T
where
    T: DataActor + Debug + 'static,
{
    fn id(&self) -> Ustr {
        self.actor_id.inner()
    }

    #[allow(unused_variables)]
    fn handle(&mut self, msg: &dyn Any) {
        // Default empty implementation - concrete actors can override if needed
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Blanket implementation: any DataActor automatically implements Component
impl<T> Component for T
where
    T: DataActor + Debug + 'static,
{
    fn component_id(&self) -> ComponentId {
        ComponentId::new(self.actor_id.inner().as_str())
    }

    fn state(&self) -> ComponentState {
        self.state
    }

    fn transition_state(&mut self, trigger: ComponentTrigger) -> anyhow::Result<()> {
        self.state = self.state.transition(&trigger)?;
        log::info!("{}", self.state.variant_name());
        Ok(())
    }

    fn register(
        &mut self,
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<()> {
        DataActorCore::register(self, trader_id, clock.clone(), cache)?;

        // Register default time event handler for this actor
        let actor_id = self.actor_id().inner();
        let callback = TimeEventCallback::from(move |event: TimeEvent| {
            if let Some(actor) = try_get_actor_unchecked::<Self>(&actor_id) {
                actor.handle_time_event(&event);
            } else {
                log::error!("Actor {actor_id} not found for time event handling");
            }
        });

        clock.borrow_mut().register_default_handler(callback);

        self.initialize()
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        DataActor::on_start(self)
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        DataActor::on_stop(self)
    }

    fn on_resume(&mut self) -> anyhow::Result<()> {
        DataActor::on_resume(self)
    }

    fn on_degrade(&mut self) -> anyhow::Result<()> {
        DataActor::on_degrade(self)
    }

    fn on_fault(&mut self) -> anyhow::Result<()> {
        DataActor::on_fault(self)
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        DataActor::on_reset(self)
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        DataActor::on_dispose(self)
    }
}

/// Core functionality for all actors.
#[allow(
    dead_code,
    reason = "TODO: Under development (pending_requests, signal_classes)"
)]
pub struct DataActorCore {
    /// The actor identifier.
    pub actor_id: ActorId,
    /// The actors configuration.
    pub config: DataActorConfig,
    trader_id: Option<TraderId>,
    clock: Option<Rc<RefCell<dyn Clock>>>, // Wired up on registration
    cache: Option<Rc<RefCell<Cache>>>,     // Wired up on registration
    state: ComponentState,
    topic_handlers: AHashMap<MStr<Topic>, ShareableMessageHandler>,
    warning_events: AHashSet<String>, // TODO: TBD
    pending_requests: AHashMap<UUID4, Option<RequestCallback>>,
    signal_classes: AHashMap<String, String>,
    #[cfg(feature = "indicators")]
    indicators: Indicators,
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

impl DataActorCore {
    /// Adds a subscription handler for the `topic`.
    ///
    //// Logs a warning if the actor is already subscribed to the topic.
    pub(crate) fn add_subscription(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
    ) {
        if self.topic_handlers.contains_key(&topic) {
            log::warn!(
                "Actor {} attempted duplicate subscription to topic '{topic}'",
                self.actor_id,
            );
            return;
        }

        self.topic_handlers.insert(topic, handler.clone());
        msgbus::subscribe_topic(topic, handler, None);
    }

    /// Removes a subscription handler for the `topic` if present.
    ///
    /// Logs a warning if the actor is not currently subscribed to the topic.
    pub(crate) fn remove_subscription(&mut self, topic: MStr<Topic>) {
        if let Some(handler) = self.topic_handlers.remove(&topic) {
            msgbus::unsubscribe_topic(topic, handler);
        } else {
            log::warn!(
                "Actor {} attempted to unsubscribe from topic '{topic}' when not subscribed",
                self.actor_id,
            );
        }
    }

    /// Creates a new [`DataActorCore`] instance.
    pub fn new(config: DataActorConfig) -> Self {
        let actor_id = config
            .actor_id
            .unwrap_or_else(|| Self::default_actor_id(&config));

        Self {
            actor_id,
            config,
            trader_id: None, // None until registered
            clock: None,     // None until registered
            cache: None,     // None until registered
            state: ComponentState::default(),
            topic_handlers: AHashMap::new(),
            warning_events: AHashSet::new(),
            pending_requests: AHashMap::new(),
            signal_classes: AHashMap::new(),
            #[cfg(feature = "indicators")]
            indicators: Indicators::default(),
        }
    }

    /// Returns the memory address of this instance as a hexadecimal string.
    #[must_use]
    pub fn mem_address(&self) -> String {
        format!("{self:p}")
    }

    /// Returns the actors state.
    pub fn state(&self) -> ComponentState {
        self.state
    }

    /// Returns the trader ID this actor is registered to.
    pub fn trader_id(&self) -> Option<TraderId> {
        self.trader_id
    }

    /// Returns the actors ID.
    pub fn actor_id(&self) -> ActorId {
        self.actor_id
    }

    fn default_actor_id(config: &DataActorConfig) -> ActorId {
        let memory_address = std::ptr::from_ref(config) as *const _ as usize;
        ActorId::from(format!("{}-{memory_address}", stringify!(DataActor)))
    }

    /// Returns a UNIX nanoseconds timestamp from the actor's internal clock.
    pub fn timestamp_ns(&self) -> UnixNanos {
        self.clock_ref().timestamp_ns()
    }

    /// Returns the clock for the actor (if registered).
    ///
    /// # Panics
    ///
    /// Panics if the actor has not been registered with a trader.
    pub fn clock(&mut self) -> RefMut<'_, dyn Clock> {
        self.clock
            .as_ref()
            .unwrap_or_else(|| {
                panic!(
                    "DataActor {} must be registered before calling `clock()` - trader_id: {:?}",
                    self.actor_id, self.trader_id
                )
            })
            .borrow_mut()
    }

    /// Returns a clone of the reference-counted clock.
    ///
    /// # Panics
    ///
    /// Panics if the actor has not yet been registered (clock is `None`).
    pub fn clock_rc(&self) -> Rc<RefCell<dyn Clock>> {
        self.clock
            .as_ref()
            .expect("DataActor must be registered before accessing clock")
            .clone()
    }

    fn clock_ref(&self) -> Ref<'_, dyn Clock> {
        self.clock
            .as_ref()
            .unwrap_or_else(|| {
                panic!(
                    "DataActor {} must be registered before calling `clock_ref()` - trader_id: {:?}",
                    self.actor_id, self.trader_id
                )
            })
            .borrow()
    }

    /// Returns a read-only reference to the cache.
    ///
    /// # Panics
    ///
    /// Panics if the actor has not yet been registered (cache is `None`).
    pub fn cache(&self) -> Ref<'_, Cache> {
        self.cache
            .as_ref()
            .expect("DataActor must be registered before accessing cache")
            .borrow()
    }

    /// Returns a clone of the reference-counted cache.
    ///
    /// # Panics
    ///
    /// Panics if the actor has not yet been registered (cache is `None`).
    pub fn cache_rc(&self) -> Rc<RefCell<Cache>> {
        self.cache
            .as_ref()
            .expect("DataActor must be registered before accessing cache")
            .clone()
    }

    // -- REGISTRATION ----------------------------------------------------------------------------

    /// Register the data actor with a trader.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor has already been registered with a trader
    /// or if the provided dependencies are invalid.
    pub fn register(
        &mut self,
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<()> {
        if let Some(existing_trader_id) = self.trader_id {
            anyhow::bail!(
                "DataActor {} already registered with trader {existing_trader_id}",
                self.actor_id
            );
        }

        // Validate clock by attempting to access it
        {
            let _timestamp = clock.borrow().timestamp_ns();
        }

        // Validate cache by attempting to access it
        {
            let _cache_borrow = cache.borrow();
        }

        self.trader_id = Some(trader_id);
        self.clock = Some(clock);
        self.cache = Some(cache);

        // Verify complete registration
        if !self.is_properly_registered() {
            anyhow::bail!(
                "DataActor {} registration incomplete - validation failed",
                self.actor_id
            );
        }

        log::info!("Registered {} with trader {trader_id}", self.actor_id);
        Ok(())
    }

    /// Register an event type for warning log levels.
    pub fn register_warning_event(&mut self, event_type: &str) {
        self.warning_events.insert(event_type.to_string());
        log::debug!("Registered event type '{event_type}' for warning logs");
    }

    /// Deregister an event type from warning log levels.
    pub fn deregister_warning_event(&mut self, event_type: &str) {
        self.warning_events.remove(event_type);
        log::debug!("Deregistered event type '{event_type}' from warning logs");
    }

    pub fn is_registered(&self) -> bool {
        self.trader_id.is_some()
    }

    pub(crate) fn check_registered(&self) {
        assert!(
            self.is_registered(),
            "Actor has not been registered with a Trader"
        );
    }

    /// Validates registration state without panicking.
    fn is_properly_registered(&self) -> bool {
        self.trader_id.is_some() && self.clock.is_some() && self.cache.is_some()
    }

    pub(crate) fn send_data_cmd(&self, command: DataCommand) {
        if self.config.log_commands {
            log::info!("{CMD}{SEND} {command:?}");
        }

        let endpoint = MessagingSwitchboard::data_engine_queue_execute();
        msgbus::send_any(endpoint, command.as_any());
    }

    #[allow(dead_code, reason = "TODO: Under development")]
    fn send_data_req(&self, request: RequestCommand) {
        if self.config.log_commands {
            log::info!("{REQ}{SEND} {request:?}");
        }

        // For now, simplified approach - data requests without dynamic handlers
        // TODO: Implement proper dynamic dispatch for response handlers
        let endpoint = MessagingSwitchboard::data_engine_queue_execute();
        msgbus::send_any(endpoint, request.as_any());
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
            self.timestamp_ns(),
        );

        let endpoint = "command.system.shutdown".into();
        msgbus::send_any(endpoint, command.as_any());
    }

    // -- SUBSCRIPTIONS ---------------------------------------------------------------------------

    /// Helper method for registering data subscriptions from the trait.
    ///
    /// # Panics
    ///
    /// Panics if the actor is not properly registered.
    pub fn subscribe_data(
        &mut self,
        handler: ShareableMessageHandler,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        if !self.is_properly_registered() {
            panic!(
                "DataActor {} is not properly registered - trader_id: {:?}, clock: {}, cache: {}",
                self.actor_id,
                self.trader_id,
                self.clock.is_some(),
                self.cache.is_some()
            );
        }

        let topic = get_custom_topic(&data_type);
        self.add_subscription(topic, handler);

        // If no client ID specified, just subscribe to the topic
        if client_id.is_none() {
            return;
        }

        let command = SubscribeCommand::Data(SubscribeCustomData {
            data_type,
            client_id,
            venue: None,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering quotes subscriptions from the trait.
    pub fn subscribe_quotes(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::Quotes(SubscribeQuotes {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering instruments subscriptions from the trait.
    pub fn subscribe_instruments(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::Instruments(SubscribeInstruments {
            client_id,
            venue,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering instrument subscriptions from the trait.
    pub fn subscribe_instrument(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::Instrument(SubscribeInstrument {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering book deltas subscriptions from the trait.
    #[allow(clippy::too_many_arguments)]
    pub fn subscribe_book_deltas(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        managed: bool,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::BookDeltas(SubscribeBookDeltas {
            instrument_id,
            book_type,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            depth,
            managed,
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering book snapshots subscriptions from the trait.
    #[allow(clippy::too_many_arguments)]
    pub fn subscribe_book_at_interval(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<NonZeroUsize>,
        interval_ms: NonZeroUsize,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::BookSnapshots(SubscribeBookSnapshots {
            instrument_id,
            book_type,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            depth,
            interval_ms,
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering trades subscriptions from the trait.
    pub fn subscribe_trades(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::Trades(SubscribeTrades {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering bars subscriptions from the trait.
    pub fn subscribe_bars(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::Bars(SubscribeBars {
            bar_type,
            client_id,
            venue: Some(bar_type.instrument_id().venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering mark prices subscriptions from the trait.
    pub fn subscribe_mark_prices(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::MarkPrices(SubscribeMarkPrices {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering index prices subscriptions from the trait.
    pub fn subscribe_index_prices(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::IndexPrices(SubscribeIndexPrices {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering funding rates subscriptions from the trait.
    pub fn subscribe_funding_rates(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::FundingRates(SubscribeFundingRates {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering instrument status subscriptions from the trait.
    pub fn subscribe_instrument_status(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::InstrumentStatus(SubscribeInstrumentStatus {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering instrument close subscriptions from the trait.
    pub fn subscribe_instrument_close(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = SubscribeCommand::InstrumentClose(SubscribeInstrumentClose {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Subscribe(command));
    }

    /// Helper method for registering order fills subscriptions from the trait.
    pub fn subscribe_order_fills(&mut self, topic: MStr<Topic>, handler: ShareableMessageHandler) {
        self.check_registered();
        self.add_subscription(topic, handler);
    }

    /// Helper method for unsubscribing from data.
    pub fn unsubscribe_data(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_custom_topic(&data_type);
        self.remove_subscription(topic);

        if client_id.is_none() {
            return;
        }

        let command = UnsubscribeCommand::Data(UnsubscribeCustomData {
            data_type,
            client_id,
            venue: None,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from instruments.
    pub fn unsubscribe_instruments(
        &mut self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instruments_topic(venue);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::Instruments(UnsubscribeInstruments {
            client_id,
            venue,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from instrument.
    pub fn unsubscribe_instrument(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_topic(instrument_id);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::Instrument(UnsubscribeInstrument {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from book deltas.
    pub fn unsubscribe_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_book_deltas_topic(instrument_id);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from book snapshots at interval.
    pub fn unsubscribe_book_at_interval(
        &mut self,
        instrument_id: InstrumentId,
        interval_ms: NonZeroUsize,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_book_snapshots_topic(instrument_id, interval_ms);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::BookSnapshots(UnsubscribeBookSnapshots {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from quotes.
    pub fn unsubscribe_quotes(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_quotes_topic(instrument_id);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::Quotes(UnsubscribeQuotes {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from trades.
    pub fn unsubscribe_trades(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_trades_topic(instrument_id);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::Trades(UnsubscribeTrades {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from bars.
    pub fn unsubscribe_bars(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_bars_topic(bar_type);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::Bars(UnsubscribeBars {
            bar_type,
            client_id,
            venue: Some(bar_type.instrument_id().venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from mark prices.
    pub fn unsubscribe_mark_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_mark_price_topic(instrument_id);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::MarkPrices(UnsubscribeMarkPrices {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from index prices.
    pub fn unsubscribe_index_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_index_price_topic(instrument_id);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::IndexPrices(UnsubscribeIndexPrices {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from funding rates.
    pub fn unsubscribe_funding_rates(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_funding_rate_topic(instrument_id);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::FundingRates(UnsubscribeFundingRates {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from instrument status.
    pub fn unsubscribe_instrument_status(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_status_topic(instrument_id);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::InstrumentStatus(UnsubscribeInstrumentStatus {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from instrument close.
    pub fn unsubscribe_instrument_close(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_instrument_close_topic(instrument_id);
        self.remove_subscription(topic);

        let command = UnsubscribeCommand::InstrumentClose(UnsubscribeInstrumentClose {
            instrument_id,
            client_id,
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::Unsubscribe(command));
    }

    /// Helper method for unsubscribing from order fills.
    pub fn unsubscribe_order_fills(&mut self, instrument_id: InstrumentId) {
        self.check_registered();

        let topic = get_order_fills_topic(instrument_id);
        self.remove_subscription(topic);
    }

    /// Helper method for requesting data.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn request_data(
        &self,
        data_type: DataType,
        client_id: ClientId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        params: Option<IndexMap<String, String>>,
        handler: ShareableMessageHandler,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock_ref().utc_now();
        check_timestamps(now, start, end)?;

        let request_id = UUID4::new();
        let command = RequestCommand::Data(RequestCustomData {
            client_id,
            data_type,
            start,
            end,
            limit,
            request_id,
            ts_init: self.timestamp_ns(),
            params,
        });

        get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler)?;

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Helper method for requesting instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    pub fn request_instrument(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
        handler: ShareableMessageHandler,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock_ref().utc_now();
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

        get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler)?;

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Helper method for requesting instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    pub fn request_instruments(
        &self,
        venue: Option<Venue>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
        handler: ShareableMessageHandler,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock_ref().utc_now();
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

        get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler)?;

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Helper method for requesting book snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    pub fn request_book_snapshot(
        &self,
        instrument_id: InstrumentId,
        depth: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
        handler: ShareableMessageHandler,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let request_id = UUID4::new();
        let command = RequestCommand::BookSnapshot(RequestBookSnapshot {
            instrument_id,
            depth,
            client_id,
            request_id,
            ts_init: self.timestamp_ns(),
            params,
        });

        get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler)?;

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Helper method for requesting quotes.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn request_quotes(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
        handler: ShareableMessageHandler,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock_ref().utc_now();
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

        get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler)?;

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Helper method for requesting trades.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn request_trades(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
        handler: ShareableMessageHandler,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock_ref().utc_now();
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

        get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler)?;

        self.send_data_cmd(DataCommand::Request(command));

        Ok(request_id)
    }

    /// Helper method for requesting bars.
    ///
    /// # Errors
    ///
    /// Returns an error if input parameters are invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn request_bars(
        &self,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
        handler: ShareableMessageHandler,
    ) -> anyhow::Result<UUID4> {
        self.check_registered();

        let now = self.clock_ref().utc_now();
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

        get_message_bus()
            .borrow_mut()
            .register_response_handler(command.request_id(), handler)?;

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
        check_predicate_true(start <= now, "start was > now")?;
    }
    if let Some(end) = end {
        check_predicate_true(end <= now, "end was > now")?;
    }

    if let (Some(start), Some(end)) = (start, end) {
        check_predicate_true(start < end, "start was >= end")?;
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
