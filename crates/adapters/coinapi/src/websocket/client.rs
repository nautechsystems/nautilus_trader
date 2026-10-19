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

//! CoinAPI WebSocket V1 connection with whole-scope replacement and replay.
//!
//! CoinAPI has no subscription acknowledgements. The connection authenticates through the
//! `X-CoinAPI-Key` upgrade header, and the server validates it when the first `hello` arrives.
//! A heartbeat or accepted data frame confirms the session and its subscriptions; an `error` frame
//! is terminal and retires the connection instead of reconnecting.
//!
//! A server `reconnect` notice is only logged. Reconnecting early would open a second connection
//! while the old one is open, which a one-connection tier rejects; the server's close triggers the
//! normal reconnect instead.

use std::{
    fmt::Debug,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use nautilus_core::string::secret::SecretString;
use nautilus_network::{
    Message, RECONNECTED, dst,
    websocket::{
        AuthTracker, MessageHandler, SubscriptionState, TransportBackend, WebSocketClient,
        WebSocketConfig, proxy::ProxyTarget, split_topic,
    },
};
use parking_lot::Mutex;
use url::Url;
use ustr::Ustr;

use super::{
    error::Error,
    messages::{Channel, CoinApiWsMessage, DataType, Frame, Hello, ScopeChange, Subscription},
};
use crate::{
    book::sync::BookSubscriptions,
    common::{consts::COINAPI_NAUTILUS_REFERRER, credential::is_valid_api_key},
};

/// Primary CoinAPI Market Data WebSocket V1 endpoint.
pub const WS_URL: &str = "wss://ws.coinapi.io/v1/";

// The server sends a heartbeat after each second of silence
const IDLE_TIMEOUT_MS: u64 = 5_000;
const RECONNECT_DELAY_INITIAL_MS: u64 = 1_000;
const RECONNECT_DELAY_MAX_MS: u64 = 30_000;
const RECONNECT_BACKOFF_FACTOR: f64 = 2.0;
const RECONNECT_JITTER_MS: u64 = 500;
const TOPIC_DELIMITER: char = ':';
const PERIOD_DELIMITER: char = '.';
const PAYLOAD_EXCERPT_BYTES: usize = 128;

// Live probes found that back-to-back hellos can leave a connection streaming nothing, and that
// CoinAPI closes a connection once hellos exceed about one every two seconds per IP
const HELLO_INTERVAL_MIN: dst::time::Duration = dst::time::Duration::from_secs(2);
const RESTART_INTERVAL_MIN: dst::time::Duration = dst::time::Duration::from_secs(5);

/// Transport configuration for the raw WebSocket client.
#[derive(Debug, Clone)]
pub struct CoinApiWebSocketConfig {
    /// Endpoint URL; accepts local simulated peers.
    pub url: String,
    /// Transport implementation; simulated peers require Tungstenite.
    pub backend: TransportBackend,
    /// Optional explicit proxy URL for the WebSocket transport.
    pub proxy_url: Option<SecretString>,
}

impl Default for CoinApiWebSocketConfig {
    fn default() -> Self {
        Self {
            url: WS_URL.into(),
            backend: TransportBackend::default(),
            proxy_url: None,
        }
    }
}

/// Authenticated, data-only CoinAPI WebSocket client using one connection.
///
/// CoinAPI tiers can limit an organization to one concurrent connection. Subscription changes send
/// a `hello` that replaces the whole session scope; changes applied together share one `hello`,
/// and the client sends at most one every two seconds, including across connection recreation.
/// Because CoinAPI combines every listed data type, symbol, and period, the client drops frames
/// outside the subscribed pairs; the provider still bills for them. A scope can list only one
/// book type. Removing the last subscription closes the connection, and the next subscription
/// opens a new one. Reconnects replay the scope and emit [`CoinApiWsMessage::Reconnected`].
#[derive(Debug)]
pub struct CoinApiWebSocketClient {
    api_key: SecretString,
    config: CoinApiWebSocketConfig,
    connection: Arc<tokio::sync::Mutex<Option<Connection>>>,
    hello_at: Arc<Mutex<Option<dst::time::Instant>>>,
    next_connection_id: AtomicU64,
    books: Option<Arc<BookSubscriptions>>,
    out_tx: tokio::sync::mpsc::UnboundedSender<CoinApiWsMessage>,
}

impl CoinApiWebSocketClient {
    /// Creates a client and the receiver for its messages without connecting.
    ///
    /// # Errors
    ///
    /// Returns an error if the credentials, endpoint, or proxy URL are invalid.
    pub fn new(
        api_key: SecretString,
        config: CoinApiWebSocketConfig,
    ) -> Result<(Self, tokio::sync::mpsc::UnboundedReceiver<CoinApiWsMessage>), Error> {
        if !is_valid_api_key(api_key.expose_secret()) {
            return Err(Error::Validation(
                "API key must contain visible ASCII characters".into(),
            ));
        }

        let url = Url::parse(&config.url).map_err(|_| Error::Validation("invalid URL".into()))?;

        if !matches!(url.scheme(), "ws" | "wss")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::Validation(
                "URL must be WS(S) without credentials, query, or fragment".into(),
            ));
        }

        if let Some(proxy) = &config.proxy_url {
            ProxyTarget::parse(proxy.expose_secret())
                .map_err(|_| Error::Validation("proxy URL must be a valid HTTP(S) URL".into()))?;
        }

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();

        let client = Self {
            api_key,
            config,
            connection: Arc::new(tokio::sync::Mutex::new(None)),
            hello_at: Arc::new(Mutex::new(None)),
            next_connection_id: AtomicU64::new(0),
            books: None,
            out_tx,
        };

        Ok((client, out_rx))
    }

    /// Adds a subscription, connecting on first use.
    ///
    /// Returns once the new scope is queued for sending; the first data frame confirms it.
    ///
    /// # Errors
    ///
    /// Returns an error if validation, connecting, or sending fails. A book type other than the one
    /// already subscribed is a validation error. A failed send leaves the pair unsubscribed.
    pub async fn subscribe(&self, subscription: Subscription) -> Result<(), Error> {
        self.apply_one(ScopeChange::Subscribe(subscription)).await
    }

    /// Removes a subscription.
    ///
    /// CoinAPI does not acknowledge the replacement scope, so frames still in flight for the
    /// pair are dropped locally.
    ///
    /// # Errors
    ///
    /// Returns an error if validation or sending fails. A failed send keeps the pair subscribed.
    pub async fn unsubscribe(&self, subscription: Subscription) -> Result<(), Error> {
        self.apply_one(ScopeChange::Unsubscribe(subscription)).await
    }

    /// Applies scope changes in order and sends one `hello` for the resulting scope.
    ///
    /// Connects when a subscribe finds no connection, and closes the connection when the scope
    /// becomes empty, since an empty `hello` is a terminal protocol error. The `hello` waits until
    /// two seconds after the client's previous one. Returns once the scope is queued for
    /// sending, with each change rejected by validation, including a book type other than the
    /// one already subscribed.
    ///
    /// # Errors
    ///
    /// Returns an error if connecting or sending fails; every change in the batch is reverted.
    pub async fn apply(&self, changes: &[ScopeChange]) -> Result<Vec<(ScopeChange, Error)>, Error> {
        self.apply_registered(changes, &[]).await
    }

    pub(crate) fn bind_books(&mut self, books: Arc<BookSubscriptions>) {
        self.books = Some(books);
    }

    pub(crate) async fn apply_registered(
        &self,
        changes: &[ScopeChange],
        registrations: &[Option<Arc<AtomicBool>>],
    ) -> Result<Vec<(ScopeChange, Error)>, Error> {
        let mut rejected = Vec::new();
        let mut valid = Vec::with_capacity(changes.len());

        for (index, change) in changes.iter().enumerate() {
            match change.subscription().validate() {
                Ok(()) => valid.push((*change, registrations.get(index).cloned().flatten())),
                Err(e) => rejected.push((*change, e)),
            }
        }

        let mut guard = self.connection.lock().await;
        let fresh = guard.is_none();

        if fresh
            && !valid
                .iter()
                .any(|(change, _)| matches!(change, ScopeChange::Subscribe(_)))
        {
            return Ok(rejected);
        }

        if fresh {
            let connection = self.connect().await?;
            connection.auth.begin();
            *guard = Some(connection);
        }

        let Some(connection) = guard.as_mut() else {
            return Ok(rejected);
        };

        let result = connection.apply_scope(&valid, &mut rejected).await;
        if (matches!(result, Ok(false)) || (fresh && result.is_err()))
            && let Some(connection) = guard.take()
        {
            connection.close().await;
        }

        result.map(|_| rejected)
    }

    async fn apply_one(&self, change: ScopeChange) -> Result<(), Error> {
        match self.apply(&[change]).await?.pop() {
            Some((_, e)) => Err(e),
            None => Ok(()),
        }
    }

    /// Restarts a subscribed book pair's stream so CoinAPI sends it a fresh snapshot.
    ///
    /// CoinAPI replays nothing for a repeated scope, but restarts a pair's sequence with a
    /// snapshot when the pair leaves the scope and returns. The client sends the scope without the
    /// pair's symbol, then the full scope two seconds later, so other symbols keep streaming. When
    /// no other symbol is subscribed, the removal scope keeps the symbol's other channels instead,
    /// or lists only its trades when the book is the whole scope; the client drops those
    /// unsubscribed trades. The connection stays open, since a one-connection tier rejects a second
    /// one.
    ///
    /// Restarts on a connection start at least five seconds apart, because CoinAPI closes a
    /// connection that exceeds its per-IP `hello` rate. Subscription changes wait while a restart
    /// is pending. Calls `paused` as soon as a scope without the pair's symbol is queued, since
    /// that pauses the symbol's other channels; the call happens even if the restart is then
    /// cancelled. Returns once both scopes are queued for sending.
    ///
    /// # Errors
    ///
    /// Returns an error if the pair is not a subscribed book pair or sending fails.
    pub async fn resubscribe(
        &self,
        subscription: Subscription,
        paused: impl FnOnce(),
    ) -> Result<(), Error> {
        subscription.validate()?;

        if !subscription.channel.data_type.is_book() {
            return Err(Error::Validation(format!(
                "{} has no snapshot to restart",
                topic(&subscription)
            )));
        }

        let mut guard = self.connection.lock().await;

        let Some(connection) = guard
            .as_mut()
            .filter(|connection| is_subscribed(&connection.subscriptions, &subscription))
        else {
            return Err(Error::Validation(format!(
                "{} is not subscribed",
                topic(&subscription)
            )));
        };

        if let Some(restarted_at) = connection.restarted_at {
            dst::time::sleep_until(restarted_at + RESTART_INTERVAL_MIN).await;
        }

        let scope = scope(&connection.subscriptions);
        let removal = removal_scope(&scope, &subscription);
        let pauses = removal
            .iter()
            .all(|other| other.symbol_id != subscription.symbol_id);

        let removed = send_paced(connection, &removal).await;
        connection.restarted_at = *connection.hello_at.lock();
        removed?;

        if pauses {
            paused();
        }

        send_paced(connection, &scope).await
    }

    /// Closes the connection and forgets its subscriptions; repeated calls are safe.
    ///
    /// Dropping the client also closes the connection.
    pub async fn close(&self) {
        let mut guard = self.connection.lock().await;
        if let Some(connection) = guard.take() {
            connection.close().await;
        }
    }

    async fn connect(&self) -> Result<Connection, Error> {
        let config = WebSocketConfig::builder()
            .url(self.config.url.clone())
            .headers(vec![
                (
                    "X-CoinAPI-Key".into(),
                    self.api_key.expose_secret().to_owned(),
                ),
                ("X-Referrer".into(), COINAPI_NAUTILUS_REFERRER.into()),
            ])
            .idle_timeout_ms(IDLE_TIMEOUT_MS)
            .reconnect_delay_initial_ms(RECONNECT_DELAY_INITIAL_MS)
            .reconnect_delay_max_ms(RECONNECT_DELAY_MAX_MS)
            .reconnect_backoff_factor(RECONNECT_BACKOFF_FACTOR)
            .reconnect_jitter_ms(RECONNECT_JITTER_MS)
            .backend(self.config.backend)
            .maybe_proxy_url(
                self.config
                    .proxy_url
                    .as_ref()
                    .map(|url| url.expose_secret().to_owned()),
            )
            .build()
            .map_err(|e| Error::Validation(e.to_string()))?;

        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let handler: MessageHandler = Arc::new(move |message| {
            let _ = raw_tx.send(message);
        });

        let client = WebSocketClient::builder()
            .config(config)
            .message_handler(handler)
            .connect()
            .await?;

        let auth = AuthTracker::new();
        client.set_auth_tracker(auth.clone(), false);
        let client = Arc::new(client);
        let subscriptions = SubscriptionState::new(TOPIC_DELIMITER);
        let id = self.next_connection_id.fetch_add(1, Ordering::Relaxed);

        let handler = ConnectionHandler {
            id,
            client: Arc::clone(&client),
            subscriptions: subscriptions.clone(),
            auth: auth.clone(),
            connection: Arc::downgrade(&self.connection),
            out_tx: self.out_tx.clone(),
            api_key: self.api_key.clone(),
            books: self.books.clone(),
        };

        let task = dst::task::spawn(handler.run(raw_rx));

        Ok(Connection {
            id,
            client,
            subscriptions,
            auth,
            task,
            hello_at: Arc::clone(&self.hello_at),
            restarted_at: None,
        })
    }
}

type RegisteredChange = (ScopeChange, Option<Arc<AtomicBool>>);

struct Connection {
    id: u64,
    client: Arc<WebSocketClient>,
    subscriptions: SubscriptionState,
    auth: AuthTracker,
    task: dst::task::JoinHandle<()>,
    hello_at: Arc<Mutex<Option<dst::time::Instant>>>,
    restarted_at: Option<dst::time::Instant>,
}

impl Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Connection))
            .field("id", &self.id)
            .field("subscriptions", &self.subscriptions)
            .field("auth", &self.auth.auth_state())
            .finish_non_exhaustive()
    }
}

impl Connection {
    async fn apply_scope(
        &self,
        changes: &[RegisteredChange],
        rejected: &mut Vec<(ScopeChange, Error)>,
    ) -> Result<bool, Error> {
        let applied = self.change_scope(changes, rejected);
        let scope = scope(&self.subscriptions);

        if scope.is_empty() {
            return Ok(false);
        }

        if applied.is_empty() {
            return Ok(true);
        }

        if let Err(e) = send_paced(self, &scope).await {
            revert(&self.subscriptions, &applied);
            return Err(e);
        }

        self.commit_scope(changes, &applied);
        Ok(true)
    }

    fn commit_scope(&self, changes: &[RegisteredChange], applied: &[ScopeChange]) {
        // Retirement takes this same connection lock, so it observes every committed registration
        for (change, registration) in changes {
            if matches!(change, ScopeChange::Subscribe(_))
                && applied.contains(change)
                && let Some(registration) = registration
            {
                registration.store(true, Ordering::Relaxed);
            }
        }

        for change in applied {
            if let ScopeChange::Unsubscribe(subscription) = change {
                self.subscriptions.confirm_unsubscribe(&topic(subscription));
            }
        }
    }

    fn change_scope(
        &self,
        changes: &[RegisteredChange],
        rejected: &mut Vec<(ScopeChange, Error)>,
    ) -> Vec<ScopeChange> {
        let mut applied = Vec::with_capacity(changes.len());

        for (change, _) in changes {
            let change = *change;
            let topic = topic(change.subscription());

            match change {
                ScopeChange::Subscribe(subscription) => {
                    let scope = scope(&self.subscriptions);

                    if let Err(e) = check_book_type(&scope, &subscription) {
                        rejected.push((change, e));
                    } else if self.subscriptions.try_mark_subscribe(&topic) {
                        applied.push(change);
                    }
                }
                ScopeChange::Unsubscribe(subscription) => {
                    if is_subscribed(&self.subscriptions, &subscription) {
                        self.subscriptions.mark_unsubscribe(&topic);
                        applied.push(change);
                    }
                }
            }
        }

        applied
    }

    async fn close(self) {
        self.task.abort();
        self.client.disconnect().await;
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.task.abort();
    }
}

// Holds the slot weakly so dropping the public client drops the connection and aborts this task
struct ConnectionHandler {
    id: u64,
    client: Arc<WebSocketClient>,
    subscriptions: SubscriptionState,
    auth: AuthTracker,
    connection: Weak<tokio::sync::Mutex<Option<Connection>>>,
    out_tx: tokio::sync::mpsc::UnboundedSender<CoinApiWsMessage>,
    api_key: SecretString,
    books: Option<Arc<BookSubscriptions>>,
}

impl ConnectionHandler {
    async fn run(self, mut raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>) {
        while let Some(message) = raw_rx.recv().await {
            let Message::Text(bytes) = message else {
                continue;
            };

            if bytes.as_ref() == RECONNECTED.as_bytes() {
                self.replay().await;
                continue;
            }

            match Frame::decode(&bytes) {
                Ok(Frame::Data(message)) => self.route(message),
                Ok(Frame::Heartbeat) => self.confirm_session(),
                Ok(Frame::Reconnect(notice)) => {
                    log::info!(
                        "CoinAPI will close the connection before {}",
                        notice.before_time
                    );
                }
                Ok(Frame::Error(error)) => {
                    let message = error
                        .message
                        .replace(self.api_key.expose_secret(), "<redacted>");
                    self.retire(message).await;
                    return;
                }
                Ok(Frame::Unknown(kind)) => log::debug!("Ignoring CoinAPI frame type {kind}"),
                Err(e) => {
                    let excerpt =
                        String::from_utf8_lossy(&bytes[..bytes.len().min(PAYLOAD_EXCERPT_BYTES)]);
                    log::warn!("Failed to decode CoinAPI frame: {e}");
                    log::debug!("Undecodable CoinAPI frame excerpt: {excerpt}");
                }
            }
        }
    }

    fn route(&self, message: CoinApiWsMessage) {
        let Some(subscription) = message.subscription() else {
            return;
        };

        // The combined scope also delivers pairs nobody subscribed to
        if !is_subscribed(&self.subscriptions, &subscription) {
            log::trace!("Dropping unsubscribed CoinAPI {subscription:?}");
            return;
        }

        self.subscriptions.confirm_subscribe(&topic(&subscription));
        self.confirm_session();
        let _ = self.out_tx.send(message);
    }

    fn confirm_session(&self) {
        if !self.auth.is_authenticated() {
            self.auth.succeed();
        }
    }

    async fn replay(&self) {
        let Some(connection) = self.connection.upgrade() else {
            return;
        };

        let mut guard = connection.lock().await;

        let Some(connection) = guard.as_mut().filter(|connection| connection.id == self.id) else {
            return;
        };

        self.subscriptions.reset_after_reconnect();
        self.auth.begin();

        // A change in flight can reach the replacement socket first, so the replay waits its turn
        match send_paced(connection, &scope(&self.subscriptions)).await {
            Ok(()) => {
                let _ = self.out_tx.send(CoinApiWsMessage::Reconnected);
            }
            Err(e) => log::error!("Failed to replay CoinAPI scope: {e}"),
        }
    }

    async fn retire(&self, message: String) {
        log::error!("CoinAPI rejected the session: {message}");
        self.auth.fail(message.clone());

        let Some(connection) = self.connection.upgrade() else {
            return;
        };

        let mut guard = connection.lock().await;
        self.client.disconnect().await;

        if guard
            .as_ref()
            .is_some_and(|connection| connection.id == self.id)
            && let Some(books) = &self.books
        {
            books.retire();
        }

        let _ = self.out_tx.send(CoinApiWsMessage::Error(message));

        // Dropping the owned connection aborts this task, so nothing may await after it
        if guard
            .as_ref()
            .is_some_and(|connection| connection.id == self.id)
        {
            guard.take();
        }
    }
}

// Waits out the client's `hello` interval, then sends `scope`
async fn send_paced(connection: &Connection, scope: &[Subscription]) -> Result<(), Error> {
    let hello_at = *connection.hello_at.lock();
    if let Some(hello_at) = hello_at {
        dst::time::sleep_until(hello_at + HELLO_INTERVAL_MIN).await;
    }

    // A caller cancelled during the write leaves the queued hello to the writer, so the interval
    // starts before the write as well as after it
    *connection.hello_at.lock() = Some(dst::time::Instant::now());
    let result = send_scope(&connection.client, scope).await;
    *connection.hello_at.lock() = Some(dst::time::Instant::now());
    result
}

// Restores the subscription state from before a batch whose `hello` failed
fn revert(subscriptions: &SubscriptionState, applied: &[ScopeChange]) {
    for change in applied.iter().rev() {
        let topic = topic(change.subscription());

        match change {
            ScopeChange::Subscribe(_) => {
                subscriptions.mark_unsubscribe(&topic);
                subscriptions.confirm_unsubscribe(&topic);
            }
            ScopeChange::Unsubscribe(_) => subscriptions.mark_subscribe(&topic),
        }
    }
}

// Writes the hello on the current connection only; the transport would otherwise buffer it
// through a reconnect and send it on the replacement just ahead of the replayed scope
async fn send_scope(client: &WebSocketClient, scope: &[Subscription]) -> Result<(), Error> {
    if scope.is_empty() {
        return Ok(());
    }

    let hello = serde_json::to_string(&Hello::new(scope))
        .expect("CoinAPI hello contains only serializable fields");
    client
        .send_text_on_connection(hello, None, client.connection_epoch())
        .await?;
    Ok(())
}

fn scope(subscriptions: &SubscriptionState) -> Vec<Subscription> {
    subscriptions
        .all_topics()
        .iter()
        .filter_map(|topic| parse_topic(topic))
        .collect()
}

// CoinAPI rejects a scope listing two book types with a terminal error frame
fn check_book_type(scope: &[Subscription], subscription: &Subscription) -> Result<(), Error> {
    let requested = subscription.channel.data_type;

    if !requested.is_book() {
        return Ok(());
    }

    match scope
        .iter()
        .map(|subscription| subscription.channel.data_type)
        .find(|active| active.is_book() && *active != requested)
    {
        Some(active) => Err(Error::Validation(format!(
            "scope already streams {}, so cannot add {}",
            active.as_ref(),
            requested.as_ref()
        ))),
        None => Ok(()),
    }
}

// Leaves the book pair out of the effective scope, which combines every listed type and symbol.
// An empty `hello` is a terminal error, so a book alone in the scope gives way to its trades.
fn removal_scope(scope: &[Subscription], pair: &Subscription) -> Vec<Subscription> {
    let others: Vec<Subscription> = scope
        .iter()
        .filter(|subscription| subscription.symbol_id != pair.symbol_id)
        .copied()
        .collect();

    if !others.is_empty() {
        return others;
    }

    let channels: Vec<Subscription> = scope
        .iter()
        .filter(|subscription| *subscription != pair)
        .copied()
        .collect();

    if !channels.is_empty() {
        return channels;
    }

    vec![Subscription {
        channel: Channel {
            data_type: DataType::Trade,
            period_id: None,
        },
        symbol_id: pair.symbol_id,
    }]
}

fn is_subscribed(subscriptions: &SubscriptionState, subscription: &Subscription) -> bool {
    subscriptions.is_subscribed(
        &Ustr::from(&channel_key(&subscription.channel)),
        &subscription.symbol_id,
    )
}

fn topic(subscription: &Subscription) -> String {
    format!(
        "{}{TOPIC_DELIMITER}{}",
        channel_key(&subscription.channel),
        subscription.symbol_id
    )
}

fn channel_key(channel: &Channel) -> String {
    match channel.period_id {
        Some(period) => format!("{}{PERIOD_DELIMITER}{period}", channel.data_type.as_ref()),
        None => channel.data_type.as_ref().to_owned(),
    }
}

fn parse_topic(topic: &str) -> Option<Subscription> {
    let (channel, symbol_id) = split_topic(topic, TOPIC_DELIMITER);

    let (data_type, period_id) = match channel.split_once(PERIOD_DELIMITER) {
        Some((data_type, period)) => (data_type, Some(Ustr::from(period))),
        None => (channel, None),
    };

    Some(Subscription {
        channel: Channel {
            data_type: data_type.parse().ok()?,
            period_id,
        },
        symbol_id: Ustr::from(symbol_id?),
    })
}

#[cfg(test)]
mod tests {
    #[cfg(not(all(feature = "simulation", madsim)))]
    use futures_util::{SinkExt, StreamExt};
    #[cfg(not(all(feature = "simulation", madsim)))]
    use nautilus_network::error::SendError;
    use rstest::rstest;

    use super::*;

    fn sub(data_type: DataType, symbol_id: &str) -> Subscription {
        Subscription {
            channel: Channel {
                data_type,
                period_id: None,
            },
            symbol_id: symbol_id.into(),
        }
    }

    #[rstest]
    #[case::other_symbols_remain(
        vec![sub(DataType::Book, "A"), sub(DataType::Trade, "A"), sub(DataType::Book, "B")],
        vec![sub(DataType::Book, "B")]
    )]
    #[case::only_symbol_keeps_other_channels(
        vec![sub(DataType::Book, "A"), sub(DataType::Trade, "A"), sub(DataType::Quote, "A")],
        vec![sub(DataType::Trade, "A"), sub(DataType::Quote, "A")]
    )]
    #[case::pair_is_whole_scope(vec![sub(DataType::Book, "A")], vec![sub(DataType::Trade, "A")])]
    fn removal_scope_leaves_pair_out_of_effective_scope(
        #[case] scope: Vec<Subscription>,
        #[case] expected: Vec<Subscription>,
    ) {
        assert_eq!(removal_scope(&scope, &sub(DataType::Book, "A")), expected);
    }

    #[rstest]
    #[case(DataType::Book, None)]
    #[case(DataType::Trade, None)]
    #[case(
        DataType::Book5,
        Some("Invalid CoinAPI WebSocket request: scope already streams book, so cannot add book5")
    )]
    #[case(
        DataType::BookL3,
        Some(
            "Invalid CoinAPI WebSocket request: scope already streams book, so cannot add book_l3"
        )
    )]
    fn scope_accepts_one_book_type(#[case] requested: DataType, #[case] expected: Option<&str>) {
        let scope = [sub(DataType::Trade, "A"), sub(DataType::Book, "A")];

        let result = check_book_type(&scope, &sub(requested, "B"));

        assert_eq!(result.err().map(|e| e.to_string()).as_deref(), expected);
    }

    #[rstest]
    fn rollback_undoes_repeated_scope_changes_in_reverse_order() {
        let subscriptions = SubscriptionState::new(TOPIC_DELIMITER);
        let a = sub(DataType::Trade, "A");
        let b = sub(DataType::Trade, "B");
        let c = sub(DataType::Trade, "C");
        let d = sub(DataType::Trade, "D");

        for subscription in [a, b] {
            subscriptions.mark_subscribe(&topic(&subscription));
            subscriptions.confirm_subscribe(&topic(&subscription));
        }

        let changes = [
            ScopeChange::Unsubscribe(a),
            ScopeChange::Subscribe(c),
            ScopeChange::Unsubscribe(c),
            ScopeChange::Subscribe(a),
            ScopeChange::Unsubscribe(b),
            ScopeChange::Subscribe(d),
        ];

        for change in &changes {
            match change {
                ScopeChange::Subscribe(subscription) => {
                    subscriptions.mark_subscribe(&topic(subscription));
                }
                ScopeChange::Unsubscribe(subscription) => {
                    subscriptions.mark_unsubscribe(&topic(subscription));
                }
            }
        }

        revert(&subscriptions, &changes);

        assert_eq!(scope(&subscriptions), [a, b]);
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn failed_send_on_existing_connection_rolls_back_scope() {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

            let (client, _messages) = CoinApiWebSocketClient::new(
                "test-key".into(),
                CoinApiWebSocketConfig {
                    url: format!("ws://{}", listener.local_addr().unwrap()),
                    backend: TransportBackend::Tungstenite,
                    ..CoinApiWebSocketConfig::default()
                },
            )
            .unwrap();

            let peer = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
                let hello = socket.next().await.unwrap().unwrap();
                while let Some(message) = socket.next().await {
                    if message.unwrap().is_close() {
                        socket.flush().await.unwrap();
                        break;
                    }
                }

                hello
            });

            let original = sub(DataType::Trade, "COINBASE_SPOT_BTC_USD");
            let replacement = sub(DataType::Quote, "COINBASE_SPOT_BTC_USD");
            assert!(
                client
                    .apply(&[ScopeChange::Subscribe(original)])
                    .await
                    .unwrap()
                    .is_empty()
            );
            let transport = client
                .connection
                .lock()
                .await
                .as_ref()
                .unwrap()
                .client
                .clone();
            transport.disconnect().await;
            let hello = peer.await.unwrap();

            let error = client
                .apply(&[
                    ScopeChange::Unsubscribe(original),
                    ScopeChange::Subscribe(replacement),
                ])
                .await
                .unwrap_err();
            let remaining = scope(
                &client
                    .connection
                    .lock()
                    .await
                    .as_ref()
                    .unwrap()
                    .subscriptions,
            );
            client.close().await;

            assert_eq!(hello.into_text().unwrap(), r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#);
            assert!(matches!(error, Error::Send(SendError::Closed)));
            assert_eq!(remaining, [original]);
        })
        .await
        .unwrap();
    }
}
