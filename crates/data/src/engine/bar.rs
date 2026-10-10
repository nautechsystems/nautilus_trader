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

use std::fmt::Debug;

use anyhow::Context;
use jiff::tz::TimeZone;
use nautilus_common::{
    messages::data::{SubscribeBars, SubscribeCommand},
    msgbus::{self, MStr, Topic, TypedHandler},
};
use nautilus_core::{UUID4, datetime::get_timezone};
use nautilus_model::data::{Bar, BarType, QuoteTick, TradeTick};

use super::{
    AggregationSource, BAR_AGGREGATOR_PRIORITY, BarAggregation, BarAggregator, BarBarHandler,
    BarQuoteHandler, BarTradeHandler, DataCommand, DataEngine, DataResponse, Instrument,
    InstrumentAny, PriceType, Rc, RefCell, RenkoBarAggregator, RequestBarAggregation,
    RequestCommand, SubscribeQuotes, SubscribeTrades, TickBarAggregator,
    TickImbalanceBarAggregator, TickRunsBarAggregator, TimeBarAggregator, UnsubscribeBars,
    UnsubscribeCommand, UnsubscribeQuotes, UnsubscribeTrades, ValueBarAggregator,
    ValueImbalanceBarAggregator, ValueRunsBarAggregator, VolumeBarAggregator,
    VolumeImbalanceBarAggregator, VolumeRunsBarAggregator, log_error_on_cache_insert,
    process_engine_bar, request_bar_aggregation_from_params, request_params,
    requests::time_zone_from_params, switchboard,
};

impl DataEngine {
    pub(super) fn prepare_request_bar_aggregators_from_state(
        &mut self,
        request_id: UUID4,
        state: &RequestBarAggregation,
    ) -> anyhow::Result<()> {
        if !self.can_start_request_bar_aggregators(request_id, state) {
            anyhow::bail!(
                "Cannot request aggregated bars: one of the aggregators in `bar_types` is already running"
            );
        }

        self.request_bar_aggregations
            .insert(request_id, state.clone());

        if let Err(e) = self.init_request_bar_aggregators(request_id, state) {
            self.cleanup_request_bar_aggregators(&request_id);
            return Err(e);
        }

        Ok(())
    }

    pub(super) fn prepare_request_bar_aggregators(
        &mut self,
        req: &RequestCommand,
    ) -> anyhow::Result<()> {
        let request_id = *req.request_id();

        let Some(state) = request_bar_aggregation_from_params(request_params(req))? else {
            return Ok(());
        };

        self.prepare_request_bar_aggregators_from_state(request_id, &state)
    }

    fn can_start_request_bar_aggregators(
        &self,
        request_id: UUID4,
        state: &RequestBarAggregation,
    ) -> bool {
        let aggregator_request_id = state.aggregator_request_id(request_id);
        state.bar_types.iter().all(|bar_type| {
            let key = bar_aggregator_key(*bar_type, aggregator_request_id);
            self.bar_aggregators
                .get(&key)
                .is_none_or(|aggregator| !aggregator.borrow().is_running())
        })
    }

    fn init_request_bar_aggregators(
        &mut self,
        request_id: UUID4,
        state: &RequestBarAggregation,
    ) -> anyhow::Result<()> {
        let aggregator_request_id = state.aggregator_request_id(request_id);

        for bar_type in &state.bar_types {
            self.create_bar_aggregator_for_key(
                *bar_type,
                aggregator_request_id,
                state.skip_first_non_full_bar,
                state.time_zone.clone(),
            )?;
            self.setup_bar_aggregator(*bar_type, true, aggregator_request_id)?;

            let key = bar_aggregator_key(*bar_type, aggregator_request_id);
            if let Some(aggregator) = self.bar_aggregators.get(&key) {
                if state.disable_build_with_no_updates {
                    aggregator.borrow_mut().set_build_with_no_updates(false);
                }

                aggregator.borrow_mut().set_is_running(true);
            }
        }

        self.set_request_bar_aggregator_chain_handlers(request_id, state);

        Ok(())
    }

    fn set_request_bar_aggregator_chain_handlers(
        &self,
        request_id: UUID4,
        state: &RequestBarAggregation,
    ) {
        let aggregator_request_id = state.aggregator_request_id(request_id);

        for bar_type in &state.bar_types {
            let key = bar_aggregator_key(*bar_type, aggregator_request_id);

            let Some(aggregator) = self.bar_aggregators.get(&key).cloned() else {
                continue;
            };

            let downstream: Vec<_> = state
                .bar_types
                .iter()
                .filter(|candidate| {
                    candidate.is_composite()
                        && candidate.composite().standard() == bar_type.standard()
                })
                .filter_map(|candidate| {
                    let key = bar_aggregator_key(*candidate, aggregator_request_id);
                    self.bar_aggregators.get(&key).cloned()
                })
                .collect();

            let cache = self.cache.clone();

            let handler: Box<dyn FnMut(Bar)> = Box::new(move |bar: Bar| {
                // Request-generated bars are delivered only through the cache.
                if let Err(e) = cache.as_ref().borrow_mut().add_bar_historical(bar) {
                    log_error_on_cache_insert(&e);
                }

                for aggregator in &downstream {
                    aggregator.borrow_mut().handle_bar(bar);
                }
            });

            aggregator.borrow_mut().set_historical_mode(true, handler);
        }
    }

    pub(super) fn cleanup_request_bar_aggregators(&mut self, request_id: &UUID4) -> bool {
        let Some(state) = self.request_bar_aggregations.remove(request_id) else {
            return false;
        };

        let aggregator_request_id = state.aggregator_request_id(*request_id);

        for bar_type in state.bar_types {
            let key = bar_aggregator_key(bar_type, aggregator_request_id);
            let has_live_handlers =
                state.update_subscriptions && self.bar_aggregator_handlers.contains_key(&key);

            let keep_running = if has_live_handlers {
                match self.setup_bar_aggregator(bar_type, false, aggregator_request_id) {
                    Ok(()) => true,
                    Err(e) => {
                        log::error!(
                            "Error starting live request bar aggregator for {bar_type}: {e}"
                        );
                        false
                    }
                }
            } else {
                false
            };

            if let Some(aggregator) = self.bar_aggregators.get(&key) {
                aggregator.borrow_mut().set_is_running(keep_running);
            }

            if !state.update_subscriptions
                && let Err(e) = self.stop_bar_aggregator(bar_type, aggregator_request_id)
            {
                log::error!("Error stopping request bar aggregator for {bar_type}: {e}");
            }
        }

        true
    }

    pub(super) fn process_request_bar_aggregation_response(&mut self, resp: &DataResponse) {
        let correlation_id = *resp.correlation_id();

        let Some(state) = self.request_bar_aggregations.get(&correlation_id).cloned() else {
            return;
        };

        match resp {
            DataResponse::Quotes(r) => {
                for quote in &r.data {
                    self.update_request_bar_aggregators_from_quote(&state, correlation_id, *quote);
                }
            }
            DataResponse::Trades(r) => {
                for trade in &r.data {
                    self.update_request_bar_aggregators_from_trade(&state, correlation_id, *trade);
                }
            }
            DataResponse::Bars(r) => {
                for bar in &r.data {
                    self.update_request_bar_aggregators_from_bar(&state, correlation_id, *bar);
                }
            }
            _ => {}
        }

        self.cleanup_request_bar_aggregators(&correlation_id);
    }

    pub(super) fn update_request_bar_aggregators_from_quote(
        &self,
        state: &RequestBarAggregation,
        request_id: UUID4,
        quote: QuoteTick,
    ) {
        let aggregator_request_id = state.aggregator_request_id(request_id);

        for bar_type in &state.bar_types {
            if bar_type.is_composite()
                || bar_type.instrument_id() != quote.instrument_id
                || bar_type.spec().price_type == PriceType::Last
            {
                continue;
            }

            self.update_request_bar_aggregator(*bar_type, aggregator_request_id, |aggregator| {
                aggregator.handle_quote(quote);
            });
        }
    }

    pub(super) fn update_request_bar_aggregators_from_trade(
        &self,
        state: &RequestBarAggregation,
        request_id: UUID4,
        trade: TradeTick,
    ) {
        let aggregator_request_id = state.aggregator_request_id(request_id);

        for bar_type in &state.bar_types {
            if bar_type.is_composite()
                || bar_type.instrument_id() != trade.instrument_id
                || bar_type.spec().price_type != PriceType::Last
            {
                continue;
            }

            self.update_request_bar_aggregator(*bar_type, aggregator_request_id, |aggregator| {
                aggregator.handle_trade(trade);
            });
        }
    }

    pub(super) fn update_request_bar_aggregators_from_bar(
        &self,
        state: &RequestBarAggregation,
        request_id: UUID4,
        bar: Bar,
    ) {
        let aggregator_request_id = state.aggregator_request_id(request_id);

        for bar_type in &state.bar_types {
            if !bar_type.is_composite()
                || bar_type.composite().standard() != bar.bar_type.standard()
            {
                continue;
            }

            self.update_request_bar_aggregator(*bar_type, aggregator_request_id, |aggregator| {
                aggregator.handle_bar(bar);
            });
        }
    }

    pub(super) fn update_request_bar_aggregator<F>(
        &self,
        bar_type: BarType,
        request_id: Option<UUID4>,
        update: F,
    ) where
        F: FnOnce(&mut dyn BarAggregator),
    {
        let key = bar_aggregator_key(bar_type, request_id);

        let Some(aggregator) = self.bar_aggregators.get(&key) else {
            log::error!("Cannot update request bar aggregator: no aggregator found for {bar_type}");
            return;
        };

        update(aggregator.borrow_mut().as_mut());
    }

    pub(super) fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        match cmd.bar_type.aggregation_source() {
            AggregationSource::Internal => self.start_bar_aggregation(cmd)?,
            AggregationSource::External => {
                if cmd.bar_type.instrument_id().is_synthetic() {
                    anyhow::bail!(
                        "Cannot subscribe for externally aggregated synthetic instrument bar data"
                    );
                }
            }
        }

        Ok(())
    }

    pub(super) fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) {
        let bar_type = cmd.bar_type;

        // Don't remove aggregator if other exact-topic subscribers still exist
        let topic = switchboard::get_bars_topic(bar_type.standard());
        if msgbus::exact_subscriber_count_bars(topic) > 0 {
            return;
        }

        let retained = self
            .subscriptions_bar_aggregation
            .get(&bar_type.standard())
            .map(|subscription| subscription.command.clone());

        let command = retained.map_or_else(
            || cmd.clone(),
            |subscribe| {
                UnsubscribeBars::new(
                    subscribe.bar_type,
                    subscribe.client_id,
                    subscribe.venue,
                    cmd.command_id,
                    cmd.ts_init,
                    Some(subscribe.command_id),
                    subscribe.params,
                )
            },
        );

        if self
            .bar_aggregators
            .contains_key(&bar_aggregator_key(bar_type, None))
        {
            match self.stop_bar_aggregator(bar_type, None) {
                Ok(()) => {
                    self.subscriptions_bar_aggregation
                        .remove(&bar_type.standard());
                    self.unsubscribe_bar_aggregator(&command);
                }
                Err(e) => log::error!("Error stopping bar aggregator for {bar_type}: {e}"),
            }
        }

        // After stopping a composite, release its source through `unsubscribe_bars`, which frees
        // the client feed recorded in the source's retained command.
        if command.bar_type.is_composite() {
            let source_type = command.bar_type.composite();

            if self
                .bar_aggregators
                .contains_key(&bar_aggregator_key(source_type, None))
            {
                self.unsubscribe_bars(&UnsubscribeBars::new(
                    source_type,
                    command.client_id,
                    command.venue,
                    UUID4::new(),
                    command.ts_init,
                    Some(command.command_id),
                    command.params.clone(),
                ));
            }
        }
    }

    fn create_bar_aggregator(
        &self,
        instrument: &InstrumentAny,
        bar_type: BarType,
        skip_first_non_full_bar: Option<bool>,
        time_zone: TimeZone,
    ) -> Box<dyn BarAggregator> {
        let cache = self.cache.clone();
        let validate_sequence = self.config.validate_data_sequence;

        let handler = move |bar: Bar| {
            process_engine_bar(&cache, validate_sequence, true, bar);
        };

        let clock = self.clock.clone();
        let config = self.config.clone();

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        if bar_type.spec().is_time_aggregated() {
            let time_bars_origin_offset = config
                .time_bars_origin_offset
                .get(&bar_type.spec().aggregation)
                .map(|duration| jiff::SignedDuration::try_from(*duration).unwrap_or_default());

            Box::new(TimeBarAggregator::new(
                bar_type,
                price_precision,
                size_precision,
                clock,
                handler,
                config.time_bars_build_with_no_updates,
                config.time_bars_timestamp_on_close,
                config.time_bars_interval_type,
                time_bars_origin_offset,
                time_zone,
                config.time_bars_build_delay,
                skip_first_non_full_bar.unwrap_or(config.time_bars_skip_first_non_full_bar),
            ))
        } else {
            match bar_type.spec().aggregation {
                BarAggregation::Tick => Box::new(TickBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::TickImbalance => Box::new(TickImbalanceBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::TickRuns => Box::new(TickRunsBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::Volume => Box::new(VolumeBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::VolumeImbalance => Box::new(VolumeImbalanceBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::VolumeRuns => Box::new(VolumeRunsBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::Value => Box::new(ValueBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::ValueImbalance => Box::new(ValueImbalanceBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::ValueRuns => Box::new(ValueRunsBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::Renko => Box::new(RenkoBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    instrument.price_increment(),
                    handler,
                )) as Box<dyn BarAggregator>,
                other => unreachable!(
                    "Unsupported internal bar aggregation dispatch for {other:?}; update `create_bar_aggregator`"
                ),
            }
        }
    }

    pub(super) fn create_bar_aggregator_for_key(
        &mut self,
        bar_type: BarType,
        request_id: Option<UUID4>,
        skip_first_non_full_bar: Option<bool>,
        time_zone: Option<TimeZone>,
    ) -> anyhow::Result<()> {
        let key = bar_aggregator_key(bar_type, request_id);

        let time_zone = match time_zone {
            Some(zone) => zone,
            None => get_timezone(self.config.time_bars_time_zone.as_deref().unwrap_or("UTC"))
                .context("invalid `time_bars_time_zone` configuration")?,
        };

        if let Some(aggregator) = self.bar_aggregators.get(&key) {
            let aggregator = aggregator.borrow();
            if let Some(aggregator) = aggregator.as_any().downcast_ref::<TimeBarAggregator>() {
                anyhow::ensure!(
                    aggregator.time_zone() == &time_zone,
                    "Cannot reuse bar aggregator {bar_type} with a different time zone"
                );
            }

            return Ok(());
        }

        let instrument = {
            let cache = self.cache.borrow();
            cache
                .instrument(&bar_type.instrument_id())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Cannot start bar aggregation: no instrument found for {}",
                        bar_type.instrument_id(),
                    )
                })?
                .clone()
        };

        let aggregator =
            self.create_bar_aggregator(&instrument, bar_type, skip_first_non_full_bar, time_zone);
        debug_assert_eq!(
            aggregator.bar_type(),
            key.0,
            "aggregator bar type must match its standardized key"
        );
        self.bar_aggregators
            .insert(key, Rc::new(RefCell::new(aggregator)));

        Ok(())
    }

    fn start_bar_aggregation(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        let time_zone = time_zone_from_params(cmd.params.as_ref())?;
        let skip_first_non_full_bar = cmd
            .params
            .as_ref()
            .and_then(|params| params.get_bool("skip_first_non_full_bar"));
        self.create_bar_aggregator_for_key(
            cmd.bar_type,
            None,
            skip_first_non_full_bar,
            time_zone.clone(),
        )?;
        let key = bar_aggregator_key(cmd.bar_type, None);

        if self
            .bar_aggregators
            .get(&key)
            .is_some_and(|aggregator| aggregator.borrow().is_running())
            && self.bar_aggregator_handlers.contains_key(&key)
        {
            if let Some(source_command) = self
                .subscriptions_bar_aggregation
                .get(&cmd.bar_type.standard())
                .and_then(|subscription| subscription.source.clone())
            {
                self.execute(DataCommand::Subscribe(source_command));
            }

            log::warn!(
                "Aggregator for {} is currently in use, subscription can't be started",
                cmd.bar_type,
            );
            return Ok(());
        }

        self.start_bar_aggregator(cmd.bar_type, None, skip_first_non_full_bar, time_zone)?;
        let source = self.subscribe_bar_aggregator(cmd);
        self.subscriptions_bar_aggregation.insert(
            cmd.bar_type.standard(),
            BarAggregationSubscription {
                command: cmd.clone(),
                source,
            },
        );

        Ok(())
    }

    fn start_bar_aggregator(
        &mut self,
        bar_type: BarType,
        request_id: Option<UUID4>,
        skip_first_non_full_bar: Option<bool>,
        time_zone: Option<TimeZone>,
    ) -> anyhow::Result<()> {
        let key = bar_aggregator_key(bar_type, request_id);
        let bar_type_std = bar_type.standard();

        self.create_bar_aggregator_for_key(
            bar_type,
            request_id,
            skip_first_non_full_bar,
            time_zone,
        )?;
        let aggregator = self
            .bar_aggregators
            .get(&key)
            .ok_or_else(|| anyhow::anyhow!("Cannot start bar aggregation for {bar_type}"))?
            .clone();
        let defer_subscription_activation = request_id.is_none()
            && aggregator.borrow().is_running()
            && !self.bar_aggregator_handlers.contains_key(&key);

        if !self.bar_aggregator_handlers.contains_key(&key) {
            // Subscribe to underlying data topics
            let mut subscriptions = Vec::new();

            if bar_type.is_composite() {
                let topic = switchboard::get_bars_topic(bar_type.composite());
                let handler = TypedHandler::new(BarBarHandler::new(&aggregator, bar_type_std));
                msgbus::subscribe_bars(topic.into(), handler.clone(), None);
                subscriptions.push(BarAggregatorSubscription::Bar { topic, handler });
            } else if bar_type.spec().price_type == PriceType::Last {
                let topic = switchboard::get_trades_topic(bar_type.instrument_id());
                let handler = TypedHandler::new(BarTradeHandler::new(&aggregator, bar_type_std));
                msgbus::subscribe_trades(
                    topic.into(),
                    handler.clone(),
                    Some(BAR_AGGREGATOR_PRIORITY),
                );
                subscriptions.push(BarAggregatorSubscription::Trade { topic, handler });
            } else {
                // Warn if imbalance/runs aggregation is wired to quotes (needs aggressor_side from trades)
                if matches!(
                    bar_type.spec().aggregation,
                    BarAggregation::TickImbalance
                        | BarAggregation::VolumeImbalance
                        | BarAggregation::ValueImbalance
                        | BarAggregation::TickRuns
                        | BarAggregation::VolumeRuns
                        | BarAggregation::ValueRuns
                ) {
                    log::warn!(
                        "Bar type {bar_type} uses imbalance/runs aggregation which requires trade \
                         data with `aggressor_side`, but `price_type` is not LAST so it will receive \
                         quote data: bars will not emit correctly",
                    );
                }

                let topic = switchboard::get_quotes_topic(bar_type.instrument_id());
                let handler = TypedHandler::new(BarQuoteHandler::new(&aggregator, bar_type_std));
                msgbus::subscribe_quotes(
                    topic.into(),
                    handler.clone(),
                    Some(BAR_AGGREGATOR_PRIORITY),
                );
                subscriptions.push(BarAggregatorSubscription::Quote { topic, handler });
            }

            self.bar_aggregator_handlers.insert(key, subscriptions);
        }

        if defer_subscription_activation {
            return Ok(());
        }

        self.setup_bar_aggregator(bar_type, false, request_id)?;

        aggregator.borrow_mut().set_is_running(true);

        Ok(())
    }

    fn subscribe_bar_aggregator(&mut self, cmd: &SubscribeBars) -> Option<SubscribeCommand> {
        let subscribe = self.bar_aggregator_source_command(cmd)?;
        self.execute(DataCommand::Subscribe(subscribe.clone()));
        Some(subscribe)
    }

    fn bar_aggregator_source_command(&self, cmd: &SubscribeBars) -> Option<SubscribeCommand> {
        let key = bar_aggregator_key(cmd.bar_type, None);
        if !self.bar_aggregators.contains_key(&key) {
            log::error!(
                "Cannot subscribe bar aggregator: no aggregator found for {}",
                cmd.bar_type,
            );
            return None;
        }

        if cmd.bar_type.is_composite() {
            let composite_bar_type = cmd.bar_type.composite();
            if composite_bar_type.is_externally_aggregated() {
                let subscribe = SubscribeBars::new(
                    composite_bar_type,
                    cmd.client_id,
                    cmd.venue,
                    UUID4::new(),
                    cmd.ts_init,
                    Some(cmd.command_id),
                    cmd.params.clone(),
                );
                return Some(SubscribeCommand::Bars(subscribe));
            }
        } else if cmd.bar_type.spec().price_type == PriceType::Last {
            let subscribe = SubscribeTrades::new(
                cmd.bar_type.instrument_id(),
                cmd.client_id,
                cmd.venue,
                UUID4::new(),
                cmd.ts_init,
                Some(cmd.command_id),
                cmd.params.clone(),
            );
            return Some(SubscribeCommand::Trades(subscribe));
        } else {
            let subscribe = SubscribeQuotes::new(
                cmd.bar_type.instrument_id(),
                cmd.client_id,
                cmd.venue,
                UUID4::new(),
                cmd.ts_init,
                Some(cmd.command_id),
                cmd.params.clone(),
            );
            return Some(SubscribeCommand::Quotes(subscribe));
        }

        None
    }

    /// Sets up a bar aggregator.
    ///
    /// This method handles historical mode, message bus subscriptions, and time bar aggregator setup.
    pub(super) fn setup_bar_aggregator(
        &self,
        bar_type: BarType,
        historical: bool,
        request_id: Option<UUID4>,
    ) -> anyhow::Result<()> {
        let key = bar_aggregator_key(bar_type, request_id);

        let aggregator = self.bar_aggregators.get(&key).ok_or_else(|| {
            anyhow::anyhow!("Cannot setup bar aggregator: no aggregator found for {bar_type}")
        })?;

        // Set historical mode and handler
        let cache = self.cache.clone();
        let validate_sequence = self.config.validate_data_sequence;
        let publish = !historical;

        let handler: Box<dyn FnMut(Bar)> = Box::new(move |bar: Bar| {
            process_engine_bar(&cache, validate_sequence, publish, bar);
        });

        aggregator
            .borrow_mut()
            .set_historical_mode(historical, handler);

        // For TimeBarAggregator, set clock and start timer
        if bar_type.spec().is_time_aggregated() {
            use nautilus_common::clock::VirtualClock;

            if historical {
                // Each aggregator gets its own independent clock
                let test_clock = Rc::new(RefCell::new(VirtualClock::new()));
                aggregator.borrow_mut().set_clock(test_clock);
                // Set weak reference for historical mode (start_timer called later from preprocess_historical_events)
                // Store weak reference so start_timer can use it when called later
                let aggregator_weak = Rc::downgrade(aggregator);
                aggregator.borrow_mut().set_aggregator_weak(aggregator_weak);
            } else {
                aggregator.borrow_mut().set_clock(self.clock.clone());
                aggregator
                    .borrow_mut()
                    .start_timer(Some(aggregator.clone()));
            }
        }

        Ok(())
    }

    fn unsubscribe_bar_aggregator(&mut self, cmd: &UnsubscribeBars) {
        if cmd.bar_type.is_composite() {
            let composite_bar_type = cmd.bar_type.composite();
            if composite_bar_type.is_externally_aggregated() {
                let unsubscribe = UnsubscribeBars::new(
                    composite_bar_type,
                    cmd.client_id,
                    cmd.venue,
                    UUID4::new(),
                    cmd.ts_init,
                    Some(cmd.command_id),
                    cmd.params.clone(),
                );
                self.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Bars(
                    unsubscribe,
                )));
            }
        } else if cmd.bar_type.spec().price_type == PriceType::Last {
            let unsubscribe = UnsubscribeTrades::new(
                cmd.bar_type.instrument_id(),
                cmd.client_id,
                cmd.venue,
                UUID4::new(),
                cmd.ts_init,
                Some(cmd.command_id),
                cmd.params.clone(),
            );
            self.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Trades(
                unsubscribe,
            )));
        } else {
            let unsubscribe = UnsubscribeQuotes::new(
                cmd.bar_type.instrument_id(),
                cmd.client_id,
                cmd.venue,
                UUID4::new(),
                cmd.ts_init,
                Some(cmd.command_id),
                cmd.params.clone(),
            );
            self.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
                unsubscribe,
            )));
        }
    }

    pub(super) fn stop_bar_aggregator(
        &mut self,
        bar_type: BarType,
        request_id: Option<UUID4>,
    ) -> anyhow::Result<()> {
        let key = bar_aggregator_key(bar_type, request_id);

        let aggregator = self.bar_aggregators.shift_remove(&key).ok_or_else(|| {
            anyhow::anyhow!("Cannot stop bar aggregator: no aggregator to stop for {bar_type}")
        })?;

        aggregator.borrow_mut().stop();

        // Unsubscribe any registered message handlers
        if let Some(subs) = self.bar_aggregator_handlers.remove(&key) {
            for sub in subs {
                match sub {
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
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(super) struct BarAggregationSubscription {
    pub(super) command: SubscribeBars,
    pub(super) source: Option<SubscribeCommand>,
}

/// Identifies a bar aggregator instance.
///
/// Live subscriptions key on `(bar_type.standard(), None)`. Request-scoped
/// aggregators carrying a `request_id` key on `(bar_type.standard(), Some(id))`
/// so they can run alongside a live aggregator on the same bar type.
pub(crate) type BarAggregatorKey = (BarType, Option<UUID4>);

#[inline]
pub(crate) fn bar_aggregator_key(bar_type: BarType, request_id: Option<UUID4>) -> BarAggregatorKey {
    (bar_type.standard(), request_id)
}

/// Typed subscription for bar aggregator handlers.
///
/// Stores the topic and handler for each data type so we can properly
/// unsubscribe from the typed routers.
#[derive(Clone)]
pub enum BarAggregatorSubscription {
    Bar {
        topic: MStr<Topic>,
        handler: TypedHandler<Bar>,
    },
    Trade {
        topic: MStr<Topic>,
        handler: TypedHandler<TradeTick>,
    },
    Quote {
        topic: MStr<Topic>,
        handler: TypedHandler<QuoteTick>,
    },
}

impl Debug for BarAggregatorSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bar { topic, handler } => f
                .debug_struct(stringify!(Bar))
                .field("topic", topic)
                .field("handler_id", &handler.id())
                .finish(),
            Self::Trade { topic, handler } => f
                .debug_struct(stringify!(Trade))
                .field("topic", topic)
                .field("handler_id", &handler.id())
                .finish(),
            Self::Quote { topic, handler } => f
                .debug_struct(stringify!(Quote))
                .field("topic", topic)
                .field("handler_id", &handler.id())
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use jiff::Timestamp;
    use nautilus_common::{
        cache::Cache,
        clock::{Clock, VirtualClock},
        messages::data::SubscribeBars,
        msgbus::MessageBus,
    };
    use nautilus_core::{UUID4, UnixNanos, datetime::get_timezone};
    use nautilus_model::{
        data::{Bar, BarType},
        identifiers::TraderId,
        instruments::{InstrumentAny, stubs::audusd_sim},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::{bar_aggregator_key, request_bar_aggregation_from_params, time_zone_from_params};
    use crate::engine::{DataEngine, config::DataEngineConfig};

    #[rstest]
    #[case::utc_default(None, None, "2026-11-02T00:00:00Z")]
    #[case::config(Some("America/New_York"), None, "2026-11-02T05:00:00Z")]
    #[case::utc_override(Some("America/New_York"), Some("UTC"), "2026-11-02T00:00:00Z")]
    fn test_calendar_zone_precedence_and_reuse(
        #[case] config_zone: Option<&str>,
        #[case] override_zone: Option<&str>,
        #[case] expected: &str,
    ) {
        let _msgbus = MessageBus::new(TraderId::new("TESTER-001"), UUID4::new(), None, None)
            .register_message_bus();
        let clock = Rc::new(RefCell::new(VirtualClock::new()));
        clock.borrow_mut().set_time(UnixNanos::from(
            "2026-11-01T12:00:00Z".parse::<Timestamp>().unwrap(),
        ));
        let cache = Rc::new(RefCell::new(Cache::default()));
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CurrencyPair(audusd_sim()))
            .unwrap();
        let config = DataEngineConfig::builder()
            .maybe_time_bars_time_zone(config_zone.map(str::to_owned))
            .time_bars_skip_first_non_full_bar(true)
            .build();
        let mut engine = DataEngine::new(clock.clone(), cache.clone(), Some(config));
        let bar_type = BarType::from("AUD/USD.SIM-1-DAY-LAST-INTERNAL");
        let mut params = nautilus_core::Params::default();
        params.insert(
            "skip_first_non_full_bar".to_owned(),
            serde_json::json!(false),
        );

        if let Some(zone) = override_zone {
            params.insert("time_zone".to_owned(), serde_json::json!(zone));
        }

        let mut cmd = SubscribeBars::new(
            bar_type,
            None,
            Some(bar_type.instrument_id().venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            Some(params),
        );
        engine.subscribe_bars(&cmd).unwrap();
        engine.subscribe_bars(&cmd).unwrap();
        cmd.params
            .as_mut()
            .unwrap()
            .insert("time_zone".to_owned(), serde_json::json!("Asia/Tokyo"));
        let e = engine.subscribe_bars(&cmd).unwrap_err();

        // Request-local instances can use another zone without replacing the live instance
        engine
            .create_bar_aggregator_for_key(
                bar_type,
                Some(UUID4::new()),
                None,
                Some(get_timezone("Asia/Tokyo").unwrap()),
            )
            .unwrap();
        assert_eq!(
            e.to_string(),
            format!("Cannot reuse bar aggregator {bar_type} with a different time zone")
        );
        assert_eq!(
            clock.borrow().next_time_ns(&format!("TIME_BAR_{bar_type}")),
            Some(UnixNanos::from(expected.parse::<Timestamp>().unwrap()))
        );
        let now = clock.borrow().timestamp_ns();
        engine
            .bar_aggregators
            .get(&bar_aggregator_key(bar_type, None))
            .unwrap()
            .borrow_mut()
            .update(Price::from("0.65000"), Quantity::from(2), now);
        let close = UnixNanos::from(expected.parse::<Timestamp>().unwrap());
        let events = clock.borrow_mut().advance_time(close, true);
        let handlers = clock.borrow().match_handlers(events);
        for handler in handlers {
            handler.callback.call(handler.event);
        }

        assert_eq!(
            cache.borrow().bar(&bar_type).copied(),
            Some(Bar::new(
                bar_type,
                Price::from("0.65000"),
                Price::from("0.65000"),
                Price::from("0.65000"),
                Price::from("0.65000"),
                Quantity::from(2),
                close,
                close
            ))
        );
    }

    #[rstest]
    #[case::wrong_type(serde_json::json!(17), "`time_zone` parameter must be a string")]
    #[case::unknown_zone(serde_json::json!("Not/A_Zone"), "invalid `time_zone` parameter \"Not/A_Zone\"")]
    fn test_calendar_zone_parameter_rejects_invalid_value(
        #[case] value: serde_json::Value,
        #[case] expected: &str,
    ) {
        let mut params = nautilus_core::Params::default();
        params.insert("time_zone".to_owned(), value);
        assert_eq!(
            time_zone_from_params(Some(&params))
                .unwrap_err()
                .to_string(),
            expected
        );
    }

    #[rstest]
    fn test_calendar_request_zone_checks_shared_instance() {
        let clock = Rc::new(RefCell::new(VirtualClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CurrencyPair(audusd_sim()))
            .unwrap();
        let mut engine = DataEngine::new(clock, cache, None);
        let bar_type = BarType::from("AUD/USD.SIM-1-MONTH-LAST-INTERNAL");
        engine
            .create_bar_aggregator_for_key(bar_type, None, None, None)
            .unwrap();
        let mut params: nautilus_core::Params = serde_json::from_value(serde_json::json!({
            "bar_types": [bar_type.to_string()], "update_subscriptions": true,
            "time_zone": "America/New_York"
        }))
        .unwrap();
        let state = request_bar_aggregation_from_params(Some(&params))
            .unwrap()
            .unwrap();
        let id = UUID4::new();
        let error = engine
            .prepare_request_bar_aggregators_from_state(id, &state)
            .unwrap_err();
        params.insert("update_subscriptions".to_owned(), serde_json::json!(false));
        let state = request_bar_aggregation_from_params(Some(&params))
            .unwrap()
            .unwrap();
        engine
            .prepare_request_bar_aggregators_from_state(id, &state)
            .unwrap();
        let instance = engine
            .bar_aggregators
            .get(&bar_aggregator_key(bar_type, Some(id)))
            .unwrap()
            .borrow();
        let request_zone = instance
            .as_any()
            .downcast_ref::<crate::aggregation::TimeBarAggregator>()
            .unwrap()
            .time_zone()
            .clone();
        let live = engine
            .bar_aggregators
            .get(&bar_aggregator_key(bar_type, None))
            .unwrap()
            .borrow();
        let live_zone = live
            .as_any()
            .downcast_ref::<crate::aggregation::TimeBarAggregator>()
            .unwrap()
            .time_zone()
            .clone();
        assert_eq!(
            error.to_string(),
            format!("Cannot reuse bar aggregator {bar_type} with a different time zone")
        );
        assert_eq!(request_zone, get_timezone("America/New_York").unwrap());
        assert_eq!(live_zone, jiff::tz::TimeZone::UTC);
        assert_eq!(engine.bar_aggregators.len(), 2);
    }
}
