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

//! WebSocket client for the Coinbase Advanced Trade API.
//!
//! Manages connection lifecycle, JWT-authenticated subscriptions, and dispatches
//! parsed Nautilus messages through the [`FeedHandler`].

use std::{
    num::NonZeroU32,
    str::FromStr,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use arc_swap::ArcSwap;
use nautilus_common::live::get_runtime;
use nautilus_core::AtomicMap;
use nautilus_model::{
    data::BarType,
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    mode::ConnectionMode,
    ratelimiter::quota::Quota,
    websocket::{
        SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use ustr::Ustr;

use crate::{
    common::{
        consts::{
            RECONNECT_BACKOFF_FACTOR, RECONNECT_BASE_BACKOFF, RECONNECT_JITTER_MS,
            RECONNECT_MAX_BACKOFF, RECONNECT_TIMEOUT, WS_DISCONNECT_TIMEOUT, WS_HEARTBEAT_SECS,
        },
        credential::CoinbaseCredential,
        enums::CoinbaseWsChannel,
    },
    websocket::{
        handler::{FeedHandler, HandlerCommand, NautilusWsMessage},
        messages::{CoinbaseWsAction, CoinbaseWsSubscription},
    },
};

/// Coinbase WebSocket connection rate limit (8 per second per IP).
pub static COINBASE_WS_CONNECTION_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(8).expect("non-zero")).expect("valid constant")
});

/// Coinbase WebSocket subscribe/unsubscribe rate limit (8 per second per IP).
pub static COINBASE_WS_SUBSCRIPTION_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(8).expect("non-zero")).expect("valid constant")
});

/// Rate-limit key for subscribe/unsubscribe operations.
pub const COINBASE_RATE_LIMIT_KEY_SUBSCRIPTION: &str = "subscription";

/// Pre-interned [`COINBASE_RATE_LIMIT_KEY_SUBSCRIPTION`] slice.
pub static COINBASE_WS_SUBSCRIPTION_KEYS: LazyLock<[Ustr; 1]> =
    LazyLock::new(|| [Ustr::from(COINBASE_RATE_LIMIT_KEY_SUBSCRIPTION)]);

/// WebSocket client for Coinbase Advanced Trade market data and user streams.
///
/// Manages connection lifecycle, subscription state, and JWT authentication.
/// Spawns a [`FeedHandler`] task that parses raw messages into Nautilus types.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.coinbase", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.coinbase")
)]
pub struct CoinbaseWebSocketClient {
    url: String,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    signal: Arc<AtomicBool>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    /// Maps a canonical wire `product_id` to the `product_id` the caller
    /// subscribed or submitted with. Coinbase rewrites aliased products to
    /// their canonical form on the wire (e.g. `BTC-USDC -> BTC-USD`), so
    /// inbound messages must be re-keyed to the caller's id before parsing.
    subscription_aliases: Arc<AtomicMap<Ustr, Ustr>>,
    bar_types: ahash::AHashMap<String, BarType>,
    subscriptions: SubscriptionState,
    credential: Option<CoinbaseCredential>,
    account_id: Option<AccountId>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
}

impl Clone for CoinbaseWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            connection_mode: Arc::clone(&self.connection_mode),
            signal: Arc::clone(&self.signal),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: None,
            instruments: Arc::clone(&self.instruments),
            subscription_aliases: Arc::clone(&self.subscription_aliases),
            bar_types: self.bar_types.clone(),
            subscriptions: self.subscriptions.clone(),
            credential: self.credential.clone(),
            account_id: self.account_id,
            task_handle: None,
            transport_backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        }
    }
}

impl CoinbaseWebSocketClient {
    /// Creates a new [`CoinbaseWebSocketClient`] for public market data.
    pub fn new(url: &str, transport_backend: TransportBackend, proxy_url: Option<String>) -> Self {
        let (placeholder_tx, _) = tokio::sync::mpsc::unbounded_channel();

        Self {
            url: url.to_string(),
            connection_mode: Arc::new(ArcSwap::from_pointee(AtomicU8::new(
                ConnectionMode::Closed.as_u8(),
            ))),
            signal: Arc::new(AtomicBool::new(false)),
            cmd_tx: Arc::new(tokio::sync::RwLock::new(placeholder_tx)),
            out_rx: None,
            instruments: Arc::new(AtomicMap::new()),
            subscription_aliases: Arc::new(AtomicMap::new()),
            bar_types: ahash::AHashMap::new(),
            subscriptions: SubscriptionState::new('|'),
            credential: None,
            account_id: None,
            task_handle: None,
            transport_backend,
            proxy_url,
        }
    }

    /// Creates a new [`CoinbaseWebSocketClient`] with credentials for authenticated channels.
    pub fn with_credential(
        url: &str,
        credential: CoinbaseCredential,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let mut client = Self::new(url, transport_backend, proxy_url);
        client.credential = Some(credential);
        client
    }

    /// Sets the account ID used when emitting user-channel execution reports.
    ///
    /// Propagates to the feed handler when the connection is active so that
    /// subsequent user events carry the correct account identifier.
    pub async fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = Some(account_id);

        let cmd_tx = self.cmd_tx.read().await;
        if let Err(e) = cmd_tx.send(HandlerCommand::SetAccountId(account_id)) {
            log::debug!("Failed to send SetAccountId: {e}");
        }
    }

    /// Bulk-populates the instrument cache.
    ///
    /// Safe to call before or after [`Self::connect`]. When called before
    /// connect, instruments are picked up by the initial `InitializeInstruments`
    /// command the client sends to the handler; when called after, a fresh
    /// `InitializeInstruments` command is sent to refresh the handler's cache.
    pub async fn initialize_instruments(&self, instruments: Vec<InstrumentAny>) {
        for instrument in &instruments {
            self.instruments.insert(instrument.id(), instrument.clone());
        }

        let cmd_tx = self.cmd_tx.read().await;
        if let Err(e) = cmd_tx.send(HandlerCommand::InitializeInstruments(instruments)) {
            log::debug!("Failed to send InitializeInstruments: {e}");
        }
    }

    // Coinbase closes clients that idle without a subscribe inside 5s, and
    // heartbeats keeps the connection alive when product topics are quiet.
    // Marking before `resubscribe_all` replays it on every reconnect.
    fn prime_default_subscriptions(&self) {
        self.subscriptions
            .mark_subscribe(CoinbaseWsChannel::Heartbeats.as_ref());
    }

    /// Establishes the WebSocket connection and spawns the feed handler.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_active() || self.is_reconnecting() {
            log::warn!("WebSocket already connected or reconnecting");
            return Ok(());
        }

        // Clear stop signal from any previous disconnect
        self.signal.store(false, Ordering::Relaxed);

        let (message_handler, raw_rx) = channel_message_handler();
        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            // Coinbase uses TCP control-frame pings for transport keep-alive;
            // application-layer liveness comes from the heartbeats channel.
            heartbeat: Some(WS_HEARTBEAT_SECS),
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(RECONNECT_TIMEOUT.as_millis() as u64),
            reconnect_delay_initial_ms: Some(RECONNECT_BASE_BACKOFF.as_millis() as u64),
            reconnect_delay_max_ms: Some(RECONNECT_MAX_BACKOFF.as_millis() as u64),
            reconnect_backoff_factor: Some(RECONNECT_BACKOFF_FACTOR),
            reconnect_jitter_ms: Some(RECONNECT_JITTER_MS),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };

        let keyed_quotas = vec![(
            COINBASE_RATE_LIMIT_KEY_SUBSCRIPTION.to_string(),
            *COINBASE_WS_SUBSCRIPTION_QUOTA,
        )];

        let client = WebSocketClient::connect(
            cfg,
            Some(message_handler),
            None,
            None,
            keyed_quotas,
            Some(*COINBASE_WS_CONNECTION_QUOTA),
        )
        .await?;

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();

        *self.cmd_tx.write().await = cmd_tx.clone();
        self.out_rx = Some(out_rx);
        self.connection_mode.store(client.connection_mode_atomic());
        log::info!("Coinbase WebSocket connected: {}", self.url);

        if let Err(e) = cmd_tx.send(HandlerCommand::SetClient(client)) {
            anyhow::bail!("Failed to send SetClient command: {e}");
        }

        let instruments_vec: Vec<InstrumentAny> =
            self.instruments.load().values().cloned().collect();

        if !instruments_vec.is_empty()
            && let Err(e) = cmd_tx.send(HandlerCommand::InitializeInstruments(instruments_vec))
        {
            log::error!("Failed to send InitializeInstruments: {e}");
        }

        // Restore bar type registrations from previous session
        for (key, bar_type) in &self.bar_types {
            if let Err(e) = cmd_tx.send(HandlerCommand::AddBarType {
                key: key.clone(),
                bar_type: *bar_type,
            }) {
                log::error!("Failed to restore bar type {key}: {e}");
            }
        }

        if let Some(account_id) = self.account_id
            && let Err(e) = cmd_tx.send(HandlerCommand::SetAccountId(account_id))
        {
            log::error!("Failed to restore account_id: {e}");
        }

        self.prime_default_subscriptions();

        // Replay retained subscriptions from previous session
        resubscribe_all(
            &self.subscriptions,
            &self.credential,
            &cmd_tx,
            Some(&out_tx),
        );

        let signal = Arc::clone(&self.signal);
        let subscriptions = self.subscriptions.clone();
        let credential = self.credential.clone();
        let cmd_tx_reconnect = cmd_tx.clone();
        let aliases_for_handler = Arc::clone(&self.subscription_aliases);

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(signal, cmd_rx, raw_rx, aliases_for_handler);

            loop {
                match handler.next().await {
                    Some(NautilusWsMessage::Reconnected) => {
                        resubscribe_all(
                            &subscriptions,
                            &credential,
                            &cmd_tx_reconnect,
                            Some(&out_tx),
                        );

                        if let Err(e) = out_tx.send(NautilusWsMessage::Reconnected) {
                            log::debug!("Output channel closed: {e}");
                            break;
                        }
                    }
                    Some(msg) => {
                        if let Err(e) = out_tx.send(msg) {
                            log::debug!("Output channel closed: {e}");
                            break;
                        }
                    }
                    None => {
                        log::info!("Feed handler stopped");
                        break;
                    }
                }
            }
        });

        self.task_handle = Some(stream_handle);
        Ok(())
    }

    /// Subscribes to a channel for the given product IDs.
    pub async fn subscribe(
        &self,
        channel: CoinbaseWsChannel,
        product_ids: &[Ustr],
    ) -> anyhow::Result<()> {
        let jwt = if channel.requires_auth() {
            let credential = self
                .credential
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Credentials required for {channel}"))?;
            Some(credential.build_ws_jwt()?)
        } else {
            self.credential.as_ref().and_then(|c| c.build_ws_jwt().ok())
        };

        let sub = CoinbaseWsSubscription {
            msg_type: CoinbaseWsAction::Subscribe,
            product_ids: product_ids.to_vec(),
            channel,
            jwt,
        };

        let channel_str = channel.as_ref();

        if product_ids.is_empty() {
            self.subscriptions.mark_subscribe(channel_str);
        } else {
            for product_id in product_ids {
                let topic = format!("{channel_str}|{product_id}");
                self.subscriptions.mark_subscribe(&topic);
            }
        }

        let cmd_tx = self.cmd_tx.read().await;
        cmd_tx
            .send(HandlerCommand::Subscribe(sub))
            .map_err(|e| anyhow::anyhow!("Failed to send Subscribe command: {e}"))
    }

    /// Unsubscribes from a channel for the given product IDs.
    pub async fn unsubscribe(
        &self,
        channel: CoinbaseWsChannel,
        product_ids: &[Ustr],
    ) -> anyhow::Result<()> {
        let jwt = self.credential.as_ref().and_then(|c| c.build_ws_jwt().ok());

        let unsub = CoinbaseWsSubscription {
            msg_type: CoinbaseWsAction::Unsubscribe,
            product_ids: product_ids.to_vec(),
            channel,
            jwt,
        };

        let channel_str = channel.as_ref();

        if product_ids.is_empty() {
            self.subscriptions.mark_unsubscribe(channel_str);
        } else {
            for product_id in product_ids {
                let topic = format!("{channel_str}|{product_id}");
                self.subscriptions.mark_unsubscribe(&topic);
            }
        }

        let cmd_tx = self.cmd_tx.read().await;
        cmd_tx
            .send(HandlerCommand::Unsubscribe(unsub))
            .map_err(|e| anyhow::anyhow!("Failed to send Unsubscribe command: {e}"))
    }

    /// Returns the next parsed message from the feed handler.
    pub async fn next_message(&mut self) -> Option<NautilusWsMessage> {
        self.out_rx.as_mut()?.recv().await
    }

    /// Disconnects the WebSocket and stops the feed handler.
    pub async fn disconnect(&mut self) {
        // Send Disconnect command before setting the signal so the handler
        // processes it and calls notify_closed() on the inner WebSocket client
        let cmd_tx = self.cmd_tx.read().await;

        if let Err(e) = cmd_tx.send(HandlerCommand::Disconnect) {
            log::debug!("Failed to send Disconnect command: {e}");
        }
        drop(cmd_tx);

        // Release pairs with the handler's Acquire load; fallback for when
        // the command channel is full or closed.
        self.signal.store(true, Ordering::Release);

        if let Some(handle) = self.task_handle.take() {
            // Capture an abort handle before awaiting so a stuck task can be
            // forcibly stopped on timeout instead of leaking.
            let abort_handle = handle.abort_handle();
            match tokio::time::timeout(WS_DISCONNECT_TIMEOUT, handle).await {
                Ok(_) => log::debug!("Feed handler task completed"),
                Err(_) => {
                    log::warn!("Feed handler task did not complete within timeout, aborting");
                    abort_handle.abort();
                }
            }
        }

        // Wait for the inner WebSocket's connection_mode atomic to reach Closed
        // before returning. Without this, a subsequent connect() can observe a
        // stale Active/Reconnect state and early-return, leaving out_rx unset
        // and causing "WebSocket output receiver not available" on take.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

        loop {
            let mode_ptr = self.connection_mode.load();

            if ConnectionMode::from_u8(mode_ptr.load(Ordering::Relaxed)).is_closed() {
                break;
            }

            if tokio::time::Instant::now() >= deadline {
                log::warn!("Timed out waiting for WebSocket to reach Closed state");
                break;
            }

            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Returns true if the WebSocket connection is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let mode_ptr = self.connection_mode.load();
        let mode_val = mode_ptr.load(Ordering::Relaxed);
        ConnectionMode::from_u8(mode_val).is_active()
    }

    /// Returns true if the WebSocket is reconnecting after a transport drop.
    #[must_use]
    pub fn is_reconnecting(&self) -> bool {
        let mode_ptr = self.connection_mode.load();
        let mode_val = mode_ptr.load(Ordering::Relaxed);
        ConnectionMode::from_u8(mode_val).is_reconnect()
    }

    /// Returns a reference to the instrument cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<AtomicMap<InstrumentId, InstrumentAny>> {
        &self.instruments
    }

    /// Returns a reference to the canonical-to-subscribed alias map.
    #[must_use]
    pub fn subscription_aliases(&self) -> &Arc<AtomicMap<Ustr, Ustr>> {
        &self.subscription_aliases
    }

    /// Records that inbound messages carrying `canonical` should be re-keyed to
    /// `subscribed`. Caller is the data/exec client at subscribe or submit time
    /// when the local product id differs from Coinbase's canonical alias.
    pub fn register_subscription_alias(&self, canonical: Ustr, subscribed: Ustr) {
        self.subscription_aliases.insert(canonical, subscribed);
    }

    /// Removes an alias registration. Safe to call if no entry exists.
    pub fn unregister_subscription_alias(&self, canonical: &Ustr) {
        self.subscription_aliases.remove(canonical);
    }

    /// Returns the subscription state.
    #[must_use]
    pub fn subscriptions(&self) -> &SubscriptionState {
        &self.subscriptions
    }

    /// Updates an instrument in the cache and notifies the handler.
    pub async fn update_instrument(&self, instrument: InstrumentAny) {
        let id = instrument.id();
        self.instruments.insert(id, instrument.clone());

        let cmd_tx = self.cmd_tx.read().await;

        if let Err(e) = cmd_tx.send(HandlerCommand::UpdateInstrument(Box::new(instrument))) {
            log::debug!("Failed to send UpdateInstrument: {e}");
        }
    }

    /// Takes the output message receiver, leaving `None` in its place.
    ///
    /// Used by the data client to move the receiver into a background consumption task.
    pub fn take_out_rx(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>> {
        self.out_rx.take()
    }

    /// Registers a bar type locally without notifying the handler.
    ///
    /// Used by the data client to persist registrations on the original client
    /// before cloning for async command dispatch.
    pub fn register_bar_type(&mut self, key: String, bar_type: BarType) {
        self.bar_types.insert(key, bar_type);
    }

    /// Registers a bar type for candle parsing.
    pub async fn add_bar_type(&mut self, key: String, bar_type: BarType) {
        self.bar_types.insert(key.clone(), bar_type);

        let cmd_tx = self.cmd_tx.read().await;

        if let Err(e) = cmd_tx.send(HandlerCommand::AddBarType { key, bar_type }) {
            log::debug!("Failed to send AddBarType: {e}");
        }
    }
}

fn resubscribe_all(
    subscriptions: &SubscriptionState,
    credential: &Option<CoinbaseCredential>,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    out_tx: Option<&tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>>,
) {
    let topics = subscriptions.all_topics();

    if topics.is_empty() {
        log::debug!("No active subscriptions to restore");
        return;
    }

    log::info!(
        "Resubscribing to {} topics after reconnection",
        topics.len()
    );

    for topic in topics {
        let (channel, product_id) = match topic.split_once('|') {
            Some((ch, pid)) => (ch, Some(pid)),
            None => (topic.as_str(), None),
        };

        let channel_enum = match CoinbaseWsChannel::from_str(channel) {
            Ok(ch) => ch,
            Err(_) => {
                log::warn!("Unknown channel in topic: {topic}");
                continue;
            }
        };

        let jwt = match credential.as_ref() {
            Some(c) => match c.build_ws_jwt() {
                Ok(token) => Some(token),
                Err(e) => {
                    if channel_enum.requires_auth() {
                        let msg = format!(
                            "JWT required for {channel} but build failed: {e}; topic {topic} not restored"
                        );
                        log::error!("{msg}");
                        if let Some(tx) = out_tx {
                            let _ = tx.send(NautilusWsMessage::Error(msg));
                        }
                        continue;
                    }
                    None
                }
            },
            None => {
                if channel_enum.requires_auth() {
                    let msg = format!(
                        "JWT required for {channel} but no credentials configured; topic {topic} not restored"
                    );
                    log::error!("{msg}");
                    if let Some(tx) = out_tx {
                        let _ = tx.send(NautilusWsMessage::Error(msg));
                    }
                    continue;
                }
                None
            }
        };

        let product_ids = match product_id {
            Some(pid) => vec![Ustr::from(pid)],
            None => vec![],
        };

        let sub = CoinbaseWsSubscription {
            msg_type: CoinbaseWsAction::Subscribe,
            product_ids,
            channel: channel_enum,
            jwt,
        };

        if let Err(e) = cmd_tx.send(HandlerCommand::Subscribe(sub)) {
            log::error!("Failed to resubscribe {topic}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_network::websocket::SubscriptionState;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_resubscribe_all_product_level_topic() {
        let subs = SubscriptionState::new('|');
        subs.mark_subscribe("level2|BTC-USD");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        resubscribe_all(&subs, &None, &tx, None);

        let cmd = rx.try_recv().unwrap();

        match cmd {
            HandlerCommand::Subscribe(sub) => {
                assert_eq!(sub.channel, CoinbaseWsChannel::Level2);
                assert_eq!(sub.product_ids.len(), 1);
                assert_eq!(sub.product_ids[0], "BTC-USD");
                assert!(sub.jwt.is_none());
            }
            other => panic!("Expected Subscribe, was {other:?}"),
        }
    }

    #[rstest]
    fn test_resubscribe_all_channel_level_topic() {
        let subs = SubscriptionState::new('|');
        subs.mark_subscribe("heartbeats");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        resubscribe_all(&subs, &None, &tx, None);

        let cmd = rx.try_recv().unwrap();

        match cmd {
            HandlerCommand::Subscribe(sub) => {
                assert_eq!(sub.channel, CoinbaseWsChannel::Heartbeats);
                assert!(sub.product_ids.is_empty());
            }
            other => panic!("Expected Subscribe, was {other:?}"),
        }
    }

    #[rstest]
    fn test_resubscribe_all_multiple_topics() {
        let subs = SubscriptionState::new('|');
        subs.mark_subscribe("market_trades|BTC-USD");
        subs.mark_subscribe("ticker|ETH-USD");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        resubscribe_all(&subs, &None, &tx, None);

        let cmd1 = rx.try_recv().unwrap();
        let cmd2 = rx.try_recv().unwrap();

        assert!(matches!(cmd1, HandlerCommand::Subscribe(_)));
        assert!(matches!(cmd2, HandlerCommand::Subscribe(_)));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_resubscribe_all_empty_subscriptions() {
        let subs = SubscriptionState::new('|');

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        resubscribe_all(&subs, &None, &tx, None);

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_resubscribe_all_unknown_channel_skipped() {
        let subs = SubscriptionState::new('|');
        subs.mark_subscribe("nonexistent_channel|BTC-USD");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        resubscribe_all(&subs, &None, &tx, None);

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case("level2|BTC-USD", CoinbaseWsChannel::Level2)]
    #[case("market_trades|ETH-USD", CoinbaseWsChannel::MarketTrades)]
    #[case("ticker|BTC-USD", CoinbaseWsChannel::Ticker)]
    #[case("ticker_batch|BTC-USD", CoinbaseWsChannel::TickerBatch)]
    #[case("candles|BTC-USD", CoinbaseWsChannel::Candles)]
    #[case("heartbeats", CoinbaseWsChannel::Heartbeats)]
    #[case("status", CoinbaseWsChannel::Status)]
    fn test_resubscribe_all_channel_mapping(
        #[case] topic: &str,
        #[case] expected_channel: CoinbaseWsChannel,
    ) {
        let subs = SubscriptionState::new('|');
        subs.mark_subscribe(topic);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        resubscribe_all(&subs, &None, &tx, None);

        let cmd = rx.try_recv().unwrap();

        match cmd {
            HandlerCommand::Subscribe(sub) => {
                assert_eq!(sub.channel, expected_channel);
            }
            other => panic!("Expected Subscribe, was {other:?}"),
        }
    }

    #[rstest]
    #[case("user|BTC-USD")]
    #[case("futures_balance_summary")]
    fn test_resubscribe_all_auth_channel_skipped_without_credentials(#[case] topic: &str) {
        let subs = SubscriptionState::new('|');
        subs.mark_subscribe(topic);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        resubscribe_all(&subs, &None, &tx, None);

        // Auth channels should be skipped when no credentials are provided
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case("user|BTC-USD", "user")]
    #[case("futures_balance_summary", "futures_balance_summary")]
    fn test_resubscribe_all_emits_error_for_auth_channel_without_credentials(
        #[case] topic: &str,
        #[case] channel: &str,
    ) {
        let subs = SubscriptionState::new('|');
        subs.mark_subscribe(topic);

        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        resubscribe_all(&subs, &None, &cmd_tx, Some(&out_tx));

        // No subscribe command should be sent for an unauthenticated auth channel.
        assert!(cmd_rx.try_recv().is_err());

        let msg = out_rx
            .try_recv()
            .expect("Error event must be emitted when auth channel cannot resubscribe");
        match msg {
            NautilusWsMessage::Error(text) => {
                assert!(
                    text.contains(channel),
                    "error must mention the channel, was: {text}"
                );
                assert!(
                    text.contains(topic),
                    "error must mention the topic, was: {text}"
                );
            }
            other => panic!("expected Error variant, was {other:?}"),
        }
    }

    #[rstest]
    fn test_resubscribe_all_emits_error_when_jwt_build_fails() {
        let subs = SubscriptionState::new('|');
        let topic = "user|BTC-USD";
        subs.mark_subscribe(topic);

        // A credential with a malformed PEM secret causes build_ws_jwt() to fail
        // every time, exercising the JWT-build error branch.
        let bad_credential = Some(CoinbaseCredential::new(
            "organizations/test/apiKeys/test".to_string(),
            "not-a-pem-key".to_string(),
        ));

        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        resubscribe_all(&subs, &bad_credential, &cmd_tx, Some(&out_tx));

        assert!(cmd_rx.try_recv().is_err(), "no subscribe should be sent");
        let msg = out_rx
            .try_recv()
            .expect("Error event must be emitted when JWT build fails for an auth channel");
        match msg {
            NautilusWsMessage::Error(text) => {
                assert!(text.contains("user"), "error must mention channel: {text}");
                assert!(text.contains(topic), "error must mention topic: {text}");
            }
            other => panic!("expected Error variant, was {other:?}"),
        }
    }

    #[rstest]
    fn test_prime_default_subscriptions_marks_heartbeats() {
        let client = CoinbaseWebSocketClient::new("wss://test", TransportBackend::default(), None);
        assert!(client.subscriptions.all_topics().is_empty());

        client.prime_default_subscriptions();

        let topics = client.subscriptions.all_topics();
        assert!(topics.iter().any(|t| t == "heartbeats"), "{topics:?}");
    }

    #[rstest]
    fn test_ws_quotas_match_documented_limits() {
        assert_eq!(COINBASE_WS_CONNECTION_QUOTA.burst_size().get(), 8);
        assert_eq!(COINBASE_WS_SUBSCRIPTION_QUOTA.burst_size().get(), 8);
    }

    #[rstest]
    fn test_ws_subscription_rate_limit_key_is_stable() {
        assert_eq!(COINBASE_RATE_LIMIT_KEY_SUBSCRIPTION, "subscription");
        assert_eq!(
            COINBASE_WS_SUBSCRIPTION_KEYS[0].as_str(),
            COINBASE_RATE_LIMIT_KEY_SUBSCRIPTION,
        );
    }
}
