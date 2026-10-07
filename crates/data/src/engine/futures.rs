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
    BAR_AGGREGATOR_PRIORITY, BarAggregatorSubscription, BarBarHandler, BarQuoteHandler,
    BarTradeHandler, BarType, BarsResponse, ClientId, Context, ContinuousFutureRequest,
    ContinuousFutureRequestState, ContinuousFutureSegment, ContinuousFutureSource, DataCommand,
    DataEngine, DataResponse, Debug, FromStr, InstrumentAny, InstrumentId, Params, Rc, RefCell,
    RequestBars, RequestCommand, RequestQuotes, RequestTrades, SubscribeBars, SubscribeCommand,
    SubscribeQuotes, SubscribeTrades, TimeEvent, TimeEventCallback, TypedHandler, UUID4, UnixNanos,
    UnsubscribeBars, UnsubscribeCommand, UnsubscribeQuotes, UnsubscribeTrades, Venue, WeakCell,
    bar_aggregator_key, continuous_future_request_from_bars,
    continuous_future_subscription_from_bars, log_error_on_cache_insert, log_if_empty_response,
    msgbus, response_params, switchboard,
};

impl DataEngine {
    pub(super) fn execute_continuous_future_request(
        &mut self,
        req: RequestCommand,
    ) -> anyhow::Result<()> {
        let RequestCommand::Bars(parent) = req else {
            anyhow::bail!("Continuous future requests require `RequestBars`");
        };

        let request_id = parent.request_id;

        let Some(continuous_request) = continuous_future_request_from_bars(&parent)? else {
            return Ok(());
        };

        self.ensure_continuous_future_target_instrument(&continuous_request);
        self.prepare_request_bar_aggregators_from_state(
            request_id,
            &continuous_request.request_bar_aggregation,
        )?;

        let response_client_id = match self.resolve_request_client_id(
            parent.client_id.as_ref(),
            Some(&continuous_request.primary_bar_type.instrument_id().venue),
        ) {
            Ok(client_id) => client_id,
            Err(e) => {
                self.cleanup_request_bar_aggregators(&request_id);
                return Err(e);
            }
        };

        let (cursor_ns, end_ns) = match self.bound_continuous_future_dates(&parent) {
            Ok(bounds) => bounds,
            Err(e) => {
                self.cleanup_request_bar_aggregators(&request_id);
                return Err(e);
            }
        };

        self.continuous_future_requests.insert(
            request_id,
            ContinuousFutureRequestState {
                parent,
                request: continuous_request,
                start_ns: cursor_ns,
                cursor_ns,
                end_ns,
                response_client_id,
                data_count: 0,
            },
        );

        if let Err(e) = self.dispatch_next_continuous_future_segment(request_id) {
            self.continuous_future_requests.remove(&request_id);
            self.cleanup_request_bar_aggregators(&request_id);
            return Err(e);
        }

        Ok(())
    }

    fn resolve_request_client_id(
        &mut self,
        client_id: Option<&ClientId>,
        venue: Option<&Venue>,
    ) -> anyhow::Result<ClientId> {
        self.get_client(client_id, venue)
            .map(|client| client.client_id())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Cannot handle request: no client found for {client_id:?} {venue:?}"
                )
            })
    }

    fn bound_continuous_future_dates(
        &self,
        request: &RequestBars,
    ) -> anyhow::Result<(UnixNanos, UnixNanos)> {
        let now = self.clock.borrow().timestamp_ns();
        let start = request
            .start
            .map(datetime_to_unix_nanos)
            .transpose()?
            .unwrap_or_default();
        let end = request
            .end
            .map(datetime_to_unix_nanos)
            .transpose()?
            .unwrap_or(now);

        Ok((start.min(now), end.min(now)))
    }

    fn ensure_continuous_future_target_instrument(&self, request: &ContinuousFutureRequest) {
        let target_id = request.primary_bar_type.instrument_id();
        if self.cache.borrow().instrument(&target_id).is_some() {
            return;
        }

        let segment_id = request.first_segment_instrument_id();
        let segment_instrument = self.cache.borrow().instrument(&segment_id).cloned();

        let Some(segment_instrument) = segment_instrument else {
            log::warn!(
                "Cannot synthesize continuous future instrument {target_id}: first segment {segment_id} not in cache"
            );
            return;
        };

        let InstrumentAny::FuturesContract(mut target) = segment_instrument else {
            log::warn!(
                "Cannot synthesize continuous future instrument {target_id}: segment {segment_id} is not a FuturesContract",
            );
            return;
        };

        target.id = target_id;
        target.raw_symbol = target_id.symbol;
        target.activation_ns = UnixNanos::default();
        target.expiration_ns = UnixNanos::default();

        if let Err(e) = self
            .cache
            .borrow_mut()
            .add_instrument(InstrumentAny::FuturesContract(target))
        {
            log_error_on_cache_insert(&e);
        }
    }

    fn dispatch_next_continuous_future_segment(&mut self, request_id: UUID4) -> anyhow::Result<()> {
        let Some(state) = self.continuous_future_requests.get(&request_id).cloned() else {
            anyhow::bail!("No active continuous future request for {request_id}");
        };

        let Some(segment) = state
            .request
            .next_segment(state.cursor_ns.as_u64(), state.end_ns.as_u64())
        else {
            self.emit_empty_continuous_future_response(request_id);
            return Ok(());
        };

        self.apply_continuous_future_adjustment(request_id, &state.request, segment.index)?;
        let child = self.build_continuous_future_child_request(request_id, &state, segment);
        if let Some(active) = self.continuous_future_requests.get_mut(&request_id) {
            active.cursor_ns = UnixNanos::from(segment.end_ns.saturating_add(1));
        }

        self.execute_request(child)
    }

    fn apply_continuous_future_adjustment(
        &self,
        request_id: UUID4,
        request: &ContinuousFutureRequest,
        segment_index: usize,
    ) -> anyhow::Result<()> {
        let adjustment = request.adjustment_for_segment(segment_index);
        let key = bar_aggregator_key(request.primary_bar_type, Some(request_id));

        let aggregator = self.bar_aggregators.get(&key).ok_or_else(|| {
            anyhow::anyhow!("No aggregator for continuous future request {request_id}")
        })?;

        aggregator
            .borrow_mut()
            .set_adjustment(adjustment, request.adjustment_mode);

        Ok(())
    }

    fn build_continuous_future_child_request(
        &self,
        request_id: UUID4,
        state: &ContinuousFutureRequestState,
        segment: ContinuousFutureSegment,
    ) -> RequestCommand {
        let source = state.request.source_for_segment(segment.instrument_id);
        let start = Some(UnixNanos::from(segment.start_ns).to_datetime_utc());
        let end = Some(UnixNanos::from(segment.end_ns).to_datetime_utc());
        let child_params = Some(
            state
                .request
                .child_params(state.parent.params.as_ref(), request_id),
        );
        let child_request_id = UUID4::new();
        let ts_init = self.clock.borrow().timestamp_ns();

        match source {
            ContinuousFutureSource::Bars(bar_type) => RequestCommand::Bars(RequestBars::new(
                bar_type,
                start,
                end,
                state.parent.limit,
                state.parent.client_id,
                child_request_id,
                ts_init,
                child_params,
            )),
            ContinuousFutureSource::Trades => RequestCommand::Trades(RequestTrades::new(
                segment.instrument_id,
                start,
                end,
                state.parent.limit,
                state.parent.client_id,
                child_request_id,
                ts_init,
                child_params,
            )),
            ContinuousFutureSource::Quotes => RequestCommand::Quotes(RequestQuotes::new(
                segment.instrument_id,
                start,
                end,
                state.parent.limit,
                state.parent.client_id,
                child_request_id,
                ts_init,
                child_params,
            )),
        }
    }

    fn emit_empty_continuous_future_response(&mut self, request_id: UUID4) {
        let Some(state) = self.continuous_future_requests.remove(&request_id) else {
            return;
        };

        let mut params = state.parent.params.unwrap_or_default();
        if state.data_count != 0 {
            params.insert(
                "data_count".to_string(),
                serde_json::json!(state.data_count),
            );
        }

        let response = DataResponse::Bars(BarsResponse::new(
            request_id,
            state.response_client_id,
            state.parent.bar_type,
            Vec::new(),
            Some(state.start_ns),
            Some(state.end_ns),
            self.clock.borrow().timestamp_ns(),
            Some(params),
        ));
        self.response(response);
    }

    pub(super) fn handle_continuous_future_child_response(
        &mut self,
        parent_id: UUID4,
        resp: &DataResponse,
    ) {
        if !self.continuous_future_requests.contains_key(&parent_id) {
            log::error!("No active continuous future request for child response {parent_id}");
            return;
        }

        let data_count = response_params(resp)
            .and_then(|params| params.get("data_count"))
            .and_then(serde_json::Value::as_u64)
            .or_else(|| resp.record_count().map(|count| count as u64))
            .unwrap_or(0);

        if let Some(state) = self.continuous_future_requests.get_mut(&parent_id) {
            state.data_count += data_count;
        }

        match resp {
            DataResponse::Quotes(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, resp.correlation_id()) {
                    self.handle_quotes(&r.data);
                }
            }
            DataResponse::Trades(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, resp.correlation_id()) {
                    self.handle_trades(&r.data);
                }
            }
            DataResponse::Bars(r) => {
                if !log_if_empty_response(&r.data, &r.bar_type, resp.correlation_id()) {
                    self.handle_bars(&r.data);
                }
            }
            _ => {
                log::error!(
                    "Continuous future child response {parent_id} must contain quotes, trades, or bars"
                );
                return;
            }
        }

        self.process_continuous_future_aggregation_response(parent_id, resp);
        if let Err(e) = self.dispatch_next_continuous_future_segment(parent_id) {
            log::error!("Error dispatching continuous future segment for {parent_id}: {e}");
            self.emit_empty_continuous_future_response(parent_id);
        }
    }

    fn process_continuous_future_aggregation_response(
        &self,
        parent_id: UUID4,
        resp: &DataResponse,
    ) {
        let Some(state) = self.continuous_future_requests.get(&parent_id) else {
            return;
        };

        let primary_bar_type = state.request.primary_bar_type;
        let aggregator_request_id = Some(parent_id);

        match resp {
            DataResponse::Quotes(r) => {
                for quote in &r.data {
                    self.update_request_bar_aggregator(
                        primary_bar_type,
                        aggregator_request_id,
                        |aggregator| {
                            aggregator.handle_quote(*quote);
                        },
                    );
                }
            }
            DataResponse::Trades(r) => {
                for trade in &r.data {
                    self.update_request_bar_aggregator(
                        primary_bar_type,
                        aggregator_request_id,
                        |aggregator| {
                            aggregator.handle_trade(*trade);
                        },
                    );
                }
            }
            DataResponse::Bars(r) => {
                for bar in &r.data {
                    self.update_request_bar_aggregator(
                        primary_bar_type,
                        aggregator_request_id,
                        |aggregator| {
                            aggregator.handle_bar(*bar);
                        },
                    );
                }
            }
            _ => {}
        }
    }

    pub(super) fn subscribe_continuous_future_bars(
        &mut self,
        cmd: &SubscribeBars,
    ) -> anyhow::Result<()> {
        let target_bar_type = cmd.bar_type;
        let target_key = target_bar_type.standard();

        if !target_bar_type.is_internally_aggregated() {
            anyhow::bail!(
                "Continuous future bar subscriptions require an internally aggregated target, was {target_bar_type}"
            );
        }

        if self.continuous_future_roller.is_none() {
            anyhow::bail!(
                "Cannot subscribe continuous future bars for {target_bar_type}: roller is not initialized; ensure `register_msgbus_handlers` runs before subscribing"
            );
        }

        let request = continuous_future_subscription_from_bars(cmd)?.ok_or_else(|| {
            anyhow::anyhow!(
                "Continuous future bar subscription requires `continuous_future_transitions`, was {cmd:?}"
            )
        })?;

        self.ensure_continuous_future_target_instrument(&request);

        if self
            .continuous_future_subscriptions
            .contains_key(&target_key)
        {
            log::warn!("Continuous future bars already subscribed for {target_bar_type}");
            return Ok(());
        }

        let aggregator_key = bar_aggregator_key(target_bar_type, None);
        if let Some(aggregator) = self.bar_aggregators.get(&aggregator_key)
            && aggregator.borrow().is_running()
        {
            log::warn!(
                "Aggregator for {target_bar_type} is currently in use, continuous future subscription can't be started"
            );
            return Ok(());
        }

        self.create_bar_aggregator_for_key(target_bar_type, None, None)?;
        self.setup_bar_aggregator(target_bar_type, false, None)?;

        let now_ns = self.clock.borrow().timestamp_ns().as_u64();

        let Some(segment) = request.next_segment(now_ns, now_ns) else {
            log::error!("Cannot determine active continuous future segment for {target_bar_type}");

            if let Err(e) = self.stop_bar_aggregator(target_bar_type, None) {
                log::error!(
                    "Error rolling back continuous future aggregator for {target_bar_type}: {e}"
                );
            }

            return Ok(());
        };

        self.apply_continuous_future_subscription_adjustment(&request, segment.index)?;
        let source = request.source_for_segment(segment.instrument_id);
        let source_subscription =
            self.subscribe_continuous_future_source(target_bar_type, source, segment.instrument_id);

        if let Some(aggregator) = self.bar_aggregators.get(&aggregator_key) {
            aggregator.borrow_mut().set_is_running(true);
        }

        let next_transition_index =
            (segment.index < request.transitions.len()).then_some(segment.index);

        self.continuous_future_subscriptions.insert(
            target_key,
            ContinuousFutureSubscriptionState {
                target_bar_type,
                client_id: cmd.client_id,
                venue: cmd.venue,
                command_id: cmd.command_id,
                params: cmd.params.clone(),
                request,
                active_segment_instrument_id: segment.instrument_id,
                active_source: source,
                active_source_subscription: Some(source_subscription),
                next_transition_index,
                timer_name: None,
            },
        );

        let child_cmd = self.build_continuous_future_subscribe_command(
            &target_key,
            source,
            segment.instrument_id,
            cmd.command_id,
            cmd.ts_init,
            true,
        );

        if let Some(child) = child_cmd {
            self.execute(child);
        }

        self.schedule_continuous_future_transition(target_key);

        Ok(())
    }

    pub(super) fn unsubscribe_continuous_future_bars(&mut self, cmd: &UnsubscribeBars) {
        let target_key = cmd.bar_type.standard();

        let Some(mut state) = self.continuous_future_subscriptions.remove(&target_key) else {
            log::warn!(
                "Cannot unsubscribe continuous future bars: no subscription state for {target_key}"
            );
            return;
        };

        if let Some(name) = state.timer_name.take() {
            self.clock.borrow_mut().cancel_timer(&name);
        }

        let ts_init = self.clock.borrow().timestamp_ns();
        let segment_instrument_id = state.active_segment_instrument_id;
        let source = state.active_source;
        let source_subscription = state.active_source_subscription.take();
        let client_id = state.client_id;
        let venue = state.venue;
        let params = state.params.clone();
        let target_bar_type = state.target_bar_type;
        drop(state);

        if let Some(subscription) = source_subscription {
            self.unsubscribe_continuous_future_source(target_bar_type, subscription);
        }

        let child_cmd = build_continuous_future_unsubscribe_command(
            source,
            segment_instrument_id,
            client_id,
            venue,
            params.as_ref(),
            cmd.command_id,
            ts_init,
        );
        self.execute(child_cmd);

        if let Err(e) = self.stop_bar_aggregator(target_bar_type, None) {
            log::error!("Error stopping continuous future aggregator for {target_bar_type}: {e}");
        }
    }

    fn handle_continuous_future_subscription_transition(&mut self, event: &TimeEvent) {
        let event_name = event.name.as_str();

        let Some((target_key, transition_index)) = parse_transition_timer_name(event_name) else {
            log::warn!(
                "Ignoring continuous future transition event with unparsable name {event_name}"
            );
            return;
        };

        let Some(state) = self.continuous_future_subscriptions.get_mut(&target_key) else {
            log::warn!(
                "Ignoring continuous future transition event {event_name}: no subscription state for {target_key}"
            );
            return;
        };

        if state.timer_name.as_deref() != Some(event_name) {
            return;
        }

        state.timer_name = None;

        let Some(next_index) = state.next_transition_index else {
            return;
        };

        if next_index != transition_index || next_index >= state.request.transitions.len() {
            return;
        }

        let prev_segment_instrument_id = state.active_segment_instrument_id;
        let next_segment_instrument_id = state.request.transitions[next_index].post_instrument_id;
        let new_segment_index = next_index + 1;
        state.active_segment_instrument_id = next_segment_instrument_id;
        state.next_transition_index =
            (new_segment_index < state.request.transitions.len()).then_some(new_segment_index);

        let old_source = state.active_source;
        let old_source_subscription = state.active_source_subscription.take();
        let client_id = state.client_id;
        let venue = state.venue;
        let params = state.params.clone();
        let command_id = state.command_id;
        let target_bar_type = state.target_bar_type;

        let ts_init = self.clock.borrow().timestamp_ns();

        if let Some(subscription) = old_source_subscription {
            self.unsubscribe_continuous_future_source(target_bar_type, subscription);
        }

        let unsub_child = build_continuous_future_unsubscribe_command(
            old_source,
            prev_segment_instrument_id,
            client_id,
            venue,
            params.as_ref(),
            command_id,
            ts_init,
        );
        self.execute(unsub_child);

        if let Err(e) = self
            .apply_continuous_future_subscription_adjustment_for(target_bar_type, new_segment_index)
        {
            log::error!("Error applying continuous future adjustment for {target_bar_type}: {e}");
            return;
        }

        let new_source = {
            let Some(state) = self.continuous_future_subscriptions.get(&target_key) else {
                return;
            };

            state.request.source_for_segment(next_segment_instrument_id)
        };

        let new_subscription = self.subscribe_continuous_future_source(
            target_bar_type,
            new_source,
            next_segment_instrument_id,
        );

        if let Some(state) = self.continuous_future_subscriptions.get_mut(&target_key) {
            state.active_source = new_source;
            state.active_source_subscription = Some(new_subscription);
        }

        let sub_child = self.build_continuous_future_subscribe_command(
            &target_key,
            new_source,
            next_segment_instrument_id,
            command_id,
            ts_init,
            true,
        );

        if let Some(child) = sub_child {
            self.execute(child);
        }

        self.schedule_continuous_future_transition(target_key);
    }

    fn apply_continuous_future_subscription_adjustment(
        &self,
        request: &ContinuousFutureRequest,
        segment_index: usize,
    ) -> anyhow::Result<()> {
        let key = bar_aggregator_key(request.primary_bar_type, None);

        let aggregator = self.bar_aggregators.get(&key).ok_or_else(|| {
            anyhow::anyhow!(
                "No live aggregator for continuous future subscription {}",
                request.primary_bar_type
            )
        })?;

        let adjustment = request.adjustment_for_segment(segment_index);
        aggregator
            .borrow_mut()
            .set_adjustment(adjustment, request.adjustment_mode);
        Ok(())
    }

    fn apply_continuous_future_subscription_adjustment_for(
        &self,
        target_bar_type: BarType,
        segment_index: usize,
    ) -> anyhow::Result<()> {
        let Some(state) = self
            .continuous_future_subscriptions
            .get(&target_bar_type.standard())
        else {
            anyhow::bail!("No continuous future subscription state for {target_bar_type}");
        };

        self.apply_continuous_future_subscription_adjustment(&state.request, segment_index)
    }

    fn subscribe_continuous_future_source(
        &mut self,
        target_bar_type: BarType,
        source: ContinuousFutureSource,
        segment_instrument_id: InstrumentId,
    ) -> BarAggregatorSubscription {
        let key = bar_aggregator_key(target_bar_type, None);
        let aggregator = self
            .bar_aggregators
            .get(&key)
            .cloned()
            .expect("aggregator was created before subscribe_continuous_future_source");

        let subscription = match source {
            ContinuousFutureSource::Bars(source_bar_type) => {
                let topic = switchboard::get_bars_topic(source_bar_type);
                let handler =
                    TypedHandler::new(BarBarHandler::new(&aggregator, target_bar_type.standard()));
                msgbus::subscribe_bars(topic.into(), handler.clone(), None);
                BarAggregatorSubscription::Bar { topic, handler }
            }
            ContinuousFutureSource::Trades => {
                let topic = switchboard::get_trades_topic(segment_instrument_id);

                let handler = TypedHandler::new(BarTradeHandler::new(
                    &aggregator,
                    target_bar_type.standard(),
                ));
                msgbus::subscribe_trades(
                    topic.into(),
                    handler.clone(),
                    Some(BAR_AGGREGATOR_PRIORITY),
                );
                BarAggregatorSubscription::Trade { topic, handler }
            }
            ContinuousFutureSource::Quotes => {
                let topic = switchboard::get_quotes_topic(segment_instrument_id);

                let handler = TypedHandler::new(BarQuoteHandler::new(
                    &aggregator,
                    target_bar_type.standard(),
                ));
                msgbus::subscribe_quotes(
                    topic.into(),
                    handler.clone(),
                    Some(BAR_AGGREGATOR_PRIORITY),
                );
                BarAggregatorSubscription::Quote { topic, handler }
            }
        };

        self.bar_aggregator_handlers
            .entry(key)
            .or_default()
            .push(subscription.clone());

        subscription
    }

    pub(super) fn unsubscribe_continuous_future_source(
        &mut self,
        target_bar_type: BarType,
        subscription: BarAggregatorSubscription,
    ) {
        let key = bar_aggregator_key(target_bar_type, None);
        if let Some(subs) = self.bar_aggregator_handlers.get_mut(&key) {
            subs.retain(|registered| !same_subscription(registered, &subscription));
        }

        match subscription {
            BarAggregatorSubscription::Bar { topic, handler } => {
                msgbus::unsubscribe_bars(topic.into(), &handler);
            }
            BarAggregatorSubscription::Trade { topic, handler } => {
                msgbus::unsubscribe_trades(topic.into(), &handler);
            }
            BarAggregatorSubscription::Quote { topic, handler } => {
                msgbus::unsubscribe_quotes(topic.into(), &handler);
            }
        }
    }

    fn build_continuous_future_subscribe_command(
        &self,
        target_key: &BarType,
        source: ContinuousFutureSource,
        segment_instrument_id: InstrumentId,
        command_id: UUID4,
        ts_init: UnixNanos,
        subscribe: bool,
    ) -> Option<DataCommand> {
        let state = self.continuous_future_subscriptions.get(target_key)?;

        if !subscribe {
            return Some(build_continuous_future_unsubscribe_command(
                source,
                segment_instrument_id,
                state.client_id,
                state.venue,
                state.params.as_ref(),
                command_id,
                ts_init,
            ));
        }

        let child_params = state
            .request
            .child_params(state.params.as_ref(), command_id);

        Some(build_continuous_future_subscribe_inner(
            source,
            segment_instrument_id,
            state.client_id,
            state.venue,
            child_params,
            command_id,
            ts_init,
        ))
    }

    fn schedule_continuous_future_transition(&mut self, target_key: BarType) {
        let Some(state) = self.continuous_future_subscriptions.get_mut(&target_key) else {
            return;
        };

        if let Some(name) = state.timer_name.take() {
            self.clock.borrow_mut().cancel_timer(&name);
        }

        let Some(transition_index) = state.next_transition_index else {
            return;
        };

        let Some(row) = state.request.transitions.get(transition_index) else {
            return;
        };

        let transition_ns = row.transition_time_ns;
        let timer_name = format!("continuous-future-roll:{target_key}:{transition_index}");

        let Some(roller) = self.continuous_future_roller.clone() else {
            log::error!(
                "Cannot schedule continuous future transition timer for {target_key}: roller not initialized"
            );
            return;
        };

        let callback_fn: Rc<dyn Fn(TimeEvent)> =
            Rc::new(move |event| roller.handle_transition(&event));
        let callback = TimeEventCallback::from(callback_fn);

        if let Err(e) = self.clock.borrow_mut().set_time_alert_ns(
            &timer_name,
            UnixNanos::from(transition_ns),
            Some(callback),
            Some(true),
        ) {
            log::error!("Failed to schedule continuous future transition {timer_name}: {e}");
            return;
        }

        if let Some(state) = self.continuous_future_subscriptions.get_mut(&target_key) {
            state.timer_name = Some(timer_name);
        }
    }
}

/// Routes continuous-future transition timer events back to the engine.
///
/// The clock owns the timer's callback closure; the closure must be able to
/// call back into the engine without creating an Rc cycle. The roller holds a
/// weak reference to the engine and upgrades on each fire.
#[derive(Debug)]
pub(super) struct ContinuousFutureRoller {
    engine: WeakCell<DataEngine>,
}

impl ContinuousFutureRoller {
    pub(super) fn new(engine: &Rc<RefCell<DataEngine>>) -> Self {
        Self {
            engine: WeakCell::from(Rc::downgrade(engine)),
        }
    }

    fn handle_transition(&self, event: &TimeEvent) {
        if let Some(engine) = self.engine.upgrade() {
            engine
                .borrow_mut()
                .handle_continuous_future_subscription_transition(event);
        }
    }
}

#[derive(Debug)]
pub(super) struct ContinuousFutureSubscriptionState {
    pub(super) target_bar_type: BarType,
    client_id: Option<ClientId>,
    venue: Option<Venue>,
    command_id: UUID4,
    params: Option<Params>,
    request: ContinuousFutureRequest,
    active_segment_instrument_id: InstrumentId,
    active_source: ContinuousFutureSource,
    pub(super) active_source_subscription: Option<BarAggregatorSubscription>,
    next_transition_index: Option<usize>,
    pub(super) timer_name: Option<String>,
}

fn same_subscription(a: &BarAggregatorSubscription, b: &BarAggregatorSubscription) -> bool {
    match (a, b) {
        (
            BarAggregatorSubscription::Bar { handler: h1, .. },
            BarAggregatorSubscription::Bar { handler: h2, .. },
        ) => h1.id() == h2.id(),
        (
            BarAggregatorSubscription::Trade { handler: h1, .. },
            BarAggregatorSubscription::Trade { handler: h2, .. },
        ) => h1.id() == h2.id(),
        (
            BarAggregatorSubscription::Quote { handler: h1, .. },
            BarAggregatorSubscription::Quote { handler: h2, .. },
        ) => h1.id() == h2.id(),
        _ => false,
    }
}

fn parse_transition_timer_name(name: &str) -> Option<(BarType, usize)> {
    let rest = name.strip_prefix("continuous-future-roll:")?;
    let (target, index) = rest.rsplit_once(':')?;
    let bar_type = BarType::from_str(target).ok()?;
    let index = index.parse::<usize>().ok()?;
    Some((bar_type, index))
}

fn build_continuous_future_subscribe_inner(
    source: ContinuousFutureSource,
    segment_instrument_id: InstrumentId,
    client_id: Option<ClientId>,
    _venue: Option<Venue>,
    child_params: Params,
    correlation_id: UUID4,
    ts_init: UnixNanos,
) -> DataCommand {
    let command_id = UUID4::new();
    let child_venue = Some(segment_instrument_id.venue);

    match source {
        ContinuousFutureSource::Bars(source_bar_type) => {
            DataCommand::Subscribe(SubscribeCommand::Bars(SubscribeBars::new(
                source_bar_type,
                client_id,
                child_venue,
                command_id,
                ts_init,
                Some(correlation_id),
                Some(child_params),
            )))
        }
        ContinuousFutureSource::Trades => {
            DataCommand::Subscribe(SubscribeCommand::Trades(SubscribeTrades::new(
                segment_instrument_id,
                client_id,
                child_venue,
                command_id,
                ts_init,
                Some(correlation_id),
                Some(child_params),
            )))
        }
        ContinuousFutureSource::Quotes => {
            DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
                segment_instrument_id,
                client_id,
                child_venue,
                command_id,
                ts_init,
                Some(correlation_id),
                Some(child_params),
            )))
        }
    }
}

fn build_continuous_future_unsubscribe_command(
    source: ContinuousFutureSource,
    segment_instrument_id: InstrumentId,
    client_id: Option<ClientId>,
    _venue: Option<Venue>,
    parent_params: Option<&Params>,
    correlation_id: UUID4,
    ts_init: UnixNanos,
) -> DataCommand {
    let mut child_params = parent_params.cloned().unwrap_or_default();
    child_params.shift_remove("continuous_future_transitions");
    child_params.shift_remove("continuous_future_adjustment_mode");
    child_params.shift_remove("last_post_instrument_id");
    child_params.shift_remove("first_pre_instrument_id");
    child_params.shift_remove("bar_types");
    let command_id = UUID4::new();
    let child_venue = Some(segment_instrument_id.venue);

    match source {
        ContinuousFutureSource::Bars(source_bar_type) => {
            DataCommand::Unsubscribe(UnsubscribeCommand::Bars(UnsubscribeBars::new(
                source_bar_type,
                client_id,
                child_venue,
                command_id,
                ts_init,
                Some(correlation_id),
                Some(child_params),
            )))
        }
        ContinuousFutureSource::Trades => {
            DataCommand::Unsubscribe(UnsubscribeCommand::Trades(UnsubscribeTrades::new(
                segment_instrument_id,
                client_id,
                child_venue,
                command_id,
                ts_init,
                Some(correlation_id),
                Some(child_params),
            )))
        }
        ContinuousFutureSource::Quotes => {
            DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
                segment_instrument_id,
                client_id,
                child_venue,
                command_id,
                ts_init,
                Some(correlation_id),
                Some(child_params),
            )))
        }
    }
}

pub(super) fn datetime_to_unix_nanos(datetime: jiff::Timestamp) -> anyhow::Result<UnixNanos> {
    let timestamp = datetime.as_nanosecond();
    let timestamp = u64::try_from(timestamp)
        .context("datetime is before the UNIX epoch and cannot be represented as UnixNanos")?;
    Ok(UnixNanos::from(timestamp))
}
