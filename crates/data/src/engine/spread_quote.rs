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
    BAR_AGGREGATOR_PRIORITY, DataCommand, DataEngine, Debug, GENERIC_SPREAD_ID_SEPARATOR,
    Instrument, InstrumentAny, InstrumentId, Params, QuoteTick, Rc, RefCell, SpreadQuoteAggregator,
    SpreadQuoteHandler, SubscribeCommand, SubscribeQuotes, TypedHandler, UUID4, UnsubscribeCommand,
    UnsubscribeQuotes, log_error_on_cache_insert, msgbus, parse_generic_spread_id_legs,
    switchboard,
};

impl DataEngine {
    pub(super) fn is_spread_quote_command(
        &self,
        instrument_id: InstrumentId,
        params: Option<&Params>,
    ) -> bool {
        if !params
            .and_then(|params| params.get_bool("aggregate_spread_quotes"))
            .unwrap_or(false)
        {
            return false;
        }

        self.cache
            .borrow()
            .instrument(&instrument_id)
            .is_some_and(InstrumentAny::is_spread)
    }

    pub(super) fn subscribe_spread_quotes(&mut self, cmd: &SubscribeQuotes) {
        if let Some(state) = self.spread_quote_states.get_mut(&cmd.instrument_id) {
            state.owners += 1;
            let sources = state.sources.clone();
            for source in sources {
                self.execute(DataCommand::Subscribe(source));
            }

            return;
        }

        let Some(instrument) = self.cache.borrow().instrument(&cmd.instrument_id).cloned() else {
            log::error!(
                "Cannot create spread quote aggregator: no instrument found for {}",
                cmd.instrument_id,
            );
            return;
        };

        let Some(legs) = spread_instrument_legs(&instrument) else {
            log::error!(
                "Cannot create spread quote aggregator: invalid spread legs for {}",
                cmd.instrument_id,
            );
            return;
        };

        if legs.len() <= 1 {
            log::error!(
                "Cannot create spread quote aggregator: spread instrument {} should have more than one leg",
                cmd.instrument_id,
            );
            return;
        }

        let cache = self.cache.clone();

        let handler = Box::new(move |quote: QuoteTick| {
            let exchange_endpoint = format!(
                "SimulatedExchange.process_new_quote.{}",
                quote.instrument_id.venue
            );
            let exchange_endpoint = exchange_endpoint.into();
            if msgbus::has_quote_endpoint(exchange_endpoint) {
                msgbus::send_quote(exchange_endpoint, &quote);
            }

            if let Err(e) = cache.borrow_mut().add_quote(quote) {
                log_error_on_cache_insert(&e);
            }

            let topic = switchboard::get_quotes_topic(quote.instrument_id);
            msgbus::publish_quote(topic, &quote);
        });

        let aggregator = Rc::new(RefCell::new(SpreadQuoteAggregator::new(
            cmd.instrument_id,
            &legs,
            matches!(
                instrument,
                InstrumentAny::FuturesSpread(_) | InstrumentAny::CryptoFuturesSpread(_)
            ),
            instrument.price_precision(),
            instrument.size_precision(),
            handler,
            self.clock.clone(),
            false,
            spread_quote_update_interval_seconds(cmd.params.as_ref()),
            cmd.params
                .as_ref()
                .and_then(|params| params.get_u64("quote_build_delay"))
                .unwrap_or(0),
            cmd.params
                .as_ref()
                .and_then(|params| params.get_bool("disable_vega_pricing"))
                .unwrap_or(false),
            cmd.params
                .as_ref()
                .and_then(|params| params.get_u64("vega_pricing_timeout_seconds"))
                .unwrap_or(60),
            None,
            None,
        )));

        let mut handlers = Vec::with_capacity(legs.len());
        for (leg_id, _) in &legs {
            let topic = switchboard::get_quotes_topic(*leg_id);

            let handler = TypedHandler::new(SpreadQuoteHandler::new(
                &aggregator,
                cmd.instrument_id,
                *leg_id,
            ));
            msgbus::subscribe_quotes(topic.into(), handler.clone(), Some(BAR_AGGREGATOR_PRIORITY));
            handlers.push((*leg_id, handler));
        }

        aggregator
            .borrow_mut()
            .start_timer(Some(aggregator.clone()));
        aggregator.borrow_mut().set_running(true);

        let source_commands = legs
            .into_iter()
            .map(|(leg_id, _)| {
                SubscribeCommand::Quotes(SubscribeQuotes::new(
                    leg_id,
                    cmd.client_id,
                    cmd.venue,
                    UUID4::new(),
                    cmd.ts_init,
                    Some(cmd.command_id),
                    cmd.params.clone(),
                ))
            })
            .collect::<Vec<_>>();

        self.spread_quote_states.insert(
            cmd.instrument_id,
            SpreadQuoteState {
                aggregator,
                handlers,
                owners: 1,
                command: cmd.clone(),
                sources: source_commands.clone(),
            },
        );

        for source_command in source_commands {
            self.execute(DataCommand::Subscribe(source_command));
        }
    }

    pub(super) fn unsubscribe_spread_quotes(&mut self, cmd: &UnsubscribeQuotes) {
        let Some(state) = self.spread_quote_states.get_mut(&cmd.instrument_id) else {
            log::warn!(
                "Cannot unsubscribe spread quotes for {}: not subscribed",
                cmd.instrument_id,
            );
            return;
        };

        if state.owners > 1 {
            state.owners -= 1;
            return;
        }

        let Some((subscribe, leg_ids)) = self.stop_spread_quote_aggregation(cmd.instrument_id)
        else {
            return;
        };

        for leg_id in leg_ids {
            let unsubscribe = UnsubscribeQuotes::new(
                leg_id,
                subscribe.client_id,
                subscribe.venue,
                UUID4::new(),
                cmd.ts_init,
                Some(subscribe.command_id),
                subscribe.params.clone(),
            );
            self.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
                unsubscribe,
            )));
        }
    }

    pub(super) fn stop_spread_quote_aggregation(
        &mut self,
        spread_instrument_id: InstrumentId,
    ) -> Option<(SubscribeQuotes, Vec<InstrumentId>)> {
        let Some(state) = self.spread_quote_states.remove(&spread_instrument_id) else {
            log::warn!("Cannot stop spread quote aggregation: no state for {spread_instrument_id}");
            return None;
        };

        state.aggregator.borrow_mut().stop_timer();
        state.aggregator.borrow_mut().set_running(false);

        let mut leg_ids = Vec::with_capacity(state.handlers.len());
        for (leg_id, handler) in state.handlers {
            let topic = switchboard::get_quotes_topic(leg_id);
            msgbus::unsubscribe_quotes(topic.into(), &handler);
            leg_ids.push(leg_id);
        }

        Some((state.command, leg_ids))
    }
}

fn spread_quote_update_interval_seconds(params: Option<&Params>) -> Option<u64> {
    match params.and_then(|params| params.get("update_interval_seconds")) {
        Some(value) if value.is_null() => None,
        Some(value) => value.as_u64().filter(|interval| *interval > 0),
        None => Some(1),
    }
}

fn spread_instrument_legs(instrument: &InstrumentAny) -> Option<Vec<(InstrumentId, i64)>> {
    if !instrument.is_spread() {
        return None;
    }

    let instrument_id = instrument.id();
    let symbol = instrument_id.symbol.as_str();
    if !symbol.contains(GENERIC_SPREAD_ID_SEPARATOR) {
        return Some(vec![(instrument_id, 1)]);
    }

    parse_generic_spread_id_legs(&instrument_id).ok()
}

#[derive(Debug)]
pub(super) struct SpreadQuoteState {
    pub(super) aggregator: Rc<RefCell<SpreadQuoteAggregator>>,
    handlers: Vec<(InstrumentId, TypedHandler<QuoteTick>)>,
    owners: usize,
    command: SubscribeQuotes,
    sources: Vec<SubscribeCommand>,
}
