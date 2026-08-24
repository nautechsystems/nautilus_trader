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

//! Market-channel WebSocket connection pool for the Polymarket CLOB API.
//!
//! [`PolymarketWebSocketClient`] stays a single-channel, single-connection
//! primitive. This pool owns a set of market-channel connections (shards) and
//! spreads unique asset subscriptions across them so no single connection carries
//! more than `ws_max_subscriptions` assets. See [`WS_DEFAULT_SUBSCRIPTIONS`] for
//! why that bound exists.
//!
//! The pool grows lazily: it starts with one shard and opens another only when the
//! current shards are full at subscribe time. A secondary shard closes once it owns
//! no assets; the primary shard (which carries new-market discovery) always
//! persists. Each shard replays only its own subscriptions on reconnect because
//! that state lives inside its own [`PolymarketWebSocketClient`]. When custom
//! features are enabled, every shard requests asset-scoped best-bid/ask events,
//! while secondary shards discard global discovery and resolution events.

use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use nautilus_common::live::get_runtime;
use nautilus_live::SocketControlFactory;
use nautilus_network::websocket::{TransportBackend, proxy::ProxyUrl};
use ustr::Ustr;

use super::{
    MARKET_STREAMS_ENDPOINT,
    client::{PolymarketWebSocketClient, WsSubscriptionHandle},
    messages::{MarketWsMessage, PolymarketWsMessage},
};
use crate::common::consts::WS_DEFAULT_SUBSCRIPTIONS;

// Primary shard carries new-market discovery and never auto-closes.
const PRIMARY_SHARD_ID: usize = 0;

/// A pool of market-channel WebSocket connections that shards asset subscriptions.
#[derive(Debug)]
pub struct PolymarketMarketConnectionPool {
    inner: Arc<PoolInner>,
}

/// Cloneable routing handle used from spawned subscription tasks.
///
/// Routes each asset to its owning shard and grows the pool on demand.
#[derive(Clone, Debug)]
pub struct PolymarketMarketPoolHandle {
    inner: Arc<PoolInner>,
}

#[derive(Debug)]
struct PoolInner {
    base_url: Option<String>,
    proxy_url: Option<ProxyUrl>,
    transport_backend: TransportBackend,
    subscribe_new_markets: bool,
    max_subscriptions: usize,
    // Serializes routing and shard growth; held across the async wire sends.
    wire_mutex: tokio::sync::Mutex<()>,
    // Never locked across an await, so routing futures stay `Send`.
    state: StdMutex<PoolState>,
    out_tx: StdMutex<Option<tokio::sync::mpsc::UnboundedSender<PolymarketWsMessage>>>,
    out_rx: StdMutex<Option<tokio::sync::mpsc::UnboundedReceiver<PolymarketWsMessage>>>,
    socket_factory: StdMutex<Option<SocketControlFactory>>,
    closed: AtomicBool,
}

#[derive(Debug)]
struct PoolState {
    shards: AHashMap<usize, ShardEntry>,
    assignments: AHashMap<Ustr, usize>,
}

impl PoolState {
    fn new() -> Self {
        Self {
            shards: AHashMap::new(),
            assignments: AHashMap::new(),
        }
    }
}

#[derive(Debug)]
struct ShardEntry {
    client: PolymarketWebSocketClient,
    handle: WsSubscriptionHandle,
    forwarder: Option<tokio::task::JoinHandle<()>>,
    owned: usize,
}

enum ReleaseOutcome {
    NotOwned,
    Unsubscribe(WsSubscriptionHandle),
    CloseShard(Box<ShardEntry>),
}

#[allow(
    clippy::missing_panics_doc,
    reason = "internal mutex locks and shard-state invariants are not expected to panic"
)]
impl PolymarketMarketConnectionPool {
    /// Creates a new market connection pool (unconnected).
    ///
    /// A `max_subscriptions` of `0` is invalid and clamps to
    /// [`WS_DEFAULT_SUBSCRIPTIONS`] with a warning.
    #[must_use]
    pub fn new(
        base_url: Option<String>,
        subscribe_new_markets: bool,
        transport_backend: TransportBackend,
        max_subscriptions: usize,
    ) -> Self {
        Self::new_with_proxy(
            base_url,
            subscribe_new_markets,
            transport_backend,
            max_subscriptions,
            None,
        )
    }

    /// Creates a new market connection pool with an optional validated proxy URL.
    #[must_use]
    pub fn new_with_proxy(
        base_url: Option<String>,
        subscribe_new_markets: bool,
        transport_backend: TransportBackend,
        max_subscriptions: usize,
        proxy_url: Option<ProxyUrl>,
    ) -> Self {
        Self {
            inner: Arc::new(PoolInner::new_with_proxy(
                base_url,
                transport_backend,
                subscribe_new_markets,
                max_subscriptions,
                proxy_url,
            )),
        }
    }

    /// Configures socket state reporting and reconnect control for every connection in the pool.
    #[must_use]
    pub(crate) fn with_socket_factory(self, factory: SocketControlFactory) -> Self {
        *self
            .inner
            .socket_factory
            .lock()
            .expect("pool socket factory mutex poisoned") = Some(factory);
        self
    }

    #[cfg(test)]
    pub(crate) fn proxy_url(&self) -> Option<&ProxyUrl> {
        self.inner.proxy_url.as_ref()
    }

    /// Returns a cloneable routing handle for use in spawned subscription tasks.
    #[must_use]
    pub fn handle(&self) -> PolymarketMarketPoolHandle {
        PolymarketMarketPoolHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Opens the primary shard and prepares the merged message stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the primary connection cannot be established.
    pub async fn connect(&self) -> anyhow::Result<()> {
        let _wire = self.inner.wire_mutex.lock().await;

        if !self.inner.closed.load(Ordering::Acquire)
            && !self
                .inner
                .state
                .lock()
                .expect("pool state mutex poisoned")
                .shards
                .is_empty()
        {
            log::warn!("Polymarket market pool already connected");
            return Ok(());
        }

        self.inner.closed.store(false, Ordering::Release);

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
        *self
            .inner
            .out_tx
            .lock()
            .expect("pool out_tx mutex poisoned") = Some(out_tx);
        *self
            .inner
            .out_rx
            .lock()
            .expect("pool out_rx mutex poisoned") = Some(out_rx);

        self.inner.connect_new_shard(true).await?;
        Ok(())
    }

    /// Sends the new-market discovery subscribe on the primary shard.
    ///
    /// # Errors
    ///
    /// Returns an error if no primary shard is available.
    pub async fn subscribe_new_markets_feed(&self) -> anyhow::Result<()> {
        let _wire = self.inner.wire_mutex.lock().await;

        let handle = self
            .inner
            .state
            .lock()
            .expect("pool state mutex poisoned")
            .shards
            .get(&PRIMARY_SHARD_ID)
            .map(|shard| shard.handle.clone());

        match handle {
            Some(handle) => handle.subscribe_market(vec![]).await,
            None => anyhow::bail!("No primary market shard available for new-market discovery"),
        }
    }

    /// Takes the merged message receiver, leaving `None` in its place.
    #[must_use]
    pub fn take_message_receiver(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<PolymarketWsMessage>> {
        self.inner
            .out_rx
            .lock()
            .expect("pool out_rx mutex poisoned")
            .take()
    }

    /// Disconnects every shard and clears routing state.
    ///
    /// # Errors
    ///
    /// Never returns an error; per-shard failures are logged and disconnect continues.
    pub async fn disconnect(&self) -> anyhow::Result<()> {
        let _wire = self.inner.wire_mutex.lock().await;
        self.inner.closed.store(true, Ordering::Release);

        let shards = self.inner.drain_shards();
        for mut shard in shards {
            if let Some(forwarder) = shard.forwarder.take() {
                forwarder.abort();
            }

            if let Err(e) = shard.client.disconnect().await {
                log::debug!("Error disconnecting market shard: {e}");
            }
        }

        *self
            .inner
            .out_tx
            .lock()
            .expect("pool out_tx mutex poisoned") = None;
        *self
            .inner
            .out_rx
            .lock()
            .expect("pool out_rx mutex poisoned") = None;
        Ok(())
    }

    /// Force-closes every shard for the sync `stop()`/`reset()` path.
    ///
    /// Prefer [`Self::disconnect`] for graceful shutdown.
    pub(crate) fn abort(&self) {
        self.inner.closed.store(true, Ordering::Release);

        let shards = self.inner.drain_shards();
        for mut shard in shards {
            if let Some(forwarder) = shard.forwarder.take() {
                forwarder.abort();
            }
            shard.client.abort();
        }

        *self
            .inner
            .out_tx
            .lock()
            .expect("pool out_tx mutex poisoned") = None;
        *self
            .inner
            .out_rx
            .lock()
            .expect("pool out_rx mutex poisoned") = None;
    }

    /// Clears retained reconnect-replay state on any remaining shards.
    pub(crate) fn clear_reconnect_state(&self) {
        let state = self.inner.state.lock().expect("pool state mutex poisoned");
        for shard in state.shards.values() {
            shard.client.clear_reconnect_state();
        }
    }

    /// Returns the number of open shard connections.
    #[must_use]
    pub fn connection_count(&self) -> usize {
        self.inner
            .state
            .lock()
            .expect("pool state mutex poisoned")
            .shards
            .len()
    }

    /// Returns the number of unique assets assigned across all shards.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.inner
            .state
            .lock()
            .expect("pool state mutex poisoned")
            .assignments
            .len()
    }
}

#[allow(
    clippy::missing_panics_doc,
    reason = "internal mutex locks and shard-state invariants are not expected to panic"
)]
impl PolymarketMarketPoolHandle {
    /// Subscribes to market data for the given asset IDs, sharding across connections.
    ///
    /// # Errors
    ///
    /// Returns an error if a shard cannot be opened or a subscribe send fails.
    pub async fn subscribe_market(&self, asset_ids: Vec<String>) -> anyhow::Result<()> {
        let _wire = self.inner.wire_mutex.lock().await;
        for asset_id in asset_ids {
            self.inner.subscribe_one(asset_id).await?;
        }
        Ok(())
    }

    /// Removes asset IDs from their owning shards, closing emptied secondary shards.
    ///
    /// # Errors
    ///
    /// Returns an error if an unsubscribe send fails.
    pub async fn unsubscribe_market(&self, asset_ids: Vec<String>) -> anyhow::Result<()> {
        let _wire = self.inner.wire_mutex.lock().await;
        for asset_id in asset_ids {
            self.inner.unsubscribe_one(asset_id).await?;
        }
        Ok(())
    }
}

impl PoolInner {
    #[cfg(test)]
    fn new(
        base_url: Option<String>,
        transport_backend: TransportBackend,
        subscribe_new_markets: bool,
        max_subscriptions: usize,
    ) -> Self {
        Self::new_with_proxy(
            base_url,
            transport_backend,
            subscribe_new_markets,
            max_subscriptions,
            None,
        )
    }

    fn new_with_proxy(
        base_url: Option<String>,
        transport_backend: TransportBackend,
        subscribe_new_markets: bool,
        max_subscriptions: usize,
        proxy_url: Option<ProxyUrl>,
    ) -> Self {
        let max_subscriptions = if max_subscriptions == 0 {
            log::warn!(
                "PolymarketDataClientConfig.ws_max_subscriptions=0 is invalid, using {WS_DEFAULT_SUBSCRIPTIONS}"
            );
            WS_DEFAULT_SUBSCRIPTIONS
        } else {
            max_subscriptions
        };

        Self {
            base_url,
            proxy_url,
            transport_backend,
            subscribe_new_markets,
            max_subscriptions,
            wire_mutex: tokio::sync::Mutex::new(()),
            state: StdMutex::new(PoolState::new()),
            out_tx: StdMutex::new(None),
            out_rx: StdMutex::new(None),
            socket_factory: StdMutex::new(None),
            closed: AtomicBool::new(false),
        }
    }

    // Callers hold `wire_mutex`.
    async fn subscribe_one(&self, asset_id: String) -> anyhow::Result<()> {
        let token = Ustr::from(asset_id.as_str());

        let Some(handle) = self.assign(token).await? else {
            return Ok(());
        };

        if let Err(e) = handle.subscribe_market(vec![asset_id]).await {
            // Roll back so a failed send leaves no stale assignment or empty shard.
            if let ReleaseOutcome::CloseShard(shard) = self.release(token) {
                close_shard(*shard).await;
            }
            return Err(e);
        }
        Ok(())
    }

    // Callers hold `wire_mutex`.
    async fn unsubscribe_one(&self, asset_id: String) -> anyhow::Result<()> {
        let token = Ustr::from(asset_id.as_str());

        match self.release(token) {
            ReleaseOutcome::NotOwned => Ok(()),
            ReleaseOutcome::Unsubscribe(handle) => handle.unsubscribe_market(vec![asset_id]).await,
            ReleaseOutcome::CloseShard(shard) => {
                // Disconnect drops the shard's subscriptions; no unsubscribe send needed.
                close_shard(*shard).await;
                Ok(())
            }
        }
    }

    // Returns `None` when the token is already owned by a shard.
    async fn assign(&self, token: Ustr) -> anyhow::Result<Option<WsSubscriptionHandle>> {
        {
            let mut state = self.state.lock().expect("pool state mutex poisoned");
            if state.assignments.contains_key(&token) {
                return Ok(None);
            }

            if let Some(id) = smallest_shard_with_capacity(&state, self.max_subscriptions) {
                let handle = {
                    let shard = state.shards.get_mut(&id).expect("shard present");
                    shard.owned += 1;
                    shard.handle.clone()
                };
                state.assignments.insert(token, id);
                return Ok(Some(handle));
            }
        }

        let id = self.connect_new_shard(false).await?;

        let mut state = self.state.lock().expect("pool state mutex poisoned");
        let handle = {
            let shard = state.shards.get_mut(&id).expect("new shard present");
            shard.owned += 1;
            shard.handle.clone()
        };
        state.assignments.insert(token, id);
        Ok(Some(handle))
    }

    fn release(&self, token: Ustr) -> ReleaseOutcome {
        let mut state = self.state.lock().expect("pool state mutex poisoned");

        let Some(id) = state.assignments.remove(&token) else {
            return ReleaseOutcome::NotOwned;
        };

        let owned = {
            let Some(shard) = state.shards.get_mut(&id) else {
                return ReleaseOutcome::NotOwned;
            };
            shard.owned = shard.owned.saturating_sub(1);
            shard.owned
        };

        if id != PRIMARY_SHARD_ID && owned == 0 {
            let shard = state.shards.remove(&id).expect("shard present");
            ReleaseOutcome::CloseShard(Box::new(shard))
        } else {
            let handle = state.shards.get(&id).expect("shard present").handle.clone();
            ReleaseOutcome::Unsubscribe(handle)
        }
    }

    async fn connect_new_shard(&self, is_primary: bool) -> anyhow::Result<usize> {
        if self.closed.load(Ordering::Acquire) {
            anyhow::bail!("Market connection pool is closed");
        }

        let id = if is_primary {
            PRIMARY_SHARD_ID
        } else {
            let state = self.state.lock().expect("pool state mutex poisoned");
            available_shard_id(&state)
        };

        let mut client = self.market_client(self.subscribe_new_markets, id);
        client.connect().await?;

        let handle = client.clone_subscription_handle();
        let rx = client
            .take_message_receiver()
            .ok_or_else(|| anyhow::anyhow!("Market shard receiver unavailable after connect"))?;
        let forwarder = self.spawn_forwarder(rx, is_primary);

        let mut state = self.state.lock().expect("pool state mutex poisoned");
        state.shards.insert(
            id,
            ShardEntry {
                client,
                handle,
                forwarder: Some(forwarder),
                owned: 0,
            },
        );
        drop(state);

        log::debug!("Opened Polymarket market shard {id}");
        Ok(id)
    }

    fn market_client(
        &self,
        subscribe_new_markets: bool,
        shard_id: usize,
    ) -> PolymarketWebSocketClient {
        let client = PolymarketWebSocketClient::new_market_with_proxy(
            self.base_url.clone(),
            subscribe_new_markets,
            self.transport_backend,
            self.proxy_url.clone(),
        );
        let factory = self
            .socket_factory
            .lock()
            .expect("pool socket factory mutex poisoned")
            .clone();

        if let Some(factory) = factory {
            let endpoint = if shard_id == PRIMARY_SHARD_ID {
                MARKET_STREAMS_ENDPOINT.to_string()
            } else {
                format!("{MARKET_STREAMS_ENDPOINT}-{shard_id}")
            };
            client.with_socket_control(factory.control(endpoint))
        } else {
            client
        }
    }

    fn spawn_forwarder(
        &self,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<PolymarketWsMessage>,
        is_primary: bool,
    ) -> tokio::task::JoinHandle<()> {
        let out_tx = self
            .out_tx
            .lock()
            .expect("pool out_tx mutex poisoned")
            .clone();

        get_runtime().spawn(async move {
            let Some(out_tx) = out_tx else {
                return;
            };

            while let Some(msg) = rx.recv().await {
                if !should_forward_from_shard(&msg, is_primary) {
                    continue;
                }

                if out_tx.send(msg).is_err() {
                    break;
                }
            }
        })
    }

    fn drain_shards(&self) -> Vec<ShardEntry> {
        let mut state = self.state.lock().expect("pool state mutex poisoned");
        state.assignments.clear();
        state.shards.drain().map(|(_, shard)| shard).collect()
    }

    #[cfg(test)]
    fn subscription_count_for_test(&self) -> usize {
        self.state
            .lock()
            .expect("pool state mutex poisoned")
            .assignments
            .len()
    }
}

fn should_forward_from_shard(message: &PolymarketWsMessage, is_primary: bool) -> bool {
    is_primary
        || !matches!(
            message,
            PolymarketWsMessage::Market(
                MarketWsMessage::NewMarket(_) | MarketWsMessage::MarketResolved(_)
            )
        )
}

fn smallest_shard_with_capacity(state: &PoolState, max_subscriptions: usize) -> Option<usize> {
    state
        .shards
        .iter()
        .filter(|(_, shard)| shard.owned < max_subscriptions)
        .map(|(id, _)| *id)
        .min()
}

fn available_shard_id(state: &PoolState) -> usize {
    let mut id = PRIMARY_SHARD_ID + 1;
    while state.shards.contains_key(&id) {
        id = id.checked_add(1).expect("market shard ID space exhausted");
    }
    id
}

async fn close_shard(mut shard: ShardEntry) {
    if let Some(forwarder) = shard.forwarder.take() {
        forwarder.abort();
    }

    if let Err(e) = shard.client.disconnect().await {
        log::debug!("Error disconnecting empty market shard: {e}");
    }
}

#[cfg(test)]
impl PolymarketMarketPoolHandle {
    /// In-memory single-shard handle backed by `sender`, `assigned` tokens
    /// pre-owned. Never connected, so growth is never triggered.
    pub(crate) fn test_single_shard(
        sender: tokio::sync::mpsc::UnboundedSender<super::handler::HandlerCommand>,
        assigned: &[&str],
    ) -> Self {
        let inner = PoolInner::new(
            None,
            TransportBackend::default(),
            false,
            WS_DEFAULT_SUBSCRIPTIONS,
        );
        {
            let mut state = inner.state.lock().expect("pool state mutex poisoned");
            state.shards.insert(
                PRIMARY_SHARD_ID,
                ShardEntry {
                    client: PolymarketWebSocketClient::new_market(
                        None,
                        false,
                        TransportBackend::default(),
                    ),
                    handle: WsSubscriptionHandle::from_sender(sender),
                    forwarder: None,
                    owned: assigned.len(),
                },
            );

            for token in assigned {
                state
                    .assignments
                    .insert(Ustr::from(token), PRIMARY_SHARD_ID);
            }
        }
        Self {
            inner: Arc::new(inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, time::Duration};

    use PolymarketMarketPoolHandle as Handle;
    use axum::{
        Router,
        extract::ws::{WebSocket, WebSocketUpgrade},
        response::Response,
        routing::get,
    };
    use nautilus_common::{
        live::runner::replace_system_event_sender,
        messages::{SystemEvent, system::SocketState},
    };
    use nautilus_live::{SocketReconnectRegistry, SocketReconnectRequestOutcome};
    use nautilus_model::identifiers::ClientId;
    use rstest::rstest;

    use super::*;
    use crate::websocket::handler::HandlerCommand;

    async fn handle_socket_upgrade(ws: WebSocketUpgrade) -> Response {
        ws.on_upgrade(handle_socket)
    }

    async fn handle_socket(mut socket: WebSocket) {
        while socket.recv().await.is_some() {}
    }

    async fn start_socket_server() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test websocket server");
        let addr = listener.local_addr().expect("test websocket address");
        let router = Router::new().route("/ws/market", get(handle_socket_upgrade));

        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("test websocket server failed");
        });

        addr
    }

    // Bare state with unconnected shards for pure capacity-accounting tests.
    fn state_with_shards(owned: &[usize]) -> PoolState {
        let mut state = PoolState::new();

        for (id, owned) in owned.iter().enumerate() {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            state.shards.insert(
                id,
                ShardEntry {
                    client: PolymarketWebSocketClient::new_market(
                        None,
                        false,
                        TransportBackend::default(),
                    ),
                    handle: WsSubscriptionHandle::from_sender(tx),
                    forwarder: None,
                    owned: *owned,
                },
            );
        }
        state
    }

    fn market_message(filename: &str) -> PolymarketWsMessage {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(filename);
        let json = std::fs::read_to_string(path).unwrap();
        PolymarketWsMessage::Market(serde_json::from_str(&json).unwrap())
    }

    #[rstest]
    #[case::primary_new_market("ws_market_new_market_msg.json", true, true)]
    #[case::secondary_new_market("ws_market_new_market_msg.json", false, false)]
    #[case::primary_resolution("ws_market_resolved_msg.json", true, true)]
    #[case::secondary_resolution("ws_market_resolved_msg.json", false, false)]
    #[case::secondary_best_bid_ask("ws_market_best_bid_ask_msg.json", false, true)]
    fn shard_forwarding_keeps_global_events_on_primary(
        #[case] filename: &str,
        #[case] is_primary: bool,
        #[case] expected: bool,
    ) {
        let message = market_message(filename);
        assert_eq!(should_forward_from_shard(&message, is_primary), expected);
    }

    #[rstest]
    #[tokio::test]
    async fn secondary_forwarder_drops_global_events_and_keeps_best_bid_ask() {
        let inner = PoolInner::new(None, TransportBackend::default(), true, 1);
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        *inner.out_tx.lock().expect("pool out_tx mutex poisoned") = Some(out_tx);
        let (shard_tx, shard_rx) = tokio::sync::mpsc::unbounded_channel();
        let forwarder = inner.spawn_forwarder(shard_rx, false);

        shard_tx
            .send(market_message("ws_market_new_market_msg.json"))
            .unwrap();
        shard_tx
            .send(market_message("ws_market_resolved_msg.json"))
            .unwrap();
        shard_tx
            .send(market_message("ws_market_best_bid_ask_msg.json"))
            .unwrap();
        drop(shard_tx);
        forwarder.await.unwrap();

        let forwarded = out_rx.try_recv().unwrap();
        let PolymarketWsMessage::Market(MarketWsMessage::BestBidAsk(message)) = forwarded else {
            panic!("unexpected forwarded message: {forwarded:?}");
        };
        assert_eq!(
            message.asset_id,
            Ustr::from(
                "85354956062430465315924116860125388538595433819574542752031640332592237464430"
            ),
        );
        assert!(out_rx.try_recv().is_err());
    }

    #[rstest]
    fn zero_max_subscriptions_clamps_to_default() {
        let inner = PoolInner::new(None, TransportBackend::default(), false, 0);
        assert_eq!(inner.max_subscriptions, WS_DEFAULT_SUBSCRIPTIONS);
    }

    #[rstest]
    fn pool_retains_proxy_for_lazily_created_shards() {
        const PROXY_URL: &str = "http://pool-user:pool-proxy-secret@127.0.0.1:18088";
        let pool = PolymarketMarketConnectionPool::new_with_proxy(
            Some("ws://market.example/ws".to_string()),
            true,
            TransportBackend::Tungstenite,
            17,
            Some(ProxyUrl::parse(PROXY_URL).unwrap()),
        );
        let primary = pool.inner.market_client(true, PRIMARY_SHARD_ID);
        let secondary = pool.inner.market_client(false, PRIMARY_SHARD_ID + 1);
        let debug = format!("{pool:?}");

        assert_eq!(pool.inner.proxy_url.as_ref().unwrap().expose(), PROXY_URL);
        assert_eq!(primary.proxy_url().unwrap().expose(), PROXY_URL);
        assert_eq!(secondary.proxy_url().unwrap().expose(), PROXY_URL);
        assert_eq!(pool.inner.max_subscriptions, 17);
        assert!(!debug.contains("pool-proxy-secret"));
    }

    #[rstest]
    #[tokio::test]
    async fn pool_assigns_distinct_endpoint_sinks_and_handles_before_connect() {
        let addr = start_socket_server().await;
        let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_system_event_sender(system_tx);
        let registry = SocketReconnectRegistry::default();
        let factory = SocketControlFactory::with_registry(
            ClientId::from("POLYMARKET"),
            Some(*crate::common::consts::POLYMARKET_VENUE),
            &registry,
        );
        let pool = PolymarketMarketConnectionPool::new(
            Some(format!("ws://{addr}/ws/market")),
            false,
            TransportBackend::Tungstenite,
            1,
        )
        .with_socket_factory(factory);

        pool.connect().await.expect("connect primary shard");
        pool.handle()
            .subscribe_market(vec!["asset-0".to_string(), "asset-1".to_string()])
            .await
            .expect("open secondary shard");

        let mut connected = Vec::new();
        while connected.len() < 2 {
            let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
                .await
                .expect("wait for socket state event")
                .expect("system event channel closed");
            let SystemEvent::SocketState(change) = event;
            if change.state == SocketState::Connected {
                connected.push(change.endpoint);
            }
        }
        connected.sort_unstable();

        assert_eq!(
            connected,
            vec![
                Ustr::from(MARKET_STREAMS_ENDPOINT),
                Ustr::from("polymarket-market-streams-1"),
            ],
        );
        let client_id = ClientId::from("POLYMARKET");
        let primary = registry
            .handle(client_id, Ustr::from(MARKET_STREAMS_ENDPOINT))
            .expect("primary reconnect handle should be registered");
        let secondary = registry
            .handle(client_id, Ustr::from("polymarket-market-streams-1"))
            .expect("secondary reconnect handle should be registered");
        assert_eq!(
            primary.request_reconnect(),
            SocketReconnectRequestOutcome::Accepted,
        );
        let event = system_rx
            .try_recv()
            .expect("selected shard should report reconnect state");
        let SystemEvent::SocketState(change) = event;
        assert_eq!(change.client_id, client_id);
        assert_eq!(change.endpoint, Ustr::from(MARKET_STREAMS_ENDPOINT));
        assert_eq!(change.state, SocketState::Disconnected);
        assert_eq!(
            secondary.request_reconnect(),
            SocketReconnectRequestOutcome::Accepted,
        );

        pool.disconnect().await.expect("disconnect pool");
        assert!(
            registry
                .handle(client_id, Ustr::from(MARKET_STREAMS_ENDPOINT))
                .is_none()
        );
        assert!(
            registry
                .handle(client_id, Ustr::from("polymarket-market-streams-1"))
                .is_none()
        );
    }

    #[rstest]
    #[case::first_has_room(&[0, 200], 200, Some(0))]
    #[case::prefers_lowest_id(&[200, 5, 5], 200, Some(1))]
    #[case::all_full(&[200, 200], 200, None)]
    #[case::exact_boundary_is_full(&[1], 1, None)]
    fn smallest_shard_with_capacity_picks_lowest_open_id(
        #[case] owned: &[usize],
        #[case] max: usize,
        #[case] expected: Option<usize>,
    ) {
        let state = state_with_shards(owned);
        assert_eq!(smallest_shard_with_capacity(&state, max), expected);
    }

    #[rstest]
    fn available_shard_id_reuses_lowest_closed_shard() {
        let mut state = state_with_shards(&[1, 1, 1]);
        state.shards.remove(&(PRIMARY_SHARD_ID + 1));

        assert_eq!(available_shard_id(&state), PRIMARY_SHARD_ID + 1);
    }

    #[rstest]
    #[tokio::test]
    async fn subscribe_routes_command_and_tracks_assignment() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let handle = Handle::test_single_shard(tx, &[]);

        handle
            .subscribe_market(vec!["token-a".to_string()])
            .await
            .expect("subscribe");

        match rx.try_recv().expect("expected SubscribeMarket") {
            HandlerCommand::SubscribeMarket(ids) => assert_eq!(ids, vec!["token-a".to_string()]),
            other => panic!("unexpected command: {other:?}"),
        }
        assert_eq!(handle.inner.subscription_count_for_test(), 1);
    }

    #[rstest]
    #[tokio::test]
    async fn duplicate_subscribe_does_not_consume_capacity_or_resend() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let handle = Handle::test_single_shard(tx, &[]);

        handle
            .subscribe_market(vec!["token-a".to_string()])
            .await
            .expect("first subscribe");
        handle
            .subscribe_market(vec!["token-a".to_string()])
            .await
            .expect("duplicate subscribe");

        assert!(matches!(
            rx.try_recv(),
            Ok(HandlerCommand::SubscribeMarket(_))
        ));
        assert!(rx.try_recv().is_err(), "duplicate must not resend");
        assert_eq!(handle.inner.subscription_count_for_test(), 1);
    }

    #[rstest]
    #[tokio::test]
    async fn unsubscribe_routes_command_and_releases_assignment() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let handle = Handle::test_single_shard(tx, &["token-a"]);

        handle
            .unsubscribe_market(vec!["token-a".to_string()])
            .await
            .expect("unsubscribe");

        match rx.try_recv().expect("expected UnsubscribeMarket") {
            HandlerCommand::UnsubscribeMarket(ids) => assert_eq!(ids, vec!["token-a".to_string()]),
            other => panic!("unexpected command: {other:?}"),
        }
        assert_eq!(handle.inner.subscription_count_for_test(), 0);
    }

    #[rstest]
    #[tokio::test]
    async fn unsubscribe_unknown_token_is_noop() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let handle = Handle::test_single_shard(tx, &[]);

        handle
            .unsubscribe_market(vec!["token-a".to_string()])
            .await
            .expect("unsubscribe");

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn subscribe_send_failure_rolls_back_assignment() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        drop(rx);
        let handle = Handle::test_single_shard(tx, &[]);

        let result = handle.subscribe_market(vec!["token-a".to_string()]).await;

        assert!(result.is_err());
        assert_eq!(handle.inner.subscription_count_for_test(), 0);
    }
}
