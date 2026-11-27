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

use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use nautilus_common::messages::{
    DataEvent, DataResponse,
    data::{
        BookResponse, CustomDataResponse, InstrumentResponse, InstrumentsResponse,
        RequestBookSnapshot, RequestCustomData, RequestInstrument, RequestInstruments,
        RequestTrades, TradesResponse,
    },
};
use nautilus_core::datetime::datetime_to_unix_nanos;
use nautilus_model::{
    data::CustomData,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

use super::{
    PolymarketDataClient,
    instruments::{
        apply_live_instrument_locked, cache_instruments_unchecked, definition_superseded,
        preserve_expiry,
    },
    runtime::is_instrument_expired_and_not_reported_open,
};
use crate::{
    common::consts::POLYMARKET_VENUE,
    filters::{market_closed, set_market_closed},
    providers::{extract_condition_id, fetch_configured_candidates, fetch_instrument_candidates},
    resolve::{
        PolymarketResolveRequestSummaryData, RESOLVE_REQUEST_TYPE_NAME, ResolveBatchErrorMode,
        ResolveRequestSummary, ResolveWatchSelectionMode, collect_resolve_watch_selection,
        fetch_and_apply_resolutions_by_condition_ids, parse_condition_ids_from_request_params,
        pause_and_reconcile_resolve_watch_entries, request_params_has_explicit_condition_selector,
    },
};

pub(super) fn request_data(client: &PolymarketDataClient, request: RequestCustomData) {
    if request.data_type.type_name() != RESOLVE_REQUEST_TYPE_NAME {
        log::debug!(
            "Ignoring unsupported custom data request type: {}",
            request.data_type.type_name()
        );
        return;
    }

    let RequestCustomData {
        data_type,
        request_id,
        client_id,
        params: request_params,
        start,
        end,
        ..
    } = request;

    let gamma_client = client.provider.http_client().clone();
    let sender = client.data_sender.clone();
    let start_nanos = datetime_to_unix_nanos(start);
    let end_nanos = datetime_to_unix_nanos(end);
    let clock = client.clock;
    let watchlist = client.resolve_poll_watchlist.clone();
    let resolve_poll_enabled = client.config.resolve_poll_enabled;
    let grace_secs = client.config.resolve_poll_grace_secs;
    let max_wait_secs = client.config.resolve_poll_max_wait_secs.max(grace_secs);
    let clob_public_client = client.clob_public_client.clone();
    let resolve_ctx = client.resolution_context();
    let future = async move {
        let mut summary = ResolveRequestSummary {
            requested_condition_ids: Vec::new(),
            fetched_markets: 0,
            resolved_markets: 0,
            skipped_non_binary_markets: 0,
            clob_fallback_successes: 0,
            emitted_condition_ids: Vec::new(),
            failed_condition_ids: Vec::new(),
            used_watchlist_fallback: false,
            timed_out_watchlist: 0,
            error: None,
        };

        let has_explicit_selector = request_params_has_explicit_condition_selector(&request_params);
        let mut condition_ids = parse_condition_ids_from_request_params(&request_params);
        if condition_ids.is_empty() {
            if has_explicit_selector {
                summary.error = Some(
                    "No valid Polymarket condition_ids could be resolved from request params"
                        .to_string(),
                );
            } else {
                summary.used_watchlist_fallback = true;
                let selection_mode = if resolve_poll_enabled {
                    ResolveWatchSelectionMode::ManualFallback
                } else {
                    ResolveWatchSelectionMode::ManualAllEligible
                };
                let selection = collect_resolve_watch_selection(
                    &watchlist.read(),
                    clock.get_time_ns(),
                    grace_secs,
                    max_wait_secs,
                    selection_mode,
                );

                pause_and_reconcile_resolve_watch_entries(
                    &resolve_ctx,
                    &selection.pause_condition_ids,
                    max_wait_secs,
                )
                .await;
                summary.timed_out_watchlist = selection.timed_out_watchlist;
                condition_ids = selection.condition_ids;
            }
        }

        summary.requested_condition_ids = condition_ids.clone();

        let stats = fetch_and_apply_resolutions_by_condition_ids(
            &gamma_client,
            &clob_public_client,
            &resolve_ctx,
            &condition_ids,
            ResolveBatchErrorMode::StopOnFirstError,
            has_explicit_selector,
            false,
        )
        .await;
        summary.fetched_markets = stats.fetched_markets;
        summary.resolved_markets = stats.resolved_markets;
        summary.skipped_non_binary_markets = stats.skipped_non_binary_markets;
        summary.clob_fallback_successes = stats.clob_fallback_successes;
        summary.emitted_condition_ids = stats.emitted_condition_ids;
        summary.failed_condition_ids = stats.failed_condition_ids;
        if summary.error.is_none() {
            summary.error = stats.error;
        }

        log::debug!(
            "Polymarket manual resolve request requested={} fetched={} resolved={} emitted={} failed={} skipped_non_binary={} clob_fallback_successes={} timed_out_watchlist={} used_watchlist_fallback={}",
            summary.requested_condition_ids.len(),
            summary.fetched_markets,
            summary.resolved_markets,
            summary.emitted_condition_ids.len(),
            summary.failed_condition_ids.len(),
            summary.skipped_non_binary_markets,
            summary.clob_fallback_successes,
            summary.timed_out_watchlist,
            summary.used_watchlist_fallback,
        );

        let ts_now = clock.get_time_ns();
        let payload = Arc::new(PolymarketResolveRequestSummaryData::from_summary(
            summary, ts_now,
        ));
        let custom = CustomData::new(payload, data_type.clone());

        let response = DataResponse::Data(CustomDataResponse::new(
            request_id,
            client_id,
            Some(*POLYMARKET_VENUE),
            data_type,
            custom,
            start_nanos,
            end_nanos,
            ts_now,
            request_params,
        ));

        if let Err(e) = sender.send(DataEvent::Response(response)) {
            log::error!("Failed to send resolve custom data response: {e}");
        }
    };

    if let Err(e) = client.tasks.spawn(future) {
        log::debug!("Skipping Polymarket data task after shutdown began: {e}");
    }
}

pub(super) fn request_instruments(client: &PolymarketDataClient, request: RequestInstruments) {
    let Ok(recovery) = client.tick_recovery() else {
        return;
    };
    let sender = client.data_sender.clone();
    let http = client.provider.http_client().clone();
    let filters = client.provider.filters();
    let instrument_config = client.provider.config().clone();
    let instruments_cache = client.instruments.clone();
    let instrument_update_state = client.instrument_update_state.clone();
    let token_meta = client.subscriptions.token_meta.clone();
    let closed_condition_ids = client.subscriptions.closed_condition_ids.clone();
    let request_id = request.request_id;
    let client_id = request.client_id.unwrap_or(client.client_id);
    let venue = *POLYMARKET_VENUE;
    let publish = request.start.is_none() && request.end.is_none();
    let start_nanos = datetime_to_unix_nanos(request.start);
    let end_nanos = datetime_to_unix_nanos(request.end);
    let params = request.params;
    let clock = client.clock;

    let future = async move {
        let snapshot = Arc::clone(&instruments_cache.load());

        let instruments = if instrument_config.should_load_all() || instrument_config.has_load_ids()
        {
            fetch_configured_candidates(&http, &instrument_config, &filters).await
        } else {
            fetch_instrument_candidates(&http, &filters)
                .await
                .map(|rows| (rows, vec![]))
        };

        let (candidates, explicit) = match instruments {
            Ok(instruments) => instruments,
            Err(e) => {
                log::error!("Failed to fetch Polymarket instruments: {e}");
                return;
            }
        };

        if publish {
            for instrument in candidates.iter().chain(&explicit) {
                recovery.observe(instrument).await;
            }
        }

        // Predicates are user code and can inspect the cache. Retry selection if
        // a definition, terminal condition or WebSocket tick changed meanwhile.
        let (update_state, terminal, cached, effective_instruments) = loop {
            let state = instrument_update_state.lock();
            let revision = state.tick_revision;
            let cached = Arc::clone(&instruments_cache.load());
            let closed = closed_condition_ids.lock().clone();
            let compose = |instrument: &InstrumentAny| {
                let instrument =
                    instrument_response_definition(instrument.clone(), &snapshot, &cached, &closed);
                state
                    .compose_instrument(&instrument, cached.get(&instrument.id()))
                    .inspect_err(|e| {
                        log::error!("Failed to compose instrument {}: {e}", instrument.id());
                    })
                    .ok()
            };
            let mut effective: Vec<_> = candidates.iter().filter_map(compose).collect();
            let scoped: Vec<_> = explicit.iter().filter_map(compose).collect();
            drop(state);

            effective.retain(|inst| filters.iter().all(|f| f.accept(inst)));
            effective.extend(scoped);
            let mut seen = AHashSet::new();
            effective.retain(|inst| seen.insert(inst.id()));

            let state = instrument_update_state.lock();
            let terminal = closed_condition_ids.lock();

            if state.tick_revision == revision
                && Arc::ptr_eq(&cached, &instruments_cache.load())
                && *terminal == closed
            {
                break (state, terminal, cached, effective);
            }
        };

        let now_ns = clock.get_time_ns();
        let cached_count = if !publish || update_state.retired {
            0
        } else {
            let live: Vec<_> = effective_instruments
                .iter()
                .filter(|instrument| {
                    !is_instrument_expired_and_not_reported_open(instrument, now_ns)
                        && !extract_condition_id(&instrument.id())
                            .is_ok_and(|condition_id| terminal.contains(&condition_id))
                })
                .cloned()
                .collect();

            cache_instruments_unchecked(&instruments_cache, &token_meta, &live);
            for instrument in &live {
                if cached
                    .get(&instrument.id())
                    .is_none_or(|previous| !previous.same_definition(instrument))
                    && let Err(e) = sender.send(DataEvent::Data(instrument.clone().into()))
                {
                    log::warn!("Failed to publish instrument {}: {e}", instrument.id());
                }
            }

            live.len()
        };

        let skipped_count = effective_instruments.len() - cached_count;
        if skipped_count > 0 {
            log::debug!(
                "Skipped {skipped_count} instruments during request_instruments cache update"
            );
        }

        let response = DataResponse::Instruments(InstrumentsResponse::new(
            request_id,
            client_id,
            venue,
            effective_instruments,
            start_nanos,
            end_nanos,
            clock.get_time_ns(),
            params,
        ));

        if let Err(e) = sender.send(DataEvent::Response(response)) {
            log::error!("Failed to send instruments response: {e}");
        }
    };

    if let Err(e) = client.tasks.spawn(future) {
        log::debug!("Skipping Polymarket data task after shutdown began: {e}");
    }
}

pub(super) fn request_instrument(
    client: &PolymarketDataClient,
    request: RequestInstrument,
) -> anyhow::Result<()> {
    let instrument_id = request.single_instrument_id()?;
    let recovery = client.tick_recovery()?;
    let http = client.provider.http_client().clone();
    let sender = client.data_sender.clone();
    let instruments_cache = client.instruments.clone();
    let instrument_update_state = client.instrument_update_state.clone();
    let token_meta = client.subscriptions.token_meta.clone();
    let closed_condition_ids = client.subscriptions.closed_condition_ids.clone();
    let client_id = request.client_id.unwrap_or(client.client_id);
    let request_id = request.request_id;
    let start = request.start;
    let end = request.end;
    let publish = start.is_none() && end.is_none();
    let params = request.params;
    let clock = client.clock;
    let future = async move {
        let condition_id = match extract_condition_id(&instrument_id) {
            Ok(cid) => cid,
            Err(e) => {
                log::error!("Failed to extract condition_id for {instrument_id}: {e}");
                return;
            }
        };

        let query_params = crate::http::query::GetGammaMarketsParams {
            condition_ids: Some(vec![condition_id]),
            ..Default::default()
        };

        let snapshot = Arc::clone(&instruments_cache.load());

        let instrument = match http.request_instruments_by_params(query_params).await {
            Ok(instruments) => instruments.into_iter().find(|i| i.id() == instrument_id),
            Err(e) => {
                log::error!("Failed to fetch instrument {instrument_id} from Gamma API: {e}");
                return;
            }
        };

        if let Some(inst) = instrument {
            if publish {
                recovery.observe(&inst).await;
            }
            let update_state = instrument_update_state.lock();
            let cached = instruments_cache.load();
            let inst = instrument_response_definition(
                inst,
                &snapshot,
                &cached,
                &closed_condition_ids.lock(),
            );

            let inst = match update_state.compose_instrument(&inst, cached.get(&instrument_id)) {
                Ok(instrument) => instrument,
                Err(e) => {
                    log::error!("Failed to apply live tick to instrument {instrument_id}: {e}");
                    return;
                }
            };

            if publish && is_instrument_expired_and_not_reported_open(&inst, clock.get_time_ns()) {
                log::debug!(
                    "Skipping expired instrument {instrument_id} during request_instrument cache update"
                );
            } else if publish {
                apply_live_instrument_locked(
                    &closed_condition_ids,
                    &update_state,
                    &instruments_cache,
                    &token_meta,
                    &inst,
                    |instrument| {
                        // Publish onto the data bus so other clients (e.g. the exec
                        // client's token map) can update from the same fetch.
                        if let Err(e) = sender.send(DataEvent::Data(instrument.clone().into())) {
                            log::warn!("Failed to publish instrument {instrument_id}: {e}");
                        }
                    },
                );
            }

            let terminal = closed_condition_ids.lock();
            let inst = instrument_response_definition(
                inst,
                &snapshot,
                &instruments_cache.load(),
                &terminal,
            );
            let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                request_id,
                client_id,
                instrument_id,
                inst,
                datetime_to_unix_nanos(start),
                datetime_to_unix_nanos(end),
                clock.get_time_ns(),
                params,
            )));

            if let Err(e) = sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send instrument response: {e}");
            }
        } else {
            log::error!("Instrument {instrument_id} not found on Polymarket");
        }
    };

    if let Err(e) = client.tasks.spawn(future) {
        log::debug!("Skipping Polymarket data task after shutdown began: {e}");
    }

    Ok(())
}

pub(super) fn request_book_snapshot(
    client: &PolymarketDataClient,
    request: RequestBookSnapshot,
) -> anyhow::Result<()> {
    let instrument_id = request.single_instrument_id()?;
    let instrument = client.ensure_market_data_request_allowed(instrument_id)?;

    let token_id = instrument.raw_symbol().as_str().to_string();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let clob_client = client.clob_public_client.clone();
    let sender = client.data_sender.clone();
    let client_id = request.client_id.unwrap_or(client.client_id);
    let request_id = request.request_id;
    let params = request.params;
    let clock = client.clock;
    let future = async move {
        match clob_client
            .request_book_snapshot(instrument_id, &token_id, price_precision, size_precision)
            .await
            .context("failed to request book snapshot from Polymarket")
        {
            Ok(book) => {
                let response = DataResponse::Book(BookResponse::new(
                    request_id,
                    client_id,
                    instrument_id,
                    book,
                    None,
                    None,
                    clock.get_time_ns(),
                    params,
                ));

                if let Err(e) = sender.send(DataEvent::Response(response)) {
                    log::error!("Failed to send book snapshot response: {e}");
                }
            }
            Err(e) => log::error!("Book snapshot request failed: {e:?}"),
        }
    };

    if let Err(e) = client.tasks.spawn(future) {
        log::debug!("Skipping Polymarket data task after shutdown began: {e}");
    }

    Ok(())
}

pub(super) fn request_trades(
    client: &PolymarketDataClient,
    request: RequestTrades,
) -> anyhow::Result<()> {
    let instrument_id = request.single_instrument_id()?;
    let instrument = client.ensure_market_data_request_allowed(instrument_id)?;

    let condition_id = extract_condition_id(&instrument_id)?;
    let token_id = instrument.raw_symbol().as_str().to_string();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let limit = request.limit.map(|n| n.get() as u32);

    let data_api_client = client.data_api_client.clone();
    let sender = client.data_sender.clone();
    let client_id = request.client_id.unwrap_or(client.client_id);
    let request_id = request.request_id;
    let params = request.params;
    let clock = client.clock;
    let start_nanos = datetime_to_unix_nanos(request.start);
    let end_nanos = datetime_to_unix_nanos(request.end);
    let future = async move {
        match data_api_client
            .request_trades(
                instrument_id,
                &condition_id,
                &token_id,
                price_precision,
                size_precision,
                start_nanos,
                end_nanos,
                limit,
            )
            .await
            .context("failed to request trades from Polymarket Data API")
        {
            Ok(trades) => {
                let response = DataResponse::Trades(TradesResponse::new(
                    request_id,
                    client_id,
                    instrument_id,
                    trades,
                    start_nanos,
                    end_nanos,
                    clock.get_time_ns(),
                    params,
                ));

                if let Err(e) = sender.send(DataEvent::Response(response)) {
                    log::error!("Failed to send trades response: {e}");
                }
            }
            Err(e) => log::error!("Trade request failed for {instrument_id}: {e:?}"),
        }
    };

    if let Err(e) = client.tasks.spawn(future) {
        log::debug!("Skipping Polymarket data task after shutdown began: {e}");
    }

    Ok(())
}

fn instrument_response_definition(
    mut incoming: InstrumentAny,
    snapshot: &AHashMap<InstrumentId, InstrumentAny>,
    cached: &AHashMap<InstrumentId, InstrumentAny>,
    terminal: &AHashSet<String>,
) -> InstrumentAny {
    let id = incoming.id();
    let known_closed = extract_condition_id(&id).is_ok_and(|id| terminal.contains(&id))
        || cached.get(&id).and_then(market_closed) == Some(true);

    if ((known_closed && market_closed(&incoming) != Some(true))
        || definition_superseded(snapshot, cached, id))
        && let Some(current) = cached.get(&id)
    {
        incoming = current.clone();
    }

    if known_closed && let InstrumentAny::BinaryOption(binary) = &mut incoming {
        set_market_closed(binary, true);
        if binary.expiration_ns.as_u64() == 0
            && let Some(current) = cached.get(&id)
        {
            preserve_expiry(binary, current);
        }
    }

    incoming
}

#[cfg(test)]
mod tests {
    use nautilus_core::{Params, UnixNanos};
    use nautilus_model::instruments::stubs::binary_option;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::stale_open(false, 200, 100)]
    #[case::confirmed_closed(true, 200, 200)]
    #[case::closed_without_expiry(true, 0, 100)]
    fn instrument_request_uses_fresh_confirmed_closed_metadata(
        #[case] closed: bool,
        #[case] incoming_expiry: u64,
        #[case] expected_expiry: u64,
    ) {
        let mut definition = binary_option();
        definition.id = InstrumentId::from("0xCOND-101.POLYMARKET");
        definition.expiration_ns = UnixNanos::from(100);
        definition.info = Some(Params::new(
            [(
                "end_date".into(),
                definition.expiration_ns.to_rfc3339().into(),
            )]
            .into(),
        ));
        set_market_closed(&mut definition, true);
        let cached = AHashMap::from([(
            definition.id,
            InstrumentAny::BinaryOption(definition.clone()),
        )]);
        let terminal = AHashSet::from([extract_condition_id(&definition.id).unwrap()]);
        definition.expiration_ns = UnixNanos::from(incoming_expiry);
        definition.info = (incoming_expiry != 0).then(|| {
            Params::new(
                [(
                    "end_date".into(),
                    definition.expiration_ns.to_rfc3339().into(),
                )]
                .into(),
            )
        });

        set_market_closed(&mut definition, closed);

        let response = instrument_response_definition(
            InstrumentAny::BinaryOption(definition),
            &cached,
            &cached,
            &terminal,
        );

        let expected_expiry = UnixNanos::from(expected_expiry);
        assert_eq!(response.expiration_ns(), Some(expected_expiry));
        assert_eq!(
            response.info().unwrap().get_str("end_date"),
            Some(expected_expiry.to_rfc3339().as_str())
        );
        assert_eq!(market_closed(&response), Some(true));
    }
}
