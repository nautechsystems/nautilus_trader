// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use super::{
    Bar, Cache, CustomData, DataEngine, Display, FundingRateUpdate, IndexPriceUpdate, Instrument,
    InstrumentAny, InstrumentClose, InstrumentStatus, MarkPriceUpdate, MarketStatusAction,
    OptionGreeks, OrderBookDelta, OrderBookDeltas, OrderBookDepth, QuoteTick, Rc, RecordFlag,
    RefCell, TradeTick, book, mem, msgbus, switchboard,
};

impl DataEngine {
    #[inline]
    fn pipeline_cache_writes_allowed(&self) -> bool {
        !self.config.disable_historical_cache
    }

    pub(crate) fn handle_instrument(&mut self, instrument: &InstrumentAny) {
        log::debug!("Handling instrument: {}", instrument.id());

        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_instrument(instrument.clone())
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_instrument_topic(instrument.id());
        log::debug!("Publishing instrument to topic: {topic}");
        msgbus::publish_instrument(topic, instrument);

        self.update_option_chains(instrument);
    }

    pub(super) fn handle_delta(&mut self, delta: OrderBookDelta) {
        let mut deltas = if self.config.buffer_deltas {
            self.buffer_delta(delta);

            if !RecordFlag::F_LAST.matches(delta.flags) {
                return; // Not the last delta for event
            }

            self.buffered_deltas_map
                .remove(&delta.instrument_id)
                .expect("buffered deltas exist")
        } else {
            self.single_delta_batch(delta)
        };

        let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
        msgbus::publish_deltas(topic, &deltas);
        self.reclaim_deltas_frame(mem::take(&mut deltas.deltas));
    }

    pub(super) fn handle_deltas(&mut self, deltas: &OrderBookDeltas) {
        if self.config.buffer_deltas {
            let instrument_id = deltas.instrument_id;

            for delta in &deltas.deltas {
                let is_last = RecordFlag::F_LAST.matches(delta.flags);
                self.buffer_delta(*delta);

                if is_last {
                    let mut deltas_to_publish = self
                        .buffered_deltas_map
                        .remove(&instrument_id)
                        .expect("buffered deltas exist");
                    let topic = switchboard::get_book_deltas_topic(instrument_id);
                    msgbus::publish_deltas(topic, &deltas_to_publish);
                    self.reclaim_deltas_frame(mem::take(&mut deltas_to_publish.deltas));
                }
            }
        } else {
            let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
            msgbus::publish_deltas(topic, deltas);
        }
    }

    pub(super) fn handle_depth(&self, depth: &OrderBookDepth) {
        let topic = switchboard::get_book_depth_topic(depth.instrument_id);
        msgbus::publish_depth(topic, depth);

        if self.config.emit_quotes_from_book_depths
            && let Some(quote) = derive_quote_from_depth(depth)
        {
            book::publish_quote_if_changed(&self.cache, quote);
        }
    }

    pub(super) fn handle_quote(&self, quote: QuoteTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_quote(quote) {
            log_error_on_cache_insert(&e);
        }

        for synthetic_quote in self.synthetic_quotes_from_quote(quote) {
            let topic = switchboard::get_quotes_topic(synthetic_quote.instrument_id);
            msgbus::publish_quote(topic, &synthetic_quote);
        }

        let topic = switchboard::get_quotes_topic(quote.instrument_id);
        msgbus::publish_quote(topic, &quote);
    }

    pub(super) fn handle_trade(&self, trade: TradeTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_trade(trade) {
            log_error_on_cache_insert(&e);
        }

        for synthetic_trade in self.synthetic_trades_from_trade(trade) {
            let topic = switchboard::get_trades_topic(synthetic_trade.instrument_id);
            msgbus::publish_trade(topic, &synthetic_trade);
        }

        let topic = switchboard::get_trades_topic(trade.instrument_id);
        msgbus::publish_trade(topic, &trade);
    }

    pub(super) fn handle_bar(&self, bar: Bar) {
        process_engine_bar(&self.cache, self.config.validate_data_sequence, true, bar);
    }

    pub(super) fn handle_mark_price(&self, mark_price: MarkPriceUpdate) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_mark_price(mark_price) {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_mark_price_topic(mark_price.instrument_id);
        msgbus::publish_mark_price(topic, &mark_price);
    }

    pub(super) fn handle_index_price(&self, index_price: IndexPriceUpdate) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_index_price(index_price)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_index_price_topic(index_price.instrument_id);
        msgbus::publish_index_price(topic, &index_price);
    }

    /// Handles a funding rate update by adding it to the cache and publishing to the message bus.
    pub fn handle_funding_rate(&mut self, funding_rate: FundingRateUpdate) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_funding_rate(funding_rate)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_funding_rate_topic(funding_rate.instrument_id);
        msgbus::publish_funding_rate(topic, &funding_rate);
    }

    pub(super) fn handle_instrument_status(&mut self, status: InstrumentStatus) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_instrument_status(status)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_instrument_status_topic(status.instrument_id);
        msgbus::publish_any(topic, &status);

        if self
            .option_chain_instrument_index
            .contains_key(&status.instrument_id)
            && matches!(
                status.action,
                MarketStatusAction::Close | MarketStatusAction::NotAvailableForTrading
            )
        {
            self.expire_option_chain_instrument(status.instrument_id);
        }
    }

    pub(super) fn handle_instrument_close(&self, close: InstrumentClose) {
        let topic = switchboard::get_instrument_close_topic(close.instrument_id);
        msgbus::publish_any(topic, &close);
    }

    pub(super) fn handle_custom_data(&self, custom: &CustomData) {
        log::debug!("Processing custom data: {}", custom.data.type_name());
        let topic = switchboard::get_custom_topic(&custom.data_type);
        msgbus::publish_any(topic, custom);
    }

    pub(super) fn handle_delta_pipeline(&mut self, delta: OrderBookDelta) {
        // Pipeline deltas are not buffered; replays arrive pre-batched
        let mut deltas = self.single_delta_batch(delta);
        let topic = switchboard::get_pipeline_book_deltas_topic(deltas.instrument_id);
        msgbus::publish_deltas(topic, &deltas);
        self.reclaim_deltas_frame(mem::take(&mut deltas.deltas));
    }

    fn buffer_delta(&mut self, delta: OrderBookDelta) {
        if let Some(buffered_deltas) = self.buffered_deltas_map.get_mut(&delta.instrument_id) {
            buffered_deltas.deltas.push(delta);
            buffered_deltas.flags = delta.flags;
            buffered_deltas.sequence = delta.sequence;
            buffered_deltas.ts_event = delta.ts_event;
            buffered_deltas.ts_init = delta.ts_init;
            return;
        }

        let instrument_id = delta.instrument_id;
        let buffered_deltas = self.single_delta_batch(delta);
        self.buffered_deltas_map
            .insert(instrument_id, buffered_deltas);
    }

    fn single_delta_batch(&mut self, delta: OrderBookDelta) -> OrderBookDeltas {
        let instrument_id = delta.instrument_id;
        let mut frame = mem::take(&mut self.deltas_frame);
        frame.clear();
        frame.push(delta);
        OrderBookDeltas::new(instrument_id, frame)
    }

    fn reclaim_deltas_frame(&mut self, mut frame: Vec<OrderBookDelta>) {
        frame.clear();
        self.deltas_frame = frame;
    }

    pub(super) fn handle_deltas_pipeline(&self, deltas: &OrderBookDeltas) {
        let topic = switchboard::get_pipeline_book_deltas_topic(deltas.instrument_id);
        msgbus::publish_deltas(topic, deltas);
    }

    pub(super) fn handle_depth_pipeline(&self, depth: &OrderBookDepth) {
        let topic = switchboard::get_pipeline_book_depth_topic(depth.instrument_id);
        msgbus::publish_depth(topic, depth);
    }

    pub(super) fn handle_quote_pipeline(&self, quote: QuoteTick) {
        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self.cache.as_ref().borrow_mut().add_quote(quote)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_quotes_topic(quote.instrument_id);
        msgbus::publish_quote(topic, &quote);
    }

    pub(super) fn handle_trade_pipeline(&self, trade: TradeTick) {
        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self.cache.as_ref().borrow_mut().add_trade(trade)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_trades_topic(trade.instrument_id);
        msgbus::publish_trade(topic, &trade);
    }

    pub(super) fn handle_bar_pipeline(&self, bar: Bar) {
        if !validate_bar_sequence(&self.cache, self.config.validate_data_sequence, &bar) {
            return;
        }

        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self.cache.as_ref().borrow_mut().add_bar(bar)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_bars_topic(bar.bar_type);
        msgbus::publish_bar(topic, &bar);
    }

    pub(super) fn handle_mark_price_pipeline(&self, mark_price: MarkPriceUpdate) {
        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self.cache.as_ref().borrow_mut().add_mark_price(mark_price)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_mark_price_topic(mark_price.instrument_id);
        msgbus::publish_mark_price(topic, &mark_price);
    }

    pub(super) fn handle_index_price_pipeline(&self, index_price: IndexPriceUpdate) {
        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self
                .cache
                .as_ref()
                .borrow_mut()
                .add_index_price(index_price)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_index_price_topic(index_price.instrument_id);
        msgbus::publish_index_price(topic, &index_price);
    }

    pub(super) fn handle_funding_rate_pipeline(&self, funding_rate: FundingRateUpdate) {
        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self
                .cache
                .as_ref()
                .borrow_mut()
                .add_funding_rate(funding_rate)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_funding_rate_topic(funding_rate.instrument_id);
        msgbus::publish_funding_rate(topic, &funding_rate);
    }

    pub(super) fn handle_instrument_status_pipeline(&self, status: InstrumentStatus) {
        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self
                .cache
                .as_ref()
                .borrow_mut()
                .add_instrument_status(status)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_instrument_status_topic(status.instrument_id);
        msgbus::publish_any(topic, &status);
    }

    pub(super) fn handle_option_greeks_pipeline(&self, greeks: OptionGreeks) {
        if self.pipeline_cache_writes_allowed() {
            self.cache.borrow_mut().add_option_greeks(greeks);
        }

        let topic = switchboard::get_pipeline_option_greeks_topic(greeks.instrument_id);
        msgbus::publish_option_greeks(topic, &greeks);
    }

    pub(super) fn handle_instrument_close_pipeline(&self, close: InstrumentClose) {
        let topic = switchboard::get_pipeline_instrument_close_topic(close.instrument_id);
        msgbus::publish_any(topic, &close);
    }

    pub(super) fn handle_custom_data_pipeline(&self, custom: &CustomData) {
        log::debug!("Pipeline custom data: {}", custom.data.type_name());
        let topic = switchboard::get_pipeline_custom_topic(&custom.data_type);
        msgbus::publish_any(topic, custom);
    }

    pub(super) fn handle_instrument_response(&self, instrument: InstrumentAny) {
        let mut cache = self.cache.as_ref().borrow_mut();
        if let Err(e) = cache.add_instrument(instrument) {
            log_error_on_cache_insert(&e);
        }
    }

    pub(super) fn handle_instruments(&self, instruments: &[InstrumentAny]) {
        // TODO: Improve by adding bulk update methods to cache and database
        let mut cache = self.cache.as_ref().borrow_mut();

        for instrument in instruments {
            if let Err(e) = cache.add_instrument(instrument.clone()) {
                log_error_on_cache_insert(&e);
            }
        }
    }

    pub(super) fn handle_quotes(&self, quotes: &[QuoteTick]) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_quotes(quotes) {
            log_error_on_cache_insert(&e);
        }
    }

    pub(super) fn handle_trades(&self, trades: &[TradeTick]) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_trades(trades) {
            log_error_on_cache_insert(&e);
        }
    }

    pub(super) fn handle_funding_rates(&self, funding_rates: &[FundingRateUpdate]) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_funding_rates(funding_rates)
        {
            log_error_on_cache_insert(&e);
        }
    }

    pub(super) fn handle_bars(&self, bars: &[Bar]) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_bars(bars) {
            log_error_on_cache_insert(&e);
        }
    }
}

#[inline(always)]
pub(super) fn log_error_on_cache_insert<T: Display>(e: &T) {
    log::error!("Error on cache insert: {e}");
}

// Top-of-book `QuoteTick` from an `OrderBookDepth`. Returns `None` for
// missing-side padding or zero size.
fn derive_quote_from_depth(depth: &OrderBookDepth) -> Option<QuoteTick> {
    let bid = depth.bids.first()?;
    let ask = depth.asks.first()?;

    if bid.side.is_none() || ask.side.is_none() || bid.size.is_zero() || ask.size.is_zero() {
        return None;
    }

    Some(QuoteTick::new(
        depth.instrument_id,
        bid.price,
        ask.price,
        bid.size,
        ask.size,
        depth.ts_event,
        depth.ts_init,
    ))
}

// Validates a bar against `last_bar` before writing and (optionally) publishing.
// Live bars and aggregator emissions honor `validate_data_sequence`;
// request-generated bars use `Cache::add_bar_historical`.
pub(super) fn process_engine_bar(
    cache: &Rc<RefCell<Cache>>,
    validate_sequence: bool,
    publish: bool,
    bar: Bar,
) {
    debug_assert!(
        bar.bar_type.is_standard(),
        "bars must be published and cached under the standard bar type"
    );

    if !validate_bar_sequence(cache, validate_sequence, &bar) {
        return;
    }

    if let Err(e) = cache.as_ref().borrow_mut().add_bar(bar) {
        log_error_on_cache_insert(&e);
    }

    if publish {
        let topic = switchboard::get_bars_topic(bar.bar_type);
        msgbus::publish_bar(topic, &bar);
    }
}

fn validate_bar_sequence(cache: &Rc<RefCell<Cache>>, validate_sequence: bool, bar: &Bar) -> bool {
    if !validate_sequence {
        return true;
    }

    let Some(last_bar) = cache.as_ref().borrow().bar(&bar.bar_type).copied() else {
        return true;
    };

    if bar.ts_event < last_bar.ts_event {
        log::warn!(
            "Bar {bar} was prior to last bar `ts_event` {}",
            last_bar.ts_event,
        );
        return false;
    }

    if bar.ts_init < last_bar.ts_init {
        log::warn!(
            "Bar {bar} was prior to last bar `ts_init` {}",
            last_bar.ts_init,
        );
        return false;
    }

    true
}
