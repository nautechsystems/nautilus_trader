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
    ClientId, DataEngine, Debug, DeferredCommand, DurationNanos, Instrument, InstrumentAny,
    InstrumentClass, InstrumentId, OptionChainManager, OptionChainReferencePriceResponse,
    OptionGreeks, OptionSeriesId, Price, Rc, RefCell, RequestCommand,
    RequestOptionChainReferencePrice, StrikeRange, SubscribeCommand, SubscribeOptionChain,
    SubscribeOptionGreeks, SubscriptionKey, TimeEvent, TimeEventCallback, TypedHandler, UUID4,
    UnixNanos, UnsubscribeCommand, UnsubscribeInstrumentStatus, UnsubscribeOptionChain,
    UnsubscribeOptionGreeks, UnsubscribeQuotes, VecDeque, Venue, WeakCell, mem, msgbus,
    publish_external_data_command, switchboard,
};

const OPTION_CHAIN_REFERENCE_PRICE_TIMEOUT: DurationNanos = DurationNanos::from_secs(30);
const OPTION_CHAIN_REFERENCE_PRICE_TIMEOUT_TIMER: &str = "option-chain-reference-price-timeout";

impl DataEngine {
    /// Returns whether an `OptionChainManager` exists for the given series.
    #[must_use]
    pub fn has_option_chain_manager(&self, series_id: &OptionSeriesId) -> bool {
        self.option_chain_managers.contains_key(series_id)
    }

    /// Returns the count of pending option-chain bootstrap requests.
    #[must_use]
    pub fn pending_option_chain_request_count(&self) -> usize {
        self.pending_option_chain_requests.len()
    }

    pub(super) fn retain_external_option_chain(
        &mut self,
        client_id: ClientId,
        command: &SubscribeOptionChain,
        subscribe: &SubscribeCommand,
    ) {
        let owner_id = command.correlation_id.unwrap_or(command.command_id);
        let current_key = (client_id, SubscriptionKey::OptionChain(command.series_id));

        let previous_key = self
            .subscriptions_external
            .iter()
            .find_map(|(key, active)| {
                (matches!(
                    &key.1,
                    SubscriptionKey::OptionChain(series_id) if *series_id == command.series_id
                ) && active.acquisitions.contains(&owner_id))
                .then(|| key.clone())
            });

        if let Some(previous_key) = previous_key
            && previous_key != current_key
        {
            let release = {
                let active = self
                    .subscriptions_external
                    .get_mut(&previous_key)
                    .expect("external option chain owner was present");
                active.acquisitions.remove(&owner_id);
                active.owners = active.owners.saturating_sub(1);
                active.owners == 0
            };

            if release {
                let active = self
                    .subscriptions_external
                    .remove(&previous_key)
                    .expect("external option chain was present for final release");
                let unsubscribe =
                    active
                        .command
                        .into_unsubscribe(UUID4::new(), command.ts_init, Some(owner_id));
                publish_external_data_command(previous_key.0, &unsubscribe);
            }
        }

        self.subscriptions_external
            .retain(current_key.clone(), owner_id, subscribe.clone());
        self.subscriptions_external
            .get_mut(&current_key)
            .expect("external option chain was retained")
            .command = subscribe.clone();
    }

    pub(super) fn feed_option_greeks_to_pre_bootstrap_chain(&mut self, greeks: &OptionGreeks) {
        let Some(series_id) = self
            .option_chain_instrument_index
            .get(&greeks.instrument_id)
            .copied()
        else {
            return;
        };

        let Some(manager_rc) = self.option_chain_managers.get(&series_id).cloned() else {
            return;
        };

        if manager_rc.borrow().is_bootstrapped() {
            return;
        }

        manager_rc.borrow_mut().handle_greeks(greeks);

        if manager_rc.borrow().is_bootstrapped() {
            // Apply the chain's own subscribes before releasing the bootstrap owner, so a sample
            // inside the active window keeps its feed without a physical unsubscribe.
            self.drain_deferred_commands();
            self.stop_option_chain_greeks_bootstrap(series_id);
        }
    }

    pub(super) fn update_option_chains(&mut self, instrument: &InstrumentAny) {
        let Some(underlying) = instrument.underlying() else {
            return;
        };

        let Some(expiration_ns) = instrument.expiration_ns() else {
            return;
        };

        let Some(strike) = instrument.strike_price() else {
            return;
        };

        let Some(kind) = instrument.option_kind() else {
            return;
        };

        let venue = instrument.id().venue;
        let settlement = instrument.settlement_currency().code;
        let series_id = OptionSeriesId::new(venue, underlying, settlement, expiration_ns);

        // Clone Rc to release borrow on self.option_chain_managers before accessing self.clients
        let Some(manager_rc) = self.option_chain_managers.get(&series_id).cloned() else {
            return;
        };

        let clock = self.clock.clone();
        let client_id = manager_rc.borrow().client_id();
        let client = self.get_command_client(client_id.as_ref(), Some(&venue));

        if manager_rc
            .borrow_mut()
            .add_instrument(instrument.id(), strike, kind, client, &clock)
        {
            self.option_chain_instrument_index
                .insert(instrument.id(), series_id);
        }
    }

    /// Removes a settled/expired instrument from its option chain manager.
    ///
    /// Looks up the owning series via the reverse index, delegates removal to
    /// the manager (which unregisters msgbus handlers and pushes deferred wire
    /// unsubscribes), then drains those commands. When the series catalog
    /// becomes empty, the entire manager is torn down.
    pub(super) fn expire_option_chain_instrument(&mut self, instrument_id: InstrumentId) {
        let Some(series_id) = self.option_chain_instrument_index.remove(&instrument_id) else {
            return;
        };

        if self
            .option_chain_greeks_bootstraps
            .get(&series_id)
            .is_some_and(|bootstrap| bootstrap.instrument_id == instrument_id)
        {
            self.stop_option_chain_greeks_bootstrap(series_id);
        }

        let Some(manager_rc) = self.option_chain_managers.get(&series_id).cloned() else {
            return;
        };

        let series_empty = manager_rc
            .borrow_mut()
            .handle_instrument_expired(&instrument_id);

        // Drain deferred unsubscribe commands pushed by the manager
        self.drain_deferred_commands();

        log::info!(
            "Expired instrument {instrument_id} from option chain {series_id} (series_empty={series_empty})",
        );

        if series_empty {
            manager_rc.borrow_mut().teardown(&self.clock);
            self.option_chain_managers.remove(&series_id);

            log::info!("Torn down empty option chain manager for {series_id}");
        }
    }

    /// Drains deferred subscribe/unsubscribe commands pushed by option chain
    /// managers (or any other component) and executes them against the appropriate
    /// data client.
    pub(super) fn drain_deferred_commands(&mut self) {
        // Loop because expire_series pushes Unsubscribe commands; converges in <= 3 iterations
        loop {
            let commands: VecDeque<DeferredCommand> =
                std::mem::take(&mut *self.deferred_cmd_queue.borrow_mut());

            if commands.is_empty() {
                break;
            }

            for cmd in commands {
                match cmd {
                    DeferredCommand::Subscribe(sub) => {
                        let client = self.get_command_client(sub.client_id(), sub.venue());
                        if let Some(client) = client {
                            client.execute_subscribe(sub);
                        }
                    }
                    DeferredCommand::Unsubscribe(unsub) => {
                        if let Err(e) = self.execute_unsubscribe(&unsub) {
                            log::error!("Failed to execute deferred unsubscribe: {e}");
                        }
                    }
                    DeferredCommand::ExpireInstrument(instrument_id) => {
                        self.expire_option_chain_instrument(instrument_id);
                    }
                    DeferredCommand::ExpireSeries(series_id) => {
                        self.expire_series(series_id);
                    }
                }
            }
        }
    }

    /// Proactively expires all instruments for a series and tears down the manager.
    ///
    /// `handle_instrument_expired` removes each instrument from the aggregator and pushes
    /// deferred unsubscribe commands. `teardown` then cancels the snapshot timer and clears
    /// the handler lists (the aggregator is already empty at that point).
    fn expire_series(&mut self, series_id: OptionSeriesId) {
        self.stop_option_chain_greeks_bootstrap(series_id);

        let Some(manager_rc) = self.option_chain_managers.get(&series_id).cloned() else {
            return;
        };

        let instrument_ids: Vec<InstrumentId> = self
            .option_chain_instrument_index
            .iter()
            .filter(|(_, sid)| **sid == series_id)
            .map(|(id, _)| *id)
            .collect();

        for id in &instrument_ids {
            self.option_chain_instrument_index.remove(id);
            manager_rc.borrow_mut().handle_instrument_expired(id);
        }

        manager_rc.borrow_mut().teardown(&self.clock);
        self.option_chain_managers.remove(&series_id);

        log::info!("Proactively torn down expired option chain {series_id}");
    }

    pub(super) fn subscribe_option_chain(&mut self, cmd: &SubscribeOptionChain) {
        self.drain_deferred_commands();
        let series_id = cmd.series_id;
        self.stop_option_chain_greeks_bootstrap(series_id);

        // Handle edits to existing subscriptions by tearing down and re-setting up the OptionChainManager.
        if let Some(old) = self.option_chain_managers.remove(&series_id) {
            log::info!("Re-subscribing option chain for {series_id}, tearing down previous");

            let (active_ids, old_venue, old_client_id) = {
                let old = old.borrow();
                let active_ids = old
                    .all_instrument_ids()
                    .into_iter()
                    .filter(|instrument_id| old.is_instrument_active(instrument_id))
                    .collect::<Vec<_>>();
                (active_ids, old.venue(), old.client_id())
            };

            old.borrow_mut().teardown(&self.clock);
            self.forward_option_chain_unsubscribes(&active_ids, old_venue, old_client_id);
        }

        self.cancel_pending_option_chain_requests(Some(series_id));

        // For dynamic strike ranges, request a reference price from the adapter
        // to enable instant bootstrap without waiting for the first WebSocket tick.
        if !matches!(cmd.strike_range, StrikeRange::Fixed(_)) {
            let resolved_client_id = self
                .get_client(cmd.client_id.as_ref(), Some(&series_id.venue))
                .map(|c| c.client_id);

            if let Some(client_id) = resolved_client_id {
                let request_id = UUID4::new();
                let ts_init = self.clock.borrow().timestamp_ns();

                let sample_instrument_id = {
                    let cache = self.cache.borrow();
                    cache
                        .instruments(&series_id.venue, Some(&series_id.underlying))
                        .iter()
                        .filter(|i| {
                            i.instrument_class() == InstrumentClass::Option
                                && i.expiration_ns() == Some(series_id.expiration_ns)
                                && i.settlement_currency().code == series_id.settlement_currency
                        })
                        .min_by_key(|i| i.id())
                        .map(|i| i.id())
                };

                if let Some(instrument_id) = sample_instrument_id {
                    let request = RequestOptionChainReferencePrice::new(
                        series_id,
                        instrument_id,
                        Some(client_id),
                        request_id,
                        ts_init,
                        None,
                    );
                    let deadline_ns = ts_init.saturating_add(OPTION_CHAIN_REFERENCE_PRICE_TIMEOUT);
                    self.pending_option_chain_requests.insert(
                        request_id,
                        PendingOptionChainRequest {
                            command: cmd.clone(),
                            sample_instrument_id: instrument_id,
                            deadline_ns,
                        },
                    );

                    if !self.schedule_option_chain_reference_price_timeout() {
                        self.bootstrap_all_pending_option_chains();
                        return;
                    }

                    let req_cmd = RequestCommand::OptionChainReferencePrice(request);
                    if let Err(e) = self.execute_request(req_cmd) {
                        log::warn!(
                            "Failed to request option-chain reference price for {series_id}: {e}"
                        );

                        if let Some(pending) =
                            self.pending_option_chain_requests.remove(&request_id)
                        {
                            self.maintain_option_chain_reference_price_timeout();
                            self.create_option_chain_manager_with_greeks_bootstrap(pending);
                        }
                    }

                    return;
                }
            }
        }

        self.create_option_chain_manager(cmd, None);
    }

    fn schedule_option_chain_reference_price_timeout(&self) -> bool {
        let Some(deadline_ns) = self
            .pending_option_chain_requests
            .values()
            .map(|pending| pending.deadline_ns)
            .min()
        else {
            self.clock
                .borrow_mut()
                .cancel_timer(OPTION_CHAIN_REFERENCE_PRICE_TIMEOUT_TIMER);
            return true;
        };

        if self
            .clock
            .borrow()
            .next_time_ns(OPTION_CHAIN_REFERENCE_PRICE_TIMEOUT_TIMER)
            == Some(deadline_ns)
        {
            return true;
        }

        let Some(bootstrapper) = self.option_chain_bootstrapper.clone() else {
            log::error!(
                "Cannot schedule option-chain reference price timeout: data engine message bus handlers are not registered"
            );
            return false;
        };

        let callback_fn: Rc<dyn Fn(TimeEvent)> = Rc::new(move |_| {
            bootstrapper.handle_timeout();
        });

        let callback = TimeEventCallback::from(callback_fn);

        if let Err(e) = self.clock.borrow_mut().set_time_alert_ns(
            OPTION_CHAIN_REFERENCE_PRICE_TIMEOUT_TIMER,
            deadline_ns,
            Some(callback),
            Some(true),
        ) {
            log::error!("Failed to schedule option-chain reference price timeout: {e}");
            return false;
        }

        true
    }

    fn maintain_option_chain_reference_price_timeout(&mut self) {
        if !self.schedule_option_chain_reference_price_timeout() {
            self.bootstrap_all_pending_option_chains();
        }
    }

    fn bootstrap_all_pending_option_chains(&mut self) {
        self.clock
            .borrow_mut()
            .cancel_timer(OPTION_CHAIN_REFERENCE_PRICE_TIMEOUT_TIMER);
        let mut pending: Vec<PendingOptionChainRequest> =
            mem::take(&mut self.pending_option_chain_requests)
                .into_values()
                .collect();
        pending.sort_unstable_by_key(|pending| pending.command.series_id);
        for pending in pending {
            self.create_option_chain_manager_with_greeks_bootstrap(pending);
        }
    }

    pub(super) fn cancel_pending_option_chain_requests(
        &mut self,
        series_id: Option<OptionSeriesId>,
    ) -> bool {
        let request_ids: Vec<UUID4> = self
            .pending_option_chain_requests
            .iter()
            .filter_map(|(request_id, pending)| {
                (series_id.is_none() || series_id == Some(pending.command.series_id))
                    .then_some(*request_id)
            })
            .collect();

        for request_id in &request_ids {
            self.pending_option_chain_requests.remove(request_id);
        }

        self.maintain_option_chain_reference_price_timeout();

        !request_ids.is_empty()
    }

    fn handle_option_chain_reference_price_timeout(&mut self) {
        let now_ns = self.clock.borrow().timestamp_ns();

        let mut requests: Vec<(OptionSeriesId, UUID4)> = self
            .pending_option_chain_requests
            .iter()
            .filter_map(|(request_id, pending)| {
                (pending.deadline_ns <= now_ns).then_some((pending.command.series_id, *request_id))
            })
            .collect();

        requests.sort_unstable_by_key(|(series_id, _)| *series_id);

        for (_, request_id) in requests {
            let Some(pending) = self.pending_option_chain_requests.remove(&request_id) else {
                continue;
            };

            let series_id = pending.command.series_id;
            log::warn!(
                "Option-chain reference price request timed out for {series_id}; bootstrapping from live data"
            );
            self.create_option_chain_manager_with_greeks_bootstrap(pending);
        }

        self.maintain_option_chain_reference_price_timeout();
    }

    /// Creates and stores an `OptionChainManager` for the given subscription.
    fn create_option_chain_manager(
        &mut self,
        cmd: &SubscribeOptionChain,
        initial_atm_price: Option<Price>,
    ) -> Rc<RefCell<OptionChainManager>> {
        let series_id = cmd.series_id;
        let cache = self.cache.clone();
        let clock = self.clock.clone();
        let priority = self.msgbus_priority;
        let deferred_cmd_queue = self.deferred_cmd_queue.clone();

        let manager_rc = {
            let client = self.get_command_client(cmd.client_id.as_ref(), Some(&series_id.venue));
            OptionChainManager::create_and_setup(
                series_id,
                &cache,
                cmd,
                &clock,
                priority,
                client,
                initial_atm_price,
                deferred_cmd_queue,
            )
        };

        // Index all instruments for reverse lookup
        for id in manager_rc.borrow().all_instrument_ids() {
            self.option_chain_instrument_index.insert(id, series_id);
        }

        self.option_chain_managers
            .insert(series_id, manager_rc.clone());
        manager_rc
    }

    fn create_option_chain_manager_with_greeks_bootstrap(
        &mut self,
        pending: PendingOptionChainRequest,
    ) {
        let cmd = pending.command;
        let series_id = cmd.series_id;
        let instrument_id = pending.sample_instrument_id;
        let manager = self.create_option_chain_manager(&cmd, None);

        let Some(client_id) = manager.borrow().client_id() else {
            return;
        };

        let ownership_handler = TypedHandler::from(|_: &OptionGreeks| {});
        let topic = switchboard::get_option_greeks_topic(instrument_id);
        msgbus::subscribe_option_greeks(
            topic.into(),
            ownership_handler.clone(),
            Some(self.msgbus_priority),
        );

        let replaced = self.option_chain_greeks_bootstraps.insert(
            series_id,
            OptionChainGreeksBootstrap {
                instrument_id,
                client_id,
                venue: series_id.venue,
                ownership_handler,
            },
        );

        debug_assert!(
            replaced.is_none(),
            "Invariant: each option series has at most one Greeks bootstrap subscription"
        );

        let ts_init = self.clock.borrow().timestamp_ns();

        if let Err(e) =
            self.execute_subscribe(SubscribeCommand::OptionGreeks(SubscribeOptionGreeks::new(
                instrument_id,
                Some(client_id),
                Some(series_id.venue),
                UUID4::new(),
                ts_init,
                None,
                None,
            )))
        {
            log::error!("Failed to subscribe option-chain bootstrap Greeks for {series_id}: {e}");
        }
    }

    fn stop_option_chain_greeks_bootstrap(&mut self, series_id: OptionSeriesId) -> bool {
        let Some(bootstrap) = self.remove_option_chain_greeks_bootstrap(series_id) else {
            return false;
        };

        self.release_option_chain_greeks_bootstrap(&bootstrap);
        true
    }

    fn remove_option_chain_greeks_bootstrap(
        &mut self,
        series_id: OptionSeriesId,
    ) -> Option<OptionChainGreeksBootstrap> {
        let bootstrap = self.option_chain_greeks_bootstraps.remove(&series_id)?;
        let topic = switchboard::get_option_greeks_topic(bootstrap.instrument_id);
        msgbus::unsubscribe_option_greeks(topic.into(), &bootstrap.ownership_handler);
        Some(bootstrap)
    }

    fn release_option_chain_greeks_bootstrap(&mut self, bootstrap: &OptionChainGreeksBootstrap) {
        let cmd = UnsubscribeCommand::OptionGreeks(UnsubscribeOptionGreeks::new(
            bootstrap.instrument_id,
            Some(bootstrap.client_id),
            Some(bootstrap.venue),
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
            None,
            None,
        ));

        if let Err(e) = self.execute_unsubscribe(&cmd) {
            log::error!(
                "Failed to unsubscribe option-chain bootstrap Greeks for {}: {e}",
                bootstrap.instrument_id
            );
        }
    }

    pub(super) fn clear_option_chain_greeks_bootstraps(&mut self) {
        let bootstraps = mem::take(&mut self.option_chain_greeks_bootstraps);
        for bootstrap in bootstraps.into_values() {
            let topic = switchboard::get_option_greeks_topic(bootstrap.instrument_id);
            msgbus::unsubscribe_option_greeks(topic.into(), &bootstrap.ownership_handler);
        }
    }

    pub(super) fn unsubscribe_option_chain(&mut self, cmd: &UnsubscribeOptionChain) {
        self.drain_deferred_commands();
        let series_id = cmd.series_id;
        let topic = switchboard::get_option_chain_topic(series_id);
        if msgbus::exact_subscriber_count_option_chain(topic) > 0 {
            return;
        }

        let canceled_pending = self.cancel_pending_option_chain_requests(Some(series_id));
        let canceled_greeks_bootstrap = self.stop_option_chain_greeks_bootstrap(series_id);

        let Some(manager_rc) = self.option_chain_managers.remove(&series_id) else {
            if !canceled_pending && !canceled_greeks_bootstrap {
                log::warn!("Cannot unsubscribe option chain for {series_id}: not subscribed");
            }

            return;
        };

        // Extract info before teardown
        let (all_ids, active_ids, venue, client_id) = {
            let manager = manager_rc.borrow();
            let all_ids = manager.all_instrument_ids();
            let active_ids = all_ids
                .iter()
                .filter(|instrument_id| manager.is_instrument_active(instrument_id))
                .copied()
                .collect::<Vec<_>>();
            (all_ids, active_ids, manager.venue(), manager.client_id())
        };

        // Remove all instruments from reverse index
        for id in &all_ids {
            self.option_chain_instrument_index.remove(id);
        }

        manager_rc.borrow_mut().teardown(&self.clock);

        // Forward wire-level unsubscribes to the data client
        self.forward_option_chain_unsubscribes(&active_ids, venue, client_id);

        log::info!("Unsubscribed option chain for {series_id}");
    }

    /// Forwards wire-level unsubscribe commands for all option chain instruments.
    fn forward_option_chain_unsubscribes(
        &mut self,
        instrument_ids: &[InstrumentId],
        venue: Venue,
        client_id: Option<ClientId>,
    ) {
        let ts_init = self.clock.borrow().timestamp_ns();

        for instrument_id in instrument_ids {
            let quote_cmd = UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
                *instrument_id,
                client_id,
                Some(venue),
                UUID4::new(),
                ts_init,
                None,
                None,
            ));
            let greeks_cmd = UnsubscribeCommand::OptionGreeks(UnsubscribeOptionGreeks::new(
                *instrument_id,
                client_id,
                Some(venue),
                UUID4::new(),
                ts_init,
                None,
                None,
            ));
            let status_cmd =
                UnsubscribeCommand::InstrumentStatus(UnsubscribeInstrumentStatus::new(
                    *instrument_id,
                    client_id,
                    Some(venue),
                    UUID4::new(),
                    ts_init,
                    None,
                    None,
                ));

            for cmd in [&quote_cmd, &greeks_cmd, &status_cmd] {
                if let Err(e) = self.execute_unsubscribe(cmd) {
                    log::error!("Failed to execute option chain unsubscribe: {e}");
                }
            }
        }
    }

    pub(super) fn handle_option_chain_reference_price_response(
        &mut self,
        correlation_id: &UUID4,
        resp: &OptionChainReferencePriceResponse,
    ) {
        let Some(pending) = self.pending_option_chain_requests.get(correlation_id) else {
            log::debug!(
                "No pending option chain request for correlation_id={correlation_id}, ignoring"
            );
            return;
        };

        if resp.series_id != pending.command.series_id {
            log::warn!(
                "Ignoring option-chain reference price response for {}: pending series is {}",
                resp.series_id,
                pending.command.series_id,
            );
            return;
        }

        let pending = self
            .pending_option_chain_requests
            .remove(correlation_id)
            .expect("checked above");
        self.maintain_option_chain_reference_price_timeout();
        let series_id = pending.command.series_id;

        if let Some(price) = resp.price {
            log::info!("Reference price for {series_id}: {price} (instant bootstrap)");
        } else {
            log::info!(
                "No reference price available for {series_id}, will bootstrap from live data",
            );
        }

        if resp.price.is_some() {
            self.create_option_chain_manager(&pending.command, resp.price);
        } else {
            self.create_option_chain_manager_with_greeks_bootstrap(pending);
        }
    }
}

#[derive(Debug)]
pub(super) struct OptionChainBootstrapper {
    engine: WeakCell<DataEngine>,
}

impl OptionChainBootstrapper {
    pub(super) fn new(engine: &Rc<RefCell<DataEngine>>) -> Self {
        Self {
            engine: WeakCell::from(Rc::downgrade(engine)),
        }
    }

    fn handle_timeout(&self) {
        if let Some(engine) = self.engine.upgrade() {
            engine
                .borrow_mut()
                .handle_option_chain_reference_price_timeout();
        }
    }
}

#[derive(Debug)]
pub(super) struct PendingOptionChainRequest {
    command: SubscribeOptionChain,
    sample_instrument_id: InstrumentId,
    deadline_ns: UnixNanos,
}

#[derive(Debug)]
pub(super) struct OptionChainGreeksBootstrap {
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
    ownership_handler: TypedHandler<OptionGreeks>,
}
