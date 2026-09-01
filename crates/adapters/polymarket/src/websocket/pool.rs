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
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use nautilus_live::{
    SocketControlFactory,
    task::{TaskJoinOutcome, TaskSlot, TaskSpawnError, finish_task},
};
use nautilus_network::websocket::{TransportBackend, proxy::ProxyUrl};
use parking_lot::Mutex;
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
    state: Mutex<PoolState>,
    out_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<PolymarketWsMessage>>>,
    out_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<PolymarketWsMessage>>>,
    socket_factory: Mutex<Option<SocketControlFactory>>,
    closed: AtomicBool,
}

#[derive(Debug)]
struct PoolState {
    shards: AHashMap<usize, ShardEntry>,
    assignments: AHashMap<Ustr, usize>,
    shutdown_errors: Vec<String>,
}

struct PoolDrain<'a> {
    owner: &'a Mutex<PoolState>,
    state: PoolState,
}

impl<'a> PoolDrain<'a> {
    fn take(owner: &'a Mutex<PoolState>) -> Self {
        let state = std::mem::replace(&mut *owner.lock(), PoolState::new());
        Self { owner, state }
    }
}

impl Drop for PoolDrain<'_> {
    fn drop(&mut self) {
        *self.owner.lock() = std::mem::replace(&mut self.state, PoolState::new());
    }
}

impl PoolState {
    fn new() -> Self {
        Self {
            shards: AHashMap::new(),
            assignments: AHashMap::new(),
            shutdown_errors: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct ShardEntry {
    client: PolymarketWebSocketClient,
    handle: WsSubscriptionHandle,
    forwarder: TaskSlot<()>,
    owned: usize,
    closing: bool,
}

enum ReleaseOutcome {
    NotOwned,
    Unsubscribe(WsSubscriptionHandle),
    CloseShard(usize, Box<ShardEntry>),
}

struct ShardClose<'a> {
    owner: &'a Mutex<PoolState>,
    id: usize,
    shard: Option<Box<ShardEntry>>,
}

impl<'a> ShardClose<'a> {
    fn new(owner: &'a Mutex<PoolState>, id: usize, mut shard: Box<ShardEntry>) -> Self {
        shard.closing = true;
        Self {
            owner,
            id,
            shard: Some(shard),
        }
    }

    fn shard_mut(&mut self) -> &mut ShardEntry {
        self.shard.as_deref_mut().expect("closing shard present")
    }

    fn complete(mut self) {
        self.shard.take();
    }
}

impl Drop for ShardClose<'_> {
    fn drop(&mut self) {
        if let Some(shard) = self.shard.take() {
            let replaced = self.owner.lock().shards.insert(self.id, *shard);
            assert!(replaced.is_none(), "closing shard ID is already present");
        }
    }
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
        *self.inner.socket_factory.lock() = Some(factory);
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
        if self.inner.closed.load(Ordering::Acquire) {
            self.disconnect().await?;
        }

        let _wire = self.inner.wire_mutex.lock().await;

        if !self.inner.closed.load(Ordering::Acquire) && !self.inner.state.lock().shards.is_empty()
        {
            log::warn!("Polymarket market pool already connected");
            return Ok(());
        }

        {
            let _state = self.inner.state.lock();
            self.inner.closed.store(false, Ordering::Release);
        }

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
        *self.inner.out_tx.lock() = Some(out_tx);
        *self.inner.out_rx.lock() = Some(out_rx);

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

        let handle = {
            let state = self.inner.state.lock();
            if self.inner.closed.load(Ordering::Acquire) {
                anyhow::bail!("Market connection pool is closed");
            }
            state
                .shards
                .get(&PRIMARY_SHARD_ID)
                .map(|shard| shard.handle.clone())
        };

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
        self.inner.out_rx.lock().take()
    }

    /// Disconnects every shard and clears routing state.
    ///
    /// # Errors
    ///
    /// Returns an error after attempting every shard when a task or connection does not stop.
    pub async fn disconnect(&self) -> anyhow::Result<()> {
        self.inner.begin_shutdown();
        let _wire = self.inner.wire_mutex.lock().await;

        let mut drain = PoolDrain::take(&self.inner.state);
        let shard_ids = drain.state.shards.keys().copied().collect::<Vec<_>>();
        for shard_id in shard_ids {
            let shard = drain
                .state
                .shards
                .get_mut(&shard_id)
                .expect("market shard ID collected from pool state");
            let mut shard_failed = false;

            shard.forwarder.abort();
            if let Some(outcome) = finish_task(
                &mut shard.forwarder,
                std::time::Duration::ZERO,
                std::time::Duration::from_secs(2),
            )
            .await
            {
                match outcome {
                    TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => {}
                    TaskJoinOutcome::Failed(error) => {
                        shard_failed = true;
                        drain
                            .state
                            .shutdown_errors
                            .push(format!("market shard {shard_id} forwarder failed: {error}"));
                    }
                    TaskJoinOutcome::Incomplete => {
                        shard_failed = true;
                        drain.state.shutdown_errors.push(format!(
                            "market shard {shard_id} forwarder did not stop after abort"
                        ));
                    }
                }
            }

            if let Err(e) = shard.client.disconnect().await {
                shard_failed = true;
                drain
                    .state
                    .shutdown_errors
                    .push(format!("market shard {shard_id} disconnect failed: {e}"));
            }

            if !shard_failed {
                drain.state.shards.remove(&shard_id);
                drain
                    .state
                    .assignments
                    .retain(|_, assigned_id| *assigned_id != shard_id);
            }
        }

        if !drain.state.shutdown_errors.is_empty() {
            let errors = std::mem::take(&mut drain.state.shutdown_errors);
            anyhow::bail!(
                "Polymarket market pool shutdown failed: {}",
                errors.join("; ")
            );
        }

        *self.inner.out_tx.lock() = None;
        *self.inner.out_rx.lock() = None;
        Ok(())
    }

    pub(crate) fn begin_shutdown(&self) {
        self.inner.begin_shutdown();
    }

    /// Clears retained reconnect-replay state on any remaining shards.
    pub(crate) fn clear_reconnect_state(&self) {
        let state = self.inner.state.lock();
        for shard in state.shards.values() {
            shard.client.clear_reconnect_state();
        }
    }

    /// Returns the number of open shard connections.
    #[must_use]
    pub fn connection_count(&self) -> usize {
        self.inner.state.lock().shards.len()
    }

    /// Returns the number of unique assets assigned across all shards.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.inner.state.lock().assignments.len()
    }
}

impl Drop for PoolInner {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);

        for shard in self.state.get_mut().shards.values_mut() {
            shard.forwarder.abort();
            shard.client.abort();
        }
    }
}

#[allow(
    clippy::missing_panics_doc,
    reason = "internal mutex locks and shard-state invariants are not expected to panic"
)]
impl PolymarketMarketPoolHandle {
    pub(crate) fn begin_shutdown(&self) {
        self.inner.begin_shutdown();
    }

    /// Subscribes to market data for the given asset IDs, sharding across connections.
    ///
    /// # Errors
    ///
    /// Returns an error if a shard cannot be opened or a subscribe send fails.
    pub async fn subscribe_market(&self, asset_ids: Vec<String>) -> anyhow::Result<()> {
        let _wire = self.inner.wire_mutex.lock().await;
        self.inner.ensure_open()?;
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
        self.inner.ensure_open()?;
        for asset_id in asset_ids {
            self.inner.unsubscribe_one(asset_id).await?;
        }
        Ok(())
    }
}

impl PoolInner {
    fn begin_shutdown(&self) {
        let state = self.state.lock();
        self.closed.store(true, Ordering::Release);

        for shard in state.shards.values() {
            shard.client.begin_shutdown();
        }
    }

    fn ensure_open(&self) -> anyhow::Result<()> {
        let _state = self.state.lock();

        if self.closed.load(Ordering::Acquire) {
            anyhow::bail!("Market connection pool is closed");
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
            state: Mutex::new(PoolState::new()),
            out_tx: Mutex::new(None),
            out_rx: Mutex::new(None),
            socket_factory: Mutex::new(None),
            closed: AtomicBool::new(false),
        }
    }

    // Callers hold `wire_mutex`.
    async fn subscribe_one(&self, asset_id: String) -> anyhow::Result<()> {
        let token = Ustr::from(asset_id.as_str());

        let Some(handle) = self.assign(token).await? else {
            return Ok(());
        };

        if let Err(e) = self.ensure_open() {
            if let ReleaseOutcome::CloseShard(id, shard) = self.release(token)
                && let Err(close_error) = self.close_shard(id, shard).await
            {
                anyhow::bail!("{e}; subscription rollback failed: {close_error}");
            }
            return Err(e);
        }

        if let Err(e) = handle.subscribe_market(vec![asset_id]).await {
            // Roll back so a failed send leaves no stale assignment or empty shard.
            if let ReleaseOutcome::CloseShard(id, shard) = self.release(token)
                && let Err(close_error) = self.close_shard(id, shard).await
            {
                anyhow::bail!("{e}; subscription rollback failed: {close_error}");
            }
            return Err(e);
        }
        Ok(())
    }

    // Callers hold `wire_mutex`.
    async fn unsubscribe_one(&self, asset_id: String) -> anyhow::Result<()> {
        self.ensure_open()?;
        let token = Ustr::from(asset_id.as_str());

        match self.release(token) {
            ReleaseOutcome::NotOwned => Ok(()),
            ReleaseOutcome::Unsubscribe(handle) => handle.unsubscribe_market(vec![asset_id]).await,
            ReleaseOutcome::CloseShard(id, shard) => {
                // Disconnect drops the shard's subscriptions; no unsubscribe send needed.
                self.close_shard(id, shard).await
            }
        }
    }

    // Returns `None` when the token is already owned by a shard.
    async fn assign(&self, token: Ustr) -> anyhow::Result<Option<WsSubscriptionHandle>> {
        {
            let mut state = self.state.lock();

            if self.closed.load(Ordering::Acquire) {
                anyhow::bail!("Market connection pool is closed");
            }

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

        let rejected_shard = {
            let mut state = self.state.lock();

            if self.closed.load(Ordering::Acquire) {
                Some(
                    state
                        .shards
                        .remove(&id)
                        .expect("new shard retained for shutdown"),
                )
            } else {
                let handle = {
                    let shard = state.shards.get_mut(&id).expect("new shard present");
                    shard.owned += 1;
                    shard.handle.clone()
                };
                state.assignments.insert(token, id);
                return Ok(Some(handle));
            }
        };

        if let Some(shard) = rejected_shard {
            if let Err(e) = self.close_shard(id, Box::new(shard)).await {
                anyhow::bail!("Market connection pool is closed; shard rollback failed: {e}");
            }
            anyhow::bail!("Market connection pool is closed");
        }
        unreachable!("open pool returned from assignment")
    }

    fn release(&self, token: Ustr) -> ReleaseOutcome {
        let mut state = self.state.lock();

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
            ReleaseOutcome::CloseShard(id, Box::new(shard))
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
            let state = self.state.lock();
            available_shard_id(&state)
        };

        let mut client = self.market_client(self.subscribe_new_markets, id);
        client.connect().await?;

        let handle = client.clone_subscription_handle();
        let rx = client
            .take_message_receiver()
            .ok_or_else(|| anyhow::anyhow!("Market shard receiver unavailable after connect"))?;
        let forwarder = match self.spawn_forwarder(rx, is_primary) {
            Ok(forwarder) => forwarder,
            Err((e, forwarder)) => {
                let shard = Box::new(ShardEntry {
                    client,
                    handle,
                    forwarder,
                    owned: 0,
                    closing: true,
                });

                if let Err(close_error) = self.close_shard(id, shard).await {
                    anyhow::bail!(
                        "Failed to start market shard forwarder: {e}; startup rollback failed: \
                         {close_error}"
                    );
                }
                anyhow::bail!("Failed to start market shard forwarder: {e}");
            }
        };

        let shard = ShardEntry {
            client,
            handle,
            forwarder,
            owned: 0,
            closing: false,
        };
        let rejected_shard = {
            let mut state = self.state.lock();

            if self.closed.load(Ordering::Acquire) {
                Some(shard)
            } else {
                state.shards.insert(id, shard);
                None
            }
        };

        if let Some(shard) = rejected_shard {
            if let Err(e) = self.close_shard(id, Box::new(shard)).await {
                anyhow::bail!("Market connection pool is closed; shard rollback failed: {e}");
            }
            anyhow::bail!("Market connection pool is closed");
        }

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
        let factory = self.socket_factory.lock().clone();

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
    ) -> Result<TaskSlot<()>, (TaskSpawnError, TaskSlot<()>)> {
        let out_tx = self.out_tx.lock().clone();

        let mut forwarder = TaskSlot::new();
        if let Err(e) = forwarder.spawn(async move {
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
        }) {
            return Err((e, forwarder));
        }
        Ok(forwarder)
    }

    async fn close_shard(&self, id: usize, shard: Box<ShardEntry>) -> anyhow::Result<()> {
        let mut close = ShardClose::new(&self.state, id, shard);
        close.shard_mut().forwarder.abort();
        let forwarder_stopped = match finish_task(
            &mut close.shard_mut().forwarder,
            std::time::Duration::ZERO,
            std::time::Duration::from_secs(2),
        )
        .await
        {
            None | Some(TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted) => true,
            Some(TaskJoinOutcome::Failed(error)) => {
                let error = format!("market shard {id} forwarder failed: {error}");
                self.state.lock().shutdown_errors.push(error);
                true
            }
            Some(TaskJoinOutcome::Incomplete) => {
                let error = format!("market shard {id} forwarder did not stop after abort");
                self.state.lock().shutdown_errors.push(error);
                false
            }
        };

        if let Err(e) = close.shard_mut().client.disconnect().await {
            let error = format!("market shard {id} disconnect failed: {e}");
            self.state.lock().shutdown_errors.push(error);
        }

        if forwarder_stopped && !close.shard_mut().client.has_task() {
            close.complete();
        }

        let errors = std::mem::take(&mut self.state.lock().shutdown_errors);

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(errors.join("; "))
        }
    }

    #[cfg(test)]
    fn subscription_count_for_test(&self) -> usize {
        self.state.lock().assignments.len()
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
        .filter(|(_, shard)| !shard.closing && shard.owned < max_subscriptions)
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
            let mut state = inner.state.lock();
            state.shards.insert(
                PRIMARY_SHARD_ID,
                ShardEntry {
                    client: PolymarketWebSocketClient::new_market(
                        None,
                        false,
                        TransportBackend::default(),
                    ),
                    handle: WsSubscriptionHandle::from_sender(sender),
                    forwarder: TaskSlot::new(),
                    owned: assigned.len(),
                    closing: false,
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
    use std::{net::SocketAddr, sync::Arc as StdArc, time::Duration};

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
    use parking_lot::{Condvar, Mutex as TestMutex};
    use rstest::rstest;

    use super::*;
    use crate::websocket::handler::HandlerCommand;

    struct BlockingDrop(StdArc<(TestMutex<(bool, bool)>, Condvar)>);

    impl Drop for BlockingDrop {
        fn drop(&mut self) {
            let (state, wake) = &*self.0;
            let mut state = state.lock();
            state.0 = true;
            wake.notify_all();
            while !state.1 {
                wake.wait(&mut state);
            }
        }
    }

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
                    forwarder: TaskSlot::new(),
                    owned: *owned,
                    closing: false,
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
        *inner.out_tx.lock() = Some(out_tx);
        let (shard_tx, shard_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut forwarder = inner
            .spawn_forwarder(shard_rx, false)
            .expect("spawn forwarder");

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
        let outcome = finish_task(
            &mut forwarder,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .await
        .expect("forwarder task");
        assert!(matches!(outcome, TaskJoinOutcome::Completed(())));

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
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn canceled_shard_close_restores_unfinished_ownership() {
        let inner = Arc::new(PoolInner::new(None, TransportBackend::default(), false, 1));
        let blocking = StdArc::new((TestMutex::new((false, false)), Condvar::new()));
        let blocking_task = StdArc::clone(&blocking);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let mut forwarder = TaskSlot::new();
        forwarder
            .spawn(async move {
                let _blocking = BlockingDrop(blocking_task);
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            })
            .expect("spawn forwarder");
        started_rx.await.expect("forwarder should start");

        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let shard = Box::new(ShardEntry {
            client: PolymarketWebSocketClient::new_market(None, false, TransportBackend::default()),
            handle: WsSubscriptionHandle::from_sender(cmd_tx),
            forwarder,
            owned: 0,
            closing: false,
        });
        let close_inner = Arc::clone(&inner);
        let close = tokio::spawn(async move {
            let _result = close_inner.close_shard(1, shard).await;
        });

        loop {
            if blocking.0.lock().0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        close.abort();
        let _ = close.await;

        {
            let restored = inner.state.lock();
            let shard = restored.shards.get(&1).expect("closing shard restored");
            assert!(shard.closing);
            assert!(shard.forwarder.is_some());
            assert_eq!(smallest_shard_with_capacity(&restored, 1), None);
        }

        {
            let (state, wake) = &*blocking;
            state.lock().1 = true;
            wake.notify_all();
        }
        let shard = inner
            .state
            .lock()
            .shards
            .remove(&1)
            .expect("restored shard");
        inner
            .close_shard(1, Box::new(shard))
            .await
            .expect("restored shard should close");

        assert!(inner.state.lock().shards.is_empty());
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
