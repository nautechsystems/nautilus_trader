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

//! `sockudo-ws` backend for the transport abstraction.
//!
//! Mirrors the layout of the [`tungstenite`](super::tungstenite) module: provides
//! `From`/`TryFrom` conversions between the neutral [`Message`] / [`TransportError`]
//! and sockudo's native types, plus a [`SockudoTransport<S>`] adapter that lifts a
//! sockudo [`WebSocketStream<S>`] into the backend-agnostic [`WsTransport`] trait.
//!
//! The `Message` enums are structurally identical: both carry payloads as `bytes::Bytes`
//! across all five variants, so conversions are zero-copy and infallible.
//!
//! sockudo's public HTTP/1.1 client API does not expose custom headers, so this
//! module provides a small handshake helper for upgrade requests that need them.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{BufMut, Bytes, BytesMut};
use futures::{Sink, Stream};
use sockudo_ws::{
    HandshakeResult,
    error::{CloseReason as SockudoCloseReason, Error as SockudoError},
    handshake,
    protocol::Message as SockudoMessage,
    stream::WebSocketStream,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::{
    error::TransportError,
    message::{CloseFrame, Message},
    stream::WsTransport,
};

const MAX_HTTP_HEADER_SIZE: usize = 8192;

// WebSocket upgrade headers we always set, plus body-framing headers that have
// no place on a GET upgrade.
const RESERVED_UPGRADE_HEADERS: &[&str] = &[
    "host",
    "upgrade",
    "connection",
    "sec-websocket-key",
    "sec-websocket-version",
    "sec-websocket-protocol",
    "sec-websocket-extensions",
    "content-length",
    "transfer-encoding",
    "te",
    "trailer",
];

/// Mirror of `sockudo_ws::handshake::client_handshake` (1.7.4) with custom headers.
///
/// Caller pre-validates `extra_headers` via [`validate_extra_headers`].
pub(crate) async fn client_handshake_with_headers<S>(
    stream: &mut S,
    host: &str,
    path: &str,
    protocol: Option<&str>,
    extra_headers: &[(String, String)],
) -> Result<HandshakeResult, SockudoError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let key = handshake::generate_key();
    let request = build_request_with_headers(host, path, &key, protocol, None, extra_headers);

    stream.write_all(&request).await?;
    stream.flush().await?;

    let mut buf = BytesMut::with_capacity(4096);

    loop {
        if buf.len() > MAX_HTTP_HEADER_SIZE {
            return Err(SockudoError::InvalidHttp("response too large"));
        }

        let n = stream.read_buf(&mut buf).await?;
        if n == 0 {
            return Err(SockudoError::ConnectionClosed);
        }

        let parsed = match handshake::parse_response(&buf) {
            Ok(parsed) => parsed,
            Err(e) => {
                log_handshake_response(host, path, &e, &buf);
                return Err(e);
            }
        };

        if let Some((res, consumed)) = parsed {
            let accept = res.accept.ok_or_else(|| {
                let e = SockudoError::HandshakeFailed("missing Sec-WebSocket-Accept");
                log_handshake_response(host, path, &e, &buf);
                e
            })?;

            if !handshake::validate_accept_key(&key, accept) {
                let e = SockudoError::HandshakeFailed("invalid Sec-WebSocket-Accept");
                log_handshake_response(host, path, &e, &buf);
                return Err(e);
            }

            let res_protocol = res.protocol.map(String::from);
            let res_extensions = res.extensions.map(String::from);
            let leftover = if consumed < buf.len() {
                Some(buf.split_off(consumed).freeze())
            } else {
                None
            };

            return Ok(HandshakeResult {
                path: path.to_string(),
                protocol: res_protocol,
                extensions: res_extensions,
                leftover,
            });
        }
    }
}

// Surface the upstream HTTP response on parse failure so non-101 statuses are visible.
fn log_handshake_response(host: &str, path: &str, err: &SockudoError, buf: &BytesMut) {
    const PREVIEW_BYTES: usize = 512;
    let take = buf.len().min(PREVIEW_BYTES);
    let preview = String::from_utf8_lossy(&buf[..take]);
    let truncated = if buf.len() > take { " (truncated)" } else { "" };
    log::error!(
        "Sockudo handshake failed for {host}{path}: {err}; response{truncated}:\n{preview}"
    );
}

// Mirror of `sockudo_ws::handshake::build_request` (1.7.4) with `extra_headers`
// appended; caller pre-validates.
fn build_request_with_headers(
    host: &str,
    path: &str,
    key: &str,
    protocol: Option<&str>,
    extensions: Option<&str>,
    extra_headers: &[(String, String)],
) -> Bytes {
    let mut buf = BytesMut::with_capacity(512);

    buf.put_slice(b"GET ");
    buf.put_slice(path.as_bytes());
    buf.put_slice(b" HTTP/1.1\r\n");
    buf.put_slice(b"Host: ");
    buf.put_slice(host.as_bytes());
    buf.put_slice(b"\r\n");
    buf.put_slice(b"Upgrade: websocket\r\n");
    buf.put_slice(b"Connection: Upgrade\r\n");
    buf.put_slice(b"Sec-WebSocket-Key: ");
    buf.put_slice(key.as_bytes());
    buf.put_slice(b"\r\n");
    buf.put_slice(b"Sec-WebSocket-Version: 13\r\n");

    if let Some(proto) = protocol {
        buf.put_slice(b"Sec-WebSocket-Protocol: ");
        buf.put_slice(proto.as_bytes());
        buf.put_slice(b"\r\n");
    }

    if let Some(ext) = extensions {
        buf.put_slice(b"Sec-WebSocket-Extensions: ");
        buf.put_slice(ext.as_bytes());
        buf.put_slice(b"\r\n");
    }

    for (name, value) in extra_headers {
        buf.put_slice(name.as_bytes());
        buf.put_slice(b": ");
        buf.put_slice(value.as_bytes());
        buf.put_slice(b"\r\n");
    }

    buf.put_slice(b"\r\n");
    buf.freeze()
}

pub(crate) fn validate_extra_headers(headers: &[(String, String)]) -> Result<(), SockudoError> {
    for (name, value) in headers {
        validate_extra_header(name, value)?;
    }
    Ok(())
}

fn validate_extra_header(name: &str, value: &str) -> Result<(), SockudoError> {
    let parsed_name = name
        .parse::<http::HeaderName>()
        .map_err(|_| SockudoError::InvalidHttp("invalid header name"))?;

    if RESERVED_UPGRADE_HEADERS.contains(&parsed_name.as_str()) {
        return Err(SockudoError::InvalidHttp(
            "reserved upgrade header not allowed in extra_headers",
        ));
    }

    http::HeaderValue::from_str(value)
        .map_err(|_| SockudoError::InvalidHttp("invalid header value"))?;
    Ok(())
}

/// Replay bytes read during the handshake before forwarding to the inner IO.
pub(crate) struct PrefixedIo<S> {
    inner: S,
    prefix: Bytes,
}

impl<S> PrefixedIo<S> {
    pub(crate) const fn new(inner: S, prefix: Bytes) -> Self {
        Self { inner, prefix }
    }
}

impl<S> AsyncRead for PrefixedIo<S>
where
    S: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if !self.prefix.is_empty() {
            let n = self.prefix.len().min(buf.remaining());
            let chunk = self.prefix.split_to(n);
            buf.put_slice(&chunk);
            return Poll::Ready(Ok(()));
        }

        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S> AsyncWrite for PrefixedIo<S>
where
    S: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl From<SockudoMessage> for Message {
    fn from(value: SockudoMessage) -> Self {
        match value {
            SockudoMessage::Text(b) => Self::Text(b),
            SockudoMessage::Binary(b) => Self::Binary(b),
            SockudoMessage::Ping(b) => Self::Ping(b),
            SockudoMessage::Pong(b) => Self::Pong(b),
            SockudoMessage::Close(reason) => Self::Close(reason.map(Into::into)),
        }
    }
}

impl From<Message> for SockudoMessage {
    /// Convert a neutral [`Message`] into a sockudo [`SockudoMessage`].
    ///
    /// Conversion is infallible: both enums carry payloads as `bytes::Bytes` across
    /// all variants. Sockudo validates UTF-8 on Text frames at parse time, not at
    /// send time, so feeding it non-UTF-8 bytes via [`Self::Text`] is the caller's
    /// responsibility.
    fn from(value: Message) -> Self {
        match value {
            Message::Text(b) => Self::Text(b),
            Message::Binary(b) => Self::Binary(b),
            Message::Ping(b) => Self::Ping(b),
            Message::Pong(b) => Self::Pong(b),
            Message::Close(frame) => Self::Close(frame.map(Into::into)),
        }
    }
}

impl From<SockudoCloseReason> for CloseFrame {
    fn from(value: SockudoCloseReason) -> Self {
        Self {
            code: value.code,
            reason: value.reason,
        }
    }
}

impl From<CloseFrame> for SockudoCloseReason {
    fn from(value: CloseFrame) -> Self {
        Self {
            code: value.code,
            reason: value.reason,
        }
    }
}

impl From<SockudoError> for TransportError {
    fn from(value: SockudoError) -> Self {
        match value {
            SockudoError::Io(e) => Self::Io(e),
            SockudoError::ConnectionClosed => Self::ConnectionClosed,
            SockudoError::ConnectionReset => Self::ConnectionReset,
            SockudoError::Closed(reason) => Self::ClosedByPeer(reason.map(Into::into)),
            SockudoError::MessageTooLarge => Self::MessageTooLarge,
            SockudoError::FrameTooLarge => Self::FrameTooLarge,
            SockudoError::InvalidUtf8 => Self::InvalidUtf8,
            SockudoError::InvalidFrame(msg) | SockudoError::Protocol(msg) => {
                Self::Protocol(msg.to_string())
            }
            SockudoError::InvalidHttp(msg) | SockudoError::HandshakeFailed(msg) => {
                Self::Handshake(msg.to_string())
            }
            other => Self::Other(other.to_string()),
        }
    }
}

/// Adapter that lifts a `sockudo-ws` [`WebSocketStream<S>`] into a
/// backend-agnostic [`WsTransport`].
///
/// Translates messages and errors to the neutral types on the way through
/// `Stream::poll_next` and `Sink<Message>::start_send` / `poll_*`. The
/// underlying stream is owned and forwarded to via pin projection.
pub struct SockudoTransport<S> {
    inner: WebSocketStream<S>,
    /// Tracks a flush of the inner write buffer that returned `Pending`. The
    /// next [`Stream::poll_next`] retries the flush before reading so queued
    /// control responses (Pong, close reply) are not stranded under sustained
    /// write backpressure on a quiet reader.
    pending_flush: bool,
}

impl<S> SockudoTransport<S> {
    /// Wrap an established sockudo WebSocket stream.
    #[inline]
    #[must_use]
    pub const fn new(inner: WebSocketStream<S>) -> Self {
        Self {
            inner,
            pending_flush: false,
        }
    }

    /// Consume the adapter and return the underlying stream.
    #[inline]
    pub fn into_inner(self) -> WebSocketStream<S> {
        self.inner
    }

    /// Borrow the underlying stream.
    #[inline]
    pub const fn get_ref(&self) -> &WebSocketStream<S> {
        &self.inner
    }
}

impl<S> std::fmt::Debug for SockudoTransport<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SockudoTransport))
            .finish_non_exhaustive()
    }
}

impl<S> Stream for SockudoTransport<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    type Item = Result<Message, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Drain any flush that returned Pending on a prior poll so queued
        // control responses (Pong, close reply) reach the peer before the
        // next read. Errors are dropped here; subsequent writes through the
        // sink half surface them.
        if self.pending_flush {
            match Pin::new(&mut self.inner).poll_flush(cx) {
                Poll::Ready(_) => self.pending_flush = false,
                Poll::Pending => {}
            }
        }

        let result = match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(msg))) => Poll::Ready(Some(Ok(Message::from(msg)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(TransportError::from(e)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        };

        // Sockudo queues automatic Pong / close-response frames into the
        // write buffer during poll_next. Nudge a flush so they reach the peer
        // promptly even on a reader-only client; track a pending flush so the
        // next poll retries when backpressure stalls the write socket.
        match Pin::new(&mut self.inner).poll_flush(cx) {
            Poll::Ready(_) => self.pending_flush = false,
            Poll::Pending => self.pending_flush = true,
        }

        result
    }
}

impl<S> Sink<Message> for SockudoTransport<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    type Error = TransportError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_ready(cx)
            .map_err(TransportError::from)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        Pin::new(&mut self.inner)
            .start_send(SockudoMessage::from(item))
            .map_err(TransportError::from)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_flush(cx)
            .map_err(TransportError::from)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner)
            .poll_close(cx)
            .map_err(TransportError::from)
    }
}

const _: fn() = || {
    fn assert_ws_transport<T: WsTransport>() {}
    assert_ws_transport::<SockudoTransport<tokio::net::TcpStream>>();
};

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use rstest::rstest;
    #[cfg(not(feature = "turmoil"))]
    use sockudo_ws::handshake::generate_accept_key;
    #[cfg(not(feature = "turmoil"))]
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, duplex};

    use super::*;

    #[cfg(not(feature = "turmoil"))]
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

    #[cfg(not(feature = "turmoil"))]
    fn build_test_response(sec_websocket_key: &str, extra_bytes: &[u8]) -> Vec<u8> {
        let accept = generate_accept_key(sec_websocket_key);
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
        response.extend_from_slice(extra_bytes);
        response
    }

    #[cfg(not(feature = "turmoil"))]
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

    #[tokio::test]
    #[cfg(not(feature = "turmoil"))]
    async fn client_handshake_with_headers_sends_custom_headers() {
        let (mut client, mut server) = duplex(4096);
        let headers = vec![
            ("ok-access-key".to_string(), "key-1".to_string()),
            ("ok-access-passphrase".to_string(), "pass-1".to_string()),
        ];

        let server_task = tokio::spawn(async move {
            let request = read_http_request(&mut server).await;
            let request = String::from_utf8(request).unwrap();

            assert!(request.starts_with("GET /ws/v5/public-sbe?instId=BTC-USDT HTTP/1.1\r\n"));
            assert_eq!(extract_header(&request, "Host"), Some("ws.okx.com:8443"));
            assert_eq!(extract_header(&request, "ok-access-key"), Some("key-1"));
            assert_eq!(
                extract_header(&request, "ok-access-passphrase"),
                Some("pass-1")
            );

            let sec_websocket_key = extract_header(&request, "Sec-WebSocket-Key").unwrap();
            let response = build_test_response(sec_websocket_key, &[]);
            server.write_all(&response).await.unwrap();
        });

        let handshake = client_handshake_with_headers(
            &mut client,
            "ws.okx.com:8443",
            "/ws/v5/public-sbe?instId=BTC-USDT",
            None,
            &headers,
        )
        .await
        .unwrap();

        assert_eq!(handshake.path, "/ws/v5/public-sbe?instId=BTC-USDT");
        assert!(handshake.leftover.is_none());
        server_task.await.unwrap();
    }

    #[rstest]
    #[cfg(not(feature = "turmoil"))]
    #[case::host("Host")]
    #[case::upgrade("Upgrade")]
    #[case::connection("Connection")]
    #[case::sec_websocket_key("Sec-WebSocket-Key")]
    #[case::sec_websocket_version("Sec-WebSocket-Version")]
    #[case::sec_websocket_protocol("Sec-WebSocket-Protocol")]
    #[case::sec_websocket_extensions("Sec-WebSocket-Extensions")]
    #[case::content_length("Content-Length")]
    #[case::transfer_encoding("Transfer-Encoding")]
    #[case::te("TE")]
    #[case::trailer("Trailer")]
    fn validate_extra_header_rejects_reserved_upgrade_headers(#[case] name: &str) {
        let err = validate_extra_header(name, "value").unwrap_err();

        assert!(matches!(
            err,
            SockudoError::InvalidHttp("reserved upgrade header not allowed in extra_headers")
        ));
    }

    #[tokio::test]
    #[cfg(not(feature = "turmoil"))]
    async fn client_handshake_with_headers_rejects_missing_accept() {
        let (mut client, mut server) = duplex(4096);

        let server_task = tokio::spawn(async move {
            let _request = read_http_request(&mut server).await;
            server
                .write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\n\
                      Upgrade: websocket\r\n\
                      Connection: Upgrade\r\n\
                      \r\n",
                )
                .await
                .unwrap();
        });

        let err = client_handshake_with_headers(&mut client, "example.com", "/ws", None, &[])
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            SockudoError::HandshakeFailed("missing Sec-WebSocket-Accept")
        ));
        server_task.await.unwrap();
    }

    #[tokio::test]
    #[cfg(not(feature = "turmoil"))]
    async fn client_handshake_with_headers_returns_leftover_bytes() {
        let (mut client, mut server) = duplex(4096);
        let extra = b"\x81\x05hello";

        let server_task = tokio::spawn(async move {
            let request = read_http_request(&mut server).await;
            let request = String::from_utf8(request).unwrap();
            let sec_websocket_key = extract_header(&request, "Sec-WebSocket-Key").unwrap();
            let response = build_test_response(sec_websocket_key, extra);
            server.write_all(&response).await.unwrap();
        });

        let handshake = client_handshake_with_headers(&mut client, "example.com", "/ws", None, &[])
            .await
            .unwrap();

        assert_eq!(handshake.leftover.as_deref(), Some(extra.as_slice()));
        server_task.await.unwrap();
    }

    #[tokio::test]
    #[cfg(not(feature = "turmoil"))]
    async fn prefixed_io_replays_leftover_before_socket() {
        let (client, mut server) = duplex(4096);
        let mut prefixed = PrefixedIo::new(client, Bytes::from_static(b"abc"));

        let server_task = tokio::spawn(async move {
            server.write_all(b"def").await.unwrap();
        });

        let mut buf = [0u8; 6];
        prefixed.read_exact(&mut buf).await.unwrap();

        assert_eq!(&buf, b"abcdef");
        server_task.await.unwrap();
    }

    #[rstest]
    fn round_trip_text() {
        let original = SockudoMessage::Text(Bytes::from_static(b"hello"));
        let neutral: Message = original.into();
        assert!(neutral.is_text());
        assert_eq!(neutral.as_bytes(), b"hello");

        let back: SockudoMessage = neutral.into();
        match back {
            SockudoMessage::Text(b) => assert_eq!(&b[..], b"hello"),
            other => panic!("expected text, was {other:?}"),
        }
    }

    #[rstest]
    fn round_trip_binary() {
        let original = SockudoMessage::Binary(Bytes::from_static(&[1, 2, 3]));
        let neutral: Message = original.into();
        assert_eq!(neutral.as_bytes(), &[1, 2, 3]);

        let back: SockudoMessage = neutral.into();
        match back {
            SockudoMessage::Binary(b) => assert_eq!(&b[..], &[1, 2, 3]),
            other => panic!("expected binary, was {other:?}"),
        }
    }

    #[rstest]
    fn round_trip_ping_pong() {
        let neutral: Message = SockudoMessage::Ping(Bytes::from_static(b"p")).into();
        assert!(neutral.is_ping());

        let neutral: Message = SockudoMessage::Pong(Bytes::from_static(b"q")).into();
        assert!(neutral.is_pong());
    }

    #[rstest]
    fn close_frame_round_trip() {
        let original = SockudoMessage::Close(Some(SockudoCloseReason {
            code: 1000,
            reason: "bye".into(),
        }));
        let neutral: Message = original.into();
        let Message::Close(Some(frame)) = &neutral else {
            panic!("expected close frame");
        };
        assert_eq!(frame.code, 1000);
        assert_eq!(frame.reason, "bye");

        let back: SockudoMessage = neutral.into();
        let SockudoMessage::Close(Some(reason)) = back else {
            panic!("expected close frame");
        };
        assert_eq!(reason.code, 1000);
        assert_eq!(reason.reason, "bye");
    }

    #[rstest]
    fn error_translation_closed() {
        let err: TransportError = SockudoError::ConnectionClosed.into();
        assert!(matches!(err, TransportError::ConnectionClosed));
    }

    #[rstest]
    fn error_translation_utf8() {
        let err: TransportError = SockudoError::InvalidUtf8.into();
        assert!(matches!(err, TransportError::InvalidUtf8));
    }

    #[rstest]
    fn error_translation_handshake() {
        let err: TransportError = SockudoError::HandshakeFailed("bad").into();
        assert!(matches!(err, TransportError::Handshake(_)));
    }
}
