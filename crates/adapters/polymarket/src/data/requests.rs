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

use std::sync::{Arc, Mutex as StdMutex};

use anyhow::Context;
use dashmap::{DashMap, mapref::entry::Entry};
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
use nautilus_core::{UUID4, UnixNanos, datetime::datetime_to_unix_nanos};
use nautilus_model::{
    data::{CustomData, Data as NautilusData, OrderBookDeltas, OrderBookDeltas_API},
    identifiers::InstrumentId,
    instruments::Instrument,
    orderbook::OrderBook,
};

use super::{
    LIVE_BOOK_RESYNC_PARAM, LiveBookResync, LiveBookResyncPhase, PendingBookSnapshotResponse,
    PolymarketDataClient, dispatch::WsMessageContext, instruments::cache_instrument_if_active,
};
use crate::{
    common::consts::POLYMARKET_VENUE,
    providers::extract_condition_id,
    resolve::{
        PolymarketResolveRequestSummaryData, RESOLVE_REQUEST_TYPE_NAME, ResolveBatchErrorMode,
        ResolveRequestSummary, ResolveWatchSelectionMode, collect_resolve_watch_selection,
        fetch_and_apply_resolutions_by_condition_ids, parse_condition_ids_from_request_params,
        pause_resolve_watch_entries, request_params_has_explicit_condition_selector,
    },
};

fn replay_buffered_book_deltas(
    order_books: &DashMap<InstrumentId, OrderBook>,
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instrument_id: InstrumentId,
    buffered_deltas: impl IntoIterator<Item = OrderBookDeltas>,
    after: Option<UnixNanos>,
) {
    for deltas in buffered_deltas {
        if after.is_some_and(|snapshot_ts| deltas.ts_event <= snapshot_ts) {
            continue;
        }

        if let Some(mut live_book) = order_books.get_mut(&instrument_id)
            && let Err(e) = live_book.apply_deltas(&deltas)
        {
            log::error!("Failed to replay buffered book deltas for {instrument_id}: {e}");
            continue;
        }

        let data: NautilusData = OrderBookDeltas_API::new(deltas).into();
        if let Err(e) = sender.send(DataEvent::Data(data)) {
            log::error!("Failed to emit buffered book deltas: {e}");
        }
    }
}

fn close_live_book_resync_admission(
    live_book_resyncs: &DashMap<InstrumentId, LiveBookResync>,
    instrument_id: InstrumentId,
    generation: UUID4,
) -> Option<Vec<PendingBookSnapshotResponse>> {
    let mut resync = live_book_resyncs.get_mut(&instrument_id)?;
    if resync.generation != generation || resync.phase != LiveBookResyncPhase::Buffering {
        return None;
    }

    resync.phase = LiveBookResyncPhase::Completing;
    let responses = std::mem::take(
        &mut *resync
            .pending_responses
            .lock()
            .expect("live book resync response mutex poisoned"),
    );
    Some(responses)
}

fn join_or_start_live_book_resync(
    live_book_resyncs: &DashMap<InstrumentId, LiveBookResync>,
    instrument_id: InstrumentId,
    generation: UUID4,
    pending_response: PendingBookSnapshotResponse,
    pending_responses: Arc<StdMutex<Vec<PendingBookSnapshotResponse>>>,
) -> bool {
    match live_book_resyncs.entry(instrument_id) {
        Entry::Occupied(mut entry) => {
            if entry.get().phase == LiveBookResyncPhase::Buffering {
                entry
                    .get_mut()
                    .pending_responses
                    .lock()
                    .expect("live book resync response mutex poisoned")
                    .push(pending_response);
                return true;
            }

            let buffered_deltas = if entry.get().phase == LiveBookResyncPhase::Completing {
                std::mem::take(&mut entry.get_mut().buffered_deltas)
            } else {
                Vec::new()
            };
            entry.insert(LiveBookResync {
                generation,
                pending_responses,
                buffered_deltas,
                phase: LiveBookResyncPhase::Buffering,
            });
        }
        Entry::Vacant(entry) => {
            entry.insert(LiveBookResync {
                generation,
                pending_responses,
                buffered_deltas: Vec::new(),
                phase: LiveBookResyncPhase::Buffering,
            });
        }
    }
    false
}

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
        live_book_resyncs: client.live_book_resyncs.clone(),
        new_market_inflight_keys: client.new_market_inflight_keys.clone(),
        new_market_fetch_semaphore: client.new_market_fetch_semaphore.clone(),
        rtds_feed: client.rtds_feed.clone(),
        subscribe_new_markets: client.config.subscribe_new_markets,
        drop_quotes_missing_side: client.config.drop_quotes_missing_side,
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

        for instrument in &instruments {
            if !cache_instrument_if_active(
                clock.get_time_ns(),
                &instruments_cache,
                &token_meta,
                instrument,
            ) {
                log::debug!(
                    "Skipping expired instrument {} during request_instruments cache update",
                    instrument.id()
                );
            }
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
            condition_ids: Some(vec![condition_id]),
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
            if cache_instrument_if_active(clock.get_time_ns(), &instruments_cache, &token_meta, &inst)
            {
                // Publish onto the data bus so other clients (e.g. the exec
                // client's token map) can update from the same fetch.
                if let Err(e) = sender.send(DataEvent::Instrument(inst.clone())) {
                    log::warn!("Failed to publish instrument {instrument_id}: {e}");
                }
            } else {
                log::debug!(
                    "Skipping expired instrument {instrument_id} during request_instrument cache update"
                );
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
    let pending_response = PendingBookSnapshotResponse {
        request_id,
        client_id,
        params: params.clone(),
    };
    let pending_responses = Arc::new(StdMutex::new(vec![pending_response.clone()]));
    let resync_live_book = params
        .as_ref()
        .and_then(|params| params.get_bool(LIVE_BOOK_RESYNC_PARAM))
        .unwrap_or(false);

    if resync_live_book
        && join_or_start_live_book_resync(
            &client.live_book_resyncs,
            instrument_id,
            request_id,
            pending_response,
            pending_responses.clone(),
        )
    {
        return Ok(());
    }

    let live_book_resyncs = client.live_book_resyncs.clone();
    let order_books = client.order_books.clone();
    let active_delta_subs = client.active_delta_subs.clone();
    let pending_snapshot_after_tick_change = client.pending_snapshot_after_tick_change.clone();

    get_runtime().spawn(async move {
        match clob_client
            .request_book_snapshot(instrument_id, &token_id, price_precision, size_precision)
            .await
            .context("failed to request book snapshot from Polymarket")
        {
            Ok(book) => {
                let pending_responses = if resync_live_book {
                    close_live_book_resync_admission(&live_book_resyncs, instrument_id, request_id)
                        .unwrap_or_else(|| {
                            std::mem::take(
                                &mut *pending_responses
                                    .lock()
                                    .expect("pending response mutex poisoned"),
                            )
                        })
                } else {
                    std::mem::take(
                        &mut *pending_responses
                            .lock()
                            .expect("pending response mutex poisoned"),
                    )
                };
                for pending in pending_responses {
                    let response = DataResponse::Book(BookResponse::new(
                        pending.request_id,
                        pending.client_id,
                        instrument_id,
                        book.clone(),
                        None,
                        None,
                        clock.get_time_ns(),
                        pending.params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send book snapshot response: {e}");
                    }
                }

                if resync_live_book
                    && let Some(mut resync) = live_book_resyncs.get_mut(&instrument_id)
                    && resync.generation == request_id
                {
                    if resync.phase == LiveBookResyncPhase::Completing
                        && active_delta_subs.contains(&instrument_id)
                        && !pending_snapshot_after_tick_change.contains(&instrument_id)
                    {
                        let snapshot_ts = book.ts_last;
                        let snapshot_deltas = book.to_deltas(snapshot_ts, clock.get_time_ns());
                        order_books.insert(instrument_id, book);

                        let data: NautilusData = OrderBookDeltas_API::new(snapshot_deltas).into();
                        if let Err(e) = sender.send(DataEvent::Data(data)) {
                            log::error!("Failed to emit live book resync snapshot: {e}");
                        }

                        replay_buffered_book_deltas(
                            &order_books,
                            &sender,
                            instrument_id,
                            resync.buffered_deltas.drain(..),
                            Some(snapshot_ts),
                        );
                    }

                    resync.buffered_deltas.clear();
                    resync.phase = LiveBookResyncPhase::Passthrough;
                }

                let _ = live_book_resyncs.remove_if(&instrument_id, |_, resync| {
                    resync.generation == request_id
                        && resync.phase == LiveBookResyncPhase::Passthrough
                });
            }
            Err(e) => {
                if resync_live_book {
                    let _ = close_live_book_resync_admission(
                        &live_book_resyncs,
                        instrument_id,
                        request_id,
                    );
                }
                if resync_live_book
                    && let Some(mut resync) = live_book_resyncs.get_mut(&instrument_id)
                    && resync.generation == request_id
                {
                    if resync.phase == LiveBookResyncPhase::Completing
                        && active_delta_subs.contains(&instrument_id)
                        && !pending_snapshot_after_tick_change.contains(&instrument_id)
                    {
                        replay_buffered_book_deltas(
                            &order_books,
                            &sender,
                            instrument_id,
                            resync.buffered_deltas.drain(..),
                            None,
                        );
                    }

                    resync.buffered_deltas.clear();
                    resync.phase = LiveBookResyncPhase::Passthrough;
                }

                let _ = live_book_resyncs.remove_if(&instrument_id, |_, resync| {
                    resync.generation == request_id
                        && resync.phase == LiveBookResyncPhase::Passthrough
                });
                log::error!("Book snapshot request failed: {e:?}");
            }
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
            Err(e) => {
                log::error!("Trade request failed for {instrument_id}: {e:?}");

                let response = DataResponse::Trades(TradesResponse::new(
                    request_id,
                    client_id,
                    instrument_id,
                    Vec::new(),
                    start_nanos,
                    end_nanos,
                    clock.get_time_ns(),
                    params,
                ));

                if let Err(e) = sender.send(DataEvent::Response(response)) {
                    log::error!("Failed to send empty trades response: {e}");
                }
            }
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use dashmap::DashMap;
    use nautilus_common::messages::DataEvent;
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::{BookOrder, Data as NautilusData, OrderBookDelta, OrderBookDeltas},
        enums::{BookAction, BookType, OrderSide, RecordFlag},
        identifiers::{ClientId, InstrumentId},
        orderbook::OrderBook,
        types::{Price, Quantity},
    };

    use super::{
        LiveBookResync, LiveBookResyncPhase, PendingBookSnapshotResponse,
        close_live_book_resync_admission, join_or_start_live_book_resync,
        replay_buffered_book_deltas,
    };

    #[test]
    fn live_resync_completion_atomically_closes_response_admission() {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let generation = UUID4::new();
        let pending_responses = Arc::new(Mutex::new(vec![PendingBookSnapshotResponse {
            request_id: generation,
            client_id: ClientId::from("POLYMARKET"),
            params: None,
        }]));
        let live_book_resyncs = DashMap::new();
        live_book_resyncs.insert(
            instrument_id,
            LiveBookResync {
                generation,
                pending_responses: pending_responses.clone(),
                buffered_deltas: Vec::new(),
                phase: LiveBookResyncPhase::Buffering,
            },
        );

        let drained =
            close_live_book_resync_admission(&live_book_resyncs, instrument_id, generation)
                .expect("matching generation should close admission");

        assert_eq!(drained.len(), 1);
        assert!(pending_responses.lock().expect("response mutex").is_empty());
        assert_eq!(
            live_book_resyncs
                .get(&instrument_id)
                .expect("live resync")
                .phase,
            LiveBookResyncPhase::Completing,
        );
    }

    #[test]
    fn replacement_generation_inherits_completing_buffer() {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let old_generation = UUID4::new();
        let live_book_resyncs = DashMap::new();
        live_book_resyncs.insert(
            instrument_id,
            LiveBookResync {
                generation: old_generation,
                pending_responses: Arc::new(Mutex::new(Vec::new())),
                buffered_deltas: vec![add_bid(instrument_id, "0.41", 90)],
                phase: LiveBookResyncPhase::Completing,
            },
        );
        let new_generation = UUID4::new();
        let pending_response = PendingBookSnapshotResponse {
            request_id: new_generation,
            client_id: ClientId::from("POLYMARKET"),
            params: None,
        };

        let joined = join_or_start_live_book_resync(
            &live_book_resyncs,
            instrument_id,
            new_generation,
            pending_response,
            Arc::new(Mutex::new(Vec::new())),
        );

        let resync = live_book_resyncs
            .get(&instrument_id)
            .expect("replacement live resync");
        assert!(!joined);
        assert_eq!(resync.generation, new_generation);
        assert_eq!(resync.phase, LiveBookResyncPhase::Buffering);
        assert_eq!(resync.buffered_deltas.len(), 1);
    }

    fn add_bid(instrument_id: InstrumentId, price: &str, ts_event: u64) -> OrderBookDeltas {
        OrderBookDeltas::new(
            instrument_id,
            vec![OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from(price),
                    Quantity::from("10"),
                    ts_event,
                ),
                RecordFlag::F_LAST as u8,
                ts_event,
                UnixNanos::from(ts_event),
                UnixNanos::from(ts_event),
            )],
        )
    }

    fn add_ask(instrument_id: InstrumentId, price: &str, ts_event: u64) -> OrderBookDeltas {
        OrderBookDeltas::new(
            instrument_id,
            vec![OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from(price),
                    Quantity::from("10"),
                    ts_event,
                ),
                RecordFlag::F_LAST as u8,
                ts_event,
                UnixNanos::from(ts_event),
                UnixNanos::from(ts_event),
            )],
        )
    }

    #[test]
    fn live_resync_replays_only_deltas_newer_than_snapshot() {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let order_books = dashmap::DashMap::new();
        order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        replay_buffered_book_deltas(
            &order_books,
            &sender,
            instrument_id,
            [
                add_bid(instrument_id, "0.41", 90),
                add_bid(instrument_id, "0.42", 110),
            ],
            Some(UnixNanos::from(100)),
        );

        let book = order_books.get(&instrument_id).expect("book entry");
        let adapter_bid = book.best_bid_price();
        assert_eq!(adapter_bid, Some(Price::from("0.42")));
        drop(book);
        let emitted = match receiver.try_recv() {
            Ok(DataEvent::Data(NautilusData::Deltas(deltas))) => deltas,
            other => panic!("expected emitted book deltas, found {other:?}"),
        };
        let mut cache_book = OrderBook::new(instrument_id, BookType::L2_MBP);
        cache_book
            .apply_deltas(&emitted)
            .expect("apply emitted deltas");
        assert_eq!(cache_book.best_bid_price(), adapter_bid);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn failed_live_resync_replays_every_buffered_delta() {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let order_books = dashmap::DashMap::new();
        let mut old_book = OrderBook::new(instrument_id, BookType::L2_MBP);
        old_book
            .apply_deltas(&add_ask(instrument_id, "0.60", 80))
            .expect("seed old book");
        order_books.insert(instrument_id, old_book);
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        replay_buffered_book_deltas(
            &order_books,
            &sender,
            instrument_id,
            [
                add_bid(instrument_id, "0.41", 90),
                add_bid(instrument_id, "0.42", 110),
            ],
            None,
        );

        let book = order_books.get(&instrument_id).expect("book entry");
        assert_eq!(book.best_bid_price(), Some(Price::from("0.42")));
        assert_eq!(book.best_ask_price(), Some(Price::from("0.60")));
        drop(book);
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());
    }
}
