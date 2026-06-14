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

use anyhow::Context;
use nautilus_common::{
    live::get_runtime,
    messages::{
        DataEvent, DataResponse,
        data::{
            BookResponse, CustomDataResponse, InstrumentResponse, InstrumentsResponse,
            RequestBookSnapshot, RequestCustomData, RequestInstrument, RequestInstruments,
            RequestTrades, TradesResponse,
        },
    },
};
use nautilus_core::datetime::datetime_to_unix_nanos;
use nautilus_model::{data::CustomData, instruments::Instrument};

use super::{PolymarketDataClient, dispatch::WsMessageContext, instruments::cache_instrument};
use crate::{
    common::consts::POLYMARKET_VENUE,
    data_runtime::is_instrument_expired,
    providers::extract_condition_id,
    resolve::{
        PolymarketResolveRequestSummaryData, RESOLVE_REQUEST_TYPE_NAME, ResolveBatchErrorMode,
        ResolveRequestSummary, ResolveWatchSelectionMode, collect_resolve_watch_selection,
        fetch_and_apply_resolutions_by_condition_ids, parse_condition_ids_from_request_params,
        pause_resolve_watch_entries, request_params_has_explicit_condition_selector,
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
    let ctx = WsMessageContext {
        clock: client.clock,
        data_sender: client.data_sender.clone(),
        token_meta: client.token_meta.clone(),
        instruments: client.instruments.clone(),
        gamma_client: client.provider.http_client().clone(),
        clob_public_client: client.clob_public_client.clone(),
        filters: client.provider.filters(),
        order_books: client.order_books.clone(),
        last_quotes: client.last_quotes.clone(),
        active_quote_subs: client.active_quote_subs.clone(),
        active_delta_subs: client.active_delta_subs.clone(),
        active_trade_subs: client.active_trade_subs.clone(),
        resolve_poll_watchlist: client.resolve_poll_watchlist.clone(),
        resolve_watch_apply_mutex: client.resolve_watch_apply_mutex.clone(),
        pending_snapshot_after_tick_change: client.pending_snapshot_after_tick_change.clone(),
        new_market_inflight_keys: client.new_market_inflight_keys.clone(),
        new_market_fetch_semaphore: client.new_market_fetch_semaphore.clone(),
        subscribe_new_markets: client.config.subscribe_new_markets,
        new_market_filter: client.config.new_market_filter.clone(),
        cancellation_token: client.cancellation_token.clone(),
    };

    get_runtime().spawn(async move {
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

        let has_explicit_selector =
            request_params_has_explicit_condition_selector(&request_params);
        let mut condition_ids = parse_condition_ids_from_request_params(&request_params);
        if condition_ids.is_empty() {
            if has_explicit_selector {
                summary.error = Some(
                    "No valid Polymarket condition_ids could be resolved from request params"
                        .to_string(),
                );
            } else {
                summary.used_watchlist_fallback = true;
                let snapshot = watchlist.load();
                let selection_mode = if resolve_poll_enabled {
                    ResolveWatchSelectionMode::ManualFallback
                } else {
                    ResolveWatchSelectionMode::ManualAllEligible
                };
                let selection = collect_resolve_watch_selection(
                    &snapshot,
                    clock.get_time_ns(),
                    grace_secs,
                    max_wait_secs,
                    selection_mode,
                );
                drop(snapshot);

                pause_resolve_watch_entries(&watchlist, &selection.pause_condition_ids);
                summary.timed_out_watchlist = selection.timed_out_watchlist;
                condition_ids = selection.condition_ids;
            }
        }

        summary.requested_condition_ids = condition_ids.clone();

        let stats = fetch_and_apply_resolutions_by_condition_ids(
            &gamma_client,
            &ctx.clob_public_client,
            &ctx.resolve_context(),
            &condition_ids,
            ResolveBatchErrorMode::StopOnFirstError,
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

        log::info!(
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
    });
}

pub(super) fn request_instruments(client: &PolymarketDataClient, request: RequestInstruments) {
    let sender = client.data_sender.clone();
    let http = client.provider.http_client().clone();
    let filters = client.provider.filters();
    let instrument_config = client.provider.config().clone();
    let instruments_cache = client.instruments.clone();
    let token_meta = client.token_meta.clone();
    let request_id = request.request_id;
    let client_id = request.client_id.unwrap_or(client.client_id);
    let venue = *POLYMARKET_VENUE;
    let start_nanos = datetime_to_unix_nanos(request.start);
    let end_nanos = datetime_to_unix_nanos(request.end);
    let params = request.params;
    let clock = client.clock;

    get_runtime().spawn(async move {
        let instruments = if instrument_config.should_load_all() || instrument_config.has_load_ids()
        {
            crate::providers::fetch_configured_instruments(&http, &instrument_config, &filters)
                .await
        } else {
            crate::providers::fetch_instruments(&http, &filters).await
        };

        let instruments = match instruments {
            Ok(instruments) => instruments,
            Err(e) => {
                log::error!("Failed to fetch Polymarket instruments: {e}");
                return;
            }
        };

        let now_ns = clock.get_time_ns();
        for instrument in &instruments {
            if is_instrument_expired(instrument, now_ns) {
                log::debug!(
                    "Skipping expired instrument {} during request_instruments cache update",
                    instrument.id()
                );
                continue;
            }

            cache_instrument(&instruments_cache, &token_meta, instrument);
        }

        let response = DataResponse::Instruments(InstrumentsResponse::new(
            request_id,
            client_id,
            venue,
            instruments,
            start_nanos,
            end_nanos,
            clock.get_time_ns(),
            params,
        ));

        if let Err(e) = sender.send(DataEvent::Response(response)) {
            log::error!("Failed to send instruments response: {e}");
        }
    });
}

pub(super) fn request_instrument(client: &PolymarketDataClient, request: RequestInstrument) {
    let instrument_id = request.instrument_id;
    let http = client.provider.http_client().clone();
    let sender = client.data_sender.clone();
    let instruments_cache = client.instruments.clone();
    let token_meta = client.token_meta.clone();
    let client_id = request.client_id.unwrap_or(client.client_id);
    let request_id = request.request_id;
    let start = request.start;
    let end = request.end;
    let params = request.params;
    let clock = client.clock;

    get_runtime().spawn(async move {
        let condition_id = match extract_condition_id(&instrument_id) {
            Ok(cid) => cid,
            Err(e) => {
                log::error!("Failed to extract condition_id for {instrument_id}: {e}");
                return;
            }
        };

        let query_params = crate::http::query::GetGammaMarketsParams {
            condition_ids: Some(condition_id),
            ..Default::default()
        };

        let instrument = match http.request_instruments_by_params(query_params).await {
            Ok(instruments) => instruments.into_iter().find(|i| i.id() == instrument_id),
            Err(e) => {
                log::error!("Failed to fetch instrument {instrument_id} from Gamma API: {e}");
                return;
            }
        };

        if let Some(inst) = instrument {
            if is_instrument_expired(&inst, clock.get_time_ns()) {
                log::debug!(
                    "Skipping expired instrument {instrument_id} during request_instrument cache update"
                );
            } else {
                cache_instrument(&instruments_cache, &token_meta, &inst);

                // Publish onto the data bus so other clients (e.g. the exec
                // client's token map) can update from the same fetch.
                if let Err(e) = sender.send(DataEvent::Instrument(inst.clone())) {
                    log::warn!("Failed to publish instrument {instrument_id}: {e}");
                }
            }

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
    });
}

pub(super) fn request_book_snapshot(
    client: &PolymarketDataClient,
    request: RequestBookSnapshot,
) -> anyhow::Result<()> {
    let instrument_id = request.instrument_id;
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

    get_runtime().spawn(async move {
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
    });

    Ok(())
}

pub(super) fn request_trades(
    client: &PolymarketDataClient,
    request: RequestTrades,
) -> anyhow::Result<()> {
    let instrument_id = request.instrument_id;
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

    get_runtime().spawn(async move {
        match data_api_client
            .request_trade_ticks(
                instrument_id,
                &condition_id,
                &token_id,
                price_precision,
                size_precision,
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
    });

    Ok(())
}
