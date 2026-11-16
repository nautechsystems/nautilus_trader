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

//! Authenticated multiplexed WebSocket lifecycle and persistent subscription intent.

use std::{
    collections::BTreeMap,
    fmt::Debug,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use nautilus_common::live::dst::time;
use nautilus_core::{string::secret::SecretString, time::get_atomic_clock_realtime};
use nautilus_live::task::TaskGroup;
use nautilus_network::websocket::{
    AuthTracker, MessageHandler, SubscriptionState, WebSocketClient, WebSocketConfig,
};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::{
    error::KalshiWsError,
    handler::{HandlerCommand, KalshiWsHandler},
    messages::{KalshiSubscription, KalshiUpdateAction, KalshiWsMessage},
};
use crate::{common::credential::KalshiCredential, config::KalshiDataClientConfig};

/// Events from one authenticated protocol generation.
#[derive(Debug, Clone, PartialEq)]
pub enum KalshiWsEvent {
    /// All retained subscription intent has authoritative confirmation.
    Ready { generation: u64 },
    /// A decoded frame whose subscription belongs to this generation.
    Message {
        generation: u64,
        message: KalshiWsMessage,
    },
    /// A sanitized protocol, command, or transport failure.
    Error { generation: u64, message: String },
    /// A sequence gap on a book subscription; every book on `sid` needs a snapshot.
    BookSequenceGap { generation: u64, sid: u64 },
    /// A managed snapshot write completed on the current connection, before its replies are forwarded.
    BookSnapshotRequested {
        generation: u64,
        sid: u64,
        id: u64,
        attempt: u64,
    },
}

impl KalshiWsEvent {
    pub(crate) fn generation(&self) -> u64 {
        match self {
            Self::Ready { generation }
            | Self::Message { generation, .. }
            | Self::Error { generation, .. }
            | Self::BookSequenceGap { generation, .. }
            | Self::BookSnapshotRequested { generation, .. } => *generation,
        }
    }
}

/// One multiplexed Predictions connection with reference-counted subscription intent.
///
/// Subscription calls record intent; events report venue confirmation or failure.
/// Intent survives disconnect and stop. Reset clears it, and dispose permanently closes admission.
/// Automatic transport reconnection is disabled. After an unexpected disconnect, call `stop`
/// before `connect` to create a freshly signed connection and restore retained intent.
/// Clones share lifecycle, subscription intent, and task ownership.
#[derive(Clone)]
pub struct KalshiWebSocketClient {
    inner: Arc<Shared>,
    tasks: Arc<TaskGroup>,
}

impl Debug for KalshiWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KalshiWebSocketClient))
            .field("active", &self.is_active())
            .finish_non_exhaustive()
    }
}

#[allow(
    clippy::missing_panics_doc,
    reason = "client state mutex poisoning is not expected"
)]
impl KalshiWebSocketClient {
    /// Creates a disconnected client and its unbounded event receiver.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration is invalid.
    pub fn new(
        config: KalshiDataClientConfig,
    ) -> anyhow::Result<(Self, tokio::sync::mpsc::UnboundedReceiver<KalshiWsEvent>)> {
        config.validate()?;
        let (events, receiver) = tokio::sync::mpsc::unbounded_channel();

        let inner = Arc::new(Shared {
            config,
            state: Mutex::new(ClientState::default()),
            lifecycle: tokio::sync::Mutex::new(()),
            generation: AtomicU64::new(0),
            subscriptions: SubscriptionState::new('|'),
            auth: AuthTracker::new(),
            ready: AtomicBool::new(false),
            ready_notify: tokio::sync::Notify::new(),
            events,
        });

        Ok((
            Self {
                inner,
                tasks: Arc::new(TaskGroup::new()),
            },
            receiver,
        ))
    }

    /// Connects, authenticates, and waits for retained subscriptions to be confirmed.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is disposed, credentials or signing fail, the connection or
    /// task admission fails, or retained subscriptions are not restored before the timeout.
    pub async fn connect(&self) -> anyhow::Result<()> {
        let _lifecycle = self.inner.lifecycle.lock().await;

        let exists = {
            let state = self
                .inner
                .state
                .lock()
                .expect("client state lock is not poisoned");
            anyhow::ensure!(!state.disposed, "Kalshi WebSocket client is disposed");
            state.sender.is_some()
        };

        if !exists {
            if !self.tasks.is_open() {
                self.tasks.start_generation()?;
            }

            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let input = sender.clone();
            let config = connection_config(&self.inner.config)?;
            self.inner.auth.begin();

            let result = WebSocketClient::epoch_builder()
                .config(config)
                .epoch_handler(Arc::new(move |epoch, message| {
                    let _ = input.send(HandlerCommand::Frame {
                        epoch,
                        received: time::Instant::now(),
                        message,
                    });
                }))
                .connect()
                .await;

            let ws_client = match result {
                Ok(ws_client) => Arc::new(ws_client),
                Err(e) => {
                    self.inner.auth.fail("Kalshi WebSocket handshake failed");
                    anyhow::bail!(
                        "Kalshi WebSocket connection failed: {}",
                        e.redacted_message()
                    );
                }
            };

            ws_client.set_auth_tracker(self.inner.auth.clone(), false);
            self.inner.auth.succeed();
            {
                let mut state = self
                    .inner
                    .state
                    .lock()
                    .expect("client state lock is not poisoned");

                let handler = KalshiWsHandler::new(
                    Arc::clone(&self.inner),
                    Arc::clone(&ws_client),
                    receiver,
                    self.tasks.cancellation_token(),
                );

                self.tasks.spawn(handler.run())?;
                state.ws_client = Some(ws_client);
                state.sender = Some(sender.clone());
                let _ = sender.send(HandlerCommand::Reconcile);
            }
        }

        let timeout = Duration::from_secs(self.inner.config.ws_timeout_secs);
        if time::timeout(timeout, self.wait_ready()).await.is_err() {
            self.stop_locked().await?;
            anyhow::bail!("Kalshi WebSocket subscription restoration timed out");
        }

        Ok(())
    }

    async fn wait_ready(&self) {
        loop {
            let notified = self.inner.ready_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            if self.is_active() {
                return;
            }

            notified.await;
        }
    }

    /// Returns whether the authenticated transport and retained subscriptions are ready.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let state = self
            .inner
            .state
            .lock()
            .expect("client state lock is not poisoned");
        self.inner.ready.load(Ordering::Acquire)
            && self.inner.auth.is_authenticated()
            && state
                .ws_client
                .as_ref()
                .is_some_and(|ws_client| ws_client.is_active())
    }

    pub(crate) fn connection_epoch(&self) -> Option<(u64, u64)> {
        if !self.is_active() {
            return None;
        }

        self.inner
            .state
            .lock()
            .expect("client state lock is not poisoned")
            .ws_client
            .as_ref()
            .map(|ws_client| {
                (
                    self.inner.generation.load(Ordering::Acquire),
                    ws_client.connection_epoch_atomic().load(Ordering::Acquire),
                )
            })
    }

    /// Adds one local reference and records intent, including before connection.
    ///
    /// # Errors
    ///
    /// Returns an error if arguments are invalid, the client is disposed, or the handler is stopped.
    pub fn subscribe(&self, subscription: KalshiSubscription) -> anyhow::Result<()> {
        let subscription = subscription.normalize()?;
        let topic = subscription.topic();
        let mut state = self
            .inner
            .state
            .lock()
            .expect("client state lock is not poisoned");
        anyhow::ensure!(!state.disposed, "Kalshi WebSocket client is disposed");

        if self.inner.subscriptions.add_reference(&topic) {
            self.inner.subscriptions.mark_subscribe(&topic);
            state.desired.insert(topic, subscription);
            self.inner.ready.store(false, Ordering::Release);
            state.notify()?;
        }

        Ok(())
    }

    /// Removes one local reference; the final removal cancels replay intent.
    ///
    /// # Errors
    ///
    /// Returns an error if arguments are invalid or the handler is stopped.
    pub fn unsubscribe(&self, subscription: &KalshiSubscription) -> anyhow::Result<()> {
        let topic = subscription.clone().normalize()?.topic();
        let mut state = self
            .inner
            .state
            .lock()
            .expect("client state lock is not poisoned");

        if self.inner.subscriptions.remove_reference(&topic) {
            self.inner.subscriptions.mark_unsubscribe(&topic);
            state.desired.remove(&topic);
            self.inner.ready.store(false, Ordering::Release);
            state.notify()?;
        }

        Ok(())
    }

    /// Updates the shared subscription identified by its original arguments.
    ///
    /// Adds and deletes change retained replay intent. Snapshot requests are connection-bound.
    /// Removing every explicitly selected market requires unsubscribe instead.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is disposed, the channel or filter change is unsupported,
    /// intent is absent, markets are empty or duplicated, all markets would be deleted, the handler
    /// is stopped, or a snapshot is requested before the connection is ready.
    pub fn update_subscription(
        &self,
        original: &KalshiSubscription,
        action: KalshiUpdateAction,
        markets: Vec<String>,
    ) -> anyhow::Result<()> {
        let original = original.clone().normalize()?;
        let topic = original.topic();
        anyhow::ensure!(
            matches!(
                original.channel,
                super::messages::KalshiChannel::OrderbookDelta
                    | super::messages::KalshiChannel::Ticker
                    | super::messages::KalshiChannel::Fill
                    | super::messages::KalshiChannel::UserOrders
            ),
            "channel does not support market updates"
        );
        anyhow::ensure!(!markets.is_empty(), "market updates require tickers");

        let requested = KalshiSubscription {
            channel: original.channel,
            market_tickers: markets,
        }
        .normalize()?;

        let mut state = self
            .inner
            .state
            .lock()
            .expect("client state lock is not poisoned");
        anyhow::ensure!(!state.disposed, "Kalshi WebSocket client is disposed");
        let subscription = state
            .desired
            .get_mut(&topic)
            .ok_or_else(|| anyhow::anyhow!("subscription intent is absent"))?;

        if action == KalshiUpdateAction::GetSnapshot {
            anyhow::ensure!(
                original.channel == super::messages::KalshiChannel::OrderbookDelta,
                "snapshots require an order-book subscription"
            );
            anyhow::ensure!(
                self.inner.ready.load(Ordering::Acquire),
                "snapshot requires a ready connection"
            );
            let sender = state
                .sender
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("snapshot requires a connected handler"))?;
            sender
                .send(HandlerCommand::Snapshot {
                    topic,
                    markets: requested.market_tickers,
                })
                .map_err(|_| anyhow::anyhow!("Kalshi WebSocket handler stopped"))?;

            return Ok(());
        }

        anyhow::ensure!(
            !subscription.market_tickers.is_empty(),
            "global subscription filters cannot be updated"
        );
        let mut updated = subscription.clone();

        match action {
            KalshiUpdateAction::AddMarkets => {
                updated.market_tickers.extend(requested.market_tickers);
                updated.market_tickers.sort();
                updated.market_tickers.dedup();
            }
            KalshiUpdateAction::DeleteMarkets => updated
                .market_tickers
                .retain(|market| !requested.market_tickers.contains(market)),
            KalshiUpdateAction::GetSnapshot => unreachable!(),
        }

        anyhow::ensure!(
            !updated.market_tickers.is_empty(),
            "unsubscribe to remove all markets"
        );
        *subscription = updated;
        self.inner.subscriptions.mark_failure(&topic);
        self.inner.ready.store(false, Ordering::Release);
        state.notify()
    }

    pub(crate) async fn request_book_snapshot(
        &self,
        subscription: &KalshiSubscription,
        market: String,
        attempt: u64,
        cancel: CancellationToken,
    ) -> Result<(), KalshiWsError> {
        let sender = {
            let state = self
                .inner
                .state
                .lock()
                .expect("client state lock is not poisoned");

            if state.disposed {
                return Err(KalshiWsError::Client(
                    "Kalshi WebSocket client is disposed".into(),
                ));
            }

            state
                .sender
                .clone()
                .ok_or_else(|| KalshiWsError::Client("handler is stopped".into()))?
        };

        let (completed, result) = tokio::sync::oneshot::channel();
        sender
            .send(HandlerCommand::BookSnapshot {
                topic: subscription.topic(),
                markets: vec![market],
                attempt,
                cancel: cancel.clone(),
                completed,
            })
            .map_err(|_| KalshiWsError::Client("handler is stopped".into()))?;

        tokio::select! {
            biased;
            () = cancel.cancelled() => Err(KalshiWsError::Client("snapshot attempt canceled".into())),
            result = result => result.unwrap_or_else(|_| Err(KalshiWsError::Client("snapshot write abandoned".into()))),
        }
    }

    /// Stops the connection and drains owned tasks while retaining subscription intent.
    ///
    /// # Errors
    ///
    /// Returns an error if bounded task teardown fails.
    pub async fn stop(&self) -> anyhow::Result<()> {
        let _lifecycle = self.inner.lifecycle.lock().await;
        self.stop_locked().await
    }

    /// Stops the connection and clears subscription intent and reference counts.
    ///
    /// # Errors
    ///
    /// Returns an error if bounded task teardown fails.
    pub async fn reset(&self) -> anyhow::Result<()> {
        let _lifecycle = self.inner.lifecycle.lock().await;
        self.stop_locked().await?;
        let mut state = self
            .inner
            .state
            .lock()
            .expect("client state lock is not poisoned");
        state.desired.clear();
        self.inner.subscriptions.clear();
        Ok(())
    }

    /// Stops the connection and permanently rejects new connection and subscription requests.
    ///
    /// # Errors
    ///
    /// Returns an error if bounded task teardown fails.
    pub async fn dispose(&self) -> anyhow::Result<()> {
        let _lifecycle = self.inner.lifecycle.lock().await;
        self.inner
            .state
            .lock()
            .expect("client state lock is not poisoned")
            .disposed = true;
        self.stop_locked().await?;
        let mut state = self
            .inner
            .state
            .lock()
            .expect("client state lock is not poisoned");
        state.desired.clear();
        self.inner.subscriptions.clear();
        Ok(())
    }

    async fn stop_locked(&self) -> anyhow::Result<()> {
        let ws_client = {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("client state lock is not poisoned");
            state.sender = None;
            self.inner.ready.store(false, Ordering::Release);
            self.inner.auth.fail("Kalshi WebSocket client stopped");
            state.ws_client.clone()
        };

        self.tasks.begin_shutdown();

        if let Some(ws_client) = ws_client {
            ws_client.disconnect().await;
        }

        self.tasks
            .finish_shutdown(Duration::from_secs(2), Duration::from_secs(2))
            .await?;
        self.inner
            .state
            .lock()
            .expect("client state lock is not poisoned")
            .ws_client = None;
        self.inner.subscriptions.reset_after_reconnect();
        Ok(())
    }
}

pub(super) struct Shared {
    pub config: KalshiDataClientConfig,
    pub state: Mutex<ClientState>,
    lifecycle: tokio::sync::Mutex<()>,
    pub generation: AtomicU64,
    pub subscriptions: SubscriptionState,
    pub auth: AuthTracker,
    pub ready: AtomicBool,
    pub ready_notify: tokio::sync::Notify,
    pub events: tokio::sync::mpsc::UnboundedSender<KalshiWsEvent>,
}

#[derive(Default)]
pub(super) struct ClientState {
    pub desired: BTreeMap<String, KalshiSubscription>,
    sender: Option<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>,
    ws_client: Option<Arc<WebSocketClient>>,
    disposed: bool,
}

impl ClientState {
    fn notify(&self) -> anyhow::Result<()> {
        if let Some(sender) = &self.sender {
            sender
                .send(HandlerCommand::Reconcile)
                .map_err(|_| anyhow::anyhow!("Kalshi WebSocket handler stopped"))?;
        }

        Ok(())
    }
}

/// Opens a raw handler-mode connection with freshly signed static headers.
///
/// This low-level entry point does not manage subscriptions, transparent reconnection, or
/// application recovery.
///
/// # Errors
///
/// Returns an error if configuration, credentials, signing, or connection establishment fails.
pub async fn connect(
    config: &KalshiDataClientConfig,
    message_handler: MessageHandler,
) -> anyhow::Result<WebSocketClient> {
    let transport = connection_config(config)?;
    WebSocketClient::builder()
        .config(transport)
        .message_handler(message_handler)
        .connect()
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Kalshi WebSocket connection failed: {}",
                e.redacted_message()
            )
        })
}

fn connection_config(config: &KalshiDataClientConfig) -> anyhow::Result<WebSocketConfig> {
    config.validate()?;
    let credential = KalshiCredential::resolve(
        config.api_key_id.clone(),
        config.private_key.clone(),
        config.environment,
    )?;
    let endpoint = Url::parse(config.ws_url())
        .map_err(|_| anyhow::anyhow!("invalid Kalshi WebSocket endpoint"))?;
    let path = SecretString::from(endpoint.path());
    let endpoint = SecretString::from(String::from(endpoint));

    let headers = credential
        .headers(
            get_atomic_clock_realtime().get_time_ms(),
            "GET",
            path.expose_secret(),
        )
        .map_err(|_| {
            anyhow::anyhow!("Kalshi WebSocket connection failed: I/O error: InvalidInput")
        })?
        .into_iter()
        .map(|(name, value)| (name, value.into_inner()))
        .collect();

    let transport = WebSocketConfig::builder()
        .url(endpoint.into_inner())
        .headers(headers)
        .connect_timeout_ms(config.ws_timeout_secs * 1_000)
        .reconnect_max_attempts(0)
        .maybe_proxy_url(
            config
                .proxy_url
                .as_ref()
                .map(|s| s.expose_secret().to_owned()),
        )
        .backend(config.transport_backend)
        .build()?;
    Ok(transport)
}

#[cfg(all(test, not(all(feature = "simulation", madsim))))]
pub(super) mod tests {
    use std::sync::Arc;

    use super::{KalshiWebSocketClient, KalshiWsEvent, Shared, WebSocketConfig};
    use crate::config::KalshiDataClientConfig;

    pub(in crate::websocket) fn transport_config(
        config: &KalshiDataClientConfig,
    ) -> WebSocketConfig {
        super::connection_config(config).unwrap()
    }

    pub(in crate::websocket) fn shared(
        config: KalshiDataClientConfig,
    ) -> (
        Arc<Shared>,
        tokio::sync::mpsc::UnboundedReceiver<KalshiWsEvent>,
    ) {
        let (client, events) = KalshiWebSocketClient::new(config).unwrap();
        (Arc::clone(&client.inner), events)
    }
}
