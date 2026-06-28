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
use nautilus_model::events::PositionEvent;

use super::{
    PolymarketDataClient,
    dispatch::{WsMessageContext, handle_ws_message},
    runtime::{retire_expired_local_instruments, seed_token_meta_from_live_instruments},
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

        seed_token_meta_from_live_instruments(
            self.clock.get_time_ns(),
            &self.instruments,
            &self.token_meta,
        );

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
        let pending_snapshot_after_tick_change = self.pending_snapshot_after_tick_change.clone();
        let pending_auto_loads = self.pending_auto_loads.clone();
        let ws_open_tokens = self.ws_open_tokens.clone();
        let ws_sub_mutex = self.ws_sub_mutex.clone();
        let ws = self.ws_client.clone_subscription_handle();

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

        if resolve_poll_enabled {
            log::debug!("Polymarket resolve poll task started");
        } else {
            log::debug!(
                "Polymarket resolve poll task started with resolution fetch disabled; expiry retirement remains active"
            );
        }

        let handle = get_runtime().spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    _ = interval.tick() => {
                        let now_ns = clock.get_time_ns();
                        retire_expired_local_instruments(
                            now_ns,
                            &instruments,
                            &token_meta,
                            &order_books,
                            &last_quotes,
                            &active_quote_subs,
                            &active_delta_subs,
                            &active_trade_subs,
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

        log::debug!("Bootstrapping instruments from Gamma API...");
        self.bootstrap_instruments().await?;
        log::debug!(
            "Bootstrap complete, {} instruments loaded",
            self.instruments.load().len(),
        );

        self.ws_client.connect().await?;

        if self.config.subscribe_new_markets {
            log::debug!("Subscribing to new markets...");
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::Ordering};

    use nautilus_common::{
        clients::DataClient,
        live::runner::replace_data_event_sender,
        messages::{
            DataEvent,
            data::{SubscribeCustomData, UnsubscribeCustomData},
        },
        testing::wait_until_async,
    };
    use nautilus_core::{Params, UUID4, UnixNanos};
    use nautilus_model::{
        data::{DataType, QuoteTick},
        enums::BookType,
        identifiers::{ClientId, InstrumentId, PositionId, Symbol},
        instruments::{Instrument, InstrumentAny, stubs::binary_option},
        orderbook::OrderBook,
        types::{Currency, Price, Quantity},
    };
    use nautilus_network::{retry::RetryConfig, websocket::TransportBackend};
    use rstest::rstest;
    use serde_json::Value;
    use ustr::Ustr;

    use super::{super::NEW_MARKET_FETCH_MAX_CONCURRENCY_CAP, *};
    use crate::{
        common::consts::POLYMARKET_CLIENT_ID,
        config::PolymarketDataClientConfig,
        data::{instruments::cache_instrument, runtime::retire_local_instrument_state},
        http::{
            clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
            gamma::PolymarketGammaHttpClient,
        },
        resolve::upsert_resolve_watch_entry_from_instrument,
        websocket::{client::PolymarketWebSocketClient, messages::PolymarketWsMessage},
    };

    fn make_client_for_reset_test() -> PolymarketDataClient {
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
        let ws = PolymarketWebSocketClient::new_market(
            Some("ws://localhost/ws/market".to_string()),
            false,
            TransportBackend::default(),
        );

        PolymarketDataClient::new(
            ClientId::from("POLY-TEST"),
            PolymarketDataClientConfig::default(),
            gamma,
            clob,
            data_api,
            ws,
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
        let ws = PolymarketWebSocketClient::new_market(
            Some("ws://localhost/ws/market".to_string()),
            false,
            TransportBackend::default(),
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
        let mut binary = binary_option();
        binary.id = InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str());
        binary.raw_symbol = Symbol::new(raw_symbol);
        binary.currency = Currency::pUSD();
        binary.activation_ns = UnixNanos::default();
        binary.expiration_ns = UnixNanos::from(
            client
                .clock
                .get_time_ns()
                .as_u64()
                .saturating_sub(1_000_000_000),
        );

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
        cache_instrument(&client.instruments, &client.token_meta, &inst);
        inst
    }

    fn seed_expired_runtime_state(client: &PolymarketDataClient, inst: &InstrumentAny) {
        let instrument_id = inst.id();

        client.active_quote_subs.insert(instrument_id);
        client.active_delta_subs.insert(instrument_id);
        client.active_trade_subs.insert(instrument_id);
        client
            .ws_open_tokens
            .insert(Ustr::from(inst.raw_symbol().as_str()));
        client
            .pending_snapshot_after_tick_change
            .insert(instrument_id);
        client
            .pending_auto_loads
            .lock()
            .expect("pending_auto_loads mutex poisoned")
            .insert(instrument_id);
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
        client.ws_open_tokens.insert(Ustr::from("0xCOND-0xTOKEN"));
        client
            .new_market_inflight_keys
            .insert("btc-updown-5m-1".to_string(), ());
        client
            .pending_snapshot_after_tick_change
            .insert(instrument_id);
        client
            .pending_auto_loads
            .lock()
            .expect("pending_auto_loads mutex poisoned")
            .insert(instrument_id);
        client.auto_load_scheduled.store(true, Ordering::Release);

        client
            .reset()
            .expect("reset should succeed for in-memory state");

        assert!(old_token.is_cancelled());
        assert!(!client.cancellation_token.is_cancelled());

        assert!(client.active_quote_subs.is_empty());
        assert!(client.active_delta_subs.is_empty());
        assert!(client.active_trade_subs.is_empty());
        assert!(client.ws_open_tokens.is_empty());
        assert!(client.new_market_inflight_keys.is_empty());
        assert!(client.pending_snapshot_after_tick_change.is_empty());
        assert!(
            client
                .pending_auto_loads
                .lock()
                .expect("pending_auto_loads mutex poisoned")
                .is_empty()
        );
        assert!(!client.auto_load_scheduled.load(Ordering::Acquire));
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
    fn reset_replaces_rtds_feed_generation() {
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

        assert_eq!(client.rtds_feed.tracked_subscription_count(), 0);
        assert_eq!(
            old_feed.tracked_subscription_count(),
            1,
            "old-generation RTDS state should remain isolated from the reset generation",
        );
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

        client.spawn_resolve_poll_task();

        wait_until_async(
            || async { !client.token_meta.contains_key(&Ustr::from("0xTOKEN_YES")) },
            tokio::time::Duration::from_secs(5),
        )
        .await;

        client.cancellation_token.cancel();
        client
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
            .await;

        assert!(!client.active_quote_subs.contains(&instrument_id));
        assert!(!client.active_delta_subs.contains(&instrument_id));
        assert!(!client.active_trade_subs.contains(&instrument_id));
        assert!(!client.ws_open_tokens.contains(&token_id));
        assert!(
            !client
                .pending_snapshot_after_tick_change
                .contains(&instrument_id)
        );
        assert!(
            client
                .pending_auto_loads
                .lock()
                .expect("pending_auto_loads mutex poisoned")
                .is_empty()
        );
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

        client.spawn_resolve_poll_task();

        wait_until_async(
            || async { !client.instruments.load().contains_key(&instrument_id) },
            tokio::time::Duration::from_secs(5),
        )
        .await;

        client.cancellation_token.cancel();
        client
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
            .await;

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

        client.spawn_resolve_poll_task();

        wait_until_async(
            || async {
                client.token_meta.is_empty()
                    && client.order_books.is_empty()
                    && client.last_quotes.is_empty()
                    && client.active_quote_subs.is_empty()
                    && client.active_delta_subs.is_empty()
                    && client.active_trade_subs.is_empty()
                    && client.ws_open_tokens.is_empty()
                    && client.pending_snapshot_after_tick_change.is_empty()
                    && client
                        .pending_auto_loads
                        .lock()
                        .expect("pending_auto_loads mutex poisoned")
                        .is_empty()
                    && client.instruments.load().len() == watched_count
                    && client.resolve_poll_watchlist.load().len() == watched_count
            },
            tokio::time::Duration::from_secs(5),
        )
        .await;

        client.cancellation_token.cancel();
        client
            .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
            .await;

        assert!(client.token_meta.is_empty());
        assert!(client.order_books.is_empty());
        assert!(client.last_quotes.is_empty());
        assert!(client.active_quote_subs.is_empty());
        assert!(client.active_delta_subs.is_empty());
        assert!(client.active_trade_subs.is_empty());
        assert!(client.ws_open_tokens.is_empty());
        assert!(client.pending_snapshot_after_tick_change.is_empty());
        assert!(
            client
                .pending_auto_loads
                .lock()
                .expect("pending_auto_loads mutex poisoned")
                .is_empty()
        );
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
            &client.resolve_poll_watchlist,
            &client.pending_snapshot_after_tick_change,
            &client.pending_auto_loads,
            &client.ws_open_tokens,
            &client.ws_sub_mutex,
            &client.ws_client.clone_subscription_handle(),
        )
        .await;

        assert!(client.instruments.load().contains_key(&instrument_id));
        assert!(!client.token_meta.contains_key(&token_id));

        for startup in 1..=2 {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<PolymarketWsMessage>();
            drop(tx);
            client.spawn_message_handler(rx);
            client
                .await_tasks_with_timeout(tokio::time::Duration::from_secs(1))
                .await;

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
}
