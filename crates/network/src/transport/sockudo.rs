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
//! Sockudo's HTTP/1.1 client handshake does not accept custom headers, so the runtime
//! selector in [`crate::websocket::config::WebSocketConfig`] rejects non-empty headers
//! when this backend is selected.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::{Sink, Stream};
use sockudo_ws::{
    error::{CloseReason as SockudoCloseReason, Error as SockudoError},
    protocol::Message as SockudoMessage,
    stream::WebSocketStream,
};
use tokio::io::{AsyncRead, AsyncWrite};

use super::{
    error::TransportError,
    message::{CloseFrame, Message},
    stream::WsTransport,
};

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

    use super::*;

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
