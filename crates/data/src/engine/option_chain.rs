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

use indexmap::IndexMap;
use nautilus_core::correctness::check_valid_string_utf8;

use super::{
    AHashSet, ClientId, DataEngine, Debug, DeferredCommand, DurationNanos, Instrument,
    InstrumentAny, InstrumentClass, InstrumentId, OptionChainManager,
    OptionChainReferencePriceResponse, OptionGreeks, OptionSeriesId, Params, Price, Rc, RefCell,
    RequestCommand, RequestOptionChainReferencePrice, StrikeRange, SubscribeCommand,
    SubscribeOptionChain, SubscribeOptionGreeks, SubscriptionKey, TimeEvent, TimeEventCallback,
    TypedHandler, UUID4, UnixNanos, UnsubscribeCommand, UnsubscribeOptionChain,
    UnsubscribeOptionGreeks, VecDeque, Venue, WeakCell, mem, msgbus, publish_external_data_command,
    switchboard,
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
    ) -> bool {
        let owner_id = command.owner_id();

        let current_key = (
            client_id,
            SubscriptionKey::OptionChain(command.series_id, owner_id),
        );

        if self.subscriptions_external.contains(&current_key) {
            return false;
        }

        let previous_key = self.subscriptions_external.iter().find_map(|(key, _)| {
            matches!(&key.1, SubscriptionKey::OptionChain(series, owner)
                if *series == command.series_id && *owner == owner_id)
            .then(|| key.clone())
        });

        if let Some(previous_key) = previous_key {
            let active = self
                .subscriptions_external
                .remove(&previous_key)
                .expect("external option chain owner was present");
            let unsubscribe =
                active
                    .command
                    .into_unsubscribe(UUID4::new(), command.ts_init, Some(owner_id));
            publish_external_data_command(previous_key.0, &unsubscribe);
        }

        self.subscriptions_external
            .retain(current_key, owner_id, subscribe.clone())
    }

    pub(super) fn unsubscribe_external_option_chain(
        &mut self,
        client_id: ClientId,
        command: &UnsubscribeOptionChain,
    ) {
        let owner_id = if let Some(owner_id) = command.correlation_id {
            owner_id
        } else {
            let owners = self.option_chain_owners(command.series_id);
            if owners.len() != 1 {
                log::warn!(
                    "Cannot unsubscribe external option chain {} without an unambiguous owner",
                    command.series_id
                );
                return;
            }

            *owners.iter().next().expect("one owner")
        };

        let key = (
            client_id,
            SubscriptionKey::OptionChain(command.series_id, owner_id),
        );

        if let Some(active) = self.subscriptions_external.remove(&key) {
            let unsubscribe = active.command.into_unsubscribe(
                command.command_id,
                command.ts_init,
                Some(owner_id),
            );
            publish_external_data_command(client_id, &unsubscribe);
        }
    }

    pub(super) fn feed_option_greeks_to_pre_bootstrap_chain(&mut self, greeks: &OptionGreeks) {
        let Some(series_id) = self
            .option_chain_instrument_index
            .get(&greeks.instrument_id)
            .copied()
        else {
            return;
        };

        let Some(managers) = self.option_chain_managers.get(&series_id) else {
            return;
        };

        let managers: Vec<_> = managers
            .iter()
            .filter(|(_, manager)| !manager.borrow().is_bootstrapped())
            .map(|(owner_id, manager)| (*owner_id, Rc::clone(manager)))
            .collect();

        for (owner_id, manager) in managers {
            manager.borrow_mut().handle_greeks(greeks);
            if manager.borrow().is_bootstrapped() {
                self.drain_deferred_commands();
                self.stop_option_chain_greeks_bootstrap(series_id, owner_id);
            }
        }
    }

    pub(super) fn update_option_chains(&mut self, instrument: &InstrumentAny) {
        if self
            .option_chain_instruments_expired
            .contains(&instrument.id())
        {
            return;
        }

        let Some(strike) = instrument.strike_price() else {
            return;
        };

        let Some(kind) = instrument.option_kind() else {
            return;
        };

        let Some(series_id) = Self::option_chain_series(instrument) else {
            return;
        };

        let Some(managers) = self.option_chain_managers.get(&series_id).cloned() else {
            return;
        };

        let clock = Rc::clone(&self.clock);

        for manager in managers.into_values() {
            let client_id = manager.borrow().client_id();
            let client = client_id.and_then(|client_id| self.clients.get_mut(&client_id));

            if manager
                .borrow_mut()
                .add_instrument(instrument.id(), strike, kind, client, &clock)
            {
                self.option_chain_instrument_index
                    .insert(instrument.id(), series_id);
            }
        }
    }

    fn option_chain_series(instrument: &InstrumentAny) -> Option<OptionSeriesId> {
        let underlying = instrument.underlying()?;
        let expiration_ns = instrument.expiration_ns()?;

        if let Err(e) = check_valid_string_utf8(underlying, "underlying") {
            log::warn!("Skipping option-chain update for {}: {e}", instrument.id());
            return None;
        }

        Some(OptionSeriesId::new_derived(
            instrument.id().venue,
            underlying,
            instrument.settlement_currency().code,
            expiration_ns,
        ))
    }

    /// Removes a settled/expired instrument from its option chain manager.
    ///
    /// Looks up the owning series via the reverse index, delegates removal to
    /// the manager (which unregisters msgbus handlers and pushes deferred wire
    /// unsubscribes), then drains those commands. When the series catalog
    /// becomes empty, the entire manager is torn down.
    pub(super) fn expire_option_chain_instrument(&mut self, instrument_id: InstrumentId) {
        // Status events can arrive before their instrument definitions
        if self
            .cache
            .borrow()
            .instrument(&instrument_id)
            .is_none_or(|instrument| instrument.option_kind().is_some())
        {
            self.option_chain_instruments_expired.insert(instrument_id);
        }

        let series_id = self
            .option_chain_instrument_index
            .remove(&instrument_id)
            .or_else(|| {
                self.cache
                    .borrow()
                    .instrument(&instrument_id)
                    .filter(|instrument| instrument.option_kind().is_some())
                    .and_then(Self::option_chain_series)
            });

        let Some(series_id) = series_id else {
            return;
        };

        let managers = self
            .option_chain_managers
            .get(&series_id)
            .cloned()
            .unwrap_or_default();

        for (owner_id, manager) in managers {
            if self
                .option_chain_greeks_bootstraps
                .get(&(series_id, owner_id))
                .is_some_and(|bootstrap| bootstrap.instrument_id == instrument_id)
            {
                self.stop_option_chain_greeks_bootstrap(series_id, owner_id);
            }

            if !manager.borrow().contains_instrument(&instrument_id) {
                continue;
            }

            if manager
                .borrow_mut()
                .handle_instrument_expired(&instrument_id)
            {
                manager.borrow_mut().teardown(&self.clock);
                self.option_chain_managers
                    .get_mut(&series_id)
                    .expect("series managers were present")
                    .shift_remove(&owner_id);
            }
        }

        if self
            .option_chain_managers
            .get(&series_id)
            .is_some_and(IndexMap::is_empty)
        {
            self.option_chain_managers.remove(&series_id);
        }

        if self
            .pending_option_chain_requests
            .values()
            .any(|pending| pending.command.series_id == series_id)
            && OptionChainManager::resolve_instruments(
                &self.cache,
                &series_id,
                &self.option_chain_instruments_expired,
            )
            .is_empty()
        {
            self.cancel_pending_option_chain_requests(Some(series_id), None);
        }

        self.drain_deferred_commands();
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
                        let client = self.get_client(sub.client_id(), sub.venue());
                        if let Some(client) = client {
                            client.execute_subscribe_intent(sub);
                        }
                    }
                    DeferredCommand::Unsubscribe(unsub) => {
                        self.unsubscribe_option_chain_feed(&unsub);
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
        self.cancel_pending_option_chain_requests(Some(series_id), None);
        let mut owners: Vec<_> = self
            .option_chain_greeks_bootstraps
            .keys()
            .filter_map(|(series, owner)| (*series == series_id).then_some(*owner))
            .collect();

        owners.sort_unstable_by_key(UUID4::as_bytes);
        for owner_id in owners {
            self.stop_option_chain_greeks_bootstrap(series_id, owner_id);
        }

        let Some(managers) = self.option_chain_managers.remove(&series_id) else {
            return;
        };

        self.option_chain_instrument_index
            .retain(|_, series| *series != series_id);

        for manager in managers.into_values() {
            let instrument_ids = manager.borrow().all_instrument_ids();
            self.option_chain_instruments_expired
                .extend(instrument_ids.iter().copied());
            for id in instrument_ids {
                manager.borrow_mut().handle_instrument_expired(&id);
            }

            manager.borrow_mut().teardown(&self.clock);
        }

        log::info!("Proactively torn down expired option chain {series_id}");
    }

    pub(super) fn subscribe_option_chain(&mut self, cmd: &SubscribeOptionChain) {
        if !cmd.series_id.is_derived() {
            log::error!(
                "Cannot subscribe option chain {}: explicit reference instruments are unsupported",
                cmd.series_id,
            );
            return;
        }

        self.drain_deferred_commands();
        let series_id = cmd.series_id;
        if self.option_chain_expired(series_id) {
            return;
        }

        let owner_id = cmd.owner_id();

        if self
            .option_chain_managers
            .get(&series_id)
            .is_some_and(|managers| managers.contains_key(&owner_id))
            || self.pending_option_chain_requests.values().any(|pending| {
                pending.command.series_id == series_id && pending.command.owner_id() == owner_id
            })
        {
            log::warn!(
                "Ignoring duplicate option chain subscription for {series_id}, owner={owner_id}"
            );
            return;
        }

        // For dynamic strike ranges, request a reference price from the adapter
        // to enable instant bootstrap without waiting for the first WebSocket tick.
        if !matches!(cmd.strike_range, StrikeRange::Fixed(_))
            && let Some(client_id) = self
                .get_client(cmd.client_id.as_ref(), Some(&series_id.venue))
                .map(|c| c.client_id)
            && let Some(instrument_id) = {
                let cache = self.cache.borrow();
                cache
                    .instruments(&series_id.venue, Some(&series_id.underlying))
                    .iter()
                    .filter(|i| {
                        i.instrument_class() == InstrumentClass::Option
                            && !self.option_chain_instruments_expired.contains(&i.id())
                            && i.expiration_ns() == Some(series_id.expiration_ns)
                            && i.settlement_currency().code == series_id.settlement_currency
                    })
                    .min_by_key(|i| i.id())
                    .map(|i| i.id())
            }
        {
            // A payload-free failure can remove the in-flight request before client cleanup
            let mut command = cmd.clone();
            command.client_id = Some(client_id);
            let request_id = UUID4::new();
            let ts_init = self.clock.borrow().timestamp_ns();

            let request = RequestOptionChainReferencePrice::new(
                series_id,
                instrument_id,
                Some(client_id),
                request_id,
                ts_init,
                cmd.params.clone(),
            );
            let deadline_ns = ts_init.saturating_add(OPTION_CHAIN_REFERENCE_PRICE_TIMEOUT);
            self.pending_option_chain_requests.insert(
                request_id,
                PendingOptionChainRequest {
                    command,
                    deadline_ns,
                },
            );

            if !self.schedule_option_chain_reference_price_timeout() {
                self.bootstrap_all_pending_option_chains();
                return;
            }

            let req_cmd = RequestCommand::OptionChainReferencePrice(request);
            if let Err(e) = self.execute_request(req_cmd) {
                self.inflight_requests.remove(&request_id);
                log::warn!("Failed to request option-chain reference price for {series_id}: {e}");

                if let Some(pending) = self.pending_option_chain_requests.shift_remove(&request_id)
                {
                    self.maintain_option_chain_reference_price_timeout();
                    self.create_option_chain_manager_with_greeks_bootstrap(pending);
                }
            }

            return;
        }

        let _ = self.create_option_chain_manager(cmd, None);
    }

    fn option_chain_expired(&self, series_id: OptionSeriesId) -> bool {
        self.clock.borrow().timestamp_ns() >= series_id.expiration_ns
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
                .into_iter()
                .map(|(request_id, pending)| {
                    self.inflight_requests.remove(&request_id);
                    pending
                })
                .collect();

        pending.sort_by_key(|pending| pending.command.series_id);
        for pending in pending {
            self.create_option_chain_manager_with_greeks_bootstrap(pending);
        }
    }

    pub(super) fn cancel_pending_option_chain_requests(
        &mut self,
        series_id: Option<OptionSeriesId>,
        owner_id: Option<UUID4>,
    ) -> bool {
        let request_ids: Vec<UUID4> = self
            .pending_option_chain_requests
            .iter()
            .filter_map(|(request_id, pending)| {
                ((series_id.is_none() || series_id == Some(pending.command.series_id))
                    && (owner_id.is_none() || owner_id == Some(pending.command.owner_id())))
                .then_some(*request_id)
            })
            .collect();

        for request_id in &request_ids {
            self.pending_option_chain_requests.shift_remove(request_id);
            self.inflight_requests.remove(request_id);
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

        requests.sort_by_key(|(series_id, _)| *series_id);
        for (_, request_id) in requests {
            let Some(pending) = self.pending_option_chain_requests.shift_remove(&request_id) else {
                continue;
            };

            self.inflight_requests.remove(&request_id);
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
    ) -> Option<Rc<RefCell<OptionChainManager>>> {
        let series_id = cmd.series_id;
        if self.option_chain_expired(series_id) {
            return None;
        }

        let instruments = OptionChainManager::resolve_instruments(
            &self.cache,
            &series_id,
            &self.option_chain_instruments_expired,
        );
        let clock = Rc::clone(&self.clock);
        let priority = self.msgbus_priority;
        let deferred_cmd_queue = Rc::clone(&self.deferred_cmd_queue);

        let capacity = self
            .option_chain_managers
            .get(&series_id)
            .map_or(0, IndexMap::len);

        let slot = (0..=capacity)
            .find(|slot| {
                self.option_chain_managers
                    .get(&series_id)
                    .is_none_or(|managers| {
                        managers
                            .values()
                            .all(|manager| manager.borrow().slot() != *slot)
                    })
            })
            .expect("a free manager slot exists");

        let manager_rc = {
            let client = self.get_command_client(cmd.client_id.as_ref(), Some(&series_id.venue));
            OptionChainManager::create_and_setup(
                series_id,
                instruments,
                cmd,
                &clock,
                priority,
                client,
                initial_atm_price,
                deferred_cmd_queue,
                slot,
            )
        };

        // Index all instruments for reverse lookup
        for id in manager_rc.borrow().all_instrument_ids() {
            self.option_chain_instrument_index.insert(id, series_id);
        }

        self.option_chain_managers
            .entry(series_id)
            .or_default()
            .insert(cmd.owner_id(), Rc::clone(&manager_rc));
        Some(manager_rc)
    }

    fn create_option_chain_manager_with_greeks_bootstrap(
        &mut self,
        pending: PendingOptionChainRequest,
    ) {
        let cmd = pending.command;
        let series_id = cmd.series_id;
        let owner_id = cmd.owner_id();

        let Some(manager) = self.create_option_chain_manager(&cmd, None) else {
            return;
        };

        let Some(instrument_id) = manager.borrow().all_instrument_ids().into_iter().min() else {
            return;
        };

        let Some(client_id) = manager.borrow().client_id() else {
            return;
        };

        let slot = manager.borrow().slot();
        let ownership_handler = TypedHandler::from_with_id(
            format!("OptionChainBootstrap({series_id},{slot})"),
            |_: &OptionGreeks| {},
        );
        let topic = switchboard::get_option_greeks_topic(instrument_id);
        msgbus::subscribe_option_greeks(
            topic.into(),
            ownership_handler.clone(),
            Some(self.msgbus_priority),
        );

        let replaced = self.option_chain_greeks_bootstraps.insert(
            (series_id, owner_id),
            OptionChainGreeksBootstrap {
                instrument_id,
                client_id,
                venue: series_id.venue,
                params: cmd.params.clone(),
                ownership_handler,
            },
        );

        debug_assert!(
            replaced.is_none(),
            "Invariant: each option chain owner has at most one Greeks bootstrap subscription"
        );

        let ts_init = self.clock.borrow().timestamp_ns();

        let command = SubscribeCommand::OptionGreeks(SubscribeOptionGreeks::new(
            instrument_id,
            Some(client_id),
            Some(series_id.venue),
            UUID4::new(),
            ts_init,
            None,
            cmd.params.clone(),
        ));

        if let Some(client) = self.get_client(Some(&client_id), Some(&series_id.venue)) {
            client.execute_subscribe_intent(command);
        }
    }

    fn stop_option_chain_greeks_bootstrap(
        &mut self,
        series_id: OptionSeriesId,
        owner_id: UUID4,
    ) -> bool {
        let Some(bootstrap) = self.remove_option_chain_greeks_bootstrap(series_id, owner_id) else {
            return false;
        };

        self.release_option_chain_greeks_bootstrap(&bootstrap);
        true
    }

    fn remove_option_chain_greeks_bootstrap(
        &mut self,
        series_id: OptionSeriesId,
        owner_id: UUID4,
    ) -> Option<OptionChainGreeksBootstrap> {
        let bootstrap = self
            .option_chain_greeks_bootstraps
            .remove(&(series_id, owner_id))?;
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
            bootstrap.params.clone(),
        ));

        self.unsubscribe_option_chain_feed(&cmd);
    }

    pub(super) fn clear_option_chain_greeks_bootstraps(&mut self) {
        let bootstraps = mem::take(&mut self.option_chain_greeks_bootstraps);
        for bootstrap in bootstraps.into_values() {
            let topic = switchboard::get_option_greeks_topic(bootstrap.instrument_id);
            msgbus::unsubscribe_option_greeks(topic.into(), &bootstrap.ownership_handler);
        }
    }

    pub(super) fn clear_option_chain_managers(&mut self) {
        // Tear down option chain managers to unregister their msgbus handlers
        let managers: Vec<_> = self.option_chain_managers.drain().collect();
        for (_, owners) in managers {
            for manager in owners.into_values() {
                manager.borrow_mut().teardown(&self.clock);
            }
        }

        self.option_chain_instrument_index.clear();
        self.option_chain_instruments_expired.clear();
        self.cancel_pending_option_chain_requests(None, None);
        self.clear_option_chain_greeks_bootstraps();
    }

    pub(super) fn unsubscribe_option_chain(&mut self, cmd: &UnsubscribeOptionChain) {
        self.drain_deferred_commands();
        let series_id = cmd.series_id;
        let owner_id = if let Some(owner_id) = cmd.correlation_id {
            owner_id
        } else {
            let owners = self.option_chain_owners(series_id);
            if owners.len() != 1 {
                log::warn!(
                    "Cannot unsubscribe option chain for {series_id} without an unambiguous owner"
                );
                return;
            }

            *owners.iter().next().expect("one owner was present")
        };

        self.cancel_pending_option_chain_requests(Some(series_id), Some(owner_id));
        self.stop_option_chain_greeks_bootstrap(series_id, owner_id);
        let manager = self
            .option_chain_managers
            .get_mut(&series_id)
            .and_then(|managers| managers.shift_remove(&owner_id));

        let Some(manager) = manager else {
            return;
        };

        if self
            .option_chain_managers
            .get(&series_id)
            .is_some_and(IndexMap::is_empty)
        {
            self.option_chain_managers.remove(&series_id);
            self.option_chain_instrument_index
                .retain(|_, series| *series != series_id);
        }

        let commands = manager.borrow().unsubscribe_commands();
        manager.borrow_mut().teardown(&self.clock);

        for command in commands {
            self.unsubscribe_option_chain_feed(&command);
        }

        log::info!("Unsubscribed option chain for {series_id}, owner={owner_id}");
    }

    fn unsubscribe_option_chain_feed(&mut self, command: &UnsubscribeCommand) {
        if let Some(client_id) = command.client_id()
            && let Some(client) = self.clients.get_mut(client_id)
        {
            client.execute_unsubscribe(command);
        }
    }

    fn option_chain_owners(&self, series_id: OptionSeriesId) -> AHashSet<UUID4> {
        let mut owners: AHashSet<_> = self
            .option_chain_managers
            .get(&series_id)
            .into_iter()
            .flat_map(|managers| managers.keys().copied())
            .collect();
        owners.extend(
            self.pending_option_chain_requests
                .values()
                .filter_map(|pending| {
                    (pending.command.series_id == series_id).then_some(pending.command.owner_id())
                }),
        );

        owners.extend(
            self.subscriptions_external
                .iter()
                .filter_map(|(key, _)| match key.1 {
                    SubscriptionKey::OptionChain(series, owner) if series == series_id => {
                        Some(owner)
                    }
                    _ => None,
                }),
        );

        owners
    }

    pub(super) fn unsubscribe_option_chains_for_client(&mut self, client_id: ClientId) {
        self.drain_deferred_commands();

        let mut owners: Vec<_> = self
            .option_chain_managers
            .iter()
            .flat_map(|(series_id, managers)| {
                managers.iter().filter_map(move |(owner_id, manager)| {
                    (manager.borrow().client_id() == Some(client_id))
                        .then_some((*series_id, *owner_id))
                })
            })
            .collect();

        owners.extend(
            self.pending_option_chain_requests
                .values()
                .filter_map(|pending| {
                    (pending.command.client_id == Some(client_id))
                        .then_some((pending.command.series_id, pending.command.owner_id()))
                }),
        );

        owners.sort_unstable_by_key(|(series_id, owner_id)| (*series_id, owner_id.as_bytes()));
        owners.dedup();
        for (series_id, owner_id) in owners {
            let mut command = UnsubscribeOptionChain::new(
                series_id,
                UUID4::new(),
                self.clock.borrow().timestamp_ns(),
                Some(client_id),
                Some(series_id.venue),
            );

            command.correlation_id = Some(owner_id);
            self.unsubscribe_option_chain(&command);
        }
    }

    pub(super) fn unsubscribe_all_option_chains(&mut self) {
        let mut series_ids: AHashSet<_> = self.option_chain_managers.keys().copied().collect();
        series_ids.extend(
            self.pending_option_chain_requests
                .values()
                .map(|pending| pending.command.series_id),
        );

        let mut series_ids: Vec<_> = series_ids.into_iter().collect();
        series_ids.sort_unstable();
        for series_id in series_ids {
            let mut owners: Vec<_> = self.option_chain_owners(series_id).into_iter().collect();
            owners.sort_unstable_by_key(UUID4::as_bytes);
            for owner_id in owners {
                let mut command = UnsubscribeOptionChain::new(
                    series_id,
                    UUID4::new(),
                    self.clock.borrow().timestamp_ns(),
                    None,
                    Some(series_id.venue),
                );

                command.correlation_id = Some(owner_id);
                self.unsubscribe_option_chain(&command);
            }
        }

        let mut external: Vec<_> = self
            .subscriptions_external
            .iter()
            .filter(|(key, _)| matches!(&key.1, SubscriptionKey::OptionChain(_, _)))
            .map(|(key, active)| (key.clone(), active.command.clone()))
            .collect();

        external.sort_unstable_by_key(|(key, _)| match key.1 {
            SubscriptionKey::OptionChain(series, owner) => (key.0, series, owner.as_bytes()),
            _ => unreachable!("filtered to option chains"),
        });

        for (key, command) in external {
            self.subscriptions_external.remove(&key);
            let unsubscribe =
                command.into_unsubscribe(UUID4::new(), self.clock.borrow().timestamp_ns(), None);
            publish_external_data_command(key.0, &unsubscribe);
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
            .shift_remove(correlation_id)
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
            let _ = self.create_option_chain_manager(&pending.command, resp.price);
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
    deadline_ns: UnixNanos,
}

#[derive(Debug)]
pub(super) struct OptionChainGreeksBootstrap {
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
    params: Option<Params>,
    ownership_handler: TypedHandler<OptionGreeks>,
}

#[cfg(test)]
mod tests {
    use nautilus_common::{
        cache::Cache,
        clock::{Clock, VirtualClock},
        messages::data::{DataResponse, InstrumentResponse},
    };
    use nautilus_model::{
        data::{Data, InstrumentStatus},
        enums::MarketStatusAction,
        identifiers::Venue,
        instruments::{BinaryOption, stubs::binary_option},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    #[case::known(false, false)]
    #[case::streamed_definition(true, false)]
    #[case::requested_definition(true, true)]
    fn test_option_chain_retirement_excludes_binary_markets(
        binary_option: BinaryOption,
        #[case] close_before_definition: bool,
        #[case] request_definition: bool,
    ) {
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let instrument_id = binary_option.id();
        let mut engine = DataEngine::new(clock, Rc::clone(&cache), None);
        if !close_before_definition {
            engine.process(&InstrumentAny::from(binary_option.clone()));
        }

        let status = InstrumentStatus::new(
            instrument_id,
            MarketStatusAction::Close,
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
            None,
            Some(false),
            None,
            None,
        );

        engine.process_data(Data::InstrumentStatus(status));

        if close_before_definition {
            let instrument = InstrumentAny::from(binary_option);

            if request_definition {
                engine.response(DataResponse::Instrument(Box::new(InstrumentResponse::new(
                    UUID4::new(),
                    ClientId::from("BINARY"),
                    instrument_id,
                    instrument,
                    None,
                    None,
                    UnixNanos::from(3),
                    None,
                ))));
            } else {
                engine.process(&instrument);
            }
        }

        assert_eq!(
            cache.borrow().instrument_status(&instrument_id),
            Some(&status)
        );
        cache.borrow_mut().purge_instrument(instrument_id);
        assert!(cache.borrow().instrument(&instrument_id).is_none());
        assert!(engine.option_chain_instruments_expired.is_empty());
    }

    #[rstest]
    fn test_option_chain_fallback_releases_inflight_request(#[values(false, true)] timeout: bool) {
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
        let mut engine = DataEngine::new(clock, Rc::new(RefCell::new(Cache::default())), None);
        let series_id = OptionSeriesId::new_derived(
            Venue::from("DERIBIT"),
            Ustr::from("BTC"),
            Ustr::from("BTC"),
            UnixNanos::from(1_704_067_200_000_000_000u64),
        );
        let instrument_id = InstrumentId::from("BTC-20240101-50000-C.DERIBIT");
        let request_id = UUID4::new();
        let command = SubscribeOptionChain::new(
            series_id,
            StrikeRange::Fixed(vec![]),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            Some(series_id.venue),
            None,
        )
        .unwrap();

        let request = RequestOptionChainReferencePrice::new(
            series_id,
            instrument_id,
            None,
            request_id,
            UnixNanos::default(),
            None,
        );

        engine.inflight_requests.insert(
            request_id,
            RequestCommand::OptionChainReferencePrice(request),
        );
        engine.pending_option_chain_requests.insert(
            request_id,
            PendingOptionChainRequest {
                command,
                deadline_ns: UnixNanos::default(),
            },
        );

        if timeout {
            engine.handle_option_chain_reference_price_timeout();
        } else {
            engine.bootstrap_all_pending_option_chains();
        }

        assert!(engine.inflight_requests.is_empty());
        assert!(engine.pending_option_chain_requests.is_empty());
        engine.unsubscribe_all_option_chains();
        assert!(engine.option_chain_managers.is_empty());
    }
}
