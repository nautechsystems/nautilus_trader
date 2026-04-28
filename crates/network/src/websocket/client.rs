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

//! WebSocket client implementation with automatic reconnection.
//!
//! This module contains the core WebSocket client implementation including:
//! - Connection management with automatic reconnection.
//! - Split read/write architecture with separate tasks.
//! - Unbounded channels on latency-sensitive paths.
//! - Event-driven state notification via `Notify` for immediate wakeup on transitions.
//! - Heartbeat support.
//! - Rate limiting integration.

use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use http::HeaderName;
use nautilus_core::CleanDrop;
use nautilus_cryptography::providers::install_cryptographic_provider;
#[cfg(any(feature = "turmoil", feature = "transport-sockudo"))]
use rustls::ClientConfig;
#[cfg(feature = "transport-sockudo")]
use sockudo_ws::{
    Config as SockudoConfig, Http1, Role, Stream as SockudoStream,
    WebSocketStream as SockudoWebSocketStream,
};
#[cfg(feature = "transport-sockudo")]
use tokio::io::{AsyncRead, AsyncWrite};
#[cfg(any(feature = "turmoil", feature = "transport-sockudo"))]
use tokio_rustls::TlsConnector;
#[cfg(feature = "turmoil")]
use tokio_tungstenite::MaybeTlsStream;
#[cfg(feature = "turmoil")]
use tokio_tungstenite::client_async;
#[cfg(not(feature = "turmoil"))]
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};
use ustr::Ustr;

#[cfg(not(feature = "turmoil"))]
use super::proxy::{ProxiedStream, ProxyKind, WsTarget, tunnel_via_proxy};
use super::{
    auth::{AuthState, AuthTracker},
    config::{TransportBackend, WebSocketConfig},
    consts::{
        CONNECTION_STATE_CHECK_INTERVAL_MS, GRACEFUL_SHUTDOWN_DELAY_MS,
        GRACEFUL_SHUTDOWN_TIMEOUT_SECS,
    },
    types::{MessageHandler, MessageReader, MessageWriter, PingHandler, WriterCommand},
};
#[cfg(feature = "turmoil")]
use crate::net::TcpConnector;
#[cfg(feature = "transport-sockudo")]
use crate::net::TcpStream;
#[cfg(feature = "transport-sockudo")]
use crate::transport::sockudo::{
    PrefixedIo, SockudoTransport, client_handshake_with_headers, validate_extra_headers,
};
use crate::{
    RECONNECTED,
    backoff::ExponentialBackoff,
    dst,
    error::SendError,
    logging::{log_task_aborted, log_task_started, log_task_stopped},
    mode::ConnectionMode,
    ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota},
    transport::{BoxedWsTransport, Message, TransportError, tungstenite::TungsteniteTransport},
};

/// `WebSocketClient` connects to a websocket server to read and send messages.
///
/// The client is opinionated about how messages are read and written. It
/// assumes that data can only have one reader but multiple writers.
///
/// The client splits the connection into read and write halves. It moves
/// the read half into a tokio task which keeps receiving messages from the
/// server and calls a handler - a Python function that takes the data
/// as its parameter. It stores the write half in the struct wrapped
/// with an Arc Mutex. This way the client struct can be used to write
/// data to the server from multiple scopes/tasks.
///
/// The client also maintains a heartbeat if given a duration in seconds.
/// It's preferable to set the duration slightly lower - heartbeat more
/// frequently - than the required amount.
pub struct WebSocketClientInner {
    config: WebSocketConfig,
    /// The function to handle incoming messages (stored separately from config).
    message_handler: Option<MessageHandler>,
    /// The handler for incoming pings (stored separately from config).
    ping_handler: Option<PingHandler>,
    read_task: Option<tokio::task::JoinHandle<()>>,
    write_task: tokio::task::JoinHandle<()>,
    writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    heartbeat_task: Option<tokio::task::JoinHandle<()>>,
    connection_mode: Arc<AtomicU8>,
    state_notify: Arc<tokio::sync::Notify>,
    reconnect_timeout: Duration,
    backoff: ExponentialBackoff,
    /// True if this is a stream-based client (created via `connect_stream`).
    /// Stream-based clients disable auto-reconnect because the reader is
    /// owned by the caller and cannot be replaced during reconnection.
    is_stream_mode: bool,
    /// Maximum number of reconnection attempts before giving up (None = unlimited).
    reconnect_max_attempts: Option<u32>,
    /// Current count of consecutive reconnection attempts.
    reconnection_attempt_count: u32,
    /// Shared auth tracker invalidated on connection drops.
    auth_tracker: Arc<OnceLock<AuthTracker>>,
    /// Controls whether buffered replay waits for the next authenticated session.
    reconnect_buffer_waits_for_auth: Arc<AtomicBool>,
}

enum ReconnectBufferAction {
    Drain,
    Wait,
    Discard,
}

impl WebSocketClientInner {
    /// Create an inner websocket client with an existing writer.
    ///
    /// This is used for stream mode where the reader is owned by the caller.
    ///
    /// # Errors
    ///
    /// Returns an error if the exponential backoff configuration is invalid.
    #[expect(
        clippy::unused_async,
        reason = "async signature for consistency with connect-based constructors"
    )]
    pub async fn new_with_writer(
        config: WebSocketConfig,
        writer: MessageWriter,
    ) -> Result<Self, TransportError> {
        install_cryptographic_provider();

        let connection_mode = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());

        // Note: We don't spawn a read task here since the reader is handled externally
        let read_task = None;

        // Stream mode ignores reconnect settings, use harmless defaults
        let backoff = ExponentialBackoff::new(
            Duration::from_secs(2),
            Duration::from_secs(30),
            1.5,
            100,
            true,
        )
        .map_err(|e| {
            TransportError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
        })?;

        let auth_tracker = Arc::new(OnceLock::new());
        let reconnect_buffer_waits_for_auth = Arc::new(AtomicBool::new(false));

        let (writer_tx, writer_rx) = tokio::sync::mpsc::unbounded_channel::<WriterCommand>();
        let write_task = Self::spawn_write_task(
            connection_mode.clone(),
            state_notify.clone(),
            writer,
            writer_rx,
            Arc::clone(&auth_tracker),
            Arc::clone(&reconnect_buffer_waits_for_auth),
        );

        let heartbeat_task = if let Some(heartbeat_interval) = config.heartbeat {
            Some(Self::spawn_heartbeat_task(
                connection_mode.clone(),
                heartbeat_interval,
                config.heartbeat_msg.clone(),
                writer_tx.clone(),
            ))
        } else {
            None
        };

        let reconnect_max_attempts = None; // Stream mode does not reconnect
        let reconnect_timeout = Duration::from_secs(10);

        Ok(Self {
            config,
            message_handler: None, // Stream mode has no handler
            ping_handler: None,
            writer_tx,
            connection_mode,
            state_notify,
            reconnect_timeout,
            heartbeat_task,
            read_task,
            write_task,
            backoff,
            is_stream_mode: true,
            reconnect_max_attempts,
            reconnection_attempt_count: 0,
            auth_tracker,
            reconnect_buffer_waits_for_auth,
        })
    }

    /// Create an inner websocket client.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The connection to the server fails.
    /// - The exponential backoff configuration is invalid.
    pub async fn connect_url(
        config: WebSocketConfig,
        message_handler: Option<MessageHandler>,
        ping_handler: Option<PingHandler>,
    ) -> Result<Self, TransportError> {
        install_cryptographic_provider();

        if config.heartbeat == Some(0) {
            return Err(TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Heartbeat interval cannot be zero",
            )));
        }

        if config.idle_timeout_ms == Some(0) {
            return Err(TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Idle timeout cannot be zero",
            )));
        }

        // Capture whether we're in stream mode before moving config
        let is_stream_mode = message_handler.is_none();
        let reconnect_max_attempts = config.reconnect_max_attempts;

        let (writer, reader) = Box::pin(Self::connect_with_server(
            &config.url,
            config.headers.clone(),
            config.backend,
            config.proxy_url.as_deref(),
        ))
        .await?;

        let connection_mode = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());

        let read_task = if message_handler.is_some() {
            Some(Self::spawn_message_handler_task(
                connection_mode.clone(),
                state_notify.clone(),
                reader,
                message_handler.as_ref(),
                ping_handler.as_ref(),
                config.idle_timeout_ms,
            ))
        } else {
            None
        };

        let auth_tracker = Arc::new(OnceLock::new());
        let reconnect_buffer_waits_for_auth = Arc::new(AtomicBool::new(false));

        let (writer_tx, writer_rx) = tokio::sync::mpsc::unbounded_channel::<WriterCommand>();
        let write_task = Self::spawn_write_task(
            connection_mode.clone(),
            state_notify.clone(),
            writer,
            writer_rx,
            Arc::clone(&auth_tracker),
            Arc::clone(&reconnect_buffer_waits_for_auth),
        );

        // Optionally spawn a heartbeat task to periodically ping server
        let heartbeat_task = config.heartbeat.map(|heartbeat_secs| {
            Self::spawn_heartbeat_task(
                connection_mode.clone(),
                heartbeat_secs,
                config.heartbeat_msg.clone(),
                writer_tx.clone(),
            )
        });

        let reconnect_timeout =
            Duration::from_millis(config.reconnect_timeout_ms.unwrap_or(10_000));
        let backoff = ExponentialBackoff::new(
            Duration::from_millis(config.reconnect_delay_initial_ms.unwrap_or(2_000)),
            Duration::from_millis(config.reconnect_delay_max_ms.unwrap_or(30_000)),
            config.reconnect_backoff_factor.unwrap_or(1.5),
            config.reconnect_jitter_ms.unwrap_or(100),
            true, // immediate-first
        )
        .map_err(|e| {
            TransportError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
        })?;

        Ok(Self {
            config,
            message_handler,
            ping_handler,
            read_task,
            write_task,
            writer_tx,
            heartbeat_task,
            connection_mode,
            state_notify,
            reconnect_timeout,
            backoff,
            // Set stream mode when no message handler (reader not managed by client)
            is_stream_mode,
            reconnect_max_attempts,
            reconnection_attempt_count: 0,
            auth_tracker,
            reconnect_buffer_waits_for_auth,
        })
    }

    /// Connect to the server and return the split halves of the active transport.
    ///
    /// Dispatches on `backend` to the matching backend helper. The
    /// [`TransportBackend::Tungstenite`] backend is always available; the
    /// [`TransportBackend::Sockudo`] requires the `transport-sockudo` Cargo
    /// feature and uses a custom HTTP/1.1 handshake path for upgrade headers.
    ///
    /// When `proxy_url` is `Some`, the Tungstenite backend establishes an HTTP
    /// `CONNECT` tunnel through the proxy before performing the WebSocket
    /// handshake. The Sockudo backend does not yet support proxying and will
    /// return an error if a proxy URL is supplied.
    ///
    /// # Errors
    ///
    /// Returns a [`TransportError`] if the URL is invalid, headers fail to
    /// parse, the TCP / TLS layer cannot be established, the proxy refuses
    /// the tunnel, or the WebSocket handshake is rejected by the peer. When
    /// the Sockudo backend is selected without the `transport-sockudo`
    /// feature, returns [`TransportError::Other`].
    #[inline]
    pub async fn connect_with_server(
        url: &str,
        headers: Vec<(String, String)>,
        backend: TransportBackend,
        proxy_url: Option<&str>,
    ) -> Result<(MessageWriter, MessageReader), TransportError> {
        match backend {
            TransportBackend::Tungstenite => match proxy_url {
                Some(proxy) => {
                    Box::pin(Self::connect_tungstenite_via_proxy(url, headers, proxy)).await
                }
                None => Self::connect_tungstenite(url, headers).await,
            },
            TransportBackend::Sockudo => {
                if proxy_url.is_some() {
                    return Err(TransportError::Other(
                        "proxy_url is not supported with the Sockudo backend".to_string(),
                    ));
                }
                #[cfg(feature = "transport-sockudo")]
                {
                    Self::connect_sockudo(url, headers).await
                }
                #[cfg(not(feature = "transport-sockudo"))]
                {
                    Err(TransportError::Other(
                        "sockudo backend selected but the transport-sockudo \
                         Cargo feature is not enabled"
                            .to_string(),
                    ))
                }
            }
        }
    }

    /// Connects with the server creating a tokio-tungstenite websocket stream.
    /// Production version that uses `connect_async_with_config` convenience helper.
    #[inline]
    #[cfg(not(feature = "turmoil"))]
    async fn connect_tungstenite(
        url: &str,
        headers: Vec<(String, String)>,
    ) -> Result<(MessageWriter, MessageReader), TransportError> {
        let mut request = url.into_client_request().map_err(TransportError::from)?;
        let req_headers = request.headers_mut();

        for (key, val) in headers {
            let header_value = HeaderValue::from_str(&val)
                .map_err(|e| TransportError::Handshake(format!("invalid header value: {e}")))?;
            let header_name: HeaderName = key
                .parse()
                .map_err(|e| TransportError::Handshake(format!("invalid header name: {e}")))?;
            req_headers.insert(header_name, header_value);
        }

        let (stream, _resp) = connect_async_with_config(request, None, true)
            .await
            .map_err(TransportError::from)?;
        let transport: BoxedWsTransport = Box::pin(TungsteniteTransport::new(stream));
        Ok(transport.split())
    }

    /// Connects via an HTTP `CONNECT` proxy and performs the WebSocket
    /// handshake over the resulting tunnel.
    ///
    /// Recognised but unsupported proxy schemes (currently SOCKS) log a
    /// warning and fall back to a direct connection so existing REST proxy
    /// configs remain usable. Only available in production builds; the
    /// turmoil simulator does not model arbitrary outbound TCP via a proxy.
    #[inline]
    #[cfg(not(feature = "turmoil"))]
    async fn connect_tungstenite_via_proxy(
        url: &str,
        headers: Vec<(String, String)>,
        proxy_url: &str,
    ) -> Result<(MessageWriter, MessageReader), TransportError> {
        let proxy = match ProxyKind::parse(proxy_url)? {
            ProxyKind::Http(target) => target,
            ProxyKind::Unsupported { scheme } => {
                log::warn!(
                    "WebSocket proxy_url scheme '{scheme}' is not yet supported; \
                     connecting without a WebSocket proxy"
                );
                return Self::connect_tungstenite(url, headers).await;
            }
        };

        let mut request = url.into_client_request().map_err(TransportError::from)?;
        let req_headers = request.headers_mut();

        for (key, val) in headers {
            let header_value = HeaderValue::from_str(&val)
                .map_err(|e| TransportError::Handshake(format!("invalid header value: {e}")))?;
            let header_name: HeaderName = key
                .parse()
                .map_err(|e| TransportError::Handshake(format!("invalid header name: {e}")))?;
            req_headers.insert(header_name, header_value);
        }

        let target = WsTarget::parse(url)?;
        let stream = tunnel_via_proxy(&target, &proxy).await?;

        // Each ProxiedStream variant carries a distinct concrete stream type,
        // so we monomorphize the handshake through `proxied_ws_handshake`
        // rather than duplicating the body four times. The arms are
        // syntactically identical post-deref, but each call instantiates a
        // different generic; the `match_same_arms` lint is a false positive
        // here. The futures are boxed because `client_async` produces a
        // large state machine.
        #[allow(clippy::match_same_arms)]
        let transport: BoxedWsTransport = match stream {
            ProxiedStream::Plain(tcp) => Box::pin(proxied_ws_handshake(request, tcp)).await?,
            ProxiedStream::PlainOverTlsProxy(s) => {
                Box::pin(proxied_ws_handshake(request, *s)).await?
            }
            ProxiedStream::Tls(s) => Box::pin(proxied_ws_handshake(request, *s)).await?,
            ProxiedStream::TlsOverTlsProxy(s) => {
                Box::pin(proxied_ws_handshake(request, *s)).await?
            }
        };

        Ok(transport.split())
    }

    /// Turmoil simulator variant: HTTP `CONNECT` tunneling is not supported
    /// under the simulator so any proxy URL is rejected up front.
    #[inline]
    #[cfg(feature = "turmoil")]
    #[expect(
        clippy::unused_async,
        reason = "signature mirrors the production variant; both are awaited in the dispatcher"
    )]
    async fn connect_tungstenite_via_proxy(
        _url: &str,
        _headers: Vec<(String, String)>,
        _proxy_url: &str,
    ) -> Result<(MessageWriter, MessageReader), TransportError> {
        Err(TransportError::Other(
            "proxy_url is not supported under the turmoil simulator".to_string(),
        ))
    }

    /// Connects with the server creating a tokio-tungstenite websocket stream.
    /// Turmoil version that uses the lower-level `client_async` API with injected stream.
    #[inline]
    #[cfg(feature = "turmoil")]
    async fn connect_tungstenite(
        url: &str,
        headers: Vec<(String, String)>,
    ) -> Result<(MessageWriter, MessageReader), TransportError> {
        let mut request = url.into_client_request().map_err(TransportError::from)?;
        let req_headers = request.headers_mut();

        for (key, val) in headers {
            let header_value = HeaderValue::from_str(&val)
                .map_err(|e| TransportError::Handshake(format!("invalid header value: {e}")))?;
            let header_name: HeaderName = key
                .parse()
                .map_err(|e| TransportError::Handshake(format!("invalid header name: {e}")))?;
            req_headers.insert(header_name, header_value);
        }

        let uri = request.uri();
        let scheme = uri.scheme_str().unwrap_or("ws");
        let host = uri
            .host()
            .ok_or_else(|| TransportError::InvalidUrl("missing hostname".to_string()))?;

        // Determine port: use explicit port if specified, otherwise default based on scheme
        let port = uri
            .port_u16()
            .unwrap_or_else(|| if scheme == "wss" { 443 } else { 80 });

        let addr = format!("{host}:{port}");

        // Use the connector to get a turmoil-compatible stream
        let connector = crate::net::RealTcpConnector;
        let tcp_stream = connector.connect(&addr).await?;
        if let Err(e) = tcp_stream.set_nodelay(true) {
            log::warn!("Failed to enable TCP_NODELAY for socket client: {e:?}");
        }

        // Wrap stream appropriately based on scheme
        let maybe_tls_stream = if scheme == "wss" {
            // Build TLS config with webpki roots
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

            let config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();

            let tls_connector = TlsConnector::from(std::sync::Arc::new(config));
            let domain = rustls::pki_types::ServerName::try_from(host.to_string())
                .map_err(|e| TransportError::Tls(format!("Invalid DNS name: {e}")))?;

            let tls_stream = tls_connector
                .connect(domain, tcp_stream)
                .await
                .map_err(TransportError::Io)?;
            MaybeTlsStream::Rustls(tls_stream)
        } else {
            MaybeTlsStream::Plain(tcp_stream)
        };

        // Use client_async with the stream (plain or TLS)
        let (stream, _resp) = client_async(request, maybe_tls_stream)
            .await
            .map_err(TransportError::from)?;
        let transport: BoxedWsTransport = Box::pin(TungsteniteTransport::new(stream));
        Ok(transport.split())
    }

    /// Connects with the server using the sockudo-ws backend.
    ///
    /// Uses a local HTTP/1.1 handshake helper so error logging and stream
    /// construction stay in our hands regardless of header count.
    ///
    /// Under the turmoil simulator, only plaintext `ws://` is supported (the
    /// simulator does not model TLS), so a `wss://` URL returns
    /// [`TransportError::Tls`] up front.
    #[inline]
    #[cfg(feature = "transport-sockudo")]
    async fn connect_sockudo(
        url: &str,
        headers: Vec<(String, String)>,
    ) -> Result<(MessageWriter, MessageReader), TransportError> {
        let target = SockudoTarget::parse(url)?;
        validate_extra_headers(&headers).map_err(TransportError::from)?;

        #[cfg(feature = "turmoil")]
        if target.is_tls {
            return Err(TransportError::Tls(
                "wss:// is not supported under the turmoil simulator; use ws://".to_string(),
            ));
        }

        let tcp_stream = TcpStream::connect((target.host.as_str(), target.port))
            .await
            .map_err(TransportError::Io)?;

        if let Err(e) = tcp_stream.set_nodelay(true) {
            log::warn!("Failed to enable TCP_NODELAY for sockudo client: {e:?}");
        }

        #[cfg(not(feature = "turmoil"))]
        if target.is_tls {
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();
            let connector = TlsConnector::from(std::sync::Arc::new(config));
            let domain = rustls::pki_types::ServerName::try_from(target.host.clone())
                .map_err(|e| TransportError::Tls(format!("Invalid DNS name: {e}")))?;
            let tls_stream = connector
                .connect(domain, tcp_stream)
                .await
                .map_err(TransportError::Io)?;
            return Self::finish_sockudo_handshake(tls_stream, &target, &headers).await;
        }

        Self::finish_sockudo_handshake(tcp_stream, &target, &headers).await
    }

    #[cfg(feature = "transport-sockudo")]
    async fn finish_sockudo_handshake<S>(
        mut stream: S,
        target: &SockudoTarget,
        headers: &[(String, String)],
    ) -> Result<(MessageWriter, MessageReader), TransportError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        // Use our helper for both paths: uniform error logging, and we own
        // stream construction since sockudo's high-level client drops the
        // handshake leftover.
        let handshake = client_handshake_with_headers(
            &mut stream,
            &target.host_header,
            &target.path,
            None,
            headers,
        )
        .await
        .map_err(TransportError::from)?;

        // Reading the HTTP 101 may also read the first WebSocket frame prefix;
        // replay it only when present so the ordinary path stays unwrapped.
        let stream = match handshake.leftover {
            Some(prefix) => SockudoStream::<Http1>::new(PrefixedIo::new(stream, prefix)),
            None => SockudoStream::<Http1>::new(stream),
        };
        let ws = SockudoWebSocketStream::from_raw(stream, Role::Client, SockudoConfig::default());
        let transport: BoxedWsTransport = Box::pin(SockudoTransport::new(ws));
        Ok(transport.split())
    }
}

/// Complete the WebSocket handshake over a stream that has already been
/// tunneled through an HTTP `CONNECT` proxy. Generic over the concrete
/// stream type so the four [`super::proxy::ProxiedStream`] variants share
/// a single body.
#[cfg(not(feature = "turmoil"))]
async fn proxied_ws_handshake<S>(
    request: tokio_tungstenite::tungstenite::handshake::client::Request,
    stream: S,
) -> Result<BoxedWsTransport, TransportError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (ws, _resp) = tokio_tungstenite::client_async(request, stream)
        .await
        .map_err(TransportError::from)?;
    Ok(Box::pin(TungsteniteTransport::new(ws)))
}

/// Parsed components of a `ws://` / `wss://` URL needed by the sockudo backend.
///
/// Sockudo's HTTP/1.1 client passes the `host` argument verbatim as the
/// HTTP `Host:` header, so it must include the explicit port when one is
/// present in the URL (RFC 7230 section 5.4). The DNS / SNI lookup uses the bare
/// host without the port.
#[cfg(feature = "transport-sockudo")]
#[derive(Debug, PartialEq, Eq)]
struct SockudoTarget {
    host: String,
    /// Value to send as the HTTP `Host:` header. Includes `:port` only when
    /// the URL specifies a non-default port explicitly.
    host_header: String,
    port: u16,
    path: String,
    is_tls: bool,
}

#[cfg(feature = "transport-sockudo")]
impl SockudoTarget {
    fn parse(url: &str) -> Result<Self, TransportError> {
        let parsed =
            url::Url::parse(url).map_err(|e| TransportError::InvalidUrl(format!("{url}: {e}")))?;

        let scheme = parsed.scheme();
        let is_tls = match scheme {
            "ws" => false,
            "wss" => true,
            other => {
                return Err(TransportError::InvalidUrl(format!(
                    "expected ws:// or wss:// scheme, was {other}"
                )));
            }
        };

        let raw_host = parsed
            .host_str()
            .ok_or_else(|| TransportError::InvalidUrl("missing hostname".to_string()))?;

        // url::Url stores IPv6 hosts in their bracketed form (e.g. `[::1]`).
        // Brackets are correct for the HTTP `Host:` header but invalid for
        // DNS/TCP and TLS SNI, so we keep two representations: a bracketed
        // `host_header` for the upgrade, and a bare `host` for socket dialing.
        let is_bracketed = raw_host.starts_with('[') && raw_host.ends_with(']');
        let host = if is_bracketed {
            raw_host[1..raw_host.len() - 1].to_string()
        } else {
            raw_host.to_string()
        };

        let explicit_port = parsed.port();
        let port = explicit_port.unwrap_or(if is_tls { 443 } else { 80 });
        let host_header = match explicit_port {
            Some(p) => format!("{raw_host}:{p}"),
            None => raw_host.to_string(),
        };

        let path = if parsed.path().is_empty() {
            "/".to_string()
        } else {
            let mut p = parsed.path().to_string();
            if let Some(query) = parsed.query() {
                p.push('?');
                p.push_str(query);
            }
            p
        };

        Ok(Self {
            host,
            host_header,
            port,
            path,
            is_tls,
        })
    }
}

impl WebSocketClientInner {
    /// Reconnect with server.
    ///
    /// Make a new connection with server. Use the new read and write halves
    /// to update self writer and read and heartbeat tasks.
    ///
    /// For stream-based clients (created via `connect_stream`), reconnection is disabled
    /// because the reader is owned by the caller and cannot be replaced. Stream users
    /// should handle disconnections by creating a new connection.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The reconnection attempt times out.
    /// - The connection to the server fails.
    pub async fn reconnect(&mut self) -> Result<(), TransportError> {
        log::debug!("Reconnecting");

        if self.is_stream_mode {
            log::warn!(
                "Auto-reconnect disabled for stream-based WebSocket client; \
                stream users must manually reconnect by creating a new connection"
            );
            // Transition to CLOSED state to stop reconnection attempts
            self.connection_mode
                .store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
            return Ok(());
        }

        if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
            log::debug!("Reconnect aborted due to disconnect state");
            return Ok(());
        }

        dst::time::timeout(self.reconnect_timeout, async {
            // Attempt to connect; abort early if a disconnect was requested
            let (new_writer, reader) = Self::connect_with_server(
                &self.config.url,
                self.config.headers.clone(),
                self.config.backend,
                self.config.proxy_url.as_deref(),
            )
            .await?;

            if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
                log::debug!("Reconnect aborted mid-flight (after connect)");
                return Ok(());
            }

            // Use a oneshot channel to synchronize the writer swap before transitioning
            // back to ACTIVE. Buffered messages stay in the writer task and replay later.
            let (tx, rx) = tokio::sync::oneshot::channel();
            if let Err(e) = self.writer_tx.send(WriterCommand::Update(new_writer, tx)) {
                log::error!("{e}");
                return Err(TransportError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    format!("Failed to send update command: {e}"),
                )));
            }

            // Wait for writer to confirm it accepted the new socket
            match rx.await {
                Ok(true) => log::debug!("Writer confirmed socket update"),
                Ok(false) => {
                    log::warn!("Writer rejected socket update, aborting reconnect");
                    return Err(TransportError::Io(std::io::Error::other(
                        "Failed to update reconnection writer",
                    )));
                }
                Err(e) => {
                    log::error!("Writer dropped update channel: {e}");
                    return Err(TransportError::Io(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "Writer task dropped response channel",
                    )));
                }
            }

            // Delay before closing connection
            dst::time::sleep(Duration::from_millis(GRACEFUL_SHUTDOWN_DELAY_MS)).await;

            if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
                log::debug!("Reconnect aborted mid-flight (after delay)");
                return Ok(());
            }

            if let Some(ref read_task) = self.read_task.take()
                && !read_task.is_finished()
            {
                read_task.abort();
                log_task_aborted("read");
            }

            // Atomically transition from Reconnect to Active
            // This prevents race condition where disconnect could be requested between check and store
            if self
                .connection_mode
                .compare_exchange(
                    ConnectionMode::Reconnect.as_u8(),
                    ConnectionMode::Active.as_u8(),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_err()
            {
                log::debug!("Reconnect aborted (state changed during reconnect)");
                return Ok(());
            }

            self.read_task = if self.message_handler.is_some() {
                Some(Self::spawn_message_handler_task(
                    self.connection_mode.clone(),
                    self.state_notify.clone(),
                    reader,
                    self.message_handler.as_ref(),
                    self.ping_handler.as_ref(),
                    self.config.idle_timeout_ms,
                ))
            } else {
                None
            };

            log::debug!("Reconnect succeeded");
            Ok(())
        })
        .await
        .map_err(|_| {
            TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "reconnection timed out after {}s",
                    self.reconnect_timeout.as_secs_f64()
                ),
            ))
        })?
    }

    /// Check if the client is still alive.
    ///
    /// Returns `true` if both the read and write tasks are still running.
    /// There may be some delay between the connection closing and the
    /// client detecting it.
    #[inline]
    #[must_use]
    pub fn is_alive(&self) -> bool {
        match &self.read_task {
            Some(read_task) => !read_task.is_finished() && !self.write_task.is_finished(),
            None => !self.write_task.is_finished(),
        }
    }

    fn spawn_message_handler_task(
        connection_state: Arc<AtomicU8>,
        state_notify: Arc<tokio::sync::Notify>,
        mut reader: MessageReader,
        message_handler: Option<&MessageHandler>,
        ping_handler: Option<&PingHandler>,
        idle_timeout_ms: Option<u64>,
    ) -> tokio::task::JoinHandle<()> {
        log::debug!("Started message handler task 'read'");

        let check_interval = Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS);
        let idle_timeout = idle_timeout_ms.map(Duration::from_millis);

        // Clone Arc handlers for the async task
        let message_handler = message_handler.cloned();
        let ping_handler = ping_handler.cloned();

        tokio::task::spawn(async move {
            let mut last_data_time = dst::time::Instant::now();

            loop {
                if !ConnectionMode::from_atomic(&connection_state).is_active() {
                    break;
                }

                match dst::time::timeout(check_interval, reader.next()).await {
                    Ok(Some(Ok(Message::Binary(data)))) => {
                        log::trace!("Received message <binary> {} bytes", data.len());
                        last_data_time = dst::time::Instant::now();

                        if let Some(ref handler) = message_handler {
                            handler(Message::Binary(data));
                        }
                    }
                    Ok(Some(Ok(Message::Text(data)))) => {
                        log::trace!("Received message: {data:?}");
                        last_data_time = dst::time::Instant::now();

                        if let Some(ref handler) = message_handler {
                            handler(Message::Text(data));
                        }
                    }
                    Ok(Some(Ok(Message::Ping(ping_data)))) => {
                        log::trace!("Received ping: {ping_data:?}");
                        // Do not reset last_data_time: pings are keep-alive frames, not application
                        // data, so a peer that emits only pings must still trip the idle timeout.

                        if let Some(ref handler) = ping_handler {
                            handler(ping_data.to_vec());
                        }
                    }
                    Ok(Some(Ok(Message::Pong(_)))) => {
                        log::trace!("Received pong");
                        // Do not reset last_data_time: pongs are keep-alive replies (not data)
                    }
                    Ok(Some(Ok(Message::Close(_)))) => {
                        log::debug!("Received close message - terminating");
                        break;
                    }
                    Ok(Some(Err(e))) => {
                        log::error!("Received error message - terminating: {e}");
                        break;
                    }
                    Ok(None) => {
                        log::debug!("No message received - terminating");
                        break;
                    }
                    Err(_) => {
                        if let Some(timeout) = idle_timeout {
                            let idle_duration = last_data_time.elapsed();
                            if idle_duration >= timeout {
                                log::warn!(
                                    "Read idle timeout: no data received for {:.1}s",
                                    idle_duration.as_secs_f64()
                                );
                                break;
                            }
                        }
                    }
                }
            }

            // Wake the controller immediately so it detects the dead read task
            state_notify.notify_one();
        })
    }

    /// Attempts to send all buffered messages after reconnection.
    ///
    /// Returns `true` if a send error occurred (caller should trigger reconnection).
    /// Messages remain in buffer if send fails, preserving them for the next reconnection attempt.
    async fn drain_reconnect_buffer(
        buffer: &mut VecDeque<Message>,
        writer: &mut MessageWriter,
    ) -> bool {
        if buffer.is_empty() {
            return false;
        }

        let initial_buffer_len = buffer.len();
        log::info!("Sending {initial_buffer_len} buffered messages after reconnection");

        let mut send_error_occurred = false;

        while let Some(buffered_msg) = buffer.front() {
            // Clone message before attempting send (to keep in buffer if send fails)
            let msg_to_send = buffered_msg.clone();

            if let Err(e) = writer.send(msg_to_send).await {
                log::error!(
                    "Failed to send buffered message after reconnection: {e}, {} messages remain in buffer",
                    buffer.len()
                );
                send_error_occurred = true;
                break; // Stop processing buffer, remaining messages preserved for next reconnection
            }

            // Only remove from buffer after successful send
            buffer.pop_front();
        }

        if buffer.is_empty() {
            log::info!("Successfully sent all {initial_buffer_len} buffered messages");
        }

        send_error_occurred
    }

    fn can_drain_reconnect_buffer(
        reconnect_buffer_waits_for_auth: &AtomicBool,
        auth_tracker: &Arc<OnceLock<AuthTracker>>,
    ) -> ReconnectBufferAction {
        if !reconnect_buffer_waits_for_auth.load(Ordering::Acquire) {
            return ReconnectBufferAction::Drain;
        }

        match auth_tracker.get().map(AuthTracker::auth_state) {
            Some(AuthState::Authenticated) => ReconnectBufferAction::Drain,
            Some(AuthState::Failed) => ReconnectBufferAction::Discard,
            Some(AuthState::Unauthenticated) | None => ReconnectBufferAction::Wait,
        }
    }

    fn spawn_write_task(
        connection_state: Arc<AtomicU8>,
        state_notify: Arc<tokio::sync::Notify>,
        writer: MessageWriter,
        mut writer_rx: tokio::sync::mpsc::UnboundedReceiver<WriterCommand>,
        auth_tracker: Arc<OnceLock<AuthTracker>>,
        reconnect_buffer_waits_for_auth: Arc<AtomicBool>,
    ) -> tokio::task::JoinHandle<()> {
        log_task_started("write");

        // Interval between checking the connection mode
        let check_interval = Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS);

        tokio::task::spawn(async move {
            let mut active_writer = writer;
            // Buffer for messages received during reconnection
            // VecDeque for efficient pop_front() operations
            let mut reconnect_buffer: VecDeque<Message> = VecDeque::new();

            loop {
                let mode = ConnectionMode::from_atomic(&connection_state);

                match mode {
                    ConnectionMode::Disconnect => {
                        // Log any buffered messages that will be lost
                        if !reconnect_buffer.is_empty() {
                            log::warn!(
                                "Discarding {} buffered messages due to disconnect",
                                reconnect_buffer.len()
                            );
                            reconnect_buffer.clear();
                        }

                        // Attempt to close the writer gracefully before exiting,
                        // we ignore any error as the writer may already be closed.
                        _ = dst::time::timeout(
                            Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                            active_writer.close(),
                        )
                        .await;
                        break;
                    }
                    ConnectionMode::Closed => {
                        // Log any buffered messages that will be lost
                        if !reconnect_buffer.is_empty() {
                            log::warn!(
                                "Discarding {} buffered messages due to closed connection",
                                reconnect_buffer.len()
                            );
                            reconnect_buffer.clear();
                        }
                        break;
                    }
                    _ => {}
                }

                if mode.is_active() && !reconnect_buffer.is_empty() {
                    match Self::can_drain_reconnect_buffer(
                        reconnect_buffer_waits_for_auth.as_ref(),
                        &auth_tracker,
                    ) {
                        ReconnectBufferAction::Drain => {
                            let send_error = Self::drain_reconnect_buffer(
                                &mut reconnect_buffer,
                                &mut active_writer,
                            )
                            .await;

                            if send_error {
                                if let Some(tracker) = auth_tracker.get() {
                                    tracker.invalidate();
                                }
                                connection_state
                                    .store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
                                state_notify.notify_one();
                            }

                            continue;
                        }
                        ReconnectBufferAction::Discard => {
                            log::warn!(
                                "Discarding {} buffered messages after authentication failed",
                                reconnect_buffer.len()
                            );
                            reconnect_buffer.clear();
                            continue;
                        }
                        ReconnectBufferAction::Wait => {}
                    }
                }

                match dst::time::timeout(check_interval, writer_rx.recv()).await {
                    Ok(Some(msg)) => {
                        // Re-check connection mode after receiving a message
                        let mode = ConnectionMode::from_atomic(&connection_state);
                        if matches!(mode, ConnectionMode::Disconnect | ConnectionMode::Closed) {
                            break;
                        }

                        match msg {
                            WriterCommand::Update(new_writer, tx) => {
                                log::debug!("Received new writer");

                                // Delay before closing connection
                                dst::time::sleep(Duration::from_millis(100)).await;

                                // Attempt to close the writer gracefully on update,
                                // we ignore any error as the writer may already be closed.
                                _ = dst::time::timeout(
                                    Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                                    active_writer.close(),
                                )
                                .await;

                                active_writer = new_writer;
                                log::debug!("Updated writer");

                                if let Err(e) = tx.send(true) {
                                    log::error!(
                                        "Failed to report writer update to controller: {e:?}"
                                    );
                                }
                            }
                            WriterCommand::Send(msg) if mode.is_reconnect() => {
                                // Buffer messages during reconnection instead of dropping them
                                log::debug!(
                                    "Buffering message during reconnection (buffer size: {})",
                                    reconnect_buffer.len() + 1
                                );
                                reconnect_buffer.push_back(msg);
                            }
                            WriterCommand::Send(msg) => {
                                if let Err(e) = active_writer.send(msg.clone()).await {
                                    log::error!("Failed to send message: {e}");
                                    log::warn!("Writer triggering reconnect");
                                    reconnect_buffer.push_back(msg);

                                    if let Some(tracker) = auth_tracker.get() {
                                        tracker.invalidate();
                                    }
                                    connection_state
                                        .store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
                                    state_notify.notify_one();
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        // Channel closed - writer task should terminate
                        log::debug!("Writer channel closed, terminating writer task");
                        break;
                    }
                    Err(_) => {
                        // Timeout - just continue the loop
                    }
                }
            }

            // Attempt to close the writer gracefully before exiting,
            // we ignore any error as the writer may already be closed.
            _ = dst::time::timeout(
                Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                active_writer.close(),
            )
            .await;

            log_task_stopped("write");
        })
    }

    fn spawn_heartbeat_task(
        connection_state: Arc<AtomicU8>,
        heartbeat_secs: u64,
        message: Option<String>,
        writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    ) -> tokio::task::JoinHandle<()> {
        log_task_started("heartbeat");

        tokio::task::spawn(async move {
            let interval = Duration::from_secs(heartbeat_secs);

            loop {
                dst::time::sleep(interval).await;

                match ConnectionMode::from_u8(connection_state.load(Ordering::SeqCst)) {
                    ConnectionMode::Active => {
                        let msg = match &message {
                            Some(text) => WriterCommand::Send(Message::Text(text.clone().into())),
                            None => WriterCommand::Send(Message::Ping(vec![].into())),
                        };

                        match writer_tx.send(msg) {
                            Ok(()) => log::trace!("Sent heartbeat to writer task"),
                            Err(e) => {
                                log::error!("Failed to send heartbeat to writer task: {e}");
                            }
                        }
                    }
                    ConnectionMode::Reconnect => {}
                    ConnectionMode::Disconnect | ConnectionMode::Closed => break,
                }
            }

            log_task_stopped("heartbeat");
        })
    }
}

impl Drop for WebSocketClientInner {
    fn drop(&mut self) {
        // Delegate to explicit cleanup handler
        self.clean_drop();
    }
}

/// Cleanup on drop: aborts background tasks and clears handlers to break reference cycles.
impl CleanDrop for WebSocketClientInner {
    fn clean_drop(&mut self) {
        if let Some(ref read_task) = self.read_task.take()
            && !read_task.is_finished()
        {
            read_task.abort();
            log_task_aborted("read");
        }

        if !self.write_task.is_finished() {
            self.write_task.abort();
            log_task_aborted("write");
        }

        if let Some(ref handle) = self.heartbeat_task.take()
            && !handle.is_finished()
        {
            handle.abort();
            log_task_aborted("heartbeat");
        }

        // Clear handlers to break potential reference cycles
        self.message_handler = None;
        self.ping_handler = None;
    }
}

#[expect(
    clippy::missing_fields_in_debug,
    reason = "handler closures and internal task handles are intentionally omitted"
)]
impl Debug for WebSocketClientInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(WebSocketClientInner))
            .field("config", &self.config)
            .field(
                "connection_mode",
                &ConnectionMode::from_atomic(&self.connection_mode),
            )
            .field("reconnect_timeout", &self.reconnect_timeout)
            .field("is_stream_mode", &self.is_stream_mode)
            .finish()
    }
}

/// WebSocket client with automatic reconnection.
///
/// Handles connection state, callbacks, and rate limiting.
/// See module docs for architecture details.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.network")
)]
pub struct WebSocketClient {
    pub(crate) controller_task: tokio::task::JoinHandle<()>,
    pub(crate) connection_mode: Arc<AtomicU8>,
    pub(crate) state_notify: Arc<tokio::sync::Notify>,
    pub(crate) reconnect_timeout: Duration,
    pub(crate) rate_limiter: Arc<RateLimiter<Ustr, MonotonicClock>>,
    pub(crate) writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    auth_tracker: Arc<OnceLock<AuthTracker>>,
    reconnect_buffer_waits_for_auth: Arc<AtomicBool>,
}

impl Debug for WebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(WebSocketClient)).finish()
    }
}

impl WebSocketClient {
    /// Creates a websocket client in **stream mode** that returns a [`MessageReader`].
    ///
    /// Returns a stream that the caller owns and reads from directly. Automatic reconnection
    /// is **disabled** because the reader cannot be replaced internally. On disconnection, the
    /// client transitions to CLOSED state and the caller must manually reconnect by calling
    /// `connect_stream` again.
    ///
    /// Use stream mode when you need custom reconnection logic, direct control over message
    /// reading, or fine-grained backpressure handling.
    ///
    /// See [`WebSocketConfig`] documentation for comparison with handler mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect_stream(
        config: WebSocketConfig,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
        post_reconnect: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<(MessageReader, Self), TransportError> {
        install_cryptographic_provider();

        // Create a single connection and split it, respecting configured headers
        let (writer, reader) = WebSocketClientInner::connect_with_server(
            &config.url,
            config.headers.clone(),
            config.backend,
            config.proxy_url.as_deref(),
        )
        .await?;

        // Create inner without connecting (we'll provide the writer)
        let inner = WebSocketClientInner::new_with_writer(config, writer).await?;

        let connection_mode = inner.connection_mode.clone();
        let state_notify = inner.state_notify.clone();
        let reconnect_timeout = inner.reconnect_timeout;
        let auth_tracker = Arc::clone(&inner.auth_tracker);
        let reconnect_buffer_waits_for_auth = Arc::clone(&inner.reconnect_buffer_waits_for_auth);
        let keyed_quotas = keyed_quotas
            .into_iter()
            .map(|(key, quota)| (Ustr::from(&key), quota))
            .collect();
        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));
        let writer_tx = inner.writer_tx.clone();

        let controller_task = Self::spawn_controller_task(
            inner,
            connection_mode.clone(),
            state_notify.clone(),
            post_reconnect,
            Arc::clone(&auth_tracker),
        );

        Ok((
            reader,
            Self {
                controller_task,
                connection_mode,
                state_notify,
                reconnect_timeout,
                rate_limiter,
                writer_tx,
                auth_tracker,
                reconnect_buffer_waits_for_auth,
            },
        ))
    }

    /// Creates a websocket client in **handler mode** with automatic reconnection.
    ///
    /// The handler is called for each incoming message on an internal task.
    /// Automatic reconnection is **enabled** with exponential backoff. On disconnection,
    /// the client automatically attempts to reconnect and replaces the internal reader
    /// (the handler continues working seamlessly).
    ///
    /// Use handler mode for simplified connection management, automatic reconnection, Python
    /// bindings, or callback-based message handling.
    ///
    /// See [`WebSocketConfig`] documentation for comparison with stream mode.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The connection cannot be established.
    /// - `message_handler` is `None` (use `connect_stream` instead).
    pub async fn connect(
        config: WebSocketConfig,
        message_handler: Option<MessageHandler>,
        ping_handler: Option<PingHandler>,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
    ) -> Result<Self, TransportError> {
        // Validate that handler mode has a message handler
        if message_handler.is_none() {
            return Err(TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Handler mode requires message_handler to be set. Use connect_stream() for stream mode without a handler.",
            )));
        }

        log::debug!("Connecting");
        let inner =
            WebSocketClientInner::connect_url(config, message_handler, ping_handler).await?;
        let connection_mode = inner.connection_mode.clone();
        let state_notify = inner.state_notify.clone();
        let writer_tx = inner.writer_tx.clone();
        let reconnect_timeout = inner.reconnect_timeout;
        let auth_tracker = Arc::clone(&inner.auth_tracker);
        let reconnect_buffer_waits_for_auth = Arc::clone(&inner.reconnect_buffer_waits_for_auth);

        let controller_task = Self::spawn_controller_task(
            inner,
            connection_mode.clone(),
            state_notify.clone(),
            post_reconnection,
            Arc::clone(&auth_tracker),
        );

        let keyed_quotas = keyed_quotas
            .into_iter()
            .map(|(key, quota)| (Ustr::from(&key), quota))
            .collect();
        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));

        Ok(Self {
            controller_task,
            connection_mode,
            state_notify,
            reconnect_timeout,
            rate_limiter,
            writer_tx,
            auth_tracker,
            reconnect_buffer_waits_for_auth,
        })
    }

    /// Returns the current connection mode.
    #[must_use]
    pub fn connection_mode(&self) -> ConnectionMode {
        ConnectionMode::from_atomic(&self.connection_mode)
    }

    /// Returns a clone of the connection mode atomic for external state tracking.
    ///
    /// This allows adapter clients to track connection state across reconnections
    /// without message-passing delays.
    #[must_use]
    pub fn connection_mode_atomic(&self) -> Arc<AtomicU8> {
        Arc::clone(&self.connection_mode)
    }

    /// Check if the client connection is active.
    ///
    /// Returns `true` if the client is connected and has not been signalled to disconnect.
    /// The client will automatically retry connection based on its configuration.
    #[inline]
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.connection_mode().is_active()
    }

    /// Check if the client is disconnected.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.controller_task.is_finished()
    }

    /// Check if the client is reconnecting.
    ///
    /// Returns `true` if the client lost connection and is attempting to reestablish it.
    /// The client will automatically retry connection based on its configuration.
    #[inline]
    #[must_use]
    pub fn is_reconnecting(&self) -> bool {
        self.connection_mode().is_reconnect()
    }

    /// Registers an [`AuthTracker`] with the client.
    ///
    /// When the controller detects a dead connection and transitions to
    /// `Reconnect`, it calls `invalidate()` on the tracker so that any
    /// pending authenticated sends see the state change immediately.
    /// Set `reconnect_buffer_waits_for_auth` for clients that must not replay
    /// buffered messages until the next session authenticates.
    ///
    /// Call this once after construction, before any authenticated sends.
    pub fn set_auth_tracker(&self, tracker: AuthTracker, reconnect_buffer_waits_for_auth: bool) {
        let _ = self.auth_tracker.set(tracker);
        self.reconnect_buffer_waits_for_auth
            .store(reconnect_buffer_waits_for_auth, Ordering::Release);
    }

    /// Check if the client is disconnecting.
    ///
    /// Returns `true` if the client is in disconnect mode.
    #[inline]
    #[must_use]
    pub fn is_disconnecting(&self) -> bool {
        self.connection_mode().is_disconnect()
    }

    /// Check if the client is closed.
    ///
    /// Returns `true` if the client has been explicitly disconnected or reached
    /// maximum reconnection attempts. In this state, the client cannot be reused
    /// and a new client must be created for further connections.
    #[inline]
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.connection_mode().is_closed()
    }

    /// Checks whether the connection is in a terminal state (disconnecting or closed).
    ///
    /// Single atomic load to fail fast before rate limiting or waiting.
    #[inline]
    fn check_not_terminal(&self) -> Result<(), SendError> {
        match self.connection_mode() {
            ConnectionMode::Disconnect | ConnectionMode::Closed => Err(SendError::Closed),
            _ => Ok(()),
        }
    }

    /// Waits for rate limiter quota, aborting early if connection enters a terminal state.
    async fn await_rate_limit_or_closed(&self, keys: Option<&[Ustr]>) -> Result<(), SendError> {
        const CHECK_INTERVAL_MS: u64 = 100;

        tokio::select! {
            biased;
            () = self.rate_limiter.await_keys_ready(keys) => Ok(()),
            () = async {
                loop {
                    let notified = self.state_notify.notified();

                    if matches!(self.connection_mode(), ConnectionMode::Disconnect | ConnectionMode::Closed) {
                        break;
                    }
                    tokio::select! {
                        biased;
                        () = notified => {}
                        () = dst::time::sleep(Duration::from_millis(CHECK_INTERVAL_MS)) => {}
                    }
                }
            } => Err(SendError::Closed),
        }
    }

    /// Waits for the client to become active before sending.
    ///
    /// Uses `state_notify` for event-driven wakeup so sends resume immediately
    /// after reconnection completes. A fallback interval guards against missed
    /// notifications.
    async fn wait_for_active(&self) -> Result<(), SendError> {
        const FALLBACK_INTERVAL_MS: u64 = 100;

        let mode = self.connection_mode();
        if mode.is_active() {
            return Ok(());
        }

        if matches!(mode, ConnectionMode::Disconnect | ConnectionMode::Closed) {
            return Err(SendError::Closed);
        }

        log::debug!("Waiting for client to become ACTIVE before sending...");

        let fallback_interval = Duration::from_millis(FALLBACK_INTERVAL_MS);

        dst::time::timeout(self.reconnect_timeout, async {
            loop {
                // Register notification interest BEFORE checking state to prevent
                // a race where the state changes between our check and the await
                let notified = self.state_notify.notified();

                let mode = self.connection_mode();
                if mode.is_active() {
                    return Ok(());
                }

                if matches!(mode, ConnectionMode::Disconnect | ConnectionMode::Closed) {
                    return Err(());
                }

                tokio::select! {
                    biased;
                    () = notified => {}
                    () = dst::time::sleep(fallback_interval) => {}
                }
            }
        })
        .await
        .map_err(|_| SendError::Timeout)?
        .map_err(|()| SendError::Closed)
    }

    /// Signals that the caller's reader has observed EOF or a fatal error.
    ///
    /// In stream mode the controller has no visibility into the caller-owned reader.
    /// Call this method when `reader.next().await` returns `None` or an unrecoverable
    /// error so the controller transitions to `Closed` and dependent tasks shut down.
    ///
    /// For peer-initiated close frames (`Message::Close`), use [`disconnect`](Self::disconnect)
    /// instead so the writer can send the close reply before shutting down.
    ///
    /// This is a no-op if the connection is already closed or disconnecting.
    pub fn notify_closed(&self) {
        let mode = self.connection_mode();
        if mode.is_disconnect() || mode.is_closed() {
            return;
        }

        log::debug!("Stream reader signalled EOF, transitioning to CLOSED");

        self.connection_mode
            .store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
        self.state_notify.notify_waiters();
    }

    /// Set disconnect mode to true.
    ///
    /// Controller task will periodically check the disconnect mode
    /// and shutdown the client if it is alive
    pub async fn disconnect(&self) {
        log::debug!("Disconnecting");
        self.connection_mode
            .store(ConnectionMode::Disconnect.as_u8(), Ordering::SeqCst);
        self.state_notify.notify_waiters();

        if dst::time::timeout(Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS), async {
            while !self.is_disconnected() {
                dst::time::sleep(Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS)).await;
            }

            if !self.controller_task.is_finished() {
                self.controller_task.abort();
                log_task_aborted("controller");
            }
        })
        .await
            == Ok(())
        {
            log::debug!("Controller task finished");
        } else {
            log::error!("Timeout waiting for controller task to finish");

            if !self.controller_task.is_finished() {
                self.controller_task.abort();
                log_task_aborted("controller");
            }
            self.connection_mode
                .store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
        }
    }

    /// Sends the given text `data` to the server.
    ///
    /// Returns `Ok(())` when the message is enqueued to the writer channel. This does NOT
    /// guarantee delivery: if a disconnect occurs concurrently, the writer task may drop the
    /// message. During reconnection, messages are buffered and replayed on the new connection.
    ///
    /// # Errors
    ///
    /// Returns a websocket error if unable to send.
    #[allow(unused_variables)]
    pub async fn send_text(&self, data: String, keys: Option<&[Ustr]>) -> Result<(), SendError> {
        self.check_not_terminal()?;

        self.await_rate_limit_or_closed(keys).await?;
        self.wait_for_active().await?;

        log::trace!("Sending text: {data:?}");

        let msg = Message::Text(data.into());
        self.writer_tx
            .send(WriterCommand::Send(msg))
            .map_err(|e| SendError::BrokenPipe(e.to_string()))
    }

    /// Sends a pong frame back to the server.
    ///
    /// # Errors
    ///
    /// Returns a websocket error if unable to send.
    pub async fn send_pong(&self, data: Vec<u8>) -> Result<(), SendError> {
        self.wait_for_active().await?;

        log::trace!("Sending pong frame ({} bytes)", data.len());

        let msg = Message::Pong(data.into());
        self.writer_tx
            .send(WriterCommand::Send(msg))
            .map_err(|e| SendError::BrokenPipe(e.to_string()))
    }

    /// Sends the given bytes `data` to the server.
    ///
    /// Returns `Ok(())` when the message is enqueued to the writer channel. This does NOT
    /// guarantee delivery: if a disconnect occurs concurrently, the writer task may drop the
    /// message. During reconnection, messages are buffered and replayed on the new connection.
    ///
    /// # Errors
    ///
    /// Returns a websocket error if unable to send.
    #[allow(unused_variables)]
    pub async fn send_bytes(&self, data: Vec<u8>, keys: Option<&[Ustr]>) -> Result<(), SendError> {
        self.check_not_terminal()?;

        self.await_rate_limit_or_closed(keys).await?;
        self.wait_for_active().await?;

        log::trace!("Sending bytes: {data:?}");

        let msg = Message::Binary(data.into());
        self.writer_tx
            .send(WriterCommand::Send(msg))
            .map_err(|e| SendError::BrokenPipe(e.to_string()))
    }

    /// Sends a close message to the server.
    ///
    /// # Errors
    ///
    /// Returns a websocket error if unable to send.
    pub async fn send_close_message(&self) -> Result<(), SendError> {
        self.wait_for_active().await?;

        let msg = Message::Close(None);
        self.writer_tx
            .send(WriterCommand::Send(msg))
            .map_err(|e| SendError::BrokenPipe(e.to_string()))
    }

    fn spawn_controller_task(
        mut inner: WebSocketClientInner,
        connection_mode: Arc<AtomicU8>,
        state_notify: Arc<tokio::sync::Notify>,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
        auth_tracker: Arc<OnceLock<AuthTracker>>,
    ) -> tokio::task::JoinHandle<()> {
        const CONTROLLER_FALLBACK_INTERVAL_MS: u64 = 100;

        tokio::task::spawn(async move {
            log_task_started("controller");

            let fallback_interval = Duration::from_millis(CONTROLLER_FALLBACK_INTERVAL_MS);

            loop {
                tokio::select! {
                    biased;
                    () = state_notify.notified() => {}
                    () = dst::time::sleep(fallback_interval) => {}
                }

                let mut mode = ConnectionMode::from_atomic(&connection_mode);

                if mode.is_disconnect() {
                    log::debug!("Disconnecting");

                    let timeout = Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS);
                    if dst::time::timeout(timeout, async {
                        // Delay awaiting graceful shutdown
                        dst::time::sleep(Duration::from_millis(GRACEFUL_SHUTDOWN_DELAY_MS)).await;

                        if let Some(task) = &inner.read_task
                            && !task.is_finished()
                        {
                            task.abort();
                            log_task_aborted("read");
                        }

                        if let Some(task) = &inner.heartbeat_task
                            && !task.is_finished()
                        {
                            task.abort();
                            log_task_aborted("heartbeat");
                        }
                    })
                    .await
                    .is_err()
                    {
                        log::error!("Shutdown timed out after {}s", timeout.as_secs());
                    }

                    log::debug!("Closed");
                    break; // Controller finished
                }

                if mode.is_closed() {
                    log::debug!("Connection closed");
                    break;
                }

                if mode.is_active() && !inner.is_alive() {
                    let target = if inner.is_stream_mode {
                        ConnectionMode::Closed
                    } else {
                        ConnectionMode::Reconnect
                    };

                    if connection_mode
                        .compare_exchange(
                            ConnectionMode::Active.as_u8(),
                            target.as_u8(),
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_ok()
                    {
                        if let Some(tracker) = auth_tracker.get() {
                            tracker.invalidate();
                        }
                        log::debug!("Detected dead connection, transitioning to {target:?}");
                    }
                    mode = ConnectionMode::from_atomic(&connection_mode);
                }

                if mode.is_reconnect() {
                    // Check if max reconnection attempts exceeded
                    if let Some(max_attempts) = inner.reconnect_max_attempts
                        && inner.reconnection_attempt_count >= max_attempts
                    {
                        log::error!(
                            "Max reconnection attempts ({max_attempts}) exceeded, transitioning to CLOSED"
                        );
                        connection_mode.store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
                        state_notify.notify_waiters();
                        break;
                    }

                    inner.reconnection_attempt_count += 1;
                    log::debug!(
                        "Reconnection attempt {} of {}",
                        inner.reconnection_attempt_count,
                        inner
                            .reconnect_max_attempts
                            .map_or_else(|| "unlimited".to_string(), |m| m.to_string())
                    );

                    // Race reconnect against disconnect notification
                    let reconnect_result = tokio::select! {
                        biased;
                        result = inner.reconnect() => Some(result),
                        () = async {
                            loop {
                                state_notify.notified().await;

                                if ConnectionMode::from_atomic(&connection_mode).is_disconnect() {
                                    break;
                                }
                            }
                        } => None,
                    };

                    match reconnect_result {
                        None => {
                            log::debug!("Reconnect interrupted by disconnect");
                        }
                        Some(Ok(())) => {
                            inner.backoff.reset();
                            inner.reconnection_attempt_count = 0;

                            state_notify.notify_waiters();

                            if ConnectionMode::from_atomic(&connection_mode).is_active() {
                                if let Some(ref handler) = inner.message_handler {
                                    let reconnected_msg =
                                        Message::Text(RECONNECTED.to_string().into());
                                    handler(reconnected_msg);
                                    log::debug!("Sent reconnected message to handler");
                                }

                                // TODO: Retain this legacy callback for use from Python
                                if let Some(ref callback) = post_reconnection {
                                    callback();
                                    log::debug!("Called `post_reconnection` handler");
                                }

                                log::debug!("Reconnected successfully");
                            } else {
                                log::debug!(
                                    "Skipping post_reconnection handlers due to disconnect state"
                                );
                            }
                        }
                        Some(Err(e)) => {
                            let duration = inner.backoff.next_duration();
                            log::warn!(
                                "Reconnect attempt {} failed: {e}",
                                inner.reconnection_attempt_count
                            );

                            if !duration.is_zero() {
                                log::warn!("Backing off for {}s...", duration.as_secs_f64());
                                // Race backoff sleep against disconnect
                                tokio::select! {
                                    biased;
                                    () = dst::time::sleep(duration) => {}
                                    () = async {
                                        loop {
                                            state_notify.notified().await;

                                            if ConnectionMode::from_atomic(&connection_mode).is_disconnect() {
                                                break;
                                            }
                                        }
                                    } => {
                                        log::debug!("Backoff interrupted by disconnect");
                                    }
                                }
                            }
                        }
                    }
                }
            }
            inner
                .connection_mode
                .store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);

            log_task_stopped("controller");
        })
    }
}

// Abort controller task on drop to clean up background tasks
impl Drop for WebSocketClient {
    fn drop(&mut self) {
        if !self.controller_task.is_finished() {
            self.controller_task.abort();
            log_task_aborted("controller");
        }
    }
}

#[cfg(test)]
#[cfg(not(feature = "turmoil"))]
#[cfg(not(all(feature = "simulation", madsim)))] // transport-layer I/O not simulated
#[cfg(target_os = "linux")] // Only run network tests on Linux (CI stability)
mod tests {
    use std::{num::NonZeroU32, sync::Arc};

    use futures_util::{SinkExt, StreamExt};
    use tokio::{
        net::TcpListener,
        task::{self, JoinHandle},
    };
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::{
            Message as WsMessage,
            handshake::server::{self, Callback},
            http::HeaderValue,
        },
    };

    use crate::{
        ratelimiter::quota::Quota,
        websocket::{TransportBackend, WebSocketClient, WebSocketConfig},
    };

    struct TestServer {
        task: JoinHandle<()>,
        port: u16,
    }

    #[derive(Debug, Clone)]
    struct TestCallback {
        key: String,
        value: HeaderValue,
    }

    impl Callback for TestCallback {
        #[expect(clippy::panic_in_result_fn)]
        fn on_request(
            self,
            request: &server::Request,
            response: server::Response,
        ) -> Result<server::Response, server::ErrorResponse> {
            let _ = response;
            let value = request.headers().get(&self.key);
            assert!(value.is_some());

            if let Some(value) = request.headers().get(&self.key) {
                assert_eq!(value, self.value);
            }

            Ok(response)
        }
    }

    impl TestServer {
        async fn setup() -> Self {
            let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = TcpListener::local_addr(&server).unwrap().port();

            let header_key = "test".to_string();
            let header_value = "test".to_string();

            let test_call_back = TestCallback {
                key: header_key,
                value: HeaderValue::from_str(&header_value).unwrap(),
            };

            let task = task::spawn(async move {
                // Keep accepting connections
                loop {
                    let (conn, _) = server.accept().await.unwrap();
                    let mut websocket = accept_hdr_async(conn, test_call_back.clone())
                        .await
                        .unwrap();

                    task::spawn(async move {
                        // Inner if consumes `msg`, cannot hoist into a match guard
                        #[expect(clippy::collapsible_match)]
                        while let Some(Ok(msg)) = websocket.next().await {
                            match msg {
                                WsMessage::Text(txt) if txt == "close-now" => {
                                    log::debug!("Forcibly closing from server side");
                                    // This sends a close frame, then stops reading
                                    let _ = websocket.close(None).await;
                                    break;
                                }
                                // Echo text/binary frames
                                WsMessage::Text(_) | WsMessage::Binary(_) => {
                                    if websocket.send(msg).await.is_err() {
                                        break;
                                    }
                                }
                                // If the client closes, we also break
                                WsMessage::Close(_frame) => {
                                    let _ = websocket.close(None).await;
                                    break;
                                }
                                // Ignore pings/pongs
                                _ => {}
                            }
                        }
                    });
                }
            });

            Self { task, port }
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn setup_test_client(port: u16) -> WebSocketClient {
        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![("test".into(), "test".into())],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };
        WebSocketClient::connect(config, Some(Arc::new(|_| {})), None, None, vec![], None)
            .await
            .expect("Failed to connect")
    }

    #[tokio::test]
    async fn test_websocket_basic() {
        let server = TestServer::setup().await;
        let client = setup_test_client(server.port).await;

        assert!(!client.is_disconnected());

        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_websocket_heartbeat() {
        let server = TestServer::setup().await;
        let client = setup_test_client(server.port).await;

        // Wait ~3s => server should see multiple "ping"
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_websocket_reconnect_exhausted() {
        let config = WebSocketConfig {
            url: "ws://127.0.0.1:9997".into(), // <-- No server
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };
        let res =
            WebSocketClient::connect(config, Some(Arc::new(|_| {})), None, None, vec![], None)
                .await;
        assert!(res.is_err(), "Should fail quickly with no server");
    }

    #[tokio::test]
    async fn test_websocket_forced_close_reconnect() {
        let server = TestServer::setup().await;
        let client = setup_test_client(server.port).await;

        // 1) Send normal message
        client.send_text("Hello".into(), None).await.unwrap();

        // 2) Trigger forced close from server
        client.send_text("close-now".into(), None).await.unwrap();

        // 3) Wait a bit => read loop sees close => reconnect
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Confirm not disconnected
        assert!(!client.is_disconnected());

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let server = TestServer::setup().await;
        let quota = Quota::per_second(NonZeroU32::new(2).unwrap()).unwrap();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{}", server.port),
            headers: vec![("test".into(), "test".into())],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(
            config,
            Some(Arc::new(|_| {})),
            None,
            None,
            vec![("default".into(), quota)],
            None,
        )
        .await
        .unwrap();

        // First 2 should succeed
        client.send_text("test1".into(), None).await.unwrap();
        client.send_text("test2".into(), None).await.unwrap();

        // Third should error
        client.send_text("test3".into(), None).await.unwrap();

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_concurrent_writers() {
        let server = TestServer::setup().await;
        let client = Arc::new(setup_test_client(server.port).await);

        let mut handles = vec![];

        for i in 0..10 {
            let client = client.clone();
            handles.push(task::spawn(async move {
                client.send_text(format!("test{i}"), None).await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }
}

#[cfg(test)]
#[cfg(not(feature = "turmoil"))]
#[cfg(not(all(feature = "simulation", madsim)))] // transport-layer I/O not simulated
mod rust_tests {
    use std::sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    };

    use futures_util::{SinkExt, StreamExt};
    use nautilus_common::testing::wait_until_async;
    use rstest::rstest;
    #[cfg(feature = "transport-sockudo")]
    use sockudo_ws::handshake as sockudo_handshake;
    #[cfg(feature = "transport-sockudo")]
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
    use tokio::{
        net::TcpListener,
        task::{self, JoinHandle},
        time::{Duration, sleep},
    };
    use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};
    #[cfg(feature = "transport-sockudo")]
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::{
            handshake::server::{self, Callback},
            http::HeaderValue,
        },
    };

    use super::*;
    use crate::websocket::types::channel_message_handler;

    struct RecordingServer {
        task: JoinHandle<()>,
        port: u16,
        messages: Arc<tokio::sync::Mutex<Vec<String>>>,
    }

    #[cfg(feature = "transport-sockudo")]
    async fn read_http_request<S>(stream: &mut S) -> Vec<u8>
    where
        S: AsyncRead + Unpin,
    {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 256];

        loop {
            let n = stream.read(&mut chunk).await.unwrap();
            assert!(n > 0, "HTTP request closed before headers completed");
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|window| window == b"\r\n\r\n") {
                return buf;
            }
        }
    }

    #[cfg(feature = "transport-sockudo")]
    fn extract_header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
        request.lines().find_map(|line| {
            let (header_name, header_value) = line.split_once(':')?;
            if header_name.eq_ignore_ascii_case(name) {
                Some(header_value.trim())
            } else {
                None
            }
        })
    }

    #[cfg(feature = "transport-sockudo")]
    #[derive(Debug, Clone)]
    struct HeaderAssertCallback {
        key: String,
        value: HeaderValue,
    }

    #[cfg(feature = "transport-sockudo")]
    impl Callback for HeaderAssertCallback {
        #[expect(
            clippy::panic_in_result_fn,
            reason = "assertion failures should fail the test"
        )]
        fn on_request(
            self,
            request: &server::Request,
            response: server::Response,
        ) -> Result<server::Response, server::ErrorResponse> {
            assert_eq!(request.headers().get(&self.key), Some(&self.value));
            Ok(response)
        }
    }

    impl RecordingServer {
        async fn setup() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let messages = Arc::new(tokio::sync::Mutex::new(Vec::new()));
            let messages_clone = Arc::clone(&messages);

            let task = task::spawn(async move {
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut websocket = accept_async(stream).await.unwrap();
                    let messages = Arc::clone(&messages_clone);

                    task::spawn(async move {
                        while let Some(Ok(msg)) = websocket.next().await {
                            match msg {
                                WsMessage::Text(text) => {
                                    messages.lock().await.push(text.to_string());
                                }
                                WsMessage::Close(_) => {
                                    let _ = websocket.close(None).await;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    });
                }
            });

            Self {
                task,
                port,
                messages,
            }
        }

        async fn messages(&self) -> Vec<String> {
            self.messages.lock().await.clone()
        }
    }

    impl Drop for RecordingServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_then_disconnect() {
        // Bind an ephemeral port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Server task: accept one ws connection then close it
        let server = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = accept_async(stream).await.unwrap();
            drop(ws);
            // Keep alive briefly
            sleep(Duration::from_secs(1)).await;
        });

        // Build a channel-based message handler for incoming messages (unused here)
        let (handler, _rx) = channel_message_handler();

        // Configure client with short reconnect backoff
        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        // Connect the client
        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        // Allow server to drop connection and client to detect
        sleep(Duration::from_millis(100)).await;
        // Now immediately disconnect the client
        client.disconnect().await;
        assert!(client.is_disconnected());
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_state_flips_when_reader_stops() {
        // Bind an ephemeral port and accept a single websocket connection which we drop.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }
            sleep(Duration::from_millis(50)).await;
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if client.is_reconnecting() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("client did not enter RECONNECT state");

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_stream_mode_disables_auto_reconnect() {
        // Test that stream-based clients (created via connect_stream) set is_stream_mode flag
        // and that reconnect() transitions to CLOSED state for stream mode
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(_ws) = accept_async(stream).await
            {
                // Keep connection alive briefly
                sleep(Duration::from_millis(100)).await;
            }
        });

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let (_reader, _client) = WebSocketClient::connect_stream(config, vec![], None, None)
            .await
            .unwrap();

        // Note: We can't easily test the reconnect behavior from the outside since
        // the inner client is private. The key fix is that WebSocketClientInner
        // now has is_stream_mode=true for connect_stream, and reconnect() will
        // transition to CLOSED state instead of creating a new reader that gets dropped.
        // This is tested implicitly by the fact that stream users won't get stuck
        // in an infinite reconnect loop.

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_message_handler_mode_allows_auto_reconnect() {
        // Test that regular clients (with message handler) can auto-reconnect
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept first connection and close it
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }
            sleep(Duration::from_millis(50)).await;
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        // Wait for the connection to be dropped and reconnection to be attempted
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if client.is_reconnecting() || client.is_closed() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("client should attempt reconnection or close");

        // Should either be reconnecting or closed (depending on timing)
        // The important thing is it's not staying active forever
        assert!(
            client.is_reconnecting() || client.is_closed(),
            "Client with message handler should attempt reconnection"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_handler_mode_reconnect_with_new_connection() {
        // Test that handler mode successfully reconnects and messages continue flowing
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // First connection - accept and immediately close
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }

            // Small delay to let client detect disconnection
            sleep(Duration::from_millis(100)).await;

            // Second connection - accept, send a message, then keep alive
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                use futures_util::SinkExt;
                let _ = ws
                    .send(WsMessage::Text("reconnected".to_string().into()))
                    .await;
                sleep(Duration::from_secs(1)).await;
            }
        });

        let (handler, mut rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(200),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(10),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        // Wait for reconnection to happen and message to arrive
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(msg) = rx.try_recv()
                    && matches!(msg, WsMessage::Text(ref text) if AsRef::<str>::as_ref(text) == "reconnected")
                {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;

        assert!(
            result.is_ok(),
            "Should receive message after reconnection within timeout"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_stream_mode_no_auto_reconnect() {
        // Test that stream mode does not automatically reconnect when connection is lost
        // The caller owns the reader and is responsible for detecting disconnection
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept connection and send one message, then close
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                use futures_util::SinkExt;
                let _ = ws.send(WsMessage::Text("hello".to_string().into())).await;
                sleep(Duration::from_millis(50)).await;
                // Connection closes when ws is dropped
            }
        });

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let (mut reader, client) = WebSocketClient::connect_stream(config, vec![], None, None)
            .await
            .unwrap();

        // Initially active
        assert!(client.is_active(), "Client should start as active");

        // Read the hello message
        let msg = reader.next().await;
        assert!(
            matches!(&msg, Some(Ok(Message::Text(bytes))) if bytes.as_ref() == b"hello"),
            "Should receive initial message"
        );

        // Read until connection closes (reader will return None or error)
        while let Some(msg) = reader.next().await {
            if msg.is_err() || matches!(msg, Ok(Message::Close(_))) {
                break;
            }
        }

        // Controller cannot detect reader EOF (reader is owned by caller),
        // so the client stays ACTIVE until the caller signals.
        sleep(Duration::from_millis(200)).await;
        assert!(
            client.is_active(),
            "Stream mode client stays ACTIVE before notify_closed()"
        );

        // Caller signals EOF via notify_closed()
        client.notify_closed();

        assert!(
            client.is_closed(),
            "Stream mode client should be CLOSED after notify_closed()"
        );
        assert!(
            !client.is_reconnecting(),
            "Stream mode client should never attempt reconnection"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_send_timeout_uses_configured_reconnect_timeout() {
        // Test that send operations respect the configured reconnect_timeout.
        // When a client is stuck in RECONNECT longer than the timeout, sends should fail with Timeout.
        use nautilus_common::testing::wait_until_async;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept first connection and immediately close it
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }
            // Don't accept second connection - client will be stuck in RECONNECT
            sleep(Duration::from_mins(1)).await;
        });

        let (handler, _rx) = channel_message_handler();

        // Configure with SHORT 2s reconnect timeout
        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(2_000), // 2s timeout
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        // Wait for client to enter RECONNECT state
        wait_until_async(
            || async { client.is_reconnecting() },
            Duration::from_secs(3),
        )
        .await;

        // Attempt send while stuck in RECONNECT - should timeout after 2s (configured timeout)
        let start = std::time::Instant::now();
        let send_result = client.send_text("test".to_string(), None).await;
        let elapsed = start.elapsed();

        assert!(
            send_result.is_err(),
            "Send should fail when client stuck in RECONNECT"
        );
        assert!(
            matches!(send_result, Err(crate::error::SendError::Timeout)),
            "Send should return Timeout error, was: {send_result:?}"
        );
        // Verify timeout respects configured value (2s), but don't check upper bound
        // as CI scheduler jitter can cause legitimate delays beyond the timeout
        assert!(
            elapsed >= Duration::from_millis(1800),
            "Send should timeout after at least 2s (configured timeout), took {elapsed:?}"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_send_waits_during_reconnection() {
        // Test that send operations wait for reconnection to complete (up to timeout)
        use nautilus_common::testing::wait_until_async;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // First connection - accept and immediately close
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }

            // Wait a bit before accepting second connection
            sleep(Duration::from_millis(500)).await;

            // Second connection - accept and keep alive
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                // Echo messages
                while let Some(Ok(msg)) = ws.next().await {
                    if ws.send(msg).await.is_err() {
                        break;
                    }
                }
            }
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(5_000), // 5s timeout - enough for reconnect
            reconnect_delay_initial_ms: Some(100),
            reconnect_delay_max_ms: Some(200),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        // Wait for reconnection to trigger
        wait_until_async(
            || async { client.is_reconnecting() },
            Duration::from_secs(2),
        )
        .await;

        // Try to send while reconnecting - should wait and succeed after reconnect
        let send_result = tokio::time::timeout(
            Duration::from_secs(3),
            client.send_text("test_message".to_string(), None),
        )
        .await;

        assert!(
            send_result.is_ok() && send_result.unwrap().is_ok(),
            "Send should succeed after waiting for reconnection"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_rate_limiter_before_active_wait() {
        // Test that rate limiting happens BEFORE active state check.
        // This prevents race conditions where connection state changes during rate limit wait.
        // We verify this by: (1) exhausting rate limit, (2) ensuring client is RECONNECTING,
        // (3) sending again and confirming it waits for rate limit THEN reconnection.
        use std::{num::NonZeroU32, sync::Arc};

        use nautilus_common::testing::wait_until_async;

        use crate::ratelimiter::quota::Quota;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // First connection - accept and close after receiving one message
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                // Receive first message then close
                if let Some(Ok(_)) = ws.next().await {
                    drop(ws);
                }
            }

            // Wait before accepting reconnection
            sleep(Duration::from_millis(500)).await;

            // Second connection - accept and keep alive
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                while let Some(Ok(msg)) = ws.next().await {
                    if ws.send(msg).await.is_err() {
                        break;
                    }
                }
            }
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        // Very restrictive rate limit: 1 request per second, burst of 1
        let quota = Quota::per_second(NonZeroU32::new(1).unwrap())
            .unwrap()
            .allow_burst(NonZeroU32::new(1).unwrap());

        let client = Arc::new(
            WebSocketClient::connect(
                config,
                Some(handler),
                None,
                None,
                vec![("test_key".to_string(), quota)],
                None,
            )
            .await
            .unwrap(),
        );

        // First send exhausts burst capacity and triggers connection close
        let test_key: [Ustr; 1] = [Ustr::from("test_key")];
        client
            .send_text("msg1".to_string(), Some(test_key.as_slice()))
            .await
            .unwrap();

        // Wait for client to enter RECONNECT state
        wait_until_async(
            || async { client.is_reconnecting() },
            Duration::from_secs(2),
        )
        .await;

        // Second send: will hit rate limit (~1s) THEN wait for reconnection (~0.5s)
        let start = std::time::Instant::now();
        let send_result = client
            .send_text("msg2".to_string(), Some(test_key.as_slice()))
            .await;
        let elapsed = start.elapsed();

        // Should succeed after both rate limit AND reconnection
        assert!(
            send_result.is_ok(),
            "Send should succeed after rate limit + reconnection, was: {send_result:?}"
        );
        // Total wait should be at least rate limit time (~1s)
        // The reconnection completes while rate limiting or after
        // Use 850ms threshold to account for timing jitter in CI
        assert!(
            elapsed >= Duration::from_millis(850),
            "Should wait for rate limit (~1s), waited {elapsed:?}"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_disconnect_during_reconnect_exits_cleanly() {
        // Test CAS race condition: disconnect called during reconnection
        // Should exit cleanly without spawning new tasks
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept first connection and immediately close
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }
            // Don't accept second connection - let reconnect hang
            sleep(Duration::from_mins(1)).await;
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(2_000), // 2s timeout - shorter than disconnect timeout
            reconnect_delay_initial_ms: Some(100),
            reconnect_delay_max_ms: Some(200),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        // Wait for reconnection to start
        tokio::time::timeout(Duration::from_secs(2), async {
            while !client.is_reconnecting() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Client should enter RECONNECT state");

        // Disconnect while reconnecting
        client.disconnect().await;

        // Should be cleanly closed
        assert!(
            client.is_disconnected(),
            "Client should be cleanly disconnected"
        );

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_send_fails_fast_when_closed_before_rate_limit() {
        // Test that send operations check connection state BEFORE rate limiting,
        // preventing unnecessary delays when the connection is already closed.
        use std::{num::NonZeroU32, sync::Arc};

        use nautilus_common::testing::wait_until_async;

        use crate::ratelimiter::quota::Quota;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept connection and immediately close
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }
            sleep(Duration::from_mins(1)).await;
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        // Very restrictive rate limit: 1 request per 10 seconds
        // This ensures that if we wait for rate limit, the test will timeout
        let quota = Quota::with_period(Duration::from_secs(10))
            .unwrap()
            .allow_burst(NonZeroU32::new(1).unwrap());

        let client = Arc::new(
            WebSocketClient::connect(
                config,
                Some(handler),
                None,
                None,
                vec![("test_key".to_string(), quota)],
                None,
            )
            .await
            .unwrap(),
        );

        // Wait for disconnection
        wait_until_async(
            || async { client.is_reconnecting() || client.is_closed() },
            Duration::from_secs(2),
        )
        .await;

        // Explicitly disconnect to move away from ACTIVE state
        client.disconnect().await;
        assert!(
            !client.is_active(),
            "Client should not be active after disconnect"
        );

        // Attempt send - should fail IMMEDIATELY without waiting for rate limit
        let start = std::time::Instant::now();
        let test_key: [Ustr; 1] = [Ustr::from("test_key")];
        let result = client
            .send_text("test".to_string(), Some(test_key.as_slice()))
            .await;
        let elapsed = start.elapsed();

        // Should fail with Closed error
        assert!(result.is_err(), "Send should fail when client is closed");
        assert!(
            matches!(result, Err(crate::error::SendError::Closed)),
            "Send should return Closed error, was: {result:?}"
        );

        // Should fail FAST (< 100ms) without waiting for rate limit (10s)
        assert!(
            elapsed < Duration::from_millis(100),
            "Send should fail fast without rate limiting, took {elapsed:?}"
        );

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_connect_rejects_none_message_handler() {
        // Test that connect() properly rejects None message_handler
        // to prevent zombie connections that appear alive but never detect disconnections

        let config = WebSocketConfig {
            url: "ws://127.0.0.1:9999".to_string(),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(100),
            reconnect_delay_max_ms: Some(500),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        // Pass None for message_handler - should be rejected
        let result = WebSocketClient::connect(config, None, None, None, vec![], None).await;

        assert!(
            result.is_err(),
            "connect() should reject None message_handler"
        );

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("Handler mode requires message_handler"),
            "Error should mention missing message_handler, was: {err_msg}"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_client_without_handler_sets_stream_mode() {
        // Test that if a client is created without a handler via connect_url,
        // it properly sets is_stream_mode=true to prevent zombie connections

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept and immediately close to simulate server disconnect
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws); // Drop connection immediately
            }
        });

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(100),
            reconnect_delay_max_ms: Some(500),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        // Create client directly via connect_url with no handler (stream mode)
        let inner = WebSocketClientInner::connect_url(config, None, None)
            .await
            .unwrap();

        // Verify is_stream_mode is true when no handler
        assert!(
            inner.is_stream_mode,
            "Client without handler should have is_stream_mode=true"
        );

        // Verify that when stream mode is enabled, reconnection is disabled
        // (documented behavior - stream mode clients close instead of reconnecting)

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_idle_timeout_triggers_reconnect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Server accepts WS connection but sends nothing (simulates silent death)
        let server = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _ws = accept_async(stream).await.unwrap();
            // Hold connection open but send nothing
            sleep(Duration::from_secs(5)).await;
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: Some(1),
            idle_timeout_ms: Some(500),
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        assert!(client.is_active());

        // Wait for idle timeout to fire and client to enter reconnect/closed
        wait_until_async(
            || async { client.is_reconnecting() || client.is_disconnected() },
            Duration::from_secs(3),
        )
        .await;

        assert!(
            !client.is_active(),
            "Client should not be active after idle timeout"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_idle_timeout_resets_on_data() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Server sends a message every 200ms (well within 1s idle timeout)
        let server = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            for _ in 0..10 {
                sleep(Duration::from_millis(200)).await;

                if ws.send(WsMessage::Text("ping".into())).await.is_err() {
                    break;
                }
            }
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: Some(1),
            idle_timeout_ms: Some(1_000),
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        assert!(client.is_active());

        // Wait 1.5s - data arrives every 200ms so idle timeout (1s) should NOT fire
        sleep(Duration::from_millis(1_500)).await;

        assert!(
            client.is_active(),
            "Client should remain active when data is flowing"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_idle_timeout_fires_when_only_pings_received() {
        // Regression: pings and pongs are keep-alive frames, not application data,
        // so a peer that only emits control frames must still trip the idle timeout.
        // The peer keeps pinging for well past the observation window so the
        // pre-fix behavior (reset-on-ping) would keep the client active; under the
        // fix the idle timer never resets and fires after ~500ms.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            for _ in 0..60 {
                sleep(Duration::from_millis(100)).await;

                if ws.send(WsMessage::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: Some(1),
            idle_timeout_ms: Some(500),
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        assert!(client.is_active());

        // Observation window is shorter than the ping stream (6s). If the idle
        // timer mistakenly reset on every ping the client would still be active
        // here; under the fix it goes inactive at ~500ms.
        wait_until_async(
            || async { client.is_reconnecting() || client.is_disconnected() },
            Duration::from_millis(1_500),
        )
        .await;

        assert!(
            !client.is_active(),
            "Client should not be active after idle timeout when only pings/pongs flow"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_idle_timeout_fires_when_only_pongs_received() {
        // Regression for the heartbeat-reply path. When the client heartbeat is
        // enabled, the peer auto-replies with pongs for every outgoing ping. If
        // those pongs refreshed last_data_time the idle timer would never fire on
        // a zombie connection (the motivating Polymarket scenario).
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            // Drain incoming frames so tungstenite's internal pong replies are
            // actually flushed to the client. Hold the connection open well past
            // the observation window.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
            while tokio::time::Instant::now() < deadline {
                if let Ok(Some(Err(_)) | None) =
                    tokio::time::timeout(Duration::from_millis(100), ws.next()).await
                {
                    break;
                }
            }
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: Some(1),
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: Some(1),
            idle_timeout_ms: Some(1_500),
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        assert!(client.is_active());

        // Heartbeat cadence is 1s; each ping draws a pong reply. Under the fix
        // the idle timer ignores those pongs and fires at ~1.5s. Under the bug
        // every pong reset the timer and the client would stay active.
        wait_until_async(
            || async { client.is_reconnecting() || client.is_disconnected() },
            Duration::from_millis(2_500),
        )
        .await;

        assert!(
            !client.is_active(),
            "Client should not be active after idle timeout when only pongs flow"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_disconnect_during_backoff_exits_promptly() {
        // Verify that disconnect interrupts backoff sleep (Finding 1).
        // Server accepts then drops, no second listener -> reconnect fails -> enters backoff.
        // We disconnect while backing off and assert the client shuts down quickly.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept first connection, close immediately
            if let Ok((stream, _)) = listener.accept().await {
                let _ = accept_async(stream).await;
            }
            // Don't accept again so reconnect fails and enters backoff
            sleep(Duration::from_mins(1)).await;
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(10_000), // 10s backoff to ensure we're sleeping
            reconnect_delay_max_ms: Some(10_000),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .unwrap();

        // Wait for client to enter reconnect
        wait_until_async(
            || async { client.is_reconnecting() },
            Duration::from_secs(3),
        )
        .await;

        // Wait a bit more for the reconnect attempt to fail and enter backoff sleep
        sleep(Duration::from_millis(1_500)).await;

        // Disconnect while backing off
        let start = std::time::Instant::now();
        client.disconnect().await;
        let elapsed = start.elapsed();

        assert!(client.is_disconnected(), "Client should be disconnected");
        // Should exit well before the 10s backoff sleep completes
        assert!(
            elapsed < Duration::from_secs(2),
            "Disconnect should interrupt backoff sleep, took {elapsed:?}"
        );

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_rate_limit_cancelled_on_disconnect() {
        // Verify that a send blocked on rate limiting returns Closed when
        // the client disconnects (Finding 6).
        use std::{num::NonZeroU32, sync::Arc};

        use crate::ratelimiter::quota::Quota;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let mut ws = accept_async(stream).await.unwrap();
                // Keep alive and echo
                while let Some(Ok(msg)) = ws.next().await {
                    if ws.send(msg).await.is_err() {
                        break;
                    }
                }
            }
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(100),
            reconnect_delay_max_ms: Some(500),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        // Very restrictive: 1 req per 60 seconds
        let quota = Quota::with_period(Duration::from_mins(1))
            .unwrap()
            .allow_burst(NonZeroU32::new(1).unwrap());

        let client = Arc::new(
            WebSocketClient::connect(
                config,
                Some(handler),
                None,
                None,
                vec![("rate_key".to_string(), quota)],
                None,
            )
            .await
            .unwrap(),
        );

        let test_key: [Ustr; 1] = [Ustr::from("rate_key")];

        // Exhaust the burst quota
        client
            .send_text("exhaust".to_string(), Some(test_key.as_slice()))
            .await
            .unwrap();

        // Spawn a send that will block on rate limiter
        let client_clone = client.clone();
        let send_handle = task::spawn(async move {
            client_clone
                .send_text("blocked".to_string(), Some(&[Ustr::from("rate_key")]))
                .await
        });

        // Let the send block on rate limit
        sleep(Duration::from_millis(200)).await;

        // Disconnect while send is blocked
        let start = std::time::Instant::now();
        client.disconnect().await;
        let elapsed_disconnect = start.elapsed();

        // The blocked send should return Closed
        let result = tokio::time::timeout(Duration::from_secs(2), send_handle)
            .await
            .expect("Send task should complete quickly")
            .expect("Send task should not panic");

        assert!(
            matches!(result, Err(crate::error::SendError::Closed)),
            "Blocked send should return Closed, was: {result:?}"
        );

        // Disconnect should be fast, not waiting for the 60s rate limit
        assert!(
            elapsed_disconnect < Duration::from_secs(3),
            "Disconnect should not wait for rate limiter, took {elapsed_disconnect:?}"
        );

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_stream_mode_transitions_to_closed_on_dead_write_task() {
        // Verify that stream mode transitions to CLOSED (not RECONNECT) when
        // the write task dies (Finding 4). We force write failure by sending
        // after the server closes the connection.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                // Close immediately to cause write errors
                drop(ws);
            }
        });

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let (_reader, client) = WebSocketClient::connect_stream(config, vec![], None, None)
            .await
            .unwrap();

        assert!(client.is_active(), "Client should start active");

        // Wait for server to close, then send to trigger write task failure
        sleep(Duration::from_millis(100)).await;

        // Keep sending until the write task detects the broken connection
        for _ in 0..20 {
            let _ = client.send_text("ping".to_string(), None).await;
            sleep(Duration::from_millis(50)).await;

            if !client.is_active() {
                break;
            }
        }

        // Wait for controller to process the state change
        wait_until_async(|| async { !client.is_active() }, Duration::from_secs(5)).await;

        // Stream mode should go to CLOSED, not RECONNECT
        assert!(
            client.is_closed() || client.is_disconnected(),
            "Stream mode should transition to CLOSED, not RECONNECT. \
             is_reconnecting={}, is_closed={}, is_disconnected={}",
            client.is_reconnecting(),
            client.is_closed(),
            client.is_disconnected(),
        );
        assert!(
            !client.is_reconnecting(),
            "Stream mode should never attempt reconnection"
        );

        server.abort();
    }

    #[tokio::test]
    async fn test_write_task_waits_for_auth_before_replaying_buffer() {
        use nautilus_common::testing::wait_until_async;

        let server = RecordingServer::setup().await;
        let url = format!("ws://127.0.0.1:{}", server.port);
        let (writer, _reader) = WebSocketClientInner::connect_with_server(
            &url,
            vec![],
            TransportBackend::Tungstenite,
            None,
        )
        .await
        .unwrap();

        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Reconnect.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let auth_tracker = Arc::new(OnceLock::new());
        let reconnect_buffer_waits_for_auth = Arc::new(AtomicBool::new(true));
        let tracker = AuthTracker::new();
        auth_tracker.set(tracker.clone()).unwrap();

        let (writer_tx, writer_rx) = tokio::sync::mpsc::unbounded_channel();
        let write_task = WebSocketClientInner::spawn_write_task(
            Arc::clone(&connection_state),
            Arc::clone(&state_notify),
            writer,
            writer_rx,
            Arc::clone(&auth_tracker),
            Arc::clone(&reconnect_buffer_waits_for_auth),
        );

        writer_tx
            .send(WriterCommand::Send(Message::Text("stale".into())))
            .unwrap();

        let (new_writer, _reader) = WebSocketClientInner::connect_with_server(
            &url,
            vec![],
            TransportBackend::Tungstenite,
            None,
        )
        .await
        .unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        writer_tx
            .send(WriterCommand::Update(new_writer, tx))
            .unwrap();
        assert!(rx.await.unwrap());

        connection_state.store(ConnectionMode::Active.as_u8(), Ordering::SeqCst);

        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            server.messages().await.is_empty(),
            "buffered messages should wait for re-authentication"
        );

        tracker.succeed();

        wait_until_async(
            || {
                let messages = Arc::clone(&server.messages);
                async move { !messages.lock().await.is_empty() }
            },
            Duration::from_secs(3),
        )
        .await;

        assert_eq!(server.messages().await, vec!["stale".to_string()]);

        connection_state.store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
        state_notify.notify_waiters();
        drop(writer_tx);
        write_task.abort();
    }

    #[tokio::test]
    async fn test_write_task_discards_buffer_after_auth_failure() {
        let server = RecordingServer::setup().await;
        let url = format!("ws://127.0.0.1:{}", server.port);
        let (writer, _reader) = WebSocketClientInner::connect_with_server(
            &url,
            vec![],
            TransportBackend::Tungstenite,
            None,
        )
        .await
        .unwrap();

        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Reconnect.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let auth_tracker = Arc::new(OnceLock::new());
        let reconnect_buffer_waits_for_auth = Arc::new(AtomicBool::new(true));
        let tracker = AuthTracker::new();
        auth_tracker.set(tracker.clone()).unwrap();

        let (writer_tx, writer_rx) = tokio::sync::mpsc::unbounded_channel();
        let write_task = WebSocketClientInner::spawn_write_task(
            Arc::clone(&connection_state),
            Arc::clone(&state_notify),
            writer,
            writer_rx,
            Arc::clone(&auth_tracker),
            Arc::clone(&reconnect_buffer_waits_for_auth),
        );

        writer_tx
            .send(WriterCommand::Send(Message::Text("stale".into())))
            .unwrap();

        let (new_writer, _reader) = WebSocketClientInner::connect_with_server(
            &url,
            vec![],
            TransportBackend::Tungstenite,
            None,
        )
        .await
        .unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        writer_tx
            .send(WriterCommand::Update(new_writer, tx))
            .unwrap();
        assert!(rx.await.unwrap());

        connection_state.store(ConnectionMode::Active.as_u8(), Ordering::SeqCst);
        tracker.fail("rejected");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            server.messages().await.is_empty(),
            "buffered messages should be discarded after authentication failure"
        );

        let _auth_receiver = tracker.begin();
        tracker.succeed();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            server.messages().await.is_empty(),
            "discarded buffered messages should not replay on a later auth success"
        );

        connection_state.store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
        state_notify.notify_waiters();
        drop(writer_tx);
        write_task.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_zero_idle_timeout_rejected() {
        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: "ws://127.0.0.1:9999".to_string(),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: Some(0),
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        };

        let result =
            WebSocketClient::connect(config, Some(handler), None, None, vec![], None).await;

        assert!(result.is_err(), "Zero idle timeout should be rejected");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Idle timeout cannot be zero"),
            "Error should mention zero idle timeout, was: {err_msg}"
        );
    }

    #[cfg(all(feature = "transport-sockudo", not(feature = "turmoil")))]
    #[rstest]
    #[tokio::test]
    async fn test_sockudo_backend_rejects_reserved_headers_before_connect() {
        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: "ws://127.0.0.1:1".to_string(),
            headers: vec![("Host".to_string(), "example.com".to_string())],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Sockudo,
            proxy_url: None,
        };

        let err = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .expect_err("reserved header should fail before TCP connect");

        assert!(
            err.to_string()
                .contains("reserved upgrade header not allowed in extra_headers"),
            "expected reserved-header failure, was: {err}"
        );
    }

    #[cfg(all(feature = "transport-sockudo", not(feature = "turmoil")))]
    #[rstest]
    #[tokio::test]
    async fn test_sockudo_backend_replays_leftover_without_custom_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let request = read_http_request(&mut stream).await;
                let request = String::from_utf8(request).unwrap();
                let sec_websocket_key = extract_header(&request, "Sec-WebSocket-Key").unwrap();
                let accept = sockudo_handshake::generate_accept_key(sec_websocket_key);
                let mut response = format!(
                    concat!(
                        "HTTP/1.1 101 Switching Protocols\r\n",
                        "Upgrade: websocket\r\n",
                        "Connection: Upgrade\r\n",
                        "Sec-WebSocket-Accept: {}\r\n",
                        "\r\n",
                    ),
                    accept
                )
                .into_bytes();
                response.extend_from_slice(b"\x81\x05hello");
                stream.write_all(&response).await.unwrap();
            }
        });

        let (handler, mut rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}/ws"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Sockudo,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .expect("sockudo connect without custom headers");

        let received = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(msg) = rx.try_recv() {
                    return msg;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("did not receive leftover frame before timeout");

        match received {
            WsMessage::Text(t) => assert_eq!(t.as_str(), "hello"),
            other => panic!("expected text, was {other:?}"),
        }

        client.disconnect().await;
        tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .expect("server did not close before timeout")
            .unwrap();
    }

    #[cfg(all(feature = "transport-sockudo", not(feature = "turmoil")))]
    #[rstest]
    #[tokio::test]
    async fn test_sockudo_backend_sends_custom_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let callback = HeaderAssertCallback {
                    key: "X-Test".to_string(),
                    value: HeaderValue::from_static("value"),
                };

                if let Ok(mut ws) = accept_hdr_async(stream, callback).await {
                    while let Some(Ok(msg)) = ws.next().await {
                        if msg.is_text() || msg.is_binary() {
                            if ws.send(msg).await.is_err() {
                                break;
                            }

                            continue;
                        }

                        if msg.is_close() {
                            let _ = ws.close(None).await;
                            break;
                        }
                    }
                }
            }
        });

        let (handler, mut rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![("X-Test".to_string(), "value".to_string())],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Sockudo,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .expect("sockudo connect with custom headers");

        client.send_text("ping".to_string(), None).await.unwrap();

        let received = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(msg) = rx.try_recv() {
                    return msg;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("did not receive echo before timeout");

        match received {
            WsMessage::Text(t) => assert_eq!(t.as_str(), "ping"),
            other => panic!("expected text, was {other:?}"),
        }

        client.disconnect().await;
        tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .expect("server did not close before timeout")
            .unwrap();
    }

    #[cfg(all(feature = "transport-sockudo", not(feature = "turmoil")))]
    #[rstest]
    #[tokio::test]
    async fn test_sockudo_backend_round_trip_text() {
        // tokio-tungstenite test peer paired with a sockudo client.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                while let Some(Ok(msg)) = ws.next().await {
                    // Inner if consumes `msg`, cannot hoist into a match guard
                    #[expect(clippy::collapsible_match)]
                    match msg {
                        WsMessage::Text(_) | WsMessage::Binary(_) => {
                            if ws.send(msg).await.is_err() {
                                break;
                            }
                        }
                        WsMessage::Close(_) => {
                            let _ = ws.close(None).await;
                            break;
                        }
                        _ => {}
                    }
                }
            }
        });

        let (handler, mut rx) = channel_message_handler();
        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Sockudo,
            proxy_url: None,
        };

        let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
            .await
            .expect("sockudo connect");

        client.send_text("ping".to_string(), None).await.unwrap();

        let received = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(msg) = rx.try_recv() {
                    return msg;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("did not receive echo before timeout");

        match received {
            WsMessage::Text(t) => assert_eq!(t.as_str(), "ping"),
            other => panic!("expected text, was {other:?}"),
        }

        client.disconnect().await;
        server.abort();
    }

    #[cfg(all(feature = "transport-sockudo", not(feature = "turmoil")))]
    #[rstest]
    #[case::ws_default_port("ws://example.com/ws", "example.com", "example.com", 80, "/ws", false)]
    #[case::wss_default_port(
        "wss://example.com/ws",
        "example.com",
        "example.com",
        443,
        "/ws",
        true
    )]
    // url::Url normalises explicit default ports (`:80` for ws, `:443` for wss)
    // away, so `parsed.port()` reports `None` here and Host stays unqualified.
    #[case::ws_explicit_default(
        "ws://example.com:80/ws",
        "example.com",
        "example.com",
        80,
        "/ws",
        false
    )]
    #[case::ws_non_default(
        "ws://example.com:8443/feed",
        "example.com",
        "example.com:8443",
        8443,
        "/feed",
        false
    )]
    #[case::wss_non_default(
        "wss://example.com:9443/feed",
        "example.com",
        "example.com:9443",
        9443,
        "/feed",
        true
    )]
    #[case::root_path(
        "ws://example.com:9000/",
        "example.com",
        "example.com:9000",
        9000,
        "/",
        false
    )]
    #[case::query_string(
        "ws://example.com/feed?token=abc&channel=trades",
        "example.com",
        "example.com",
        80,
        "/feed?token=abc&channel=trades",
        false
    )]
    // IPv6: bare host strips brackets for DNS/TCP/SNI; Host header keeps them.
    #[case::ipv6_default("ws://[::1]/feed", "::1", "[::1]", 80, "/feed", false)]
    #[case::ipv6_explicit_port("ws://[::1]:9000/feed", "::1", "[::1]:9000", 9000, "/feed", false)]
    #[case::ipv6_wss(
        "wss://[2001:db8::1]:8443/",
        "2001:db8::1",
        "[2001:db8::1]:8443",
        8443,
        "/",
        true
    )]
    fn sockudo_target_parses_url(
        #[case] url: &str,
        #[case] host: &str,
        #[case] host_header: &str,
        #[case] port: u16,
        #[case] path: &str,
        #[case] is_tls: bool,
    ) {
        let target = super::SockudoTarget::parse(url).expect("parse should succeed");
        assert_eq!(target.host, host);
        assert_eq!(target.host_header, host_header);
        assert_eq!(target.port, port);
        assert_eq!(target.path, path);
        assert_eq!(target.is_tls, is_tls);
    }

    #[cfg(all(feature = "transport-sockudo", not(feature = "turmoil")))]
    #[rstest]
    fn sockudo_target_rejects_unsupported_scheme() {
        let err = super::SockudoTarget::parse("http://example.com/feed").expect_err("not a ws URL");
        let msg = err.to_string();
        assert!(
            msg.contains("expected ws:// or wss://"),
            "unexpected error: {msg}"
        );
    }

    #[cfg(all(feature = "transport-sockudo", not(feature = "turmoil")))]
    #[rstest]
    fn sockudo_target_rejects_malformed_url() {
        let err = super::SockudoTarget::parse("not a url").expect_err("malformed URL");
        assert!(
            matches!(err, super::TransportError::InvalidUrl(_)),
            "expected InvalidUrl, was: {err:?}"
        );
    }
}

#[cfg(test)]
#[cfg(feature = "turmoil")]
mod turmoil_tests {
    use std::{sync::Arc, time::Duration};

    use futures_util::{SinkExt, StreamExt};
    use nautilus_common::testing::wait_until_async;
    use rstest::rstest;
    use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};
    use turmoil::{Builder, net};

    use super::*;
    use crate::websocket::types::channel_message_handler;

    #[rstest]
    fn test_turmoil_reconnect_buffer_waits_for_auth() {
        let mut sim = Builder::new().build();
        let messages = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let server_messages = Arc::clone(&messages);

        sim.host("server", move || {
            let messages = Arc::clone(&server_messages);
            auth_buffer_server(messages)
        });

        sim.client("client", async move {
            let tracker = AuthTracker::new();
            let (handler, _rx) = channel_message_handler();
            let client = WebSocketClient::connect(
                turmoil_websocket_config(),
                Some(handler),
                None,
                None,
                vec![],
                None,
            )
            .await
            .expect("Should connect");

            client.set_auth_tracker(tracker.clone(), true);
            assert!(client.is_active(), "Client should start active");

            wait_until_async(
                || async { client.is_reconnecting() },
                Duration::from_secs(3),
            )
            .await;

            client
                .writer_tx
                .send(WriterCommand::Send(Message::Text("stale".into())))
                .unwrap();

            wait_until_async(|| async { client.is_active() }, Duration::from_secs(3)).await;

            let _auth_receiver = tracker.begin();

            tokio::time::sleep(Duration::from_millis(300)).await;
            assert!(
                messages.lock().await.is_empty(),
                "buffered messages should wait for auth after reconnect"
            );

            tracker.succeed();

            wait_until_async(
                || {
                    let messages = Arc::clone(&messages);
                    async move { messages.lock().await.as_slice() == ["stale"] }
                },
                Duration::from_secs(3),
            )
            .await;

            assert_eq!(messages.lock().await.as_slice(), ["stale"]);

            client.disconnect().await;
            assert!(client.is_disconnected());

            Ok(())
        });

        sim.run().unwrap();
    }

    #[rstest]
    fn test_turmoil_reconnect_buffer_discards_after_auth_failure() {
        let mut sim = Builder::new().build();
        let messages = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let server_messages = Arc::clone(&messages);

        sim.host("server", move || {
            let messages = Arc::clone(&server_messages);
            auth_buffer_server(messages)
        });

        sim.client("client", async move {
            let tracker = AuthTracker::new();
            let (handler, _rx) = channel_message_handler();
            let client = WebSocketClient::connect(
                turmoil_websocket_config(),
                Some(handler),
                None,
                None,
                vec![],
                None,
            )
            .await
            .expect("Should connect");

            client.set_auth_tracker(tracker.clone(), true);
            assert!(client.is_active(), "Client should start active");

            wait_until_async(
                || async { client.is_reconnecting() },
                Duration::from_secs(3),
            )
            .await;

            client
                .writer_tx
                .send(WriterCommand::Send(Message::Text("stale".into())))
                .unwrap();

            wait_until_async(|| async { client.is_active() }, Duration::from_secs(3)).await;

            let _auth_receiver = tracker.begin();
            tracker.fail("rejected");

            tokio::time::sleep(Duration::from_millis(300)).await;
            assert!(
                messages.lock().await.is_empty(),
                "buffered messages should be discarded after auth failure"
            );

            let _retry_auth_receiver = tracker.begin();
            tracker.succeed();

            tokio::time::sleep(Duration::from_millis(300)).await;
            assert!(
                messages.lock().await.is_empty(),
                "discarded messages should not replay on a later auth success"
            );

            client.disconnect().await;
            assert!(client.is_disconnected());

            Ok(())
        });

        sim.run().unwrap();
    }

    fn turmoil_websocket_config() -> WebSocketConfig {
        WebSocketConfig {
            url: "ws://server:8080".to_string(),
            headers: vec![],
            heartbeat: None,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(200),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: TransportBackend::Tungstenite,
            proxy_url: None,
        }
    }

    async fn auth_buffer_server(
        messages: Arc<tokio::sync::Mutex<Vec<String>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

        let (stream, _) = listener.accept().await?;
        let mut websocket = accept_async(stream).await?;
        let _ = websocket.send(WsMessage::Text("first".into())).await;
        drop(websocket);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let (stream, _) = listener.accept().await?;
        let mut websocket = accept_async(stream).await?;

        while let Some(msg) = websocket.next().await {
            match msg {
                Ok(WsMessage::Text(text)) => {
                    messages.lock().await.push(text.to_string());
                }
                Ok(WsMessage::Close(_)) => {
                    let _ = websocket.close(None).await;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        Ok(())
    }
}
