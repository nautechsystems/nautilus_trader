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

use std::time::Duration;

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use nautilus_common::msgbus::{self, TypedHandler};
use nautilus_core::{AtomicMap, AtomicSet};
use nautilus_live::task::TaskGroupGuard;
use nautilus_model::events::PositionEvent;
use parking_lot::Mutex;

use super::{
    PolymarketDataClient,
    dispatch::{WsMessageContext, handle_ws_message},
    instruments::{InstrumentUpdateState, refresh_expired_market_closure},
    runtime::{
        retire_closed_condition_state, retire_expired_local_instruments,
        seed_token_meta_from_live_instruments,
    },
};
use crate::{
    data_types::register_polymarket_custom_data,
    resolve::{
        ResolveBatchErrorMode, ResolveWatchSelectionMode, collect_resolve_watch_selection,
        fetch_and_apply_resolutions_by_condition_ids, pause_resolve_watch_entries,
        update_resolve_watchlist_from_position_event_serialized,
    },
    websocket::messages::PolymarketWsMessage,
};

const TASK_GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);

impl PolymarketDataClient {
    fn ensure_position_event_subscription(&mut self) {
        if self.position_event_handler.is_some() {
            return;
        }

        let watchlist = self.resolve_poll_watchlist.clone();
        let instruments = self.instruments.clone();
        let owner_lock = self.resolve_watch_apply_mutex.clone();
        let closed_condition_ids = self.closed_condition_ids.clone();
        let handler = TypedHandler::from(move |event: &PositionEvent| {
            update_resolve_watchlist_from_position_event_serialized(
                &owner_lock,
                &closed_condition_ids,
                &watchlist,
                &instruments,
                event,
            );
        });

        msgbus::subscribe_position_events("events.position.*".into(), handler.clone(), Some(10));
        self.position_event_handler = Some(handler);
    }

    fn clear_position_event_subscription(&mut self) {
        if let Some(handler) = self.position_event_handler.take() {
            msgbus::unsubscribe_position_events("events.position.*".into(), &handler);
        }
    }

    fn register_message_handler(
        &self,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<PolymarketWsMessage>,
    ) -> anyhow::Result<()> {
        let cancellation = self.cancellation_token.clone();

        seed_token_meta_from_live_instruments(
            self.clock.get_time_ns(),
            &self.closed_condition_ids,
            &self.instruments,
            &self.token_meta,
        );

        let task_spawner = self
            .tasks
            .spawner()
            .map_err(|e| anyhow::anyhow!("Polymarket task admission is closed: {e}"))?;
        let ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_meta: self.token_meta.clone(),
            instruments: self.instruments.clone(),
            instrument_update_state: self.instrument_update_state.clone(),
            gamma_client: self.provider.http_client().clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            active_instrument_status_subs: self.active_instrument_status_subs.clone(),
            active_instrument_close_subs: self.active_instrument_close_subs.clone(),
            closed_condition_ids: self.closed_condition_ids.clone(),
            ws_open_tokens: self.ws_open_tokens.clone(),
            ws_sub_mutex: self.ws_sub_mutex.clone(),
            ws: self.ws_client.handle(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: self.resolve_watch_apply_mutex.clone(),
            pending_resolutions: self.pending_resolutions.clone(),
            pending_snapshot_after_tick_change: self.pending_snapshot_after_tick_change.clone(),
            new_market_inflight_keys: self.new_market_inflight_keys.clone(),
            new_market_fetch_semaphore: self.new_market_fetch_semaphore.clone(),
            tasks: task_spawner,
            rtds_feed: self.rtds_feed.clone(),
            subscribe_new_markets: self.config.subscribe_new_markets,
            new_market_filter: self.config.new_market_filter.clone(),
            drop_quotes_missing_side: self.config.drop_quotes_missing_side,
            compute_effective_deltas: self.config.compute_effective_deltas,
            cancellation_token: cancellation.clone(),
        };

        let future = async move {
            log::debug!("Polymarket message handler started");

            loop {
                tokio::select! {
                    maybe_msg = rx.recv() => {
                        match maybe_msg {
                            Some(msg) => handle_ws_message(msg, &ctx),
                            None => {
                                log::debug!("WebSocket message channel closed");
                                break;
                            }
                        }
                    }
                    () = cancellation.cancelled() => {
                        log::debug!("Polymarket message handler cancelled");
                        break;
                    }
                }
            }

            log::debug!("Polymarket message handler ended");
        };
        self.tasks
            .spawn(future)
            .map_err(|e| anyhow::anyhow!("failed to register Polymarket message handler: {e}"))?;
        Ok(())
    }

    pub(super) fn register_resolve_poll_task(&self) -> anyhow::Result<()> {
        let cancellation = self.cancellation_token.clone();
        let gamma_client = self.provider.http_client().clone();
        let clob_public_client = self.clob_public_client.clone();
        let clock = self.clock;
        let resolve_poll_enabled = self.config.resolve_poll_enabled;
        let interval_secs = self.config.resolve_poll_interval_secs.max(1);
        let grace_secs = self.config.resolve_poll_grace_secs;
        let max_wait_secs = self.config.resolve_poll_max_wait_secs.max(grace_secs);
        let instruments = self.instruments.clone();
        let token_meta = self.token_meta.clone();
        let order_books = self.order_books.clone();
        let last_quotes = self.last_quotes.clone();
        let active_quote_subs = self.active_quote_subs.clone();
        let active_delta_subs = self.active_delta_subs.clone();
        let active_trade_subs = self.active_trade_subs.clone();
        let active_instrument_status_subs = self.active_instrument_status_subs.clone();
        let active_instrument_close_subs = self.active_instrument_close_subs.clone();
        let pending_snapshot_after_tick_change = self.pending_snapshot_after_tick_change.clone();
        let pending_auto_loads = self.pending_auto_loads.clone();
        let ws_open_tokens = self.ws_open_tokens.clone();
        let ws_sub_mutex = self.ws_sub_mutex.clone();
        let ws = self.ws_client.handle();
        let closure_client = gamma_client.clone();
        let closure_sender = self.data_sender.clone();
        let closed_condition_ids = self.closed_condition_ids.clone();

        let task_spawner = self
            .tasks
            .spawner()
            .map_err(|e| anyhow::anyhow!("Polymarket task admission is closed: {e}"))?;
        let ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_meta: self.token_meta.clone(),
            instruments: self.instruments.clone(),
            instrument_update_state: self.instrument_update_state.clone(),
            gamma_client: gamma_client.clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            active_instrument_status_subs: self.active_instrument_status_subs.clone(),
            active_instrument_close_subs: self.active_instrument_close_subs.clone(),
            closed_condition_ids: self.closed_condition_ids.clone(),
            ws_open_tokens: self.ws_open_tokens.clone(),
            ws_sub_mutex: self.ws_sub_mutex.clone(),
            ws: self.ws_client.handle(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: self.resolve_watch_apply_mutex.clone(),
            pending_resolutions: self.pending_resolutions.clone(),
            pending_snapshot_after_tick_change: self.pending_snapshot_after_tick_change.clone(),
            new_market_inflight_keys: self.new_market_inflight_keys.clone(),
            new_market_fetch_semaphore: self.new_market_fetch_semaphore.clone(),
            tasks: task_spawner,
            rtds_feed: self.rtds_feed.clone(),
            subscribe_new_markets: self.config.subscribe_new_markets,
            new_market_filter: self.config.new_market_filter.clone(),
            drop_quotes_missing_side: self.config.drop_quotes_missing_side,
            compute_effective_deltas: self.config.compute_effective_deltas,
            cancellation_token: cancellation.clone(),
        };

        let watchlist = self.resolve_poll_watchlist.clone();

        if resolve_poll_enabled {
            log::debug!("Polymarket resolve poll task started");
        } else {
            log::debug!(
                "Polymarket resolve poll task started with resolution fetch disabled; expiry retirement remains active"
            );
        }

        let future = async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut retired_condition_ids: AHashSet<String> = AHashSet::new();

            loop {
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    _ = interval.tick() => {
                        let now_ns = clock.get_time_ns();

                        // Runs on every tick so retirement never trails closure by more than one
                        // cycle. Without an expired instrument reported open, no request is sent.
                        let refresh_result = tokio::select! {
                            result = refresh_expired_market_closure(
                                &closure_client,
                                &instruments,
                                &closure_sender,
                                now_ns,
                                &closed_condition_ids,
                                &ws_sub_mutex,
                                Some(&cancellation),
                            ) => result,
                            () = cancellation.cancelled() => break,
                        };

                        if let Err(e) = refresh_result {
                            log::warn!("Failed to refresh Polymarket market closure state: {e}");
                        }

                        if cancellation.is_cancelled() {
                            break;
                        }

                        // A set-wide sweep never converges and grows for the process lifetime
                        let pending_retirement = {
                            let terminal_conditions = closed_condition_ids
                                .lock();

                            terminal_conditions
                                .difference(&retired_condition_ids)
                                .cloned()
                                .collect::<Vec<_>>()
                        };

                        for condition_id in pending_retirement {
                            let converged = retire_closed_condition_state(
                                &condition_id,
                                std::iter::empty(),
                                &closed_condition_ids,
                                &instruments,
                                &token_meta,
                                &order_books,
                                &last_quotes,
                                &active_quote_subs,
                                &active_delta_subs,
                                &active_trade_subs,
                                &active_instrument_status_subs,
                                &active_instrument_close_subs,
                                &watchlist,
                                &pending_snapshot_after_tick_change,
                                &pending_auto_loads,
                                &ws_open_tokens,
                                &ws_sub_mutex,
                                &ws,
                                Some(&cancellation),
                            )
                            .await;

                            if cancellation.is_cancelled() {
                                break;
                            }

                            // Watchlisted or recreated state survives a pass, so retry until clear
                            if converged {
                                retired_condition_ids.insert(condition_id);
                            }
                        }

                        retire_expired_local_instruments(
                            now_ns,
                            &instruments,
                            &token_meta,
                            &order_books,
                            &last_quotes,
                            &active_quote_subs,
                            &active_delta_subs,
                            &active_trade_subs,
                            &active_instrument_status_subs,
                            &active_instrument_close_subs,
                            &closed_condition_ids,
                            &watchlist,
                            &pending_snapshot_after_tick_change,
                            &pending_auto_loads,
                            &ws_open_tokens,
                            &ws_sub_mutex,
                            &ws,
                        )
                        .await;

                        if !resolve_poll_enabled {
                            continue;
                        }

                        let snapshot = watchlist.load();
                        let watched_conditions = snapshot.len();
                        let watched_instruments = snapshot
                            .values()
                            .map(|entry| entry.tracked.len())
                            .sum::<usize>();
                        let selection = collect_resolve_watch_selection(
                            &snapshot,
                            now_ns,
                            grace_secs,
                            max_wait_secs,
                            ResolveWatchSelectionMode::AutoPoll,
                        );
                        drop(snapshot);

                        if !selection.pause_condition_ids.is_empty() {
                            log::warn!(
                                "Polymarket resolve poll paused {} timed-out condition(s) for manual recovery",
                                selection.pause_condition_ids.len(),
                            );
                        }

                        if !selection.condition_ids.is_empty()
                            || !selection.pause_condition_ids.is_empty()
                        {
                            log::debug!(
                                "Polymarket resolve poll selected={} watched_conditions={} watched_instruments={} skipped_not_expired={} timed_out={} paused={} min_ready_in_secs={:?}",
                                selection.condition_ids.len(),
                                watched_conditions,
                                watched_instruments,
                                selection.skipped_not_expired,
                                selection.timed_out_watchlist,
                                selection.paused_watchlist,
                                selection.min_ready_in_secs,
                            );
                        } else if selection.timed_out_watchlist > 0
                            && selection.paused_watchlist > 0
                        {
                            log::debug!(
                                "Polymarket resolve poll waiting for manual recovery: timed_out={} paused={} watched_conditions={watched_conditions}",
                                selection.timed_out_watchlist,
                                selection.paused_watchlist,
                            );
                        }

                        pause_resolve_watch_entries(&watchlist, &selection.pause_condition_ids);

                        let _ = fetch_and_apply_resolutions_by_condition_ids(
                            &gamma_client,
                            &clob_public_client,
                            &ctx.resolve_context(),
                            &selection.condition_ids,
                            ResolveBatchErrorMode::Continue,
                        )
                        .await;
                    }
                }
            }
        };
        self.tasks
            .spawn(future)
            .map_err(|e| anyhow::anyhow!("failed to register Polymarket resolve poll: {e}"))?;
        Ok(())
    }

    pub(super) async fn await_tasks_with_timeout(
        &self,
        timeout: tokio::time::Duration,
    ) -> anyhow::Result<()> {
        self.tasks.begin_shutdown();
        let graceful_timeout = (timeout / 2).min(TASK_GRACEFUL_SHUTDOWN_TIMEOUT);
        let abort_timeout = timeout.saturating_sub(graceful_timeout);
        self.tasks
            .finish_shutdown(graceful_timeout, abort_timeout)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to terminate Polymarket data tasks: {e}"))?;
        Ok(())
    }

    pub(super) fn start_client(&mut self) {
        log::info!("Starting Polymarket data client: {}", self.client_id);
        self.ensure_position_event_subscription();
    }

    pub(super) fn stop_client(&mut self) {
        log::info!("Stopping Polymarket data client: {}", self.client_id);
        self.tasks.begin_shutdown();
        self.ws_client.begin_shutdown();
        self.rtds_feed.begin_shutdown();
        self.is_connected
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.clear_position_event_subscription();
    }

    pub(super) fn reset_client(&mut self) {
        log::debug!("Resetting Polymarket data client: {}", self.client_id);
        self.tasks.begin_shutdown();
        self.ws_client.begin_shutdown();
        self.rtds_feed.begin_shutdown();
        self.is_connected
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.reset_pending = true;

        // Hard reset contract: discard all retained reconnect replay state from
        // the previous generation. Callers must rebuild instrument/data
        // subscriptions after connect().
        self.resolve_poll_watchlist.store(AHashMap::new());
        self.pending_resolutions.clear();
        self.clear_position_event_subscription();

        let old_instrument_update_state = self.instrument_update_state.clone();
        let _update_guard = old_instrument_update_state.lock();
        let old_closed_condition_ids = self.closed_condition_ids.clone();
        let _generation_guard = old_closed_condition_ids.lock();

        self.instruments = std::sync::Arc::new(AtomicMap::new());
        self.instrument_update_state =
            std::sync::Arc::new(Mutex::new(InstrumentUpdateState::default()));
        self.token_meta = std::sync::Arc::new(DashMap::new());
        self.order_books = std::sync::Arc::new(DashMap::new());
        self.last_quotes = std::sync::Arc::new(DashMap::new());

        self.active_quote_subs = std::sync::Arc::new(AtomicSet::new());
        self.active_delta_subs = std::sync::Arc::new(AtomicSet::new());
        self.active_trade_subs = std::sync::Arc::new(AtomicSet::new());
        self.active_instrument_status_subs = std::sync::Arc::new(AtomicSet::new());
        self.active_instrument_close_subs = std::sync::Arc::new(AtomicSet::new());
        self.pending_snapshot_after_tick_change = std::sync::Arc::new(AtomicSet::new());
        self.new_market_inflight_keys = std::sync::Arc::new(DashMap::new());
        self.pending_resolutions = std::sync::Arc::new(DashMap::new());
        self.ws_open_tokens = std::sync::Arc::new(AtomicSet::new());

        self.pending_auto_loads = std::sync::Arc::new(parking_lot::Mutex::new(AHashSet::new()));
        self.closed_condition_ids = std::sync::Arc::new(parking_lot::Mutex::new(AHashSet::new()));
        self.auto_load_scheduled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    }

    pub(super) async fn connect_client(&mut self) -> anyhow::Result<()> {
        if self.is_connected() && self.tasks.is_open() && !self.reset_pending {
            return Ok(());
        }

        if self.reset_pending
            || !self.tasks.is_empty()
            || !self.tasks.is_open()
            || self.ws_client.connection_count() > 0
        {
            self.disconnect_client().await?;
        }

        if !self.tasks.is_open() {
            self.tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to start Polymarket task generation: {e}"))?;
            self.cancellation_token = self.tasks.cancellation_token();
        }

        let ws_client = self.ws_client.handle();
        let rtds_feed = self.rtds_feed.clone();
        let setup_guard = TaskGroupGuard::new(&[&self.tasks], move || {
            ws_client.begin_shutdown();
            rtds_feed.begin_shutdown();
        });
        self.ensure_position_event_subscription();
        register_polymarket_custom_data();

        log::info!("Connecting Polymarket data client");

        log::debug!("Bootstrapping instruments from Gamma API...");
        self.bootstrap_instruments().await?;
        log::debug!(
            "Bootstrap complete, {} instruments loaded",
            self.instruments.load().len(),
        );

        self.ws_client.connect().await?;

        let session_result = async {
            if self.config.subscribe_new_markets {
                log::debug!("Subscribing to new markets...");
                self.ws_client.subscribe_new_markets_feed().await?;
            }

            let rx = self.ws_client.take_message_receiver().ok_or_else(|| {
                anyhow::anyhow!("WS message receiver not available after connect")
            })?;

            self.register_message_handler(rx)?;
            self.register_instrument_refresh_task()?;
            self.register_resolve_poll_task()?;

            // Connect unconditionally: this clears the feed's closing latch from a prior
            // disconnect; without retained subscriptions no RTDS socket is opened.
            self.rtds_feed.connect().await
        }
        .await;

        if let Err(e) = session_result {
            if let Err(teardown_error) = self.disconnect_client().await {
                log::warn!(
                    "Error tearing down partial Polymarket data connection: {teardown_error:?}"
                );
            }
            return Err(e);
        }

        setup_guard.disarm();
        self.is_connected
            .store(true, std::sync::atomic::Ordering::Relaxed);
        log::info!("Connected Polymarket data client");

        Ok(())
    }

    pub(super) async fn disconnect_client(&mut self) -> anyhow::Result<()> {
        if !self.is_connected()
            && self.tasks.is_empty()
            && self.tasks.is_open()
            && self.ws_client.connection_count() == 0
            && self.shutdown_errors.is_empty()
        {
            return Ok(());
        }

        log::info!("Disconnecting Polymarket data client");

        self.tasks.begin_shutdown();
        self.ws_client.begin_shutdown();
        self.rtds_feed.begin_shutdown();

        if let Err(e) = self
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(5))
            .await
        {
            self.shutdown_errors.push(e.to_string());
        }

        if let Err(e) = self.ws_client.disconnect().await {
            self.shutdown_errors.push(e.to_string());
        }

        if let Err(e) = self.rtds_feed.disconnect().await {
            self.shutdown_errors.push(e.to_string());
        }
        self.clear_position_event_subscription();

        let drained = self.tasks.is_empty()
            && self.ws_client.connection_count() == 0
            && !self.rtds_feed.has_retained_tasks().await;

        if drained {
            if self.reset_pending {
                self.ws_client.clear_reconnect_state();
                self.rtds_feed = crate::rtds::PolymarketRtdsFeed::new_with_proxy_and_socket_control(
                    self.config.rtds_url(),
                    self.config.transport_backend,
                    self.clock,
                    self.data_sender.clone(),
                    self.proxy_url.clone(),
                    self.rtds_socket_control.clone(),
                );
                self.reset_pending = false;
            }

            self.is_connected
                .store(false, std::sync::atomic::Ordering::Relaxed);
            log::info!("Disconnected Polymarket data client");
        }

        if self.shutdown_errors.is_empty() {
            Ok(())
        } else {
            let errors = std::mem::take(&mut self.shutdown_errors);
            anyhow::bail!("Polymarket data shutdown failed: {}", errors.join("; "))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, atomic::Ordering},
    };

    use nautilus_common::{
        cache::Cache,
        clients::{DataClient, ExecutionClient},
        clock::{Clock, TestClock},
        live::runner::{replace_data_event_sender, replace_exec_event_sender},
        messages::{
            DataEvent, ExecutionEvent,
            data::{
                SubscribeBookDeltas, SubscribeCustomData, UnsubscribeBookDeltas,
                UnsubscribeCustomData,
            },
        },
        testing::wait_until_async,
    };
    use nautilus_core::{
        Params, UUID4, UnixNanos, datetime::NANOSECONDS_IN_SECOND, string::secret::SecretString,
    };
    use nautilus_execution::client::core::ExecutionClientCore;
    use nautilus_model::{
        data::{DataType, QuoteTick},
        enums::BookType,
        identifiers::{ClientId, InstrumentId, PositionId, Symbol, TraderId},
        instruments::{Instrument, InstrumentAny, stubs::binary_option},
        orderbook::OrderBook,
        types::{Currency, Price, Quantity},
    };
    use nautilus_network::{
        retry::RetryConfig,
        websocket::{TransportBackend, proxy::ProxyUrl},
    };
    use nautilus_sandbox::{SandboxExecutionClient, SandboxExecutionClientConfig};
    use rstest::rstest;
    use serde_json::Value;
    use ustr::Ustr;

    use super::{super::NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP, *};
    use crate::{
        common::consts::{POLYMARKET_CLIENT_ID, POLYMARKET_VENUE, WS_DEFAULT_SUBSCRIPTIONS},
        config::PolymarketDataClientConfig,
        data::{
            instruments::{apply_live_instrument, cache_instrument_unchecked},
            runtime::retire_local_instrument_state,
        },
        http::{
            clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
            gamma::PolymarketGammaHttpClient,
        },
        resolve::upsert_resolve_watch_entry_from_instrument,
        websocket::{messages::PolymarketWsMessage, pool::PolymarketMarketConnectionPool},
    };

    struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    fn make_client_for_reset_test() -> PolymarketDataClient {
        make_client_for_reset_test_with_proxy(None)
    }

    fn make_client_for_reset_test_with_proxy(proxy_url: Option<ProxyUrl>) -> PolymarketDataClient {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(tx);

        let gamma = PolymarketGammaHttpClient::new_with_proxy(
            Some("http://localhost".to_string()),
            1,
            RetryConfig::default(),
            proxy_url.clone(),
        )
        .expect("gamma client");
        let clob = PolymarketClobPublicClient::new_with_proxy(
            Some("http://localhost".to_string()),
            1,
            proxy_url.clone(),
        )
        .expect("clob client");
        let data_api = PolymarketDataApiHttpClient::new_with_proxy(
            Some("http://localhost".to_string()),
            1,
            proxy_url.clone(),
        )
        .expect("data api client");
        let ws = PolymarketMarketConnectionPool::new_with_proxy(
            Some("ws://localhost/ws/market".to_string()),
            false,
            TransportBackend::default(),
            WS_DEFAULT_SUBSCRIPTIONS,
            proxy_url.clone(),
        );
        let config = PolymarketDataClientConfig {
            proxy_url: proxy_url
                .as_ref()
                .map(|proxy_url| SecretString::from(proxy_url.expose())),
            ..PolymarketDataClientConfig::default()
        };

        PolymarketDataClient::new_with_proxy(
            ClientId::from("POLY-TEST"),
            config,
            gamma,
            clob,
            data_api,
            ws,
            proxy_url,
        )
    }

    fn make_client_with_fetch_concurrency(concurrency: usize) -> PolymarketDataClient {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(tx);

        let gamma = PolymarketGammaHttpClient::new(
            Some("http://localhost".to_string()),
            1,
            RetryConfig::default(),
        )
        .expect("gamma client");
        let clob = PolymarketClobPublicClient::new(Some("http://localhost".to_string()), 1)
            .expect("clob client");
        let data_api = PolymarketDataApiHttpClient::new(Some("http://localhost".to_string()), 1)
            .expect("data api client");
        let ws = PolymarketMarketConnectionPool::new(
            Some("ws://localhost/ws/market".to_string()),
            false,
            TransportBackend::default(),
            WS_DEFAULT_SUBSCRIPTIONS,
        );

        let config = PolymarketDataClientConfig {
            new_market_fetch_max_concurrency: concurrency,
            ..PolymarketDataClientConfig::default()
        };

        PolymarketDataClient::new(
            ClientId::from("POLY-TEST"),
            config,
            gamma,
            clob,
            data_api,
            ws,
        )
    }

    fn rtds_crypto_data_type(symbol: &str) -> DataType {
        let mut metadata = Params::new();
        metadata.insert("symbol".to_string(), Value::String(symbol.to_string()));
        DataType::new("PolymarketRtdsCryptoPrice", Some(metadata), None)
    }

    fn rtds_equity_data_type(symbol: &str) -> DataType {
        let mut metadata = Params::new();
        metadata.insert("symbol".to_string(), Value::String(symbol.to_string()));
        DataType::new("PolymarketRtdsEquityPrice", Some(metadata), None)
    }

    fn seed_expired_instrument(
        client: &PolymarketDataClient,
        raw_symbol: &str,
        condition_id: &str,
    ) -> InstrumentAny {
        let expiration_ns = UnixNanos::from(
            client
                .clock
                .get_time_ns()
                .as_u64()
                .saturating_sub(1_000_000_000),
        );

        seed_instrument(client, raw_symbol, condition_id, expiration_ns)
    }

    fn seed_instrument(
        client: &PolymarketDataClient,
        raw_symbol: &str,
        condition_id: &str,
        expiration_ns: UnixNanos,
    ) -> InstrumentAny {
        let mut binary = binary_option();
        binary.id = InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str());
        binary.raw_symbol = Symbol::new(raw_symbol);
        binary.currency = Currency::pUSD();
        binary.activation_ns = UnixNanos::default();
        binary.expiration_ns = expiration_ns;

        let mut info = Params::new();
        info.insert(
            "token_id".to_string(),
            serde_json::Value::String(raw_symbol.to_string()),
        );
        info.insert(
            "condition_id".to_string(),
            serde_json::Value::String(condition_id.to_string()),
        );
        binary.info = Some(info);

        let inst = InstrumentAny::BinaryOption(binary);
        cache_instrument_unchecked(&client.instruments, &client.token_meta, &inst);
        inst
    }

    fn seed_expired_runtime_state(client: &PolymarketDataClient, inst: &InstrumentAny) {
        let instrument_id = inst.id();

        client.active_quote_subs.insert(instrument_id);
        client.active_delta_subs.insert(instrument_id);
        client.active_trade_subs.insert(instrument_id);
        client.active_instrument_status_subs.insert(instrument_id);
        client.active_instrument_close_subs.insert(instrument_id);
        client
            .ws_open_tokens
            .insert(Ustr::from(inst.raw_symbol().as_str()));
        client
            .pending_snapshot_after_tick_change
            .insert(instrument_id);
        client.pending_auto_loads.lock().insert(instrument_id);
        client.order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );
        client.last_quotes.insert(
            instrument_id,
            QuoteTick::new(
                instrument_id,
                Price::from("0.504"),
                Price::from("0.506"),
                Quantity::from("5.00"),
                Quantity::from("8.00"),
                UnixNanos::default(),
                UnixNanos::default(),
            ),
        );
    }

    #[rstest]
    fn reset_cancels_old_generation_and_clears_connection_state() {
        let mut client = make_client_for_reset_test();
        let old_token = client.cancellation_token.clone();

        let instrument_id = InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET");
        client.active_quote_subs.insert(instrument_id);
        client.active_delta_subs.insert(instrument_id);
        client.active_trade_subs.insert(instrument_id);
        client.active_instrument_status_subs.insert(instrument_id);
        client.active_instrument_close_subs.insert(instrument_id);
        client.ws_open_tokens.insert(Ustr::from("0xCOND-0xTOKEN"));
        client
            .new_market_inflight_keys
            .insert("btc-updown-5m-1".to_string(), ());
        client
            .pending_snapshot_after_tick_change
            .insert(instrument_id);
        client.pending_auto_loads.lock().insert(instrument_id);
        client.order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );
        client.last_quotes.insert(
            instrument_id,
            QuoteTick::new(
                instrument_id,
                Price::from("0.49"),
                Price::from("0.51"),
                Quantity::from("10"),
                Quantity::from("8"),
                UnixNanos::default(),
                UnixNanos::default(),
            ),
        );
        client.auto_load_scheduled.store(true, Ordering::Release);

        client
            .reset()
            .expect("reset should succeed for in-memory state");

        assert!(old_token.is_cancelled());
        assert!(client.cancellation_token.is_cancelled());
        assert!(!client.tasks.is_open());

        assert!(client.active_quote_subs.is_empty());
        assert!(client.active_delta_subs.is_empty());
        assert!(client.active_trade_subs.is_empty());
        assert!(client.active_instrument_status_subs.is_empty());
        assert!(client.active_instrument_close_subs.is_empty());
        assert!(client.ws_open_tokens.is_empty());
        assert!(client.order_books.is_empty());
        assert!(client.last_quotes.is_empty());
        assert!(client.new_market_inflight_keys.is_empty());
        assert!(client.pending_snapshot_after_tick_change.is_empty());
        assert!(client.pending_auto_loads.lock().is_empty());
        assert!(!client.auto_load_scheduled.load(Ordering::Acquire));
    }

    #[rstest]
    #[tokio::test]
    async fn reset_closes_old_generation_until_tasks_drain() {
        let mut client = make_client_for_reset_test();
        let old_token = client.cancellation_token.clone();
        let old_spawner = client.tasks.spawner().expect("old task spawner");
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();

        client
            .tasks
            .spawn(async move {
                let _drop_signal = DropSignal(Some(dropped_tx));
                started_tx.send(()).expect("task start receiver");
                std::future::pending::<()>().await;
            })
            .expect("old generation task spawn");
        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .expect("old generation task start timeout")
            .expect("old generation task started");

        client.reset().expect("reset data client");

        assert!(old_token.is_cancelled());
        assert!(client.cancellation_token.is_cancelled());
        assert!(!client.tasks.is_open());
        assert_eq!(client.tasks.len(), 1);

        let (late_dropped_tx, late_dropped_rx) = tokio::sync::oneshot::channel();
        let late_drop_signal = DropSignal(Some(late_dropped_tx));

        let result = old_spawner.spawn(async move {
            let _drop_signal = late_drop_signal;
            std::future::pending::<()>().await;
        });

        assert!(result.is_err());

        tokio::time::timeout(Duration::from_secs(1), late_dropped_rx)
            .await
            .expect("late old generation task stop timeout")
            .expect("late old generation task stopped");
        client
            .await_tasks_with_timeout(Duration::from_secs(1))
            .await
            .expect("old generation tasks drained");
        tokio::time::timeout(Duration::from_secs(1), dropped_rx)
            .await
            .expect("old generation task stop timeout")
            .expect("old generation task stopped");
        assert!(client.tasks.is_empty());

        client.tasks.start_generation().expect("fresh generation");
        client.cancellation_token = client.tasks.cancellation_token();
        client.register_resolve_poll_task().unwrap();

        assert_eq!(client.tasks.len(), 1);
        assert!(!client.tasks.all_finished());

        client.tasks.begin_shutdown();
        client
            .await_tasks_with_timeout(Duration::from_secs(1))
            .await
            .expect("fresh generation task terminated");

        assert!(client.tasks.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn disconnect_allows_cancellation_cleanup_before_abort() {
        let mut client = make_client_for_reset_test();
        client.auto_load_scheduled.store(true, Ordering::Release);
        let cancellation = client.cancellation_token.clone();
        let scheduled = client.auto_load_scheduled.clone();
        let future = async move {
            cancellation.cancelled().await;
            scheduled.store(false, Ordering::Release);
        };

        client.tasks.spawn(future).expect("cleanup task spawn");

        client.disconnect().await.expect("disconnect data client");

        assert!(!client.auto_load_scheduled.load(Ordering::Acquire));
        assert!(client.tasks.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn stop_drops_unpolled_auto_load_scheduling_guard() {
        let mut client = make_client_for_reset_test();
        client.config.auto_load_debounce_ms = 60_000;
        let instrument_id = InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET");
        client.active_quote_subs.insert(instrument_id);

        client.queue_pending_load(instrument_id);
        assert!(client.auto_load_scheduled.load(Ordering::Acquire));
        assert_eq!(client.tasks.len(), 1);

        client.stop().expect("stop data client");
        client
            .await_tasks_with_timeout(Duration::from_secs(1))
            .await
            .expect("auto-load task terminated");

        assert!(!client.auto_load_scheduled.load(Ordering::Acquire));
        assert!(client.tasks.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn disconnect_aborts_owned_tasks_when_connection_flag_is_false() {
        let mut client = make_client_for_reset_test();
        let token = client.cancellation_token.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();

        client
            .tasks
            .spawn(async move {
                let _drop_signal = DropSignal(Some(dropped_tx));
                started_tx.send(()).expect("task start receiver");
                std::future::pending::<()>().await;
            })
            .expect("owned task spawn");
        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .expect("owned task start timeout")
            .expect("owned task started");

        client.disconnect().await.expect("disconnect data client");

        assert!(token.is_cancelled());
        assert!(client.tasks.is_empty());
        assert!(!client.is_connected());
        tokio::time::timeout(Duration::from_secs(1), dropped_rx)
            .await
            .expect("owned task stop timeout")
            .expect("owned task stopped");
    }

    #[rstest]
    #[case::disabled(false)]
    #[case::enabled(true)]
    fn book_delta_subscription_gates_and_cleans_local_book_state(#[case] enabled: bool) {
        let mut client = make_client_for_reset_test();
        client.config.compute_effective_deltas = enabled;
        client.cancellation_token.cancel();
        let instrument_id = InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET");
        let subscribe = || {
            SubscribeBookDeltas::new(
                instrument_id,
                BookType::L2_MBP,
                Some(*POLYMARKET_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                true,
                None,
                None,
            )
        };
        let unsubscribe = || {
            UnsubscribeBookDeltas::new(
                instrument_id,
                Some(*POLYMARKET_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        };

        client
            .subscribe_book_deltas(subscribe())
            .expect("subscribe book deltas");

        assert!(client.active_delta_subs.contains(&instrument_id));
        assert!(client.pending_auto_loads.lock().contains(&instrument_id));
        assert_eq!(client.order_books.contains_key(&instrument_id), enabled);

        if let Some(mut book) = client.order_books.get_mut(&instrument_id) {
            book.update_count = 7;
        }

        client.active_quote_subs.insert(instrument_id);
        client.last_quotes.insert(
            instrument_id,
            QuoteTick::new(
                instrument_id,
                Price::from("0.49"),
                Price::from("0.51"),
                Quantity::from("10"),
                Quantity::from("8"),
                UnixNanos::default(),
                UnixNanos::default(),
            ),
        );
        client
            .unsubscribe_book_deltas(&unsubscribe())
            .expect("unsubscribe book deltas");

        assert!(!client.active_delta_subs.contains(&instrument_id));
        assert!(!client.order_books.contains_key(&instrument_id));
        assert!(client.last_quotes.contains_key(&instrument_id));
        assert!(client.pending_auto_loads.lock().contains(&instrument_id));

        client
            .subscribe_book_deltas(subscribe())
            .expect("resubscribe book deltas");

        assert_eq!(client.order_books.contains_key(&instrument_id), enabled);
        if let Some(book) = client.order_books.get(&instrument_id) {
            assert_eq!(book.update_count, 0);
        }

        client.active_quote_subs.remove(&instrument_id);
        client
            .unsubscribe_book_deltas(&unsubscribe())
            .expect("final unsubscribe book deltas");

        assert!(!client.active_delta_subs.contains(&instrument_id));
        assert!(!client.order_books.contains_key(&instrument_id));
        assert!(!client.last_quotes.contains_key(&instrument_id));
        assert!(client.pending_auto_loads.lock().is_empty());
    }

    #[rstest]
    fn new_market_fetch_concurrency_clamps_zero_to_one() {
        let client = make_client_with_fetch_concurrency(0);
        assert_eq!(client.new_market_fetch_semaphore.available_permits(), 1);
        assert_eq!(client.config.new_market_fetch_max_concurrency, 1);
    }

    #[rstest]
    fn new_market_fetch_concurrency_clamps_high_value_to_cap() {
        let client = make_client_with_fetch_concurrency(1_000);
        assert_eq!(
            client.new_market_fetch_semaphore.available_permits(),
            NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP,
        );
        assert_eq!(
            client.config.new_market_fetch_max_concurrency,
            NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP,
        );
    }

    #[rstest]
    fn reset_replaces_new_market_inflight_keys_generation() {
        let mut client = make_client_for_reset_test();
        let old_inflight_keys = client.new_market_inflight_keys.clone();

        old_inflight_keys.insert("cond:0xold".to_string(), ());
        client.reset().expect("reset should succeed");

        client
            .new_market_inflight_keys
            .insert("cond:0xold".to_string(), ());
        old_inflight_keys.remove("cond:0xold");

        assert!(
            client.new_market_inflight_keys.contains_key("cond:0xold"),
            "old-generation guard cleanup should not remove reset-generation dedupe keys",
        );
        assert!(
            !Arc::ptr_eq(&old_inflight_keys, &client.new_market_inflight_keys),
            "reset should replace in-flight dedupe map generation",
        );
    }

    #[rstest]
    fn subscribe_unsupported_custom_data_is_ignored() {
        let mut client = make_client_for_reset_test();
        let data_type = DataType::new("UnsupportedPolymarketCustomData", None, None);

        client
            .subscribe(SubscribeCustomData::new(
                Some(*POLYMARKET_CLIENT_ID),
                None,
                data_type,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("unsupported custom data subscribe should be ignored");

        assert_eq!(client.rtds_feed.tracked_subscription_count(), 0);
    }

    #[rstest]
    fn unsubscribe_unsupported_custom_data_is_ignored() {
        let mut client = make_client_for_reset_test();
        let data_type = DataType::new("UnsupportedPolymarketCustomData", None, None);

        client
            .unsubscribe(&UnsubscribeCustomData::new(
                Some(*POLYMARKET_CLIENT_ID),
                None,
                data_type,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("unsupported custom data unsubscribe should be ignored");

        assert_eq!(client.rtds_feed.tracked_subscription_count(), 0);
    }

    #[rstest]
    fn subscribe_custom_rtds_reuses_single_wire_subscription_for_same_symbol() {
        let mut client = make_client_for_reset_test();
        let crypto_upper = rtds_crypto_data_type("BTCUSDT");
        let crypto_lower = rtds_crypto_data_type("btcusdt");

        client
            .subscribe(SubscribeCustomData::new(
                Some(*POLYMARKET_CLIENT_ID),
                None,
                crypto_upper,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("first RTDS subscribe");
        client
            .subscribe(SubscribeCustomData::new(
                Some(*POLYMARKET_CLIENT_ID),
                None,
                crypto_lower,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("second RTDS subscribe");

        assert_eq!(client.rtds_feed.tracked_subscription_count(), 1);
        assert_eq!(
            client
                .rtds_feed
                .tracked_data_type_count("crypto_prices:btcusdt"),
            2,
        );
    }

    #[rstest]
    fn unsubscribe_custom_rtds_last_reference_removes_wire_subscription() {
        let mut client = make_client_for_reset_test();
        let equity_data_type = rtds_equity_data_type("AAPL");

        client
            .subscribe(SubscribeCustomData::new(
                Some(*POLYMARKET_CLIENT_ID),
                None,
                equity_data_type.clone(),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("RTDS subscribe");

        client
            .unsubscribe(&UnsubscribeCustomData::new(
                Some(*POLYMARKET_CLIENT_ID),
                None,
                equity_data_type,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("RTDS unsubscribe");

        assert_eq!(client.rtds_feed.tracked_subscription_count(), 0);
    }

    #[rstest]
    #[tokio::test]
    async fn reset_replaces_rtds_feed_generation_after_shutdown_finishes() {
        let mut client = make_client_for_reset_test();
        let old_feed = client.rtds_feed.clone();
        let data_type = rtds_crypto_data_type("btcusdt");

        client
            .subscribe(SubscribeCustomData::new(
                Some(*POLYMARKET_CLIENT_ID),
                None,
                data_type,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .expect("RTDS subscribe");

        assert_eq!(old_feed.tracked_subscription_count(), 1);

        client.reset().expect("reset should succeed");

        assert_eq!(client.rtds_feed.tracked_subscription_count(), 1);
        assert!(client.reset_pending);

        client
            .disconnect()
            .await
            .expect("deferred reset shutdown should finish");

        assert_eq!(client.rtds_feed.tracked_subscription_count(), 0);
        assert!(!client.reset_pending);
        assert_eq!(
            old_feed.tracked_subscription_count(),
            1,
            "old-generation RTDS state should remain isolated from the reset generation",
        );
    }

    #[rstest]
    fn reset_preserves_rtds_proxy_url() {
        const PROXY_URL: &str = "http://reset-user:reset-proxy-secret@127.0.0.1:18090";
        let proxy_url = ProxyUrl::parse(PROXY_URL).unwrap();
        let mut client = make_client_for_reset_test_with_proxy(Some(proxy_url));
        let debug = format!("{client:?}");

        assert_eq!(client.rtds_feed.proxy_url().unwrap().expose(), PROXY_URL);
        assert!(!debug.contains("reset-proxy-secret"));

        client.reset().expect("reset should succeed");

        assert_eq!(client.proxy_url.as_ref().unwrap().expose(), PROXY_URL);
        assert_eq!(client.rtds_feed.proxy_url().unwrap().expose(), PROXY_URL);
    }

    #[rstest]
    #[tokio::test]
    async fn resolve_poll_task_retires_expired_runtime_state_when_auto_poll_disabled() {
        let mut client = make_client_for_reset_test();
        client.config.resolve_poll_enabled = false;
        client.config.resolve_poll_interval_secs = 1;

        let inst = seed_expired_instrument(&client, "0xTOKEN_YES", "0xCOND-POLL");
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst,
            PositionId::new("P-1"),
        );

        let instrument_id = inst.id();
        let token_id = Ustr::from(inst.raw_symbol().as_str());
        seed_expired_runtime_state(&client, &inst);

        client.register_resolve_poll_task().unwrap();

        wait_until_async(
            || async { !client.token_meta.contains_key(&Ustr::from("0xTOKEN_YES")) },
            tokio::time::Duration::from_secs(5),
        )
        .await;

        client.cancellation_token.cancel();
        client
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
            .await
            .expect("resolve poll task terminated");

        assert!(!client.active_quote_subs.contains(&instrument_id));
        assert!(!client.active_delta_subs.contains(&instrument_id));
        assert!(!client.active_trade_subs.contains(&instrument_id));
        assert!(
            client
                .active_instrument_status_subs
                .contains(&instrument_id)
        );
        assert!(client.active_instrument_close_subs.contains(&instrument_id));
        assert!(client.ws_open_tokens.contains(&token_id));
        assert!(
            !client
                .pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        assert!(client.pending_auto_loads.lock().is_empty());
        assert!(!client.order_books.contains_key(&instrument_id));
        assert!(!client.last_quotes.contains_key(&instrument_id));
        assert!(!client.token_meta.contains_key(&Ustr::from("0xTOKEN_YES")));
        assert!(client.instruments.load().contains_key(&instrument_id));
        assert!(
            client
                .resolve_poll_watchlist
                .contains_key(&"0xCOND-POLL".to_string())
        );
    }

    #[rstest]
    #[tokio::test]
    async fn resolve_poll_task_removes_unwatched_expired_instrument_from_cache() {
        let mut client = make_client_for_reset_test();
        client.config.resolve_poll_enabled = false;
        client.config.resolve_poll_interval_secs = 1;

        let inst = seed_expired_instrument(&client, "0xTOKEN_PURGED", "0xCOND-PURGED");
        let instrument_id = inst.id();

        seed_expired_runtime_state(&client, &inst);

        client.register_resolve_poll_task().unwrap();

        wait_until_async(
            || async { !client.instruments.load().contains_key(&instrument_id) },
            tokio::time::Duration::from_secs(5),
        )
        .await;

        client.cancellation_token.cancel();
        client
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
            .await
            .expect("resolve poll task terminated");

        assert!(!client.instruments.load().contains_key(&instrument_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from("0xTOKEN_PURGED"))
        );
        assert!(!client.order_books.contains_key(&instrument_id));
        assert!(!client.last_quotes.contains_key(&instrument_id));
        assert!(client.resolve_poll_watchlist.load().is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn resolve_poll_task_retires_each_terminal_condition_once() {
        let mut client = make_client_for_reset_test();
        client.config.resolve_poll_enabled = false;
        client.config.resolve_poll_interval_secs = 1;

        // Unexpired, so only the terminal sweep can retire it
        let expiration_ns = UnixNanos::from(u64::MAX);
        let inst = seed_instrument(
            &client,
            "0xCOND-ONCE-0xTOKEN_ONCE",
            "0xCOND-ONCE",
            expiration_ns,
        );
        let instrument_id = inst.id();
        client.active_quote_subs.insert(instrument_id);
        client
            .closed_condition_ids
            .lock()
            .insert("0xCOND-ONCE".to_string());

        client.register_resolve_poll_task().unwrap();

        wait_until_async(
            || async { !client.instruments.load().contains_key(&instrument_id) },
            tokio::time::Duration::from_secs(5),
        )
        .await;

        assert!(!client.active_quote_subs.contains(&instrument_id));

        // The live boundary refuses re-application, so no production path recreates this
        let republished = apply_live_instrument(
            &client.closed_condition_ids,
            &client.instrument_update_state,
            &client.instruments,
            &client.token_meta,
            &inst,
            |_| {},
        );
        assert!(!republished);

        // Retirement is one-shot: a later sweep must not walk the whole terminal set again
        cache_instrument_unchecked(&client.instruments, &client.token_meta, &inst);
        tokio::time::sleep(tokio::time::Duration::from_millis(2500)).await;

        client.cancellation_token.cancel();
        client
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
            .await
            .expect("resolve poll task terminated");

        assert!(client.instruments.load().contains_key(&instrument_id));
    }

    #[rstest]
    #[tokio::test]
    async fn resolve_poll_task_reretires_watchlisted_terminal_condition_until_settled() {
        let mut client = make_client_for_reset_test();
        client.config.resolve_poll_enabled = false;
        client.config.resolve_poll_interval_secs = 1;

        let expiration_ns = UnixNanos::from(u64::MAX);
        let inst = seed_instrument(
            &client,
            "0xCOND-WATCH-0xTOKEN_WATCH",
            "0xCOND-WATCH",
            expiration_ns,
        );
        let instrument_id = inst.id();
        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst,
            PositionId::new("P-WATCH"),
        );
        client.active_quote_subs.insert(instrument_id);
        client
            .closed_condition_ids
            .lock()
            .insert("0xCOND-WATCH".to_string());

        client.register_resolve_poll_task().unwrap();

        // Live subscription retires, but settlement metadata is kept
        wait_until_async(
            || async { !client.active_quote_subs.contains(&instrument_id) },
            tokio::time::Duration::from_secs(5),
        )
        .await;

        assert!(client.instruments.load().contains_key(&instrument_id));

        // Settlement drops the watch entry, so the next cycle must revisit the condition
        client
            .resolve_poll_watchlist
            .remove(&"0xCOND-WATCH".to_string());

        wait_until_async(
            || async { !client.instruments.load().contains_key(&instrument_id) },
            tokio::time::Duration::from_secs(5),
        )
        .await;

        client.cancellation_token.cancel();
        client
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
            .await
            .expect("resolve poll task terminated");

        assert!(!client.instruments.load().contains_key(&instrument_id));
        assert!(
            !client
                .token_meta
                .contains_key(&Ustr::from("0xCOND-WATCH-0xTOKEN_WATCH"))
        );
    }

    #[rstest]
    #[tokio::test]
    async fn resolve_poll_task_bulk_retirement_keeps_only_watchlist_required_state() {
        let mut client = make_client_for_reset_test();
        client.config.resolve_poll_enabled = false;
        client.config.resolve_poll_interval_secs = 1;

        let watched_count = 8usize;
        let unwatched_count = 5usize;

        for index in 0..watched_count {
            let raw_symbol = format!("0xTOKEN_WATCHED_{index}");
            let condition_id = format!("0xCOND-WATCHED-{index}");
            let position_id = format!("P-WATCHED-{index}");
            let inst = seed_expired_instrument(&client, &raw_symbol, &condition_id);

            upsert_resolve_watch_entry_from_instrument(
                &client.resolve_poll_watchlist,
                &inst,
                PositionId::new(position_id.as_str()),
            );

            seed_expired_runtime_state(&client, &inst);
        }

        for index in 0..unwatched_count {
            let raw_symbol = format!("0xTOKEN_PURGED_{index}");
            let condition_id = format!("0xCOND-PURGED-{index}");
            let inst = seed_expired_instrument(&client, &raw_symbol, &condition_id);

            seed_expired_runtime_state(&client, &inst);
        }

        client.register_resolve_poll_task().unwrap();

        wait_until_async(
            || async {
                client.token_meta.is_empty()
                    && client.order_books.is_empty()
                    && client.last_quotes.is_empty()
                    && client.active_quote_subs.is_empty()
                    && client.active_delta_subs.is_empty()
                    && client.active_trade_subs.is_empty()
                    && client.active_instrument_status_subs.len() == watched_count
                    && client.active_instrument_close_subs.len() == watched_count
                    && client.ws_open_tokens.len() == watched_count
                    && client.pending_snapshot_after_tick_change.is_empty()
                    && client.pending_auto_loads.lock().is_empty()
                    && client.instruments.load().len() == watched_count
                    && client.resolve_poll_watchlist.load().len() == watched_count
            },
            tokio::time::Duration::from_secs(5),
        )
        .await;

        client.cancellation_token.cancel();
        client
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
            .await
            .expect("resolve poll task terminated");

        assert!(client.token_meta.is_empty());
        assert!(client.order_books.is_empty());
        assert!(client.last_quotes.is_empty());
        assert!(client.active_quote_subs.is_empty());
        assert!(client.active_delta_subs.is_empty());
        assert!(client.active_trade_subs.is_empty());
        assert_eq!(client.active_instrument_status_subs.len(), watched_count);
        assert_eq!(client.active_instrument_close_subs.len(), watched_count);
        assert_eq!(client.ws_open_tokens.len(), watched_count);
        assert!(client.pending_snapshot_after_tick_change.is_empty());
        assert!(client.pending_auto_loads.lock().is_empty());
        assert_eq!(client.instruments.load().len(), watched_count);
        assert_eq!(client.resolve_poll_watchlist.load().len(), watched_count);
    }

    #[rstest]
    #[tokio::test]
    async fn spawn_message_handler_does_not_reseed_token_meta_for_watched_expired_instrument() {
        let mut client = make_client_for_reset_test();
        let inst = seed_expired_instrument(&client, "0xTOKEN_RETAINED", "0xCOND-RETAINED");

        upsert_resolve_watch_entry_from_instrument(
            &client.resolve_poll_watchlist,
            &inst,
            PositionId::new("P-1"),
        );

        let instrument_id = inst.id();
        let token_id = Ustr::from(inst.raw_symbol().as_str());

        retire_local_instrument_state(
            instrument_id,
            &client.instruments,
            &client.token_meta,
            &client.order_books,
            &client.last_quotes,
            &client.active_quote_subs,
            &client.active_delta_subs,
            &client.active_trade_subs,
            &client.active_instrument_status_subs,
            &client.active_instrument_close_subs,
            &client.closed_condition_ids,
            &client.resolve_poll_watchlist,
            &client.pending_snapshot_after_tick_change,
            &client.pending_auto_loads,
            &client.ws_open_tokens,
            &client.ws_sub_mutex,
            &client.ws_client.handle(),
        )
        .await;

        assert!(client.instruments.load().contains_key(&instrument_id));
        assert!(!client.token_meta.contains_key(&token_id));

        for startup in 1..=2 {
            if !client.tasks.is_open() {
                client.tasks.start_generation().expect("fresh generation");
                client.cancellation_token = client.tasks.cancellation_token();
            }
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<PolymarketWsMessage>();
            drop(tx);
            client.register_message_handler(rx).unwrap();
            client
                .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
                .await
                .expect("message handler terminated");

            assert!(
                client.instruments.load().contains_key(&instrument_id),
                "watched expired instrument metadata should remain available until resolution",
            );
            assert!(
                !client.token_meta.contains_key(&token_id),
                "message-handler startup #{startup} must not re-seed token_meta for retained expired instruments",
            );
        }
    }

    #[rstest]
    #[tokio::test]
    async fn spawn_message_handler_does_not_reseed_terminal_condition_routing() {
        let client = make_client_for_reset_test();
        let expiration_ns = UnixNanos::from(
            client
                .clock
                .get_time_ns()
                .as_u64()
                .saturating_add(1_000_000_000),
        );
        let inst = seed_instrument(
            &client,
            "0xCOND-TERMINAL-0xTOKEN_TERMINAL",
            "0xCOND-TERMINAL",
            expiration_ns,
        );
        let token_id = Ustr::from(inst.raw_symbol().as_str());

        client
            .closed_condition_ids
            .lock()
            .insert("0xCOND-TERMINAL".to_string());
        client.token_meta.clear();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<PolymarketWsMessage>();
        drop(tx);
        client.register_message_handler(rx).unwrap();
        client
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
            .await
            .expect("message handler terminated");

        assert!(client.instruments.load().contains_key(&inst.id()));
        assert!(!client.token_meta.contains_key(&token_id));
    }

    // Matches EXPIRED_ENGINE_SWEEP_INTERVAL_NS in crates/adapters/sandbox/src/execution.rs.
    const SANDBOX_SWEEP_INTERVAL_NS: u64 = 60 * NANOSECONDS_IN_SECOND;
    const CHURN_CYCLES: u64 = 5;
    const CHURN_INSTRUMENTS_PER_CYCLE: usize = 4;

    struct ChurnSandbox {
        client: SandboxExecutionClient,
        cache: Rc<RefCell<Cache>>,
        test_clock: Rc<RefCell<TestClock>>,
        rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    }

    fn setup_churn_sandbox() -> ChurnSandbox {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let test_clock = Rc::new(RefCell::new(TestClock::new()));
        let clock: Rc<RefCell<dyn Clock>> = test_clock.clone();

        let config = SandboxExecutionClientConfig::builder()
            .venue(*POLYMARKET_VENUE)
            .build();
        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            ClientId::new("SANDBOX"),
            config.venue,
            config.oms_type,
            config.account_id,
            config.account_type,
            config.base_currency,
            cache.clone(),
        );
        let mut client = SandboxExecutionClient::new(core, config, clock, cache.clone());

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        replace_exec_event_sender(tx);
        client.start().expect("sandbox client should start");

        ChurnSandbox {
            client,
            cache,
            test_clock,
            rx,
        }
    }

    fn churn_quote(instrument_id: InstrumentId) -> QuoteTick {
        QuoteTick::new(
            instrument_id,
            Price::from("0.504"),
            Price::from("0.506"),
            Quantity::from("5.00"),
            Quantity::from("8.00"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    // The data-runtime maps and sets that a retired Polymarket instrument must vacate, labelled so
    // a count mismatch names the owner that retained state.
    fn data_runtime_owner_counts(client: &PolymarketDataClient) -> Vec<(&'static str, usize)> {
        vec![
            ("instruments", client.instruments.len()),
            ("token_meta", client.token_meta.len()),
            ("order_books", client.order_books.len()),
            ("last_quotes", client.last_quotes.len()),
            ("active_quote_subs", client.active_quote_subs.len()),
            ("active_delta_subs", client.active_delta_subs.len()),
            ("active_trade_subs", client.active_trade_subs.len()),
            (
                "active_instrument_status_subs",
                client.active_instrument_status_subs.len(),
            ),
            (
                "active_instrument_close_subs",
                client.active_instrument_close_subs.len(),
            ),
            ("ws_open_tokens", client.ws_open_tokens.len()),
            (
                "pending_snapshot_after_tick_change",
                client.pending_snapshot_after_tick_change.len(),
            ),
            ("pending_auto_loads", client.pending_auto_loads.lock().len()),
        ]
    }

    // Deterministic owner-count regression for the reported high-churn topology: Polymarket data
    // plus Sandbox execution, streaming quote-only short-lived instruments. Each cycle loads a
    // fresh batch, creates Sandbox matching engines from quotes, advances past expiry, and runs
    // both the Sandbox periodic sweep and the Polymarket expiry retirement. Owner counts (not
    // allocator RSS) are the assertion: `develop` enables mimalloc, whose segment caching means a
    // logical release need not lower resident memory.
    #[rstest]
    #[tokio::test]
    async fn instrument_churn_returns_data_runtime_cache_and_engine_owners_to_baseline() {
        let client = make_client_for_reset_test();
        let mut sandbox = setup_churn_sandbox();

        for cycle in 0..CHURN_CYCLES {
            let sweep_ns = SANDBOX_SWEEP_INTERVAL_NS * (cycle + 1);
            let mut cycle_ids = Vec::with_capacity(CHURN_INSTRUMENTS_PER_CYCLE);

            for index in 0..CHURN_INSTRUMENTS_PER_CYCLE {
                let raw_symbol = format!("0xTOKEN_CHURN_{cycle}_{index}");
                let condition_id = format!("0xCOND-CHURN-{cycle}-{index}");
                let instrument = seed_instrument(
                    &client,
                    &raw_symbol,
                    &condition_id,
                    UnixNanos::from(sweep_ns - 1),
                );
                let instrument_id = instrument.id();

                seed_expired_runtime_state(&client, &instrument);

                let quote = churn_quote(instrument_id);
                sandbox
                    .cache
                    .borrow_mut()
                    .add_instrument(instrument)
                    .expect("instrument should enter the global cache");
                sandbox
                    .client
                    .process_quote_tick(&quote)
                    .expect("quote should create a sandbox matching engine");
                sandbox
                    .cache
                    .borrow_mut()
                    .add_quote(quote)
                    .expect("quote should enter the global cache");

                cycle_ids.push(instrument_id);
            }

            assert_eq!(
                sandbox.client.matching_engine_count(),
                CHURN_INSTRUMENTS_PER_CYCLE,
                "cycle {cycle} should hold one matching engine per streamed instrument",
            );
            assert_eq!(
                sandbox.cache.borrow().instrument_ids(None).len(),
                CHURN_INSTRUMENTS_PER_CYCLE,
                "cycle {cycle} global-cache instruments must not carry earlier cycles",
            );

            // Pins the quote before the sweep so the post-sweep absence check cannot pass vacuously.
            for instrument_id in &cycle_ids {
                assert_eq!(
                    sandbox.cache.borrow().quote(instrument_id),
                    Some(&churn_quote(*instrument_id)),
                    "cycle {cycle} cache should hold the streamed quote for {instrument_id}",
                );
            }

            for (owner, count) in data_runtime_owner_counts(&client) {
                assert_eq!(
                    count, CHURN_INSTRUMENTS_PER_CYCLE,
                    "cycle {cycle} data-runtime {owner} should hold one entry per streamed instrument",
                );
            }

            // Quote-only churn opens no position, so nothing reaches the resolution watchlist and
            // no instrument is retained as watchlist metadata.
            assert_eq!(client.resolve_poll_watchlist.len(), 0);

            let sweep_events = sandbox
                .test_clock
                .borrow_mut()
                .advance_time(UnixNanos::from(sweep_ns), true);
            assert_eq!(
                sweep_events.len(),
                1,
                "cycle {cycle} should release exactly one sandbox expiry sweep",
            );

            for handler in sandbox.test_clock.borrow().match_handlers(sweep_events) {
                handler.run();
            }

            assert_eq!(
                sandbox.client.matching_engine_count(),
                0,
                "cycle {cycle} sandbox sweep should retire every expired quote-only engine",
            );
            assert_eq!(
                sandbox.cache.borrow().instrument_ids(None).len(),
                0,
                "cycle {cycle} sandbox sweep should purge every expired instrument from the cache",
            );

            for instrument_id in &cycle_ids {
                assert!(
                    sandbox.cache.borrow().quote(instrument_id).is_none(),
                    "cycle {cycle} cache quotes should be purged with {instrument_id}",
                );
            }

            retire_expired_local_instruments(
                UnixNanos::from(sweep_ns),
                &client.instruments,
                &client.token_meta,
                &client.order_books,
                &client.last_quotes,
                &client.active_quote_subs,
                &client.active_delta_subs,
                &client.active_trade_subs,
                &client.active_instrument_status_subs,
                &client.active_instrument_close_subs,
                &client.closed_condition_ids,
                &client.resolve_poll_watchlist,
                &client.pending_snapshot_after_tick_change,
                &client.pending_auto_loads,
                &client.ws_open_tokens,
                &client.ws_sub_mutex,
                &client.ws_client.handle(),
            )
            .await;

            for (owner, count) in data_runtime_owner_counts(&client) {
                assert_eq!(
                    count, 0,
                    "cycle {cycle} retirement should release data-runtime {owner}",
                );
            }

            assert_eq!(client.resolve_poll_watchlist.len(), 0);
        }

        // Neither sweep settles: an expired quote-only instrument has no exposure to close.
        let execution_events: Vec<ExecutionEvent> =
            std::iter::from_fn(|| sandbox.rx.try_recv().ok()).collect();
        assert!(
            execution_events.is_empty(),
            "quote-only churn must not emit execution events, was {execution_events:?}",
        );

        sandbox.client.stop().expect("sandbox client should stop");
    }
}
