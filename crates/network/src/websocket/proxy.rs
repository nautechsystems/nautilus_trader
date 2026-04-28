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

//! Proxy support for outbound WebSocket connections.
//!
//! Implements HTTP `CONNECT` tunneling so a `WebSocketClient` can be reached
//! through an HTTP or HTTPS forward proxy. The same `proxy_url` field is used
//! by the HTTP client (via `reqwest::Proxy::all`), keeping a single config
//! field for both transports.
//!
//! `socks5://` / `socks5h://` URLs are recognized but not yet implemented
//! for the WebSocket path. The dispatcher logs a warning and falls back to
//! a direct connection so that REST configs that already point at a SOCKS
//! proxy keep working unchanged. SOCKS support requires the optional
//! `tokio-socks` crate, which is not yet a workspace dependency.
//!
//! The tunnel is established as follows:
//! 1. TCP connect to the proxy host / port.
//! 2. If the proxy URL scheme is `https`, layer TLS using the proxy host as
//!    the SNI and certificate domain.
//! 3. Send `CONNECT target_host:target_port HTTP/1.1` plus the matching
//!    `Host:` header (and optional `Proxy-Authorization:` derived from the
//!    proxy URL user-info).
//! 4. Read the response line and headers; require a `2xx` status.
//! 5. If the upstream WebSocket scheme is `wss`, layer a second TLS session
//!    using the upstream host name.
//! 6. Hand the resulting stream to `tokio-tungstenite`'s `client_async` so the
//!    WebSocket handshake completes over the tunnel.

use std::fmt::Write as _;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_rustls::{TlsConnector, client::TlsStream};
use url::Url;

use crate::{net::TcpStream, transport::TransportError};

/// Maximum size of a `CONNECT` proxy response we are willing to read.
///
/// Bounds the buffer so a malicious or broken proxy cannot make us allocate
/// indefinitely while we wait for the header terminator.
const MAX_PROXY_RESPONSE_BYTES: usize = 16 * 1024;

/// Stream produced by `tunnel_via_proxy` when the upstream is `ws://`
/// (no upstream TLS, but the proxy hop itself may have been TLS-protected).
///
/// The TLS-bearing variants are boxed because [`tokio_rustls::client::TlsStream`]
/// is large enough that a flat enum trips `clippy::large_enum_variant`. Boxing
/// keeps the discriminant cheap to move while leaving the rare TLS path on the
/// heap.
#[derive(Debug)]
pub enum ProxiedStream {
    /// Plain TCP after a plain proxy hop.
    Plain(TcpStream),
    /// Plain TCP after a TLS proxy hop.
    PlainOverTlsProxy(Box<TlsStream<TcpStream>>),
    /// Upstream TLS over a plain proxy hop.
    Tls(Box<TlsStream<TcpStream>>),
    /// Upstream TLS over a TLS proxy hop.
    TlsOverTlsProxy(Box<TlsStream<TlsStream<TcpStream>>>),
}

/// Parsed components of a target WebSocket URL needed by the proxy hop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsTarget {
    /// Host name for DNS / SNI / `CONNECT` request line.
    pub host: String,
    /// TCP port of the WebSocket origin.
    pub port: u16,
    /// `true` when the WebSocket scheme is `wss://`.
    pub is_tls: bool,
}

impl WsTarget {
    /// Parse a `ws://` or `wss://` URL into the host/port/TLS components.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::InvalidUrl`] when the URL fails to parse,
    /// is missing a hostname, or uses a scheme other than `ws`/`wss`.
    pub fn parse(url: &str) -> Result<Self, TransportError> {
        let parsed =
            Url::parse(url).map_err(|e| TransportError::InvalidUrl(format!("{url}: {e}")))?;

        let is_tls = match parsed.scheme() {
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

        // url::Url stores IPv6 literals in bracketed form (`[::1]`); the
        // `CONNECT` request line and TLS SNI both want the unbracketed form.
        let host = if raw_host.starts_with('[') && raw_host.ends_with(']') {
            raw_host[1..raw_host.len() - 1].to_string()
        } else {
            raw_host.to_string()
        };

        let port = parsed.port().unwrap_or(if is_tls { 443 } else { 80 });

        Ok(Self { host, port, is_tls })
    }
}

/// Outcome of parsing a proxy URL prior to opening a tunnel.
///
/// SOCKS schemes are recognized but not implemented for the WebSocket path
/// yet. They are surfaced as [`ProxyKind::Unsupported`] so callers can log
/// a warning and fall back to a direct connection, preserving compatibility
/// with REST configs that already pointed at a SOCKS proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyKind {
    /// HTTP / HTTPS forward proxy reachable via `CONNECT` tunneling.
    Http(ProxyTarget),
    /// Recognized scheme without a working tunnel (currently SOCKS).
    Unsupported {
        /// Original URL scheme (e.g. `socks5`).
        scheme: String,
    },
}

impl ProxyKind {
    /// Parse a proxy URL into a [`ProxyKind`]. Returns
    /// [`TransportError::InvalidUrl`] for malformed input or non-proxy
    /// schemes (`ftp://`, `ws://`, etc.).
    ///
    /// # Errors
    ///
    /// See [`ProxyTarget::parse`] for the underlying validation.
    pub fn parse(url: &str) -> Result<Self, TransportError> {
        let parsed =
            Url::parse(url).map_err(|e| TransportError::InvalidUrl(format!("{url}: {e}")))?;

        match parsed.scheme() {
            "http" | "https" => ProxyTarget::parse(url).map(ProxyKind::Http),
            scheme @ ("socks5" | "socks5h" | "socks4" | "socks4a") => {
                // Reject malformed inputs like `socks5:host:port` that parse as
                // scheme + opaque path with no authority: surfacing them as
                // Unsupported would silently fall back to a direct connection
                // and hide the typo.
                if parsed.host_str().is_none_or(str::is_empty) {
                    return Err(TransportError::InvalidUrl(format!(
                        "proxy URL '{url}' is missing a host (did you mean {scheme}://...)?"
                    )));
                }
                Ok(Self::Unsupported {
                    scheme: scheme.to_string(),
                })
            }
            other => Err(TransportError::InvalidUrl(format!(
                "unsupported proxy scheme '{other}'; expected http:// or https://"
            ))),
        }
    }
}

/// Parsed components of a forward proxy URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyTarget {
    /// Host name of the proxy (used for both DNS and TLS SNI when
    /// [`ProxyTarget::is_tls`] is `true`).
    pub host: String,
    /// TCP port of the proxy.
    pub port: u16,
    /// `true` when the proxy URL scheme is `https`.
    pub is_tls: bool,
    /// Pre-computed `Proxy-Authorization` header value, if the URL embeds
    /// `user:pass@`.
    pub auth_header: Option<String>,
}

impl ProxyTarget {
    /// Parse a proxy URL into the components needed to establish the tunnel.
    ///
    /// Only `http://` and `https://` schemes are accepted here. Use
    /// [`ProxyKind::parse`] when callers need to distinguish recognised but
    /// unsupported schemes (currently SOCKS) from malformed input.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::InvalidUrl`] for malformed URLs, missing
    /// hosts, or any scheme other than `http`/`https`.
    pub fn parse(url: &str) -> Result<Self, TransportError> {
        let parsed =
            Url::parse(url).map_err(|e| TransportError::InvalidUrl(format!("{url}: {e}")))?;

        let is_tls = match parsed.scheme() {
            "http" => false,
            "https" => true,
            "socks5" | "socks5h" | "socks4" | "socks4a" => {
                return Err(TransportError::InvalidUrl(format!(
                    "SOCKS proxy scheme '{}' is not yet supported for WebSocket connections; \
                    use an http:// or https:// proxy",
                    parsed.scheme()
                )));
            }
            other => {
                return Err(TransportError::InvalidUrl(format!(
                    "unsupported proxy scheme '{other}'; expected http:// or https://"
                )));
            }
        };

        let raw_host = parsed
            .host_str()
            .ok_or_else(|| TransportError::InvalidUrl("proxy URL missing hostname".to_string()))?;

        // url::Url stores IPv6 literals bracketed (`[::1]`); the bracketed
        // form is only valid in the HTTP `Host:` header, not for DNS or
        // TLS SNI, so we keep both representations.
        let host = if raw_host.starts_with('[') && raw_host.ends_with(']') {
            raw_host[1..raw_host.len() - 1].to_string()
        } else {
            raw_host.to_string()
        };

        let port = parsed.port().unwrap_or(if is_tls { 443 } else { 80 });

        let auth_header = if parsed.username().is_empty() {
            None
        } else {
            let username = decode_userinfo(parsed.username());
            let password = decode_userinfo(parsed.password().unwrap_or(""));
            let credentials = format!("{username}:{password}");
            Some(format!("Basic {}", BASE64.encode(credentials)))
        };

        Ok(Self {
            host,
            port,
            is_tls,
            auth_header,
        })
    }
}

/// Percent-decode a userinfo field from a proxy URL. `url::Url` keeps the
/// raw percent-encoded form, so we decode it here before assembling the
/// `Basic` credentials.
fn decode_userinfo(value: &str) -> String {
    let bytes = nautilus_core::string::urlencoding::decode_bytes(value.as_bytes());
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Establish a tunneled connection through `proxy` to the WebSocket `target`.
///
/// On success the returned stream is positioned right after the proxy's
/// `200`/`2xx` response, ready for the WebSocket handshake. The function does
/// not perform the WebSocket handshake itself; callers wrap the stream in
/// `tokio-tungstenite::client_async`.
///
/// # Errors
///
/// Returns a [`TransportError`] when:
/// - The TCP connection to the proxy fails ([`TransportError::Io`]).
/// - The TLS layer to the proxy or upstream cannot be established
///   ([`TransportError::Tls`]).
/// - The proxy returns a non-success status, malformed headers, or closes the
///   stream before completing the response ([`TransportError::Handshake`]).
pub async fn tunnel_via_proxy(
    target: &WsTarget,
    proxy: &ProxyTarget,
) -> Result<ProxiedStream, TransportError> {
    let tcp = TcpStream::connect((proxy.host.as_str(), proxy.port))
        .await
        .map_err(TransportError::Io)?;

    if let Err(e) = tcp.set_nodelay(true) {
        log::warn!("Failed to enable TCP_NODELAY on proxy connection: {e:?}");
    }

    if proxy.is_tls {
        let proxy_tls = wrap_tls(tcp, &proxy.host).await?;
        let tunneled = send_connect(proxy_tls, target, proxy).await?;
        if target.is_tls {
            let upstream = wrap_tls(tunneled, &target.host).await?;
            Ok(ProxiedStream::TlsOverTlsProxy(Box::new(upstream)))
        } else {
            Ok(ProxiedStream::PlainOverTlsProxy(Box::new(tunneled)))
        }
    } else {
        let tunneled = send_connect(tcp, target, proxy).await?;
        if target.is_tls {
            let upstream = wrap_tls(tunneled, &target.host).await?;
            Ok(ProxiedStream::Tls(Box::new(upstream)))
        } else {
            Ok(ProxiedStream::Plain(tunneled))
        }
    }
}

/// Send a `CONNECT` request and return the underlying stream once a `2xx`
/// status is received. The returned stream is positioned after the empty line
/// terminating the proxy response headers.
async fn send_connect<S>(
    mut stream: S,
    target: &WsTarget,
    proxy: &ProxyTarget,
) -> Result<S, TransportError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let host_header = format_host_header(&target.host, target.port);
    let mut request = format!(
        "CONNECT {host_header} HTTP/1.1\r\n\
         Host: {host_header}\r\n\
         Proxy-Connection: Keep-Alive\r\n"
    );

    if let Some(auth) = &proxy.auth_header {
        write!(request, "Proxy-Authorization: {auth}\r\n").expect("writing to String never fails");
    }
    request.push_str("\r\n");

    stream
        .write_all(request.as_bytes())
        .await
        .map_err(TransportError::Io)?;
    stream.flush().await.map_err(TransportError::Io)?;

    read_connect_response(&mut stream).await?;
    Ok(stream)
}

fn format_host_header(host: &str, port: u16) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// Read the proxy's response up to the empty line that terminates the
/// headers, validating the status line.
async fn read_connect_response<S>(stream: &mut S) -> Result<(), TransportError>
where
    S: AsyncRead + Unpin,
{
    let mut buf = Vec::with_capacity(512);
    let mut byte = [0u8; 1];

    loop {
        let n = stream.read(&mut byte).await.map_err(TransportError::Io)?;
        if n == 0 {
            return Err(TransportError::Handshake(
                "proxy closed connection before sending CONNECT response".to_string(),
            ));
        }

        buf.push(byte[0]);

        if buf.ends_with(b"\r\n\r\n") {
            break;
        }

        if buf.len() > MAX_PROXY_RESPONSE_BYTES {
            return Err(TransportError::Handshake(format!(
                "proxy CONNECT response exceeded {MAX_PROXY_RESPONSE_BYTES} bytes without terminator"
            )));
        }
    }

    let text = std::str::from_utf8(&buf).map_err(|_| {
        TransportError::Handshake("proxy CONNECT response was not valid UTF-8".to_string())
    })?;

    let status_line = text.lines().next().ok_or_else(|| {
        TransportError::Handshake("proxy CONNECT response missing status line".to_string())
    })?;

    // Expect: `HTTP/1.1 200 Connection established` (or any 2xx).
    let mut parts = status_line.splitn(3, ' ');
    let _version = parts.next().ok_or_else(|| {
        TransportError::Handshake(format!("malformed status line: {status_line}"))
    })?;
    let status_code = parts
        .next()
        .ok_or_else(|| TransportError::Handshake(format!("malformed status line: {status_line}")))?
        .parse::<u16>()
        .map_err(|_| TransportError::Handshake(format!("non-numeric status: {status_line}")))?;

    if !(200..300).contains(&status_code) {
        return Err(TransportError::Handshake(format!(
            "proxy refused CONNECT: {status_line}"
        )));
    }

    Ok(())
}

/// Wrap a stream in a `rustls`-backed TLS session using `webpki_roots`.
async fn wrap_tls<S>(stream: S, server_name: &str) -> Result<TlsStream<S>, TransportError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = TlsConnector::from(std::sync::Arc::new(config));
    let domain = ServerName::try_from(server_name.to_string())
        .map_err(|e| TransportError::Tls(format!("invalid DNS name '{server_name}': {e}")))?;

    connector
        .connect(domain, stream)
        .await
        .map_err(TransportError::Io)
}

#[cfg(test)]
#[cfg(not(feature = "turmoil"))] // proxy hop is not modelled under the turmoil simulator
mod tests {
    use std::net::SocketAddr;

    use rstest::rstest;
    use tokio::net::TcpListener;

    use super::*;

    #[rstest]
    fn ws_target_parses_wss() {
        let target = WsTarget::parse("wss://stream.binance.com:9443/ws/btcusdt@trade").unwrap();
        assert_eq!(target.host, "stream.binance.com");
        assert_eq!(target.port, 9443);
        assert!(target.is_tls);
    }

    #[rstest]
    fn ws_target_default_ports() {
        let plain = WsTarget::parse("ws://example.com/path").unwrap();
        assert_eq!(plain.port, 80);
        assert!(!plain.is_tls);

        let tls = WsTarget::parse("wss://example.com/path").unwrap();
        assert_eq!(tls.port, 443);
        assert!(tls.is_tls);
    }

    #[rstest]
    fn ws_target_strips_ipv6_brackets() {
        let target = WsTarget::parse("wss://[::1]:9443/ws").unwrap();
        assert_eq!(target.host, "::1");
        assert_eq!(target.port, 9443);
    }

    #[rstest]
    fn ws_target_rejects_non_ws_scheme() {
        let err = WsTarget::parse("https://example.com").unwrap_err();
        assert!(matches!(err, TransportError::InvalidUrl(_)));
    }

    #[rstest]
    fn proxy_target_parses_http() {
        let proxy = ProxyTarget::parse("http://127.0.0.1:9999").unwrap();
        assert_eq!(proxy.host, "127.0.0.1");
        assert_eq!(proxy.port, 9999);
        assert!(!proxy.is_tls);
        assert!(proxy.auth_header.is_none());
    }

    #[rstest]
    fn proxy_target_default_ports() {
        let plain = ProxyTarget::parse("http://proxy.example.com").unwrap();
        assert_eq!(plain.port, 80);
        let tls = ProxyTarget::parse("https://proxy.example.com").unwrap();
        assert_eq!(tls.port, 443);
        assert!(tls.is_tls);
    }

    #[rstest]
    fn proxy_target_basic_auth() {
        let proxy =
            ProxyTarget::parse("http://proxytest:fixture42@proxy.example.com:8080").unwrap();
        // base64("proxytest:fixture42") == "cHJveHl0ZXN0OmZpeHR1cmU0Mg=="
        assert_eq!(
            proxy.auth_header.unwrap(),
            "Basic cHJveHl0ZXN0OmZpeHR1cmU0Mg=="
        );
    }

    #[rstest]
    fn proxy_target_basic_auth_decodes_percent_encoded() {
        // `p%40ss` should decode to `p@ss` before assembling Basic credentials
        let proxy = ProxyTarget::parse("http://us%2Fer:p%40ss@proxy.example.com:8080").unwrap();
        let header = proxy.auth_header.unwrap();
        // base64("us/er:p@ss") == "dXMvZXI6cEBzcw=="
        assert_eq!(header, "Basic dXMvZXI6cEBzcw==");
    }

    #[rstest]
    fn proxy_target_strips_ipv6_brackets() {
        let proxy = ProxyTarget::parse("http://[::1]:8080").unwrap();
        assert_eq!(proxy.host, "::1");
        assert_eq!(proxy.port, 8080);
    }

    #[rstest]
    fn proxy_target_rejects_socks() {
        let err = ProxyTarget::parse("socks5://127.0.0.1:1080").unwrap_err();
        let TransportError::InvalidUrl(msg) = err else {
            panic!("expected InvalidUrl");
        };
        assert!(msg.contains("SOCKS"));
    }

    #[rstest]
    fn proxy_kind_classifies_http() {
        let kind = ProxyKind::parse("http://127.0.0.1:9999").unwrap();
        assert!(matches!(kind, ProxyKind::Http(_)));
    }

    #[rstest]
    fn proxy_kind_classifies_socks_as_unsupported() {
        let kind = ProxyKind::parse("socks5://127.0.0.1:1080").unwrap();
        let ProxyKind::Unsupported { scheme } = kind else {
            panic!("expected Unsupported");
        };
        assert_eq!(scheme, "socks5");
    }

    #[rstest]
    fn proxy_kind_rejects_garbage() {
        assert!(ProxyKind::parse("ftp://x").is_err());
        assert!(ProxyKind::parse("").is_err());
    }

    #[rstest]
    fn proxy_kind_rejects_socks_without_authority() {
        // `socks5:host:port` (no `//`) parses as scheme + opaque path; surface
        // as a real error instead of a silent direct-fallback.
        let err = ProxyKind::parse("socks5:127.0.0.1:1080").unwrap_err();
        assert!(matches!(err, TransportError::InvalidUrl(_)));
    }

    #[rstest]
    fn proxy_target_rejects_unknown_scheme() {
        let err = ProxyTarget::parse("ftp://proxy.example.com").unwrap_err();
        assert!(matches!(err, TransportError::InvalidUrl(_)));
    }

    #[rstest]
    fn proxy_target_rejects_empty() {
        let err = ProxyTarget::parse("").unwrap_err();
        assert!(matches!(err, TransportError::InvalidUrl(_)));
    }

    #[rstest]
    fn host_header_brackets_ipv6() {
        assert_eq!(format_host_header("example.com", 443), "example.com:443");
        assert_eq!(format_host_header("::1", 443), "[::1]:443");
        assert_eq!(format_host_header("[::1]", 443), "[::1]:443");
    }

    /// Spawn a fake HTTP proxy that returns the configured response after
    /// reading one CONNECT request line. Returns the bound address.
    async fn spawn_fake_proxy(response: &'static [u8]) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            // Read until we see the CONNECT terminator.
            loop {
                let n = AsyncReadExt::read(&mut stream, &mut buf).await.unwrap();
                if n == 0 {
                    break;
                }

                if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            stream.write_all(response).await.unwrap();
            stream.flush().await.unwrap();
        });
        addr
    }

    #[tokio::test]
    async fn read_connect_response_accepts_2xx() {
        let addr = spawn_fake_proxy(b"HTTP/1.1 200 Connection established\r\n\r\n").await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"CONNECT host:443 HTTP/1.1\r\nHost: host:443\r\n\r\n")
            .await
            .unwrap();
        stream.flush().await.unwrap();
        read_connect_response(&mut stream).await.unwrap();
    }

    #[tokio::test]
    async fn read_connect_response_rejects_403() {
        let addr = spawn_fake_proxy(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"CONNECT host:443 HTTP/1.1\r\nHost: host:443\r\n\r\n")
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let err = read_connect_response(&mut stream).await.unwrap_err();
        let TransportError::Handshake(msg) = err else {
            panic!("expected Handshake error");
        };
        assert!(msg.contains("403"));
    }

    /// 300 sits on the upper boundary of the accepted `200..300` range; if
    /// the check is ever loosened to `200..=300` this test fails. 407 is the
    /// classic "Proxy Authentication Required" response. Non-numeric status
    /// probes the parse path.
    #[rstest]
    #[case::status_300(&b"HTTP/1.1 300 Multiple Choices\r\n\r\n"[..], "300")]
    #[case::status_407(
        &b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic\r\n\r\n"[..],
        "407",
    )]
    #[case::malformed_status(&b"HTTP/1.1 abc Boom\r\n\r\n"[..], "non-numeric")]
    #[tokio::test]
    async fn read_connect_response_rejects_non_2xx(
        #[case] response: &'static [u8],
        #[case] expected_msg_substring: &'static str,
    ) {
        let addr = spawn_fake_proxy(response).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"CONNECT host:443 HTTP/1.1\r\nHost: host:443\r\n\r\n")
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let err = read_connect_response(&mut stream).await.unwrap_err();
        let TransportError::Handshake(msg) = err else {
            panic!("expected Handshake error, was {err:?}");
        };
        assert!(
            msg.contains(expected_msg_substring),
            "expected error message to contain {expected_msg_substring:?}, was {msg:?}"
        );
    }

    /// Closing the connection mid-response should produce a clear handshake
    /// error rather than spinning on a zero-byte read.
    #[tokio::test]
    async fn read_connect_response_rejects_eof_before_terminator() {
        // Truncated response: missing the empty line that ends the headers
        let addr = spawn_fake_proxy(b"HTTP/1.1 200 OK\r\n").await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"CONNECT host:443 HTTP/1.1\r\nHost: host:443\r\n\r\n")
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let err = read_connect_response(&mut stream).await.unwrap_err();
        let TransportError::Handshake(msg) = err else {
            panic!("expected Handshake error, was {err:?}");
        };
        assert!(
            msg.contains("closed connection"),
            "unexpected handshake error: {msg}"
        );
    }

    /// A proxy that streams headers without ever emitting `\r\n\r\n` should
    /// trip the size cap rather than allocating without bound.
    #[tokio::test]
    async fn read_connect_response_rejects_oversize_headers() {
        let mut response = b"HTTP/1.1 200 OK\r\n".to_vec();
        while response.len() <= MAX_PROXY_RESPONSE_BYTES {
            response.extend_from_slice(b"X-Pad: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n");
        }
        let leaked: &'static [u8] = response.leak();
        let addr = spawn_fake_proxy(leaked).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"CONNECT host:443 HTTP/1.1\r\nHost: host:443\r\n\r\n")
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let err = read_connect_response(&mut stream).await.unwrap_err();
        let TransportError::Handshake(msg) = err else {
            panic!("expected Handshake error, was {err:?}");
        };
        assert!(
            msg.contains("exceeded"),
            "unexpected handshake error: {msg}"
        );
    }

    /// After accepting the 2xx response, the stream cursor must sit immediately
    /// after the terminating `\r\n\r\n` so the WebSocket handshake can read its
    /// own response. Regression guard against over-reading the terminator.
    #[tokio::test]
    async fn read_connect_response_preserves_trailing_bytes() {
        let addr = spawn_fake_proxy(b"HTTP/1.1 200 Connection established\r\n\r\nLEFTOVER").await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"CONNECT host:443 HTTP/1.1\r\nHost: host:443\r\n\r\n")
            .await
            .unwrap();
        stream.flush().await.unwrap();
        read_connect_response(&mut stream).await.unwrap();

        let mut tail = [0u8; b"LEFTOVER".len()];
        AsyncReadExt::read_exact(&mut stream, &mut tail)
            .await
            .unwrap();
        assert_eq!(&tail, b"LEFTOVER");
    }
}
