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

//! A local relay that injects faults between an adapter and its venue.
//!
//! Each proxied socket opens its own venue connection and its own [`WireConnection`]. The relay
//! passes every venue message to [`WireConnection::upstream`] first, so the harness oracle sees
//! the venue feed before any fault, then applies a pending cut, corruption, drops, silence, and
//! holds in that order. An adapter subscribe that a reject rule matches never reaches the venue;
//! the relay answers it with a venue rejection instead. Messages the relay does not rewrite are
//! forwarded byte for byte.
//!
//! A route relays WebSocket messages, or CRLF-delimited lines over raw TCP for a venue such as
//! Betfair. A line route has no path to route by, so it serves the proxy address alone; the relay
//! presents each line to the wire handling as a text message, and [`WireCodec::connect`] opens
//! its venue connection.

use std::{
    collections::{HashMap, VecDeque},
    fmt::Write,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Router,
    extract::{
        WebSocketUpgrade,
        ws::{Message as ClientMessage, WebSocket},
    },
    http::HeaderMap,
    routing::get,
};
use futures_util::{SinkExt, StreamExt, future::BoxFuture};
use nautilus_common::live::dst::{
    self,
    time::{Duration, Instant},
};
use nautilus_network::net::{TcpListener, TcpStream};
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};

const HELD_MAX: usize = 20_000;

/// A local relay between an adapter and its venue that injects faults.
pub(crate) struct FaultProxy {
    addr: SocketAddr,
    server: dst::task::JoinHandle<()>,
    state: Arc<ProxyState>,
}

impl FaultProxy {
    // Serves `routes` on a local port, with `router` serving every other path
    pub(super) async fn start(
        routes: Vec<Route>,
        codec: Arc<dyn WireCodec>,
        router: Option<Router>,
    ) -> Self {
        let (release, _) = tokio::sync::watch::channel(());

        let state = Arc::new(ProxyState {
            routes: routes
                .into_iter()
                .map(|route| RouteState {
                    route,
                    connections: AtomicUsize::new(0),
                    frames: AtomicUsize::new(0),
                })
                .collect(),
            codec,
            faults: Mutex::new(HashMap::new()),
            cut: Mutex::new(None),
            freeze_until: Mutex::new(None),
            release,
            active: AtomicUsize::new(0),
            cuts: AtomicUsize::new(0),
            upstream_failures: AtomicUsize::new(0),
        });

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("proxy listener binds");
        let addr = listener
            .local_addr()
            .expect("proxy listener has an address");

        let server = if state.routes.iter().any(|route| route.route.is_line()) {
            assert!(
                state.routes.len() == 1 && router.is_none(),
                "a line route serves the proxy address alone"
            );
            let state = Arc::clone(&state);

            dst::task::spawn(async move {
                loop {
                    let (adapter, _) = listener.accept().await.expect("proxy accepts");
                    dst::task::spawn(relay_lines(adapter, Arc::clone(&state), 0));
                }
            })
        } else {
            let mut app = Router::new();

            for (index, route) in state.routes.iter().enumerate() {
                let state = Arc::clone(&state);

                let handler = move |ws: WebSocketUpgrade, headers: HeaderMap| {
                    let state = Arc::clone(&state);
                    async move { ws.on_upgrade(move |socket| relay(socket, state, index, headers)) }
                };

                app = app.route(route.route.path, get(handler));
            }

            if let Some(router) = router {
                app = app.merge(router);
            }

            dst::task::spawn(async move {
                axum::serve(listener, app).await.expect("proxy serves");
            })
        };

        Self {
            addr,
            server,
            state,
        }
    }

    pub(super) const fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub(super) fn routes(&self) -> impl Iterator<Item = &Route> {
        self.state.routes.iter().map(|route| &route.route)
    }

    /// Returns every book's fault rules and counters by fault key.
    pub(crate) fn faults(&self) -> MutexGuard<'_, HashMap<String, Fault>> {
        self.state.faults.lock()
    }

    /// Returns the fault rules and counters for fault key `key`, creating them if absent.
    pub(crate) fn fault(&self, key: &str) -> MappedMutexGuard<'_, Fault> {
        MutexGuard::map(self.state.faults.lock(), |faults| {
            faults.entry(key.to_string()).or_default()
        })
    }

    /// Cuts the connection instead of forwarding each of the next `count` book frames of `kind`.
    ///
    /// The rule applies to `route`, or to every route when `route` is `None`, and replaces any
    /// pending cut rule.
    pub(crate) fn cut(&self, route: Option<&'static str>, kind: FrameKind, count: usize) {
        *self.state.cut.lock() = Some(Cut {
            route,
            kind,
            remaining: count,
        });
    }

    /// Returns the cuts the pending cut rule has left.
    #[must_use]
    pub(crate) fn cuts_pending(&self) -> usize {
        self.state
            .cut
            .lock()
            .as_ref()
            .map_or(0, |cut| cut.remaining)
    }

    /// Pauses relaying in both directions on every connection for `duration`.
    pub(crate) fn freeze(&self, duration: Duration) {
        *self.state.freeze_until.lock() = Some(Instant::now() + duration);
    }

    /// Sends held frames whose book no longer holds, in arrival order.
    pub(crate) fn release(&self) {
        self.state.release.send_replace(());
    }

    /// Returns the venue connections opened on `route`.
    ///
    /// # Panics
    ///
    /// Panics if no route is named `route`.
    #[must_use]
    pub(crate) fn connections(&self, route: &str) -> usize {
        self.state
            .routes
            .iter()
            .find(|state| state.route.name == route)
            .unwrap_or_else(|| panic!("no proxied route named {route}"))
            .connections
            .load(Ordering::SeqCst)
    }

    /// Returns the venue connections opened on every route.
    #[must_use]
    pub(crate) fn connections_total(&self) -> usize {
        self.state
            .routes
            .iter()
            .map(|state| state.connections.load(Ordering::SeqCst))
            .sum()
    }

    pub(super) fn active(&self) -> usize {
        self.state.active.load(Ordering::SeqCst)
    }

    /// Returns the connections cut by a cut rule or `cut_unsubscribe`.
    #[must_use]
    pub(crate) fn cuts(&self) -> usize {
        self.state.cuts.load(Ordering::SeqCst)
    }

    // The relay closes the adapter socket after such a failure, as a venue outage would
    pub(super) fn upstream_failures(&self) -> usize {
        self.state.upstream_failures.load(Ordering::SeqCst)
    }

    pub(super) fn stats(&self) -> String {
        let faults = self.state.faults.lock();
        let total = |field: fn(&Fault) -> usize| faults.values().map(field).sum::<usize>();
        let mut stats = String::new();

        for state in &self.state.routes {
            let _ = write!(
                stats,
                "connections_{}={} ",
                state.route.name,
                state.connections.load(Ordering::SeqCst)
            );
        }

        let _ = write!(
            stats,
            "cuts={} dropped={} held={} corrupted={} rejected={} upstream_failures={}",
            self.cuts(),
            total(|fault| fault.dropped),
            total(|fault| fault.held),
            total(|fault| fault.corrupted),
            total(|fault| fault.rejected),
            self.upstream_failures(),
        );
        stats
    }

    // Venue messages received on each route, which tell a silent route from an adapter failure
    pub(super) fn frames(&self) -> Vec<(&'static str, usize)> {
        self.state
            .routes
            .iter()
            .map(|state| (state.route.name, state.frames.load(Ordering::SeqCst)))
            .collect()
    }

    pub(super) fn stop(&self) {
        self.server.abort();
    }
}

/// A proxied endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Route {
    /// Name used in counters and report fields.
    pub(crate) name: &'static str,
    /// Local path the adapter connects to; a line route has none.
    pub(crate) path: &'static str,
    /// Venue URL the relay connects to. A `ws://` or `wss://` URL makes a WebSocket route, and
    /// any other URL a line route.
    pub(crate) upstream: String,
    /// Reconnect registry endpoint of the adapter socket on this route.
    pub(crate) endpoint: &'static str,
    /// Handshake headers copied from the adapter's request to the venue.
    pub(crate) headers: &'static [&'static str],
}

impl Route {
    fn is_line(&self) -> bool {
        !(self.upstream.starts_with("ws://") || self.upstream.starts_with("wss://"))
    }
}

/// The kind of book frame a fault rule applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameKind {
    /// A frame that replaces the book.
    Snapshot,
    /// A frame that changes some levels.
    Update,
}

/// A venue message as the venue's wire handling classifies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Upstream {
    /// A book frame for the book with this fault key.
    Book { key: String, kind: FrameKind },
    /// The venue acknowledged an unsubscribe for this fault key.
    Unsubscribed(String),
    /// Any other message.
    Other,
}

/// Venue wire handling for one proxied socket.
pub(crate) trait WireConnection: Send {
    /// Classifies a venue message, recording it in the harness oracle before any fault applies.
    fn upstream(&mut self, message: &Message) -> Upstream;

    /// Returns the fault keys an adapter message unsubscribes.
    fn client(&mut self, _message: &Message) -> Vec<String> {
        Vec::new()
    }

    /// Rewrites a book frame so the adapter must detect a fault in it.
    ///
    /// Returns `false`, leaving `message` unchanged, when this frame cannot carry the fault; the
    /// rule then waits for a later frame.
    fn corrupt(&mut self, _message: &mut Message, _key: &str, _kind: FrameKind) -> bool {
        false
    }

    /// Returns the fault key an adapter book subscribe targets and a venue reply rejecting it.
    ///
    /// Returns `None` when `message` is not a book subscribe.
    fn reject(&mut self, _message: &Message) -> Option<(String, Message)> {
        None
    }
}

/// Opens venue wire handling for each proxied socket.
pub(crate) trait WireCodec: Send + Sync + 'static {
    /// Opens wire handling for connection `number` on `route`, counting from one.
    fn open(&self, route: &Route, number: usize) -> Box<dyn WireConnection>;

    /// Opens the venue connection for a line route.
    ///
    /// The default connects over plain TCP to the address after the upstream URL's scheme. A
    /// venue overrides it to wrap the connection, for example in TLS.
    fn connect(&self, route: &Route) -> BoxFuture<'static, std::io::Result<Box<dyn LineStream>>> {
        let addr = route
            .upstream
            .split_once("://")
            .map_or_else(|| route.upstream.clone(), |(_, addr)| addr.to_string());

        Box::pin(async move {
            let stream = TcpStream::connect(addr).await?;
            Ok(Box::new(stream) as Box<dyn LineStream>)
        })
    }
}

/// A venue connection that a line route relays.
pub(crate) trait LineStream: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> LineStream for T {}

/// Fault rules and counters for one book.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent rule a scenario sets"
)]
pub(crate) struct Fault {
    /// Book frames left to rewrite through [`WireConnection::corrupt`].
    pub(crate) corrupt: usize,
    /// Snapshot frames left to drop.
    pub(crate) drop_snapshots: usize,
    /// Incremental frames left to drop.
    pub(crate) drop_updates: usize,
    /// Holds every frame until cleared and [`FaultProxy::release`] runs.
    pub(crate) hold: bool,
    /// Sets `hold` at the next snapshot frame.
    pub(crate) hold_snapshot: bool,
    /// Drops every frame until the adapter unsubscribes the book or a frame arrives on a later
    /// connection of its route.
    pub(crate) silence: bool,
    /// Cuts the connection when the adapter unsubscribes the book.
    pub(crate) cut_unsubscribe: bool,
    /// Adapter subscribes left to answer with [`WireConnection::reject`] instead of the venue.
    pub(crate) reject: usize,
    /// Frames rewritten.
    pub(crate) corrupted: usize,
    /// Frames dropped by `drop_snapshots` or `drop_updates`.
    pub(crate) dropped: usize,
    /// Frames held.
    pub(crate) held: usize,
    /// Adapter subscribes rejected.
    pub(crate) rejected: usize,
    /// Frames forwarded as they arrived.
    pub(crate) forwarded: usize,
    /// When the first frame was forwarded.
    pub(crate) first_forwarded: Option<Instant>,
    /// Unsubscribes the venue acknowledged.
    pub(crate) unsubscribes: usize,
}

async fn relay(mut socket: WebSocket, state: Arc<ProxyState>, index: usize, headers: HeaderMap) {
    let route_state = &state.routes[index];
    let route = &route_state.route;
    let mut request = route
        .upstream
        .as_str()
        .into_client_request()
        .expect("upstream URL forms a request");

    for name in route.headers {
        if let Some(value) = headers
            .get(*name)
            .and_then(|value| HeaderValue::from_bytes(value.as_bytes()).ok())
        {
            request.headers_mut().insert(*name, value);
        }
    }

    let mut upstream = match tokio_tungstenite::connect_async(request).await {
        Ok((upstream, _)) => upstream,
        Err(e) => {
            state.upstream_failures.fetch_add(1, Ordering::SeqCst);
            eprintln!("Upstream connect failed on route {}: {e}", route.name);
            return;
        }
    };

    state.active.fetch_add(1, Ordering::SeqCst);
    let _active = ActiveRelay(Arc::clone(&state));
    let number = route_state.connections.fetch_add(1, Ordering::SeqCst) + 1;
    let mut wire = state.codec.open(route, number);
    let mut held = VecDeque::<(String, Message)>::new();
    let mut release = state.release.subscribe();

    'relay: loop {
        state.thaw().await;

        tokio::select! {
            biased;

            Ok(()) = release.changed() => {
                for message in take_released(&state, &mut held) {
                    if socket.send(to_client(message)).await.is_err() {
                        break 'relay;
                    }
                }
            }
            message = socket.recv() => {
                let message = match message {
                    Some(Ok(ClientMessage::Text(text))) => Message::Text(text.as_str().into()),
                    Some(Ok(ClientMessage::Binary(bytes))) => Message::Binary(bytes),
                    Some(Ok(ClientMessage::Ping(_) | ClientMessage::Pong(_))) => continue,
                    Some(Ok(ClientMessage::Close(_)) | Err(_)) | None => break,
                };

                if let Some(reply) = state.reject(wire.as_mut(), &message) {
                    if socket.send(to_client(reply)).await.is_err() {
                        break;
                    }
                    continue;
                }

                let keys = wire.client(&message);

                if upstream.send(message).await.is_err() || state.unsubscribe(&keys) {
                    break;
                }
            }
            message = upstream.next() => {
                let mut message = match message {
                    Some(Ok(message @ (Message::Text(_) | Message::Binary(_)))) => message,
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                    Some(Ok(_)) => continue,
                };
                route_state.frames.fetch_add(1, Ordering::SeqCst);

                match state.inspect(route.name, number, wire.as_mut(), &mut message, &held) {
                    Action::Forward => {
                        if socket.send(to_client(message)).await.is_err() {
                            break;
                        }
                    }
                    Action::Drop => {}
                    Action::Hold(key) => {
                        held.push_back((key, message));
                        assert!(held.len() < HELD_MAX, "held-frame queue stays bounded");
                    }
                    Action::Cut => break,
                }
            }
        }
    }

    let _ = dst::time::timeout(Duration::from_secs(1), upstream.close(None)).await;
}

async fn relay_lines(adapter: TcpStream, state: Arc<ProxyState>, index: usize) {
    let route_state = &state.routes[index];
    let route = &route_state.route;

    let upstream = match state.codec.connect(route).await {
        Ok(upstream) => upstream,
        Err(e) => {
            state.upstream_failures.fetch_add(1, Ordering::SeqCst);
            eprintln!("Upstream connect failed on route {}: {e}", route.name);
            return;
        }
    };

    state.active.fetch_add(1, Ordering::SeqCst);
    let _active = ActiveRelay(Arc::clone(&state));
    let number = route_state.connections.fetch_add(1, Ordering::SeqCst) + 1;
    let mut wire = state.codec.open(route, number);
    let mut held = VecDeque::<(String, Message)>::new();
    let mut release = state.release.subscribe();
    let (adapter_read, mut adapter_write) = adapter.into_split();
    let mut adapter_lines = BufReader::new(adapter_read).lines();
    let (upstream_read, mut upstream_write) = tokio::io::split(upstream);
    let mut upstream_lines = BufReader::new(upstream_read).lines();

    'relay: loop {
        state.thaw().await;

        tokio::select! {
            biased;

            Ok(()) = release.changed() => {
                for message in take_released(&state, &mut held) {
                    if write_line(&mut adapter_write, &message).await.is_err() {
                        break 'relay;
                    }
                }
            }
            line = adapter_lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                let message = Message::Text(line.into());

                if let Some(reply) = state.reject(wire.as_mut(), &message) {
                    if write_line(&mut adapter_write, &reply).await.is_err() {
                        break;
                    }
                    continue;
                }

                let keys = wire.client(&message);

                if write_line(&mut upstream_write, &message).await.is_err()
                    || state.unsubscribe(&keys)
                {
                    break;
                }
            }
            line = upstream_lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                route_state.frames.fetch_add(1, Ordering::SeqCst);
                let mut message = Message::Text(line.into());

                match state.inspect(route.name, number, wire.as_mut(), &mut message, &held) {
                    Action::Forward => {
                        if write_line(&mut adapter_write, &message).await.is_err() {
                            break;
                        }
                    }
                    Action::Drop => {}
                    Action::Hold(key) => {
                        held.push_back((key, message));
                        assert!(held.len() < HELD_MAX, "held-frame queue stays bounded");
                    }
                    Action::Cut => break,
                }
            }
        }
    }

    let _ = dst::time::timeout(Duration::from_secs(1), upstream_write.shutdown()).await;
}

async fn write_line(
    writer: &mut (impl AsyncWrite + Unpin),
    message: &Message,
) -> std::io::Result<()> {
    let text = message.to_text().map_err(std::io::Error::other)?;
    writer.write_all(format!("{text}\r\n").as_bytes()).await
}

// Removes held frames whose book no longer holds and returns them in arrival order
fn take_released(state: &ProxyState, held: &mut VecDeque<(String, Message)>) -> Vec<Message> {
    let (released, remaining): (Vec<_>, Vec<_>) =
        held.drain(..).partition(|(key, _)| !state.is_held(key));
    *held = remaining.into();
    released.into_iter().map(|(_, message)| message).collect()
}

fn to_client(message: Message) -> ClientMessage {
    match message {
        Message::Text(text) => ClientMessage::Text(text.as_str().into()),
        Message::Binary(bytes) => ClientMessage::Binary(bytes),
        other => unreachable!("relay forwards only text and binary messages, was {other:?}"),
    }
}

struct ProxyState {
    routes: Vec<RouteState>,
    codec: Arc<dyn WireCodec>,
    faults: Mutex<HashMap<String, Fault>>,
    cut: Mutex<Option<Cut>>,
    freeze_until: Mutex<Option<Instant>>,
    release: tokio::sync::watch::Sender<()>,
    active: AtomicUsize,
    cuts: AtomicUsize,
    upstream_failures: AtomicUsize,
}

impl ProxyState {
    // Applies fault rules to one venue message after the wire handling records it
    fn inspect(
        &self,
        route: &str,
        number: usize,
        wire: &mut dyn WireConnection,
        message: &mut Message,
        held: &VecDeque<(String, Message)>,
    ) -> Action {
        let (key, kind) = match wire.upstream(message) {
            Upstream::Book { key, kind } => (key, kind),
            Upstream::Unsubscribed(key) => {
                self.faults.lock().entry(key).or_default().unsubscribes += 1;
                return Action::Forward;
            }
            Upstream::Other => return Action::Forward,
        };

        if self.take_cut(route, kind) {
            return Action::Cut;
        }

        let mut faults = self.faults.lock();
        let fault = faults.entry(key.clone()).or_default();

        if fault.corrupt > 0 && wire.corrupt(message, &key, kind) {
            fault.corrupt -= 1;
            fault.corrupted += 1;
        }

        let drops = match kind {
            FrameKind::Snapshot => &mut fault.drop_snapshots,
            FrameKind::Update => &mut fault.drop_updates,
        };

        if *drops > 0 {
            *drops -= 1;
            fault.dropped += 1;
            return Action::Drop;
        }

        if number > 1 {
            fault.silence = false;
        }

        if fault.silence {
            return Action::Drop;
        }

        if kind == FrameKind::Snapshot && fault.hold_snapshot {
            fault.hold = true;
            fault.hold_snapshot = false;
        }

        // Frames queued behind a hold keep their order until released
        if fault.hold || held.iter().any(|(held_key, _)| *held_key == key) {
            fault.held += 1;
            return Action::Hold(key);
        }

        fault.forwarded += 1;
        fault.first_forwarded.get_or_insert_with(Instant::now);
        Action::Forward
    }

    fn take_cut(&self, route: &str, kind: FrameKind) -> bool {
        let mut cut = self.cut.lock();

        let Some(rule) = cut.as_mut() else {
            return false;
        };

        if rule.remaining == 0 || rule.kind != kind || rule.route.is_some_and(|name| name != route)
        {
            return false;
        }

        rule.remaining -= 1;
        self.cuts.fetch_add(1, Ordering::SeqCst);
        true
    }

    // Returns the venue rejection for an adapter subscribe that a reject rule answers
    fn reject(&self, wire: &mut dyn WireConnection, message: &Message) -> Option<Message> {
        let (key, reply) = wire.reject(message)?;
        let mut faults = self.faults.lock();
        let fault = faults.get_mut(&key).filter(|fault| fault.reject > 0)?;
        fault.reject -= 1;
        fault.rejected += 1;
        Some(reply)
    }

    // Returns whether an unsubscribed book asked for the connection to be cut
    fn unsubscribe(&self, keys: &[String]) -> bool {
        let mut faults = self.faults.lock();
        let mut cut = false;

        for key in keys {
            if let Some(fault) = faults.get_mut(key) {
                fault.silence = false;
                cut |= std::mem::take(&mut fault.cut_unsubscribe);
            }
        }

        if cut {
            self.cuts.fetch_add(1, Ordering::SeqCst);
        }

        cut
    }

    fn is_held(&self, key: &str) -> bool {
        self.faults.lock().get(key).is_some_and(|fault| fault.hold)
    }

    async fn thaw(&self) {
        let until = *self.freeze_until.lock();

        if let Some(until) = until {
            dst::time::sleep_until(until).await;
            let mut freeze_until = self.freeze_until.lock();

            if *freeze_until == Some(until) {
                *freeze_until = None;
            }
        }
    }
}

struct RouteState {
    route: Route,
    connections: AtomicUsize,
    frames: AtomicUsize,
}

struct Cut {
    route: Option<&'static str>,
    kind: FrameKind,
    remaining: usize,
}

enum Action {
    Forward,
    Drop,
    Hold(String),
    Cut,
}

struct ActiveRelay(Arc<ProxyState>);

impl Drop for ActiveRelay {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::{Value, json};
    use tokio::{
        io::Lines,
        net::tcp::{OwnedReadHalf, OwnedWriteHalf},
    };
    use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

    use super::*;

    type Client = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
    type LineReader = Lines<BufReader<OwnedReadHalf>>;

    // Frames are JSON objects: `{"key": "A", "kind": "snapshot"}`, `{"unsubscribed": "A"}`,
    // `{"rejected": "A"}`, and adapter commands `{"subscribe": "A"}` and `{"unsubscribe": "A"}`
    struct TestCodec;

    impl WireCodec for TestCodec {
        fn open(&self, _route: &Route, _number: usize) -> Box<dyn WireConnection> {
            Box::new(TestConnection)
        }
    }

    struct TestConnection;

    impl WireConnection for TestConnection {
        fn upstream(&mut self, message: &Message) -> Upstream {
            let frame = parse(message);

            if let Some(key) = frame["unsubscribed"].as_str() {
                return Upstream::Unsubscribed(key.to_string());
            }

            match (frame["key"].as_str(), frame["kind"].as_str()) {
                (Some(key), Some("snapshot")) => Upstream::Book {
                    key: key.to_string(),
                    kind: FrameKind::Snapshot,
                },
                (Some(key), Some("update")) => Upstream::Book {
                    key: key.to_string(),
                    kind: FrameKind::Update,
                },
                _ => Upstream::Other,
            }
        }

        fn client(&mut self, message: &Message) -> Vec<String> {
            parse(message)["unsubscribe"]
                .as_str()
                .map(|key| vec![key.to_string()])
                .unwrap_or_default()
        }

        fn corrupt(&mut self, message: &mut Message, _key: &str, kind: FrameKind) -> bool {
            if kind == FrameKind::Snapshot {
                return false;
            }

            let mut frame = parse(message);
            frame["corrupt"] = json!(true);
            *message = Message::Text(frame.to_string().into());
            true
        }

        fn reject(&mut self, message: &Message) -> Option<(String, Message)> {
            let key = parse(message)["subscribe"].as_str()?.to_string();
            let reply = json!({"rejected": key}).to_string();
            Some((key, Message::Text(reply.into())))
        }
    }

    fn parse(message: &Message) -> Value {
        serde_json::from_str(message.to_text().unwrap()).unwrap_or_default()
    }

    // A venue that sends every frame the test pushes and reports what the adapter sends
    struct TestVenue {
        url: String,
        frames: tokio::sync::broadcast::Sender<String>,
        commands: tokio::sync::mpsc::UnboundedReceiver<(String, Option<String>)>,
    }

    async fn start_venue() -> TestVenue {
        let (frames, _) = tokio::sync::broadcast::channel::<String>(64);
        let (command_tx, commands) = tokio::sync::mpsc::unbounded_channel();
        let venue_frames = frames.clone();

        let handler = move |ws: WebSocketUpgrade, headers: HeaderMap| {
            let mut frames = venue_frames.subscribe();
            let commands = command_tx.clone();
            let key = headers
                .get("x-test-key")
                .and_then(|value| value.to_str().ok())
                .map(ToString::to_string);

            async move {
                ws.on_upgrade(move |mut socket| async move {
                    loop {
                        tokio::select! {
                            frame = frames.recv() => {
                                let Ok(frame) = frame else { break };
                                if socket.send(ClientMessage::Text(frame.into())).await.is_err() {
                                    break;
                                }
                            }
                            message = socket.recv() => match message {
                                Some(Ok(ClientMessage::Text(text))) => {
                                    let _ = commands.send((text.to_string(), key.clone()));
                                }
                                Some(Ok(_)) => {}
                                Some(Err(_)) | None => break,
                            },
                        }
                    }
                })
            }
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route("/venue", get(handler));

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        TestVenue {
            url: format!("ws://{addr}/venue"),
            frames,
            commands,
        }
    }

    async fn start_proxy(upstream: &str) -> FaultProxy {
        let route = Route {
            name: "test",
            path: "/test",
            upstream: upstream.to_string(),
            endpoint: "test-endpoint",
            headers: &["x-test-key"],
        };

        FaultProxy::start(vec![route], Arc::new(TestCodec), None).await
    }

    // Returns once the relay has connected upstream, so the venue receives every later frame
    async fn connect(proxy: &FaultProxy) -> Client {
        let connections = proxy.connections("test");
        let (client, _) = connect_async(format!("ws://{}/test", proxy.addr()))
            .await
            .unwrap();
        wait_for(|| proxy.connections("test") > connections).await;
        client
    }

    fn frame(key: &str, kind: &str, n: u32) -> String {
        json!({"key": key, "kind": kind, "n": n}).to_string()
    }

    fn send(venue: &TestVenue, frames: &[String]) {
        for frame in frames {
            venue.frames.send(frame.clone()).unwrap();
        }
    }

    async fn receive(client: &mut Client) -> Option<String> {
        match tokio::time::timeout(std::time::Duration::from_millis(500), client.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => Some(text.to_string()),
            _ => None,
        }
    }

    async fn receive_all(client: &mut Client) -> Vec<String> {
        let mut received = Vec::new();

        while let Some(text) = receive(client).await {
            received.push(text);
        }

        received
    }

    async fn wait_for(condition: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

        while !condition() {
            assert!(
                std::time::Instant::now() < deadline,
                "condition holds in time"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn forwards_frames_unchanged_and_counts_them() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        let frames = [
            frame("A", "snapshot", 1),
            frame("A", "update", 2),
            "{\"other\": 1}".to_string(),
        ];

        send(&venue, &frames);
        let received = receive_all(&mut client).await;

        assert_eq!(received, frames);
        let fault = proxy.fault("A").clone();
        assert_eq!(fault.forwarded, 2);
        assert!(fault.first_forwarded.is_some());
        assert_eq!(proxy.connections("test"), 1);
        assert_eq!(proxy.active(), 1);
        assert_eq!(proxy.frames(), vec![("test", 3)]);
    }

    #[tokio::test]
    async fn drop_rules_drop_only_their_kind() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        {
            let mut fault = proxy.fault("A");
            fault.drop_snapshots = 1;
            fault.drop_updates = 1;
        }

        send(
            &venue,
            &[
                frame("A", "snapshot", 1),
                frame("A", "update", 2),
                frame("B", "update", 3),
                frame("A", "snapshot", 4),
                frame("A", "update", 5),
            ],
        );
        let received = receive_all(&mut client).await;

        assert_eq!(
            received,
            [
                frame("B", "update", 3),
                frame("A", "snapshot", 4),
                frame("A", "update", 5),
            ]
        );
        let fault = proxy.fault("A").clone();
        assert_eq!(fault.dropped, 2);
        assert_eq!(fault.drop_snapshots, 0);
        assert_eq!(fault.drop_updates, 0);
        assert_eq!(fault.forwarded, 2);
    }

    #[tokio::test]
    async fn hold_queues_book_frames_until_released() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        proxy.fault("A").hold = true;

        send(
            &venue,
            &[
                frame("A", "update", 1),
                frame("B", "update", 2),
                frame("A", "update", 3),
            ],
        );
        let before = receive_all(&mut client).await;
        proxy.fault("A").hold = false;
        proxy.release();
        let after = receive_all(&mut client).await;

        assert_eq!(before, [frame("B", "update", 2)]);
        assert_eq!(after, [frame("A", "update", 1), frame("A", "update", 3)]);
        assert_eq!(proxy.fault("A").held, 2);
    }

    #[tokio::test]
    async fn release_keeps_frames_of_books_still_held() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        proxy.fault("A").hold = true;
        proxy.fault("B").hold = true;

        send(&venue, &[frame("A", "update", 1), frame("B", "update", 2)]);
        wait_for(|| proxy.fault("B").held == 1).await;
        proxy.fault("A").hold = false;
        proxy.release();
        let released = receive_all(&mut client).await;
        send(&venue, &[frame("B", "update", 3)]);
        let still_held = receive_all(&mut client).await;

        assert_eq!(released, [frame("A", "update", 1)]);
        assert!(still_held.is_empty());
        assert_eq!(proxy.fault("B").held, 2);
    }

    #[tokio::test]
    async fn frames_after_a_cleared_hold_queue_behind_held_frames() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        proxy.fault("A").hold = true;

        send(&venue, &[frame("A", "update", 1)]);
        wait_for(|| proxy.fault("A").held == 1).await;
        proxy.fault("A").hold = false;
        send(&venue, &[frame("A", "update", 2)]);
        let before_release = receive_all(&mut client).await;
        proxy.release();
        let after_release = receive_all(&mut client).await;

        assert!(before_release.is_empty());
        assert_eq!(
            after_release,
            [frame("A", "update", 1), frame("A", "update", 2)]
        );
        assert_eq!(proxy.fault("A").held, 2);
    }

    #[tokio::test]
    async fn hold_snapshot_holds_from_the_next_snapshot() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        proxy.fault("A").hold_snapshot = true;

        send(
            &venue,
            &[
                frame("A", "update", 1),
                frame("A", "snapshot", 2),
                frame("A", "update", 3),
            ],
        );
        let received = receive_all(&mut client).await;

        assert_eq!(received, [frame("A", "update", 1)]);
        let fault = proxy.fault("A").clone();
        assert!(fault.hold);
        assert!(!fault.hold_snapshot);
        assert_eq!(fault.held, 2);
    }

    #[tokio::test]
    async fn silence_drops_until_the_adapter_unsubscribes() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        proxy.fault("A").silence = true;

        send(&venue, &[frame("A", "snapshot", 1)]);
        let silenced = receive_all(&mut client).await;
        client
            .send(Message::Text(
                json!({"unsubscribe": "A"}).to_string().into(),
            ))
            .await
            .unwrap();
        wait_for(|| !proxy.fault("A").silence).await;
        send(&venue, &[frame("A", "snapshot", 2)]);
        let received = receive_all(&mut client).await;

        assert!(silenced.is_empty());
        assert_eq!(received, [frame("A", "snapshot", 2)]);
        assert_eq!(proxy.fault("A").dropped, 0);
    }

    #[tokio::test]
    async fn silence_ends_on_a_later_connection() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut first = connect(&proxy).await;
        proxy.fault("A").silence = true;
        first.close(None).await.unwrap();
        wait_for(|| proxy.active() == 0).await;

        let mut second = connect(&proxy).await;
        send(&venue, &[frame("A", "snapshot", 1)]);
        let received = receive_all(&mut second).await;

        assert_eq!(received, [frame("A", "snapshot", 1)]);
        assert_eq!(proxy.connections("test"), 2);
        assert!(!proxy.fault("A").silence);
    }

    #[tokio::test]
    async fn unsubscribe_forwards_the_command_and_cuts_when_asked() {
        let mut venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        proxy.fault("A").cut_unsubscribe = true;
        let command = json!({"unsubscribe": "A"}).to_string();

        client
            .send(Message::Text(command.clone().into()))
            .await
            .unwrap();
        let (forwarded, _) = venue.commands.recv().await.unwrap();
        wait_for(|| proxy.active() == 0).await;

        assert_eq!(forwarded, command);
        assert_eq!(proxy.cuts(), 1);
        assert!(!proxy.fault("A").cut_unsubscribe);
        assert!(receive(&mut client).await.is_none());
    }

    #[tokio::test]
    async fn reject_answers_subscribes_instead_of_the_venue() {
        let mut venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        proxy.fault("A").reject = 1;
        let command = |key: &str| json!({"subscribe": key}).to_string();

        for key in ["A", "B", "A"] {
            client
                .send(Message::Text(command(key).into()))
                .await
                .unwrap();
        }

        let replies = receive_all(&mut client).await;
        let (first, _) = venue.commands.recv().await.unwrap();
        let (second, _) = venue.commands.recv().await.unwrap();

        assert_eq!(replies, [json!({"rejected": "A"}).to_string()]);
        assert_eq!([first, second], [command("B"), command("A")]);
        assert!(venue.commands.try_recv().is_err());
        let fault = proxy.fault("A").clone();
        assert_eq!(fault.reject, 0);
        assert_eq!(fault.rejected, 1);
        assert_eq!(proxy.fault("B").rejected, 0);
    }

    #[rstest]
    #[case::matching_route(Some("test"), FrameKind::Snapshot, 1)]
    #[case::any_route(None, FrameKind::Snapshot, 1)]
    #[case::other_route(Some("other"), FrameKind::Snapshot, 0)]
    #[case::other_kind(Some("test"), FrameKind::Update, 0)]
    #[tokio::test]
    async fn cut_rule_cuts_matching_frames(
        #[case] route: Option<&'static str>,
        #[case] kind: FrameKind,
        #[case] expected_cuts: usize,
    ) {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        proxy.cut(route, kind, 1);

        send(&venue, &[frame("A", "snapshot", 1)]);
        let received = receive(&mut client).await;

        assert_eq!(proxy.cuts(), expected_cuts);
        assert_eq!(proxy.cuts_pending(), 1 - expected_cuts);

        if expected_cuts == 1 {
            wait_for(|| proxy.active() == 0).await;
            assert_eq!(received, None);
        } else {
            assert_eq!(received, Some(frame("A", "snapshot", 1)));
        }
    }

    #[tokio::test]
    async fn corrupt_waits_for_a_frame_the_wire_can_corrupt() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        proxy.fault("A").corrupt = 1;

        send(
            &venue,
            &[
                frame("A", "snapshot", 1),
                frame("A", "update", 2),
                frame("A", "update", 3),
            ],
        );
        let received = receive_all(&mut client).await;

        let corrupted = json!({"key": "A", "kind": "update", "n": 2, "corrupt": true});
        assert_eq!(
            received,
            [
                frame("A", "snapshot", 1),
                corrupted.to_string(),
                frame("A", "update", 3),
            ]
        );
        let fault = proxy.fault("A").clone();
        assert_eq!(fault.corrupt, 0);
        assert_eq!(fault.corrupted, 1);
    }

    #[tokio::test]
    async fn venue_unsubscribe_acknowledgements_are_counted() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        let ack = json!({"unsubscribed": "A"}).to_string();

        send(&venue, std::slice::from_ref(&ack));
        let received = receive_all(&mut client).await;

        assert_eq!(received, [ack]);
        assert_eq!(proxy.fault("A").unsubscribes, 1);
    }

    #[tokio::test]
    async fn freeze_delays_frames_until_it_ends() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        let freeze = std::time::Duration::from_millis(300);
        let started = std::time::Instant::now();

        proxy.freeze(freeze);
        // Wakes the relay so it observes the freeze before the frame arrives
        proxy.release();
        send(&venue, &[frame("A", "update", 1)]);
        let received = tokio::time::timeout(std::time::Duration::from_secs(5), client.next())
            .await
            .unwrap();

        assert!(started.elapsed() >= freeze);
        assert!(matches!(received, Some(Ok(Message::Text(_)))));
    }

    #[tokio::test]
    async fn handshake_headers_reach_the_venue() {
        let mut venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;

        let request = format!("ws://{}/test", proxy.addr())
            .into_client_request()
            .map(|mut request| {
                request
                    .headers_mut()
                    .insert("x-test-key", HeaderValue::from_static("secret"));
                request
            })
            .unwrap();

        let (mut client, _) = connect_async(request).await.unwrap();

        client
            .send(Message::Text("{}".to_string().into()))
            .await
            .unwrap();
        let (_, key) = venue.commands.recv().await.unwrap();

        assert_eq!(key.as_deref(), Some("secret"));
    }

    #[tokio::test]
    async fn upstream_failure_closes_the_adapter_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed = format!("ws://{}/venue", listener.local_addr().unwrap());
        drop(listener);
        let proxy = start_proxy(&closed).await;

        let (mut client, _) = connect_async(format!("ws://{}/test", proxy.addr()))
            .await
            .unwrap();
        wait_for(|| proxy.upstream_failures() == 1).await;
        let next = tokio::time::timeout(std::time::Duration::from_secs(5), client.next())
            .await
            .unwrap();

        assert!(!matches!(next, Some(Ok(Message::Text(_)))));
        assert_eq!(proxy.connections("test"), 0);
        assert_eq!(proxy.active(), 0);
    }

    #[tokio::test]
    async fn stats_report_connections_and_fault_totals() {
        let venue = start_venue().await;
        let proxy = start_proxy(&venue.url).await;
        let mut client = connect(&proxy).await;
        {
            let mut fault = proxy.fault("A");
            fault.drop_updates = 1;
            fault.corrupt = 1;
            fault.reject = 1;
        }

        send(&venue, &[frame("A", "update", 1), frame("A", "update", 2)]);
        client
            .send(Message::Text(json!({"subscribe": "A"}).to_string().into()))
            .await
            .unwrap();
        let _ = receive_all(&mut client).await;

        assert_eq!(
            proxy.stats(),
            "connections_test=1 cuts=0 dropped=1 held=0 corrupted=1 rejected=1 upstream_failures=0"
        );
    }

    // A line venue that sends every line the test pushes and reports what the adapter sends
    struct LineVenue {
        url: String,
        lines: tokio::sync::broadcast::Sender<String>,
        commands: tokio::sync::mpsc::UnboundedReceiver<String>,
    }

    async fn start_line_venue() -> LineVenue {
        let (lines, _) = tokio::sync::broadcast::channel::<String>(64);
        let (command_tx, commands) = tokio::sync::mpsc::unbounded_channel();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let venue_lines = lines.clone();

        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let mut lines = venue_lines.subscribe();
                let commands = command_tx.clone();

                tokio::spawn(async move {
                    let (read, mut write) = stream.into_split();
                    let mut reader = BufReader::new(read).lines();

                    loop {
                        tokio::select! {
                            line = lines.recv() => {
                                let Ok(line) = line else { break };
                                let framed = format!("{line}\r\n");
                                if write.write_all(framed.as_bytes()).await.is_err() {
                                    break;
                                }
                            }
                            command = reader.next_line() => match command {
                                Ok(Some(command)) => {
                                    let _ = commands.send(command);
                                }
                                _ => break,
                            },
                        }
                    }
                });
            }
        });

        LineVenue {
            url: format!("tcp://{addr}"),
            lines,
            commands,
        }
    }

    async fn start_line_proxy(upstream: &str) -> FaultProxy {
        let route = Route {
            name: "test",
            path: "",
            upstream: upstream.to_string(),
            endpoint: "test-endpoint",
            headers: &[],
        };

        FaultProxy::start(vec![route], Arc::new(TestCodec), None).await
    }

    // Returns once the relay has connected upstream, so the venue receives every later line
    async fn connect_lines(proxy: &FaultProxy) -> (LineReader, OwnedWriteHalf) {
        let connections = proxy.connections("test");
        let stream = tokio::net::TcpStream::connect(proxy.addr()).await.unwrap();
        wait_for(|| proxy.connections("test") > connections).await;
        let (read, write) = stream.into_split();
        (BufReader::new(read).lines(), write)
    }

    async fn send_line(write: &mut OwnedWriteHalf, line: &str) {
        write
            .write_all(format!("{line}\r\n").as_bytes())
            .await
            .unwrap();
    }

    async fn receive_lines(reader: &mut LineReader) -> Vec<String> {
        let mut received = Vec::new();

        while let Ok(Ok(Some(line))) =
            tokio::time::timeout(std::time::Duration::from_millis(500), reader.next_line()).await
        {
            received.push(line);
        }

        received
    }

    #[tokio::test]
    async fn line_route_relays_lines_under_fault_rules() {
        let mut venue = start_line_venue().await;
        let proxy = start_line_proxy(&venue.url).await;
        let (mut reader, mut write) = connect_lines(&proxy).await;
        {
            let mut fault = proxy.fault("A");
            fault.drop_updates = 1;
            fault.reject = 1;
        }

        let subscribe = |key: &str| json!({"subscribe": key}).to_string();

        for frame in [
            frame("A", "snapshot", 1),
            frame("A", "update", 2),
            frame("B", "update", 3),
        ] {
            venue.lines.send(frame).unwrap();
        }

        let relayed = receive_lines(&mut reader).await;
        send_line(&mut write, &subscribe("A")).await;
        send_line(&mut write, &subscribe("B")).await;
        let replies = receive_lines(&mut reader).await;
        let command = venue.commands.recv().await.unwrap();

        assert_eq!(
            relayed,
            [frame("A", "snapshot", 1), frame("B", "update", 3)]
        );
        assert_eq!(replies, [json!({"rejected": "A"}).to_string()]);
        assert_eq!(command, subscribe("B"));
        assert!(venue.commands.try_recv().is_err());
        let fault = proxy.fault("A").clone();
        assert_eq!(fault.dropped, 1);
        assert_eq!(fault.rejected, 1);
        assert_eq!(fault.forwarded, 1);
        assert_eq!(proxy.frames(), vec![("test", 3)]);
        assert_eq!(proxy.active(), 1);
    }

    #[tokio::test]
    async fn line_route_cut_closes_the_adapter_connection() {
        let venue = start_line_venue().await;
        let proxy = start_line_proxy(&venue.url).await;
        let (mut reader, _write) = connect_lines(&proxy).await;
        proxy.cut(Some("test"), FrameKind::Snapshot, 1);

        venue.lines.send(frame("A", "snapshot", 1)).unwrap();
        let next = tokio::time::timeout(std::time::Duration::from_secs(5), reader.next_line())
            .await
            .unwrap();
        wait_for(|| proxy.active() == 0).await;

        assert!(matches!(next, Ok(None)));
        assert_eq!(proxy.cuts(), 1);
    }

    #[tokio::test]
    async fn line_route_upstream_failure_closes_the_adapter_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed = format!("tcp://{}", listener.local_addr().unwrap());
        drop(listener);
        let proxy = start_line_proxy(&closed).await;

        let stream = tokio::net::TcpStream::connect(proxy.addr()).await.unwrap();
        wait_for(|| proxy.upstream_failures() == 1).await;
        let mut reader = BufReader::new(stream).lines();
        let next = tokio::time::timeout(std::time::Duration::from_secs(5), reader.next_line())
            .await
            .unwrap();

        assert!(matches!(next, Ok(None)));
        assert_eq!(proxy.connections("test"), 0);
        assert_eq!(proxy.active(), 0);
    }
}
