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

use ahash::AHashMap;
use dashmap::DashMap;
use nautilus_common::{
    live::get_runtime,
    msgbus::{self, TypedHandler},
};
use nautilus_core::AtomicSet;
use nautilus_model::{events::PositionEvent, instruments::Instrument};

use super::{
    PolymarketDataClient,
    dispatch::{WsMessageContext, handle_ws_message},
    instruments::TokenMeta,
};
use crate::{
    data_types::register_polymarket_custom_data,
    resolve::{
        ResolveBatchErrorMode, ResolveWatchSelectionMode, collect_resolve_watch_selection,
        fetch_and_apply_resolutions_by_condition_ids, pause_resolve_watch_entries,
        update_resolve_watchlist_from_position_event,
    },
    websocket::messages::PolymarketWsMessage,
};

impl PolymarketDataClient {
    fn ensure_position_event_subscription(&mut self) {
        if self.position_event_handler.is_some() {
            return;
        }

        let watchlist = self.resolve_poll_watchlist.clone();
        let instruments = self.instruments.clone();
        let handler = TypedHandler::from(move |event: &PositionEvent| {
            update_resolve_watchlist_from_position_event(&watchlist, &instruments, event);
        });

        msgbus::subscribe_position_events("events.position.*".into(), handler.clone(), Some(10));
        self.position_event_handler = Some(handler);
    }

    fn clear_position_event_subscription(&mut self) {
        if let Some(handler) = self.position_event_handler.take() {
            msgbus::unsubscribe_position_events("events.position.*".into(), &handler);
        }
    }

    fn spawn_message_handler(
        &mut self,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<PolymarketWsMessage>,
    ) {
        let cancellation = self.cancellation_token.clone();

        for (token_id, instrument) in self.provider.build_token_map() {
            self.token_meta.insert(
                token_id,
                TokenMeta {
                    instrument_id: instrument.id(),
                    price_precision: instrument.price_precision(),
                    size_precision: instrument.size_precision(),
                },
            );
        }

        let ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_meta: self.token_meta.clone(),
            instruments: self.instruments.clone(),
            gamma_client: self.provider.http_client().clone(),
            clob_public_client: self.clob_public_client.clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: self.resolve_watch_apply_mutex.clone(),
            pending_snapshot_after_tick_change: self.pending_snapshot_after_tick_change.clone(),
            new_market_inflight_keys: self.new_market_inflight_keys.clone(),
            new_market_fetch_semaphore: self.new_market_fetch_semaphore.clone(),
            subscribe_new_markets: self.config.subscribe_new_markets,
            new_market_filter: self.config.new_market_filter.clone(),
            cancellation_token: cancellation.clone(),
        };

        let handle = get_runtime().spawn(async move {
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
        });

        self.tasks.push(handle);
    }

    pub(super) fn spawn_resolve_poll_task(&mut self) {
        if !self.config.resolve_poll_enabled {
            log::info!("Polymarket resolve polling disabled");
            return;
        }

        let cancellation = self.cancellation_token.clone();
        let gamma_client = self.provider.http_client().clone();
        let clob_public_client = self.clob_public_client.clone();
        let clock = self.clock;
        let interval_secs = self.config.resolve_poll_interval_secs.max(1);
        let grace_secs = self.config.resolve_poll_grace_secs;
        let max_wait_secs = self.config.resolve_poll_max_wait_secs.max(grace_secs);

        let ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            token_meta: self.token_meta.clone(),
            instruments: self.instruments.clone(),
            gamma_client: gamma_client.clone(),
            clob_public_client: clob_public_client.clone(),
            filters: self.provider.filters(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            resolve_poll_watchlist: self.resolve_poll_watchlist.clone(),
            resolve_watch_apply_mutex: self.resolve_watch_apply_mutex.clone(),
            pending_snapshot_after_tick_change: self.pending_snapshot_after_tick_change.clone(),
            new_market_inflight_keys: self.new_market_inflight_keys.clone(),
            new_market_fetch_semaphore: self.new_market_fetch_semaphore.clone(),
            subscribe_new_markets: self.config.subscribe_new_markets,
            new_market_filter: self.config.new_market_filter.clone(),
            cancellation_token: cancellation.clone(),
        };

        let watchlist = self.resolve_poll_watchlist.clone();

        let handle = get_runtime().spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    _ = interval.tick() => {
                        let now_ns = clock.get_time_ns();
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
                            log::info!(
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
        });

        self.tasks.push(handle);
    }

    pub(super) async fn await_tasks_with_timeout(&mut self, timeout: tokio::time::Duration) {
        for handle in self.tasks.drain(..) {
            let _ = tokio::time::timeout(timeout, handle).await;
        }
    }

    pub(super) fn start_client(&mut self) {
        log::info!("Starting Polymarket data client: {}", self.client_id);
        self.ensure_position_event_subscription();
    }

    pub(super) fn stop_client(&mut self) {
        log::info!("Stopping Polymarket data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.clear_position_event_subscription();
    }

    pub(super) fn reset_client(&mut self) {
        log::debug!("Resetting Polymarket data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected
            .store(false, std::sync::atomic::Ordering::Relaxed);

        // Hard reset contract: discard all retained reconnect replay state from
        // the previous generation. Callers must rebuild instrument/data
        // subscriptions after connect().
        // Stop the WS handler path even when reset is called without a graceful disconnect.
        self.ws_client.abort();
        self.ws_client.clear_reconnect_state();
        self.rtds_feed.abort();
        self.resolve_poll_watchlist.store(AHashMap::new());
        self.clear_position_event_subscription();

        for handle in self.tasks.drain(..) {
            handle.abort();
        }

        self.instruments.store(AHashMap::new());
        self.token_meta.clear();
        self.order_books.clear();
        self.last_quotes.clear();

        self.active_quote_subs = std::sync::Arc::new(AtomicSet::new());
        self.active_delta_subs = std::sync::Arc::new(AtomicSet::new());
        self.active_trade_subs = std::sync::Arc::new(AtomicSet::new());
        self.pending_snapshot_after_tick_change = std::sync::Arc::new(AtomicSet::new());
        self.new_market_inflight_keys = std::sync::Arc::new(DashMap::new());
        self.ws_open_tokens = std::sync::Arc::new(AtomicSet::new());
        self.rtds_feed = crate::rtds::PolymarketRtdsFeed::new(
            self.config.rtds_url(),
            self.config.transport_backend,
            self.clock,
            self.data_sender.clone(),
        );

        self.pending_auto_loads
            .lock()
            .expect("pending_auto_loads mutex poisoned")
            .clear();
        self.auto_load_scheduled
            .store(false, std::sync::atomic::Ordering::Release);

        self.cancellation_token = tokio_util::sync::CancellationToken::new();
    }

    pub(super) async fn connect_client(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        self.cancellation_token = tokio_util::sync::CancellationToken::new();
        self.ensure_position_event_subscription();
        register_polymarket_custom_data();

        log::info!("Connecting Polymarket data client");

        log::info!("Bootstrapping instruments from Gamma API...");
        self.bootstrap_instruments().await?;
        log::info!(
            "Bootstrap complete, {} instruments loaded",
            self.instruments.load().len(),
        );

        self.ws_client.connect().await?;

        if self.config.subscribe_new_markets {
            log::info!("Subscribing to new markets...");
            self.ws_client.subscribe_market(vec![]).await?;
        }

        let rx = self
            .ws_client
            .take_message_receiver()
            .ok_or_else(|| anyhow::anyhow!("WS message receiver not available after connect"))?;

        self.spawn_message_handler(rx);
        self.spawn_instrument_refresh_task();
        self.spawn_resolve_poll_task();

        if self.rtds_feed.has_subscriptions() {
            self.rtds_feed.connect().await?;
        }

        self.is_connected
            .store(true, std::sync::atomic::Ordering::Relaxed);
        log::info!("Connected Polymarket data client");

        Ok(())
    }

    pub(super) async fn disconnect_client(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        log::info!("Disconnecting Polymarket data client");

        self.cancellation_token.cancel();
        self.await_tasks_with_timeout(tokio::time::Duration::from_secs(5))
            .await;

        self.ws_client.disconnect().await?;
        self.rtds_feed.disconnect().await;

        self.is_connected
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.clear_position_event_subscription();
        log::info!("Disconnected Polymarket data client");

        Ok(())
    }
}
