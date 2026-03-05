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

//! Per-series option chain manager.
//!
//! Each [`OptionChainManager`] instance is self-contained: it owns its aggregator,
//! msgbus handlers, and timer for a single option series. The `DataEngine` holds
//! one manager per active series in
//! `AHashMap<OptionSeriesId, Rc<RefCell<OptionChainManager>>>`.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::Clock,
    messages::data::{
        SubscribeCommand, SubscribeOptionChain, SubscribeOptionGreeks, SubscribeQuotes,
        UnsubscribeCommand, UnsubscribeOptionGreeks, UnsubscribeQuotes,
    },
    msgbus::{self, MStr, Topic, TypedHandler, switchboard},
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{UUID4, correctness::FAILED, datetime::millis_to_nanos_unchecked};
use nautilus_model::{
    data::{QuoteTick, option_chain::OptionGreeks},
    enums::OptionKind,
    identifiers::{InstrumentId, OptionSeriesId, Venue},
    instruments::Instrument,
    types::Price,
};
use ustr::Ustr;

use super::{
    AtmTracker, OptionChainAggregator,
    handlers::{OptionChainGreeksHandler, OptionChainQuoteHandler, OptionChainSlicePublisher},
};
use crate::{
    client::DataClientAdapter,
    engine::{DeferredCommand, DeferredCommandQueue},
};

/// Per-series option chain manager.
///
/// Each instance manages a single option series: its aggregator,
/// handlers, timer, and lifecycle. The `DataEngine` holds one
/// manager per active series.
#[derive(Debug)]
pub struct OptionChainManager {
    aggregator: OptionChainAggregator,
    topic: MStr<Topic>,
    quote_handlers: Vec<TypedHandler<QuoteTick>>,
    greeks_handlers: Vec<TypedHandler<OptionGreeks>>,
    timer_name: Option<Ustr>,
    msgbus_priority: u8,
    /// Whether the first ATM price has been received and the active set bootstrapped.
    bootstrapped: bool,
    /// Shared deferred command queue — the `DataEngine` drains this on each data tick.
    deferred_cmd_queue: DeferredCommandQueue,
    /// Clock reference for constructing command timestamps.
    clock: Rc<RefCell<dyn Clock>>,
    /// When `true`, every quote/greeks update for an active instrument immediately publishes a snapshot.
    raw_mode: bool,
}

impl OptionChainManager {
    /// Factory method that creates a per-series manager, registers all msgbus
    /// handlers, forwards subscribe commands to the data client, and sets up
    /// the snapshot timer.
    ///
    /// Returns the manager wrapped in `Rc<RefCell<>>` (needed for `WeakCell`
    /// handler pattern).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_and_setup(
        series_id: OptionSeriesId,
        cache: Rc<RefCell<Cache>>,
        cmd: &SubscribeOptionChain,
        clock: &Rc<RefCell<dyn Clock>>,
        msgbus_priority: u8,
        client: Option<&mut DataClientAdapter>,
        initial_atm_price: Option<Price>,
        deferred_cmd_queue: DeferredCommandQueue,
    ) -> Rc<RefCell<Self>> {
        let topic = switchboard::get_option_chain_topic(series_id);
        let instruments = Self::resolve_instruments(&cache, &series_id);

        let mut tracker = AtmTracker::new();

        // Derive forward price precision from instrument strike prices
        if let Some((strike, _)) = instruments.values().next() {
            tracker.set_forward_precision(strike.precision);
        }

        if let Some(price) = initial_atm_price {
            tracker.set_initial_price(price);
            log::info!("Pre-populated ATM with forward price: {price}");
        }
        let aggregator =
            OptionChainAggregator::new(series_id, cmd.strike_range.clone(), tracker, instruments);

        // Initial active set for msgbus handlers (subset of all instruments).
        // When ATM is unknown (ATM-based ranges), this is empty — deferred until bootstrap.
        let active_instrument_ids = aggregator.instrument_ids();
        let all_instrument_ids = aggregator.all_instrument_ids();
        // If active set is already populated (Fixed range or ATM provided), we're bootstrapped
        let bootstrapped = !active_instrument_ids.is_empty() || all_instrument_ids.is_empty();

        let raw_mode = cmd.snapshot_interval_ms.is_none();

        let manager = Self {
            aggregator,
            topic,
            quote_handlers: Vec::new(),
            greeks_handlers: Vec::new(),
            timer_name: None,
            msgbus_priority,
            bootstrapped,
            deferred_cmd_queue,
            clock: clock.clone(),
            raw_mode,
        };
        let manager_rc = Rc::new(RefCell::new(manager));

        // Register msgbus handlers for initial active set only
        let (quote_handlers, _quote_handler) = Self::register_quote_handlers(
            &manager_rc,
            &active_instrument_ids,
            series_id,
            msgbus_priority,
        );
        let greeks_handlers = Self::register_greeks_handlers(
            &manager_rc,
            &active_instrument_ids,
            series_id,
            msgbus_priority,
        );

        // Forward wire-level subscriptions for the active set.
        // When ATM is unknown, active set is empty — deferred until bootstrap.
        Self::forward_client_subscriptions(
            client,
            &active_instrument_ids,
            cmd,
            series_id.venue,
            clock,
        );

        let timer_name = cmd
            .snapshot_interval_ms
            .map(|ms| Self::setup_timer(&manager_rc, series_id, ms, clock));

        {
            let mut mgr = manager_rc.borrow_mut();
            mgr.quote_handlers = quote_handlers;
            mgr.greeks_handlers = greeks_handlers;
            mgr.timer_name = timer_name;
        }

        let mode_str = match cmd.snapshot_interval_ms {
            Some(ms) => format!("interval={ms}ms"),
            None => "mode=raw".to_string(),
        };
        log::info!(
            "Subscribed option chain for {series_id} ({} active/{} total instruments, {mode_str})",
            active_instrument_ids.len(),
            all_instrument_ids.len(),
        );

        manager_rc
    }

    /// Registers quote handlers on the msgbus for each instrument.
    ///
    /// Always stores the handler prototype as the first element so that
    /// `register_handlers_for_instrument` can clone it during deferred bootstrap.
    fn register_quote_handlers(
        manager_rc: &Rc<RefCell<Self>>,
        instrument_ids: &[InstrumentId],
        series_id: OptionSeriesId,
        priority: u8,
    ) -> (Vec<TypedHandler<QuoteTick>>, TypedHandler<QuoteTick>) {
        let quote_handler =
            TypedHandler::new(OptionChainQuoteHandler::new(manager_rc.clone(), series_id));
        // Always store prototype as first element for bootstrap cloning
        let mut handlers = Vec::with_capacity(instrument_ids.len() + 1);
        handlers.push(quote_handler.clone());
        for instrument_id in instrument_ids {
            let topic = switchboard::get_quotes_topic(*instrument_id);
            msgbus::subscribe_quotes(topic.into(), quote_handler.clone(), Some(priority));
            handlers.push(quote_handler.clone());
        }
        (handlers, quote_handler)
    }

    /// Registers greeks handlers on the msgbus for each instrument.
    ///
    /// Always stores the handler prototype as the first element so that
    /// `register_handlers_for_instrument` can clone it during deferred bootstrap.
    fn register_greeks_handlers(
        manager_rc: &Rc<RefCell<Self>>,
        instrument_ids: &[InstrumentId],
        series_id: OptionSeriesId,
        priority: u8,
    ) -> Vec<TypedHandler<OptionGreeks>> {
        let greeks_handler =
            TypedHandler::new(OptionChainGreeksHandler::new(manager_rc.clone(), series_id));
        // Always store prototype as first element for bootstrap cloning
        let mut handlers = Vec::with_capacity(instrument_ids.len() + 1);
        handlers.push(greeks_handler.clone());
        for instrument_id in instrument_ids {
            let topic = switchboard::get_option_greeks_topic(*instrument_id);
            msgbus::subscribe_option_greeks(topic.into(), greeks_handler.clone(), Some(priority));
            handlers.push(greeks_handler.clone());
        }
        handlers
    }

    /// Forwards subscribe commands to the data client for all instruments.
    fn forward_client_subscriptions(
        client: Option<&mut DataClientAdapter>,
        instrument_ids: &[InstrumentId],
        cmd: &SubscribeOptionChain,
        venue: Venue,
        clock: &Rc<RefCell<dyn Clock>>,
    ) {
        let ts_init = clock.borrow().timestamp_ns();

        let Some(client) = client else {
            log::error!(
                "Cannot forward option chain subscriptions: no client found for venue={venue}",
            );
            return;
        };

        for instrument_id in instrument_ids {
            client.execute_subscribe(&SubscribeCommand::Quotes(SubscribeQuotes {
                instrument_id: *instrument_id,
                client_id: cmd.client_id,
                venue: Some(venue),
                command_id: UUID4::new(),
                ts_init,
                correlation_id: None,
                params: None,
            }));
            client.execute_subscribe(&SubscribeCommand::OptionGreeks(SubscribeOptionGreeks {
                instrument_id: *instrument_id,
                client_id: cmd.client_id,
                venue: Some(venue),
                command_id: UUID4::new(),
                ts_init,
                correlation_id: None,
                params: None,
            }));
        }

        log::info!(
            "Forwarded {} quote + {} greeks subscriptions to DataClient",
            instrument_ids.len(),
            instrument_ids.len(),
        );
    }

    /// Sets up the snapshot timer for periodic publishing.
    fn setup_timer(
        manager_rc: &Rc<RefCell<Self>>,
        series_id: OptionSeriesId,
        interval_ms: u64,
        clock: &Rc<RefCell<dyn Clock>>,
    ) -> Ustr {
        let interval_ns = millis_to_nanos_unchecked(interval_ms as f64);
        let publisher = OptionChainSlicePublisher::new(manager_rc.clone());
        let timer_name = Ustr::from(&format!("OptionChain|{series_id}|{interval_ms}"));

        let now_ns = clock.borrow().timestamp_ns().as_u64();
        let start_time_ns = now_ns - (now_ns % interval_ns) + interval_ns;

        let callback_fn: Rc<dyn Fn(TimeEvent)> = Rc::new(move |event| publisher.publish(event));
        let callback = TimeEventCallback::from(callback_fn);

        clock
            .borrow_mut()
            .set_timer_ns(
                &timer_name,
                interval_ns,
                Some(start_time_ns.into()),
                None,
                Some(callback),
                None,
                None,
            )
            .expect(FAILED);

        timer_name
    }

    /// Returns all instrument IDs in the full catalog (not just the active set).
    #[must_use]
    pub fn all_instrument_ids(&self) -> Vec<InstrumentId> {
        self.aggregator.all_instrument_ids()
    }

    /// Returns the venue for this option chain.
    #[must_use]
    pub fn venue(&self) -> Venue {
        self.aggregator.series_id().venue
    }

    /// Tears down this manager: unregisters all msgbus handlers and cancels the timer.
    pub fn teardown(&mut self, clock: &Rc<RefCell<dyn Clock>>) {
        // Unsubscribe from all currently active instruments
        let instrument_ids = self.aggregator.instrument_ids();

        // Unregister quote handlers
        if let Some(handler) = self.quote_handlers.first() {
            for instrument_id in &instrument_ids {
                let topic = switchboard::get_quotes_topic(*instrument_id);
                msgbus::unsubscribe_quotes(topic.into(), handler);
            }
        }

        // Unregister greeks handlers
        if let Some(handler) = self.greeks_handlers.first() {
            for instrument_id in &instrument_ids {
                let topic = switchboard::get_option_greeks_topic(*instrument_id);
                msgbus::unsubscribe_option_greeks(topic.into(), handler);
            }
        }

        // Cancel timer
        if let Some(timer_name) = self.timer_name.take() {
            let mut clk = clock.borrow_mut();
            if clk.timer_exists(&timer_name) {
                clk.cancel_timer(&timer_name);
            }
        }

        self.quote_handlers.clear();
        self.greeks_handlers.clear();
    }

    /// Routes incoming greeks to the aggregator.
    ///
    /// Also updates the ATM tracker from the forward price if `ForwardPrice` source is active,
    /// and triggers deferred bootstrap on the first arrival.
    pub fn handle_greeks(&mut self, greeks: &OptionGreeks) {
        // Update ATM tracker from forward price (ForwardPrice source only)
        self.aggregator
            .atm_tracker_mut()
            .update_from_option_greeks(greeks);
        // Route greeks to aggregator for storage
        self.aggregator.update_greeks(greeks);
        // Check if first ATM arrival triggers deferred bootstrap
        self.maybe_bootstrap();

        if self.raw_mode
            && self.bootstrapped
            && self.aggregator.active_ids().contains(&greeks.instrument_id)
        {
            self.publish_slice(greeks.ts_event);
        }
    }

    /// Handles an expired/settled instrument by removing it from the aggregator,
    /// unregistering msgbus handlers, and pushing deferred wire unsubscribes.
    ///
    /// Returns `true` if the aggregator catalog is now empty (all instruments expired),
    /// signaling the engine to tear down this entire manager.
    pub fn handle_instrument_expired(&mut self, instrument_id: &InstrumentId) -> bool {
        let was_active = self.aggregator.active_ids().contains(instrument_id);

        if !self.aggregator.remove_instrument(instrument_id) {
            return self.aggregator.is_catalog_empty();
        }

        if was_active {
            // Unregister msgbus handlers for this instrument
            if let Some(qh) = self.quote_handlers.first() {
                let topic = switchboard::get_quotes_topic(*instrument_id);
                msgbus::unsubscribe_quotes(topic.into(), qh);
            }

            if let Some(gh) = self.greeks_handlers.first() {
                let topic = switchboard::get_option_greeks_topic(*instrument_id);
                msgbus::unsubscribe_option_greeks(topic.into(), gh);
            }

            // Push deferred wire unsubscribes
            self.push_unsubscribe_pair(*instrument_id);
        }

        log::info!(
            "Removed expired instrument {instrument_id} from option chain {} (was_active={was_active}, remaining={})",
            self.aggregator.series_id(),
            self.aggregator.instruments().len(),
        );

        self.aggregator.is_catalog_empty()
    }

    /// Routes an incoming quote tick to the aggregator, then bootstraps if ready.
    ///
    /// This handles both option instrument quotes (aggregator) and ATM source quotes
    /// (the aggregator's ATM tracker handles filtering internally).
    pub fn handle_quote(&mut self, quote: &QuoteTick) {
        self.aggregator.update_quote(quote);
        self.maybe_bootstrap();

        if self.raw_mode
            && self.bootstrapped
            && self.aggregator.active_ids().contains(&quote.instrument_id)
        {
            self.publish_slice(quote.ts_event);
        }
    }

    /// Bootstraps the active instrument set on the first ATM price arrival.
    ///
    /// Computes active strikes, registers msgbus handlers for those instruments,
    /// and pushes deferred wire subscriptions into the shared command queue.
    fn maybe_bootstrap(&mut self) {
        if self.bootstrapped {
            return;
        }

        if self.aggregator.atm_tracker().atm_price().is_none() {
            return;
        }

        // First ATM received — compute active set and register handlers
        let active_ids = self.aggregator.recompute_active_set();
        self.register_handlers_for_instruments_bulk(&active_ids);

        for &id in &active_ids {
            self.push_subscribe_pair(id);
        }

        self.bootstrapped = true;

        log::info!(
            "Bootstrapped option chain for {} ({} active instruments)",
            self.aggregator.series_id(),
            active_ids.len(),
        );
    }

    /// Registers msgbus handlers for a batch of instruments.
    fn register_handlers_for_instruments_bulk(&mut self, instrument_ids: &[InstrumentId]) {
        for &id in instrument_ids {
            self.register_handlers_for_instrument(id);
        }
    }

    /// Adds a dynamically discovered instrument to this option chain.
    ///
    /// Registers msgbus handlers when the instrument falls in the active
    /// range and forwards wire-level subscriptions via `client`.
    /// Returns `true` if the instrument was newly inserted.
    pub fn add_instrument(
        &mut self,
        instrument_id: InstrumentId,
        strike: Price,
        kind: OptionKind,
        client: Option<&mut DataClientAdapter>,
        clock: &Rc<RefCell<dyn Clock>>,
    ) -> bool {
        if !self.aggregator.add_instrument(instrument_id, strike, kind) {
            return false;
        }

        if self.aggregator.active_ids().contains(&instrument_id) {
            self.register_handlers_for_instrument(instrument_id);
        }

        let venue = self.aggregator.series_id().venue;
        Self::forward_instrument_subscriptions(client, instrument_id, venue, clock);

        log::info!(
            "Added instrument {instrument_id} to option chain {} (active={})",
            self.aggregator.series_id(),
            self.aggregator.active_ids().contains(&instrument_id),
        );

        true
    }

    fn register_handlers_for_instrument(&mut self, instrument_id: InstrumentId) {
        if let Some(qh) = self.quote_handlers.first().cloned() {
            let topic = switchboard::get_quotes_topic(instrument_id);
            msgbus::subscribe_quotes(topic.into(), qh, Some(self.msgbus_priority));
        }

        if let Some(gh) = self.greeks_handlers.first().cloned() {
            let topic = switchboard::get_option_greeks_topic(instrument_id);
            msgbus::subscribe_option_greeks(topic.into(), gh, Some(self.msgbus_priority));
        }
    }

    /// Pushes deferred subscribe commands (quotes + greeks) for a single instrument.
    fn push_subscribe_pair(&self, instrument_id: InstrumentId) {
        let venue = self.aggregator.series_id().venue;
        let ts_init = self.clock.borrow().timestamp_ns();
        let mut queue = self.deferred_cmd_queue.borrow_mut();
        queue.push_back(DeferredCommand::Subscribe(SubscribeCommand::Quotes(
            SubscribeQuotes {
                instrument_id,
                client_id: None,
                venue: Some(venue),
                command_id: UUID4::new(),
                ts_init,
                correlation_id: None,
                params: None,
            },
        )));
        queue.push_back(DeferredCommand::Subscribe(SubscribeCommand::OptionGreeks(
            SubscribeOptionGreeks {
                instrument_id,
                client_id: None,
                venue: Some(venue),
                command_id: UUID4::new(),
                ts_init,
                correlation_id: None,
                params: None,
            },
        )));
    }

    /// Pushes deferred unsubscribe commands (quotes + greeks) for a single instrument.
    fn push_unsubscribe_pair(&self, instrument_id: InstrumentId) {
        let venue = self.aggregator.series_id().venue;
        let ts_init = self.clock.borrow().timestamp_ns();
        let mut queue = self.deferred_cmd_queue.borrow_mut();
        queue.push_back(DeferredCommand::Unsubscribe(UnsubscribeCommand::Quotes(
            UnsubscribeQuotes {
                instrument_id,
                client_id: None,
                venue: Some(venue),
                command_id: UUID4::new(),
                ts_init,
                correlation_id: None,
                params: None,
            },
        )));
        queue.push_back(DeferredCommand::Unsubscribe(
            UnsubscribeCommand::OptionGreeks(UnsubscribeOptionGreeks {
                instrument_id,
                client_id: None,
                venue: Some(venue),
                command_id: UUID4::new(),
                ts_init,
                correlation_id: None,
                params: None,
            }),
        ));
    }

    /// Forwards quote and greeks subscriptions for a single instrument to the data client.
    fn forward_instrument_subscriptions(
        client: Option<&mut DataClientAdapter>,
        instrument_id: InstrumentId,
        venue: Venue,
        clock: &Rc<RefCell<dyn Clock>>,
    ) {
        let Some(client) = client else {
            log::error!(
                "Cannot forward subscriptions for {instrument_id}: no client for venue={venue}",
            );
            return;
        };

        let ts_init = clock.borrow().timestamp_ns();

        client.execute_subscribe(&SubscribeCommand::Quotes(SubscribeQuotes {
            instrument_id,
            client_id: None,
            venue: Some(venue),
            command_id: UUID4::new(),
            ts_init,
            correlation_id: None,
            params: None,
        }));
        client.execute_subscribe(&SubscribeCommand::OptionGreeks(SubscribeOptionGreeks {
            instrument_id,
            client_id: None,
            venue: Some(venue),
            command_id: UUID4::new(),
            ts_init,
            correlation_id: None,
            params: None,
        }));
    }

    /// Checks if ATM has shifted and rebalances msgbus subscriptions if needed.
    fn maybe_rebalance(&mut self, now_ns: nautilus_core::UnixNanos) {
        let Some(action) = self.aggregator.check_rebalance(now_ns) else {
            return;
        };

        // Unsubscribe removed instruments from msgbus
        if let Some(qh) = self.quote_handlers.first() {
            for id in &action.remove {
                msgbus::unsubscribe_quotes(switchboard::get_quotes_topic(*id).into(), qh);
            }
        }

        if let Some(gh) = self.greeks_handlers.first() {
            for id in &action.remove {
                msgbus::unsubscribe_option_greeks(
                    switchboard::get_option_greeks_topic(*id).into(),
                    gh,
                );
            }
        }

        // Subscribe new instruments on msgbus
        if let Some(qh) = self.quote_handlers.first().cloned() {
            for id in &action.add {
                msgbus::subscribe_quotes(
                    switchboard::get_quotes_topic(*id).into(),
                    qh.clone(),
                    Some(self.msgbus_priority),
                );
            }
        }

        if let Some(gh) = self.greeks_handlers.first().cloned() {
            for id in &action.add {
                msgbus::subscribe_option_greeks(
                    switchboard::get_option_greeks_topic(*id).into(),
                    gh.clone(),
                    Some(self.msgbus_priority),
                );
            }
        }

        // Push deferred wire-level changes into the shared command queue
        for &id in &action.add {
            self.push_subscribe_pair(id);
        }
        for &id in &action.remove {
            self.push_unsubscribe_pair(id);
        }

        if !action.add.is_empty() || !action.remove.is_empty() {
            log::info!(
                "Rebalanced option chain for {}: +{} -{} instruments",
                self.aggregator.series_id(),
                action.add.len(),
                action.remove.len(),
            );
        }

        // Apply state changes to aggregator
        self.aggregator.apply_rebalance(&action, now_ns);
    }

    /// Takes the accumulated snapshot and publishes it to the msgbus.
    pub fn publish_slice(&mut self, ts: nautilus_core::UnixNanos) {
        self.maybe_rebalance(ts);

        let series_id = self.aggregator.series_id();
        let slice = self.aggregator.snapshot(ts);

        if slice.is_empty() {
            log::debug!("OptionChainSlice empty for {series_id}, skipping publish");
            return;
        }

        log::debug!(
            "Publishing OptionChainSlice for {} (calls={}, puts={})",
            series_id,
            slice.call_count(),
            slice.put_count(),
        );
        msgbus::publish_option_chain(self.topic, &slice);
    }

    /// Resolves instruments from cache that match the given option series.
    fn resolve_instruments(
        cache: &Rc<RefCell<Cache>>,
        series_id: &OptionSeriesId,
    ) -> HashMap<InstrumentId, (Price, OptionKind)> {
        let cache = cache.borrow();
        let mut map = HashMap::new();

        for instrument in cache.instruments(&series_id.venue, Some(&series_id.underlying)) {
            let Some(expiration) = instrument.expiration_ns() else {
                continue;
            };

            if expiration != series_id.expiration_ns {
                continue;
            }

            if instrument.settlement_currency().code != series_id.settlement_currency {
                continue;
            }

            let Some(strike) = instrument.strike_price() else {
                continue;
            };

            let Some(kind) = instrument.option_kind() else {
                continue;
            };

            map.insert(instrument.id(), (strike, kind));
        }

        map
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use nautilus_common::clock::TestClock;
    use nautilus_core::UnixNanos;
    use nautilus_model::{data::option_chain::StrikeRange, identifiers::Venue, types::Quantity};
    use rstest::*;

    use super::*;

    fn make_series_id() -> OptionSeriesId {
        OptionSeriesId::new(
            Venue::new("DERIBIT"),
            ustr::Ustr::from("BTC"),
            ustr::Ustr::from("BTC"),
            UnixNanos::from(1_700_000_000_000_000_000u64),
        )
    }

    fn make_test_queue() -> DeferredCommandQueue {
        Rc::new(RefCell::new(VecDeque::new()))
    }

    fn make_manager() -> (OptionChainManager, DeferredCommandQueue) {
        let series_id = make_series_id();
        let topic = switchboard::get_option_chain_topic(series_id);
        let tracker = AtmTracker::new();
        let aggregator = OptionChainAggregator::new(
            series_id,
            StrikeRange::Fixed(vec![]),
            tracker,
            HashMap::new(),
        );
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let queue = make_test_queue();

        let manager = OptionChainManager {
            aggregator,
            topic,
            quote_handlers: Vec::new(),
            greeks_handlers: Vec::new(),
            timer_name: None,
            msgbus_priority: 0,
            bootstrapped: true,
            deferred_cmd_queue: queue.clone(),
            clock,
            raw_mode: false,
        };
        (manager, queue)
    }

    #[rstest]
    fn test_manager_handle_quote_no_instrument() {
        let (mut manager, _queue) = make_manager();

        // Should not panic — quote for unknown instrument
        let quote = QuoteTick::new(
            InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            Price::from("100.00"),
            Price::from("101.00"),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(1u64),
            UnixNanos::from(1u64),
        );
        manager.handle_quote(&quote);
    }

    #[rstest]
    fn test_manager_publish_slice_empty() {
        let (mut manager, _queue) = make_manager();
        // Should not panic — empty slice skips publish
        manager.publish_slice(UnixNanos::from(100u64));
    }

    #[rstest]
    fn test_manager_teardown_no_handlers() {
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let (mut manager, _queue) = make_manager();
        // Should not panic — no handlers to unregister
        manager.teardown(&clock);
        assert!(manager.quote_handlers.is_empty());
    }

    fn make_option_chain_manager() -> (OptionChainManager, DeferredCommandQueue) {
        let series_id = make_series_id();
        let topic = switchboard::get_option_chain_topic(series_id);

        let strikes = [45000, 47500, 50000, 52500, 55000];
        let mut instruments = HashMap::new();
        for s in &strikes {
            let strike = Price::from(&s.to_string());
            let call_id = InstrumentId::from(&format!("BTC-20240101-{s}-C.DERIBIT"));
            let put_id = InstrumentId::from(&format!("BTC-20240101-{s}-P.DERIBIT"));
            instruments.insert(call_id, (strike, OptionKind::Call));
            instruments.insert(put_id, (strike, OptionKind::Put));
        }

        let tracker = AtmTracker::new();
        let aggregator = OptionChainAggregator::new(
            series_id,
            StrikeRange::AtmRelative {
                strikes_above: 1,
                strikes_below: 1,
            },
            tracker,
            instruments,
        );
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let queue = make_test_queue();

        let manager = OptionChainManager {
            aggregator,
            topic,
            quote_handlers: Vec::new(),
            greeks_handlers: Vec::new(),
            timer_name: None,
            msgbus_priority: 0,
            bootstrapped: false,
            deferred_cmd_queue: queue.clone(),
            clock,
            raw_mode: false,
        };
        (manager, queue)
    }

    fn bootstrap_via_greeks(manager: &mut OptionChainManager) {
        use nautilus_model::data::option_chain::OptionGreeks;
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            underlying_price: Some(50000.0),
            ..Default::default()
        };
        manager.handle_greeks(&greeks);
    }

    #[rstest]
    fn test_manager_publish_slice_triggers_rebalance() {
        let (mut manager, queue) = make_option_chain_manager();
        // Initially no instruments active (ATM unknown, deferred)
        assert_eq!(manager.aggregator.instrument_ids().len(), 0);

        // Feed ATM near 50000 via greeks — bootstrap computes active set (3 strikes × 2 = 6)
        bootstrap_via_greeks(&mut manager);
        assert!(manager.bootstrapped);
        assert_eq!(manager.aggregator.instrument_ids().len(), 6); // 3 strikes × 2

        // Deferred queue should contain subscribe commands (6 instruments × 2 = 12 commands)
        assert_eq!(queue.borrow().len(), 12);

        // publish_slice should still work normally after bootstrap
        manager.publish_slice(UnixNanos::from(100u64));
        assert!(manager.aggregator.last_atm_strike().is_some());
    }

    #[rstest]
    fn test_manager_add_instrument_new() {
        let (mut manager, _queue) = make_option_chain_manager();
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let new_id = InstrumentId::from("BTC-20240101-57500-C.DERIBIT");
        let strike = Price::from("57500");
        let count_before = manager.aggregator.instruments().len();

        let result = manager.add_instrument(new_id, strike, OptionKind::Call, None, &clock);

        assert!(result);
        assert_eq!(manager.aggregator.instruments().len(), count_before + 1);
    }

    #[rstest]
    fn test_manager_add_instrument_already_known() {
        let (mut manager, _queue) = make_option_chain_manager();
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let existing_id = InstrumentId::from("BTC-20240101-50000-C.DERIBIT");
        let strike = Price::from("50000");
        let count_before = manager.aggregator.instruments().len();

        let result = manager.add_instrument(existing_id, strike, OptionKind::Call, None, &clock);

        assert!(!result);
        assert_eq!(manager.aggregator.instruments().len(), count_before);
    }

    #[rstest]
    fn test_manager_deferred_bootstrap_on_first_atm() {
        let (mut manager, queue) = make_option_chain_manager();
        // Initially not bootstrapped, no active instruments
        assert!(!manager.bootstrapped);
        assert_eq!(manager.aggregator.instrument_ids().len(), 0);
        assert!(queue.borrow().is_empty());

        // Feed ATM via greeks → triggers bootstrap
        bootstrap_via_greeks(&mut manager);

        assert!(manager.bootstrapped);
        assert_eq!(manager.aggregator.instrument_ids().len(), 6); // 3 strikes × 2
        // 6 instruments × 2 commands each (quotes + greeks) = 12 deferred commands
        assert_eq!(queue.borrow().len(), 12);

        // All commands should be Subscribe variants
        assert!(
            queue
                .borrow()
                .iter()
                .all(|cmd| matches!(cmd, DeferredCommand::Subscribe(_)))
        );
    }

    #[rstest]
    fn test_manager_bootstrap_idempotent() {
        use nautilus_model::data::option_chain::OptionGreeks;

        let (mut manager, _queue) = make_option_chain_manager();
        bootstrap_via_greeks(&mut manager);
        assert!(manager.bootstrapped);
        let count = manager.aggregator.instrument_ids().len();

        // Feed another ATM update — bootstrap should not fire again
        let greeks2 = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            underlying_price: Some(50200.0),
            ..Default::default()
        };
        manager.handle_greeks(&greeks2);
        assert_eq!(manager.aggregator.instrument_ids().len(), count);
    }

    #[rstest]
    fn test_manager_fixed_range_bootstrapped_immediately() {
        // Fixed range manager is bootstrapped at creation (no ATM needed)
        let (manager, queue) = make_manager();
        assert!(manager.bootstrapped);
        assert!(queue.borrow().is_empty());
    }

    #[rstest]
    fn test_manager_forward_price_bootstrap_from_greeks() {
        use nautilus_model::data::option_chain::OptionGreeks;

        let (mut manager, _queue) = make_option_chain_manager();
        assert!(!manager.bootstrapped);

        // First greeks with underlying_price → updates ATM tracker and triggers bootstrap
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            underlying_price: Some(50000.0),
            ..Default::default()
        };
        manager.handle_greeks(&greeks);
        assert!(manager.bootstrapped);
        // 3 strikes × 2 sides = 6 active instruments
        assert_eq!(manager.aggregator.instrument_ids().len(), 6);
    }

    #[rstest]
    fn test_manager_forward_price_no_bootstrap_without_underlying() {
        use nautilus_model::data::option_chain::OptionGreeks;

        let (mut manager, _queue) = make_option_chain_manager();
        assert!(!manager.bootstrapped);

        // Greeks with no underlying_price → should not bootstrap
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            underlying_price: None,
            ..Default::default()
        };
        manager.handle_greeks(&greeks);
        assert!(!manager.bootstrapped);
    }

    #[rstest]
    fn test_handle_instrument_expired_removes_from_aggregator() {
        let (mut manager, queue) = make_option_chain_manager();
        // Bootstrap so instruments are active
        bootstrap_via_greeks(&mut manager);
        assert!(manager.bootstrapped);
        let initial_count = manager.aggregator.instruments().len();
        queue.borrow_mut().clear(); // clear bootstrap commands

        let expired_id = InstrumentId::from("BTC-20240101-50000-C.DERIBIT");
        let is_empty = manager.handle_instrument_expired(&expired_id);

        assert!(!is_empty);
        assert_eq!(manager.aggregator.instruments().len(), initial_count - 1);
        assert!(!manager.aggregator.active_ids().contains(&expired_id));
    }

    #[rstest]
    fn test_handle_instrument_expired_pushes_deferred_unsubscribes() {
        let (mut manager, queue) = make_option_chain_manager();
        bootstrap_via_greeks(&mut manager);
        queue.borrow_mut().clear();

        let expired_id = InstrumentId::from("BTC-20240101-50000-C.DERIBIT");
        manager.handle_instrument_expired(&expired_id);

        // Should push 2 unsubscribe commands (quotes + greeks)
        let cmds: Vec<_> = queue.borrow().iter().cloned().collect();
        assert_eq!(cmds.len(), 2);
        assert!(
            cmds.iter()
                .all(|c| matches!(c, DeferredCommand::Unsubscribe(_)))
        );
    }

    #[rstest]
    fn test_handle_instrument_expired_returns_true_when_last() {
        let series_id = make_series_id();
        let topic = switchboard::get_option_chain_topic(series_id);
        let call_id = InstrumentId::from("BTC-20240101-50000-C.DERIBIT");
        let strike = Price::from("50000");
        let mut instruments = HashMap::new();
        instruments.insert(call_id, (strike, OptionKind::Call));
        let tracker = AtmTracker::new();
        let aggregator = OptionChainAggregator::new(
            series_id,
            StrikeRange::Fixed(vec![strike]),
            tracker,
            instruments,
        );
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let queue = make_test_queue();

        let mut manager = OptionChainManager {
            aggregator,
            topic,
            quote_handlers: Vec::new(),
            greeks_handlers: Vec::new(),
            timer_name: None,
            msgbus_priority: 0,
            bootstrapped: true,
            deferred_cmd_queue: queue,
            clock,
            raw_mode: false,
        };

        let is_empty = manager.handle_instrument_expired(&call_id);
        assert!(is_empty);
        assert!(manager.aggregator.is_catalog_empty());
    }

    #[rstest]
    fn test_handle_instrument_expired_unknown_noop() {
        let (mut manager, queue) = make_manager();
        queue.borrow_mut().clear();

        let unknown = InstrumentId::from("ETH-20240101-3000-C.DERIBIT");
        let is_empty = manager.handle_instrument_expired(&unknown);

        // Empty manager returns true (catalog was already empty)
        assert!(is_empty);
        assert!(queue.borrow().is_empty()); // no deferred commands pushed
    }
}
