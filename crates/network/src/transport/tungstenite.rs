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

//! `tokio-tungstenite` backend for the transport abstraction.
//!
//! Provides `From` conversions between the neutral [`Message`] and
//! [`TransportError`] types and tungstenite's native types, plus the
//! [`TungsteniteTransport<S>`] adapter that lifts a tungstenite
//! `WebSocketStream<S>` into a backend-agnostic [`WsTransport`].
//!
//! The message conversions are structural (no payload copies): tungstenite
//! stores payloads in `Bytes` and `Utf8Bytes`, which we re-wrap directly.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Sink, Stream};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        self, Utf8Bytes,
        protocol::{CloseFrame as TgCloseFrame, frame::coding::CloseCode},
    },
};

use super::{
    error::TransportError,
    message::{CloseFrame, Message},
    stream::WsTransport,
};

impl From<tungstenite::Message> for Message {
    fn from(value: tungstenite::Message) -> Self {
        match value {
            tungstenite::Message::Text(text) => Self::Text(Bytes::from(text)),
            tungstenite::Message::Binary(data) => Self::Binary(data),
            tungstenite::Message::Ping(data) => Self::Ping(data),
            tungstenite::Message::Pong(data) => Self::Pong(data),
            tungstenite::Message::Close(frame) => Self::Close(frame.map(Into::into)),

            // Tungstenite only emits Frame when explicitly constructed; treat as binary
            tungstenite::Message::Frame(frame) => Self::Binary(frame.into_payload()),
        }
    }
}

impl TryFrom<Message> for tungstenite::Message {
    type Error = TransportError;

    /// Convert a neutral [`Message`] into a tungstenite [`tungstenite::Message`].
    ///
    /// Validates the `Text` payload as UTF-8 because tungstenite refuses to
    /// transmit a Text frame whose body is not valid UTF-8. Other variants
    /// are infallible.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::InvalidUtf8`] if a `Text` payload is not
    /// valid UTF-8.
    fn try_from(value: Message) -> Result<Self, Self::Error> {
        Ok(match value {
            Message::Text(bytes) => match Utf8Bytes::try_from(bytes) {
                Ok(text) => Self::Text(text),
                Err(_) => return Err(TransportError::InvalidUtf8),
            },
            Message::Binary(bytes) => Self::Binary(bytes),
            Message::Ping(bytes) => Self::Ping(bytes),
            Message::Pong(bytes) => Self::Pong(bytes),
            Message::Close(frame) => Self::Close(frame.map(Into::into)),
        })
    }
}

impl From<TgCloseFrame> for CloseFrame {
    fn from(value: TgCloseFrame) -> Self {
        Self {
            code: u16::from(value.code),
            reason: value.reason.as_str().to_owned(),
        }
    }
}

impl From<CloseFrame> for TgCloseFrame {
    fn from(value: CloseFrame) -> Self {
        Self {
            code: CloseCode::from(value.code),
            reason: value.reason.into(),
        }
    }
}

impl From<tungstenite::Error> for TransportError {
    fn from(value: tungstenite::Error) -> Self {
        match value {
            tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
                Self::ConnectionClosed
            }
            tungstenite::Error::Io(e) => Self::Io(e),
            tungstenite::Error::Tls(e) => Self::Tls(e.to_string()),
            tungstenite::Error::Capacity(e) => match e {
                tungstenite::error::CapacityError::MessageTooLong { .. } => Self::MessageTooLarge,
                e @ tungstenite::error::CapacityError::TooManyHeaders => Self::Other(e.to_string()),
            },
            tungstenite::Error::Protocol(e) => Self::Protocol(e.to_string()),
            tungstenite::Error::Utf8(_) => Self::InvalidUtf8,
            tungstenite::Error::Url(e) => Self::InvalidUrl(e.to_string()),
            tungstenite::Error::Http(resp) => {
                Self::Handshake(format!("HTTP status {}", resp.status()))
            }
            tungstenite::Error::HttpFormat(e) => Self::Handshake(e.to_string()),
            other => Self::Other(other.to_string()),
        }
    }
}

/// Adapter that lifts a `tokio-tungstenite` [`WebSocketStream<S>`] into a
/// backend-agnostic [`WsTransport`].
///
/// Translates messages and errors to the neutral types on the way through
/// `Stream::poll_next` and `Sink<Message>::start_send` / `poll_*`. The
/// underlying stream is owned and forwarded to via pin projection.
#[derive(Debug)]
pub struct TungsteniteTransport<S> {
    inner: WebSocketStream<S>,
}

impl<S> TungsteniteTransport<S> {
    /// Wrap an established tungstenite WebSocket stream.
    #[inline]
    #[must_use]
    pub const fn new(inner: WebSocketStream<S>) -> Self {
        Self { inner }
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

impl<S> Stream for TungsteniteTransport<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    type Item = Result<Message, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(msg))) => Poll::Ready(Some(Ok(Message::from(msg)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(TransportError::from(e)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S> Sink<Message> for TungsteniteTransport<S>
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
        let native = tungstenite::Message::try_from(item)?;
        Pin::new(&mut self.inner)
            .start_send(native)
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
    assert_ws_transport::<TungsteniteTransport<tokio::net::TcpStream>>();
};

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use rstest::rstest;
    use tokio_tungstenite::tungstenite::{self, Utf8Bytes};

    use super::*;

    #[rstest]
    fn round_trip_text() {
        let original = tungstenite::Message::Text(Utf8Bytes::from("hello"));
        let neutral: Message = original.into();
        assert!(neutral.is_text());
        assert_eq!(neutral.as_bytes(), b"hello");

        let back = tungstenite::Message::try_from(neutral).unwrap();
        match back {
            tungstenite::Message::Text(t) => assert_eq!(t.as_str(), "hello"),
            other => panic!("expected text, was {other:?}"),
        }
    }

    #[rstest]
    fn try_from_text_rejects_invalid_utf8() {
        let neutral = Message::Text(Bytes::from_static(&[0xFF, 0xFE]));
        let err = tungstenite::Message::try_from(neutral).unwrap_err();
        assert!(matches!(err, TransportError::InvalidUtf8));
    }

    #[rstest]
    fn round_trip_binary() {
        let original = tungstenite::Message::Binary(Bytes::from_static(&[1, 2, 3]));
        let neutral: Message = original.into();
        assert_eq!(neutral.as_bytes(), &[1, 2, 3]);

        let back = tungstenite::Message::try_from(neutral).unwrap();
        match back {
            tungstenite::Message::Binary(b) => assert_eq!(&b[..], &[1, 2, 3]),
            other => panic!("expected binary, was {other:?}"),
        }
    }

    #[rstest]
    fn round_trip_ping_pong() {
        let ping = tungstenite::Message::Ping(Bytes::from_static(b"p"));
        let neutral: Message = ping.into();
        assert!(neutral.is_ping());

        let pong = tungstenite::Message::Pong(Bytes::from_static(b"q"));
        let neutral: Message = pong.into();
        assert!(neutral.is_pong());
    }

    #[rstest]
    fn close_frame_round_trip() {
        let original = tungstenite::Message::Close(Some(TgCloseFrame {
            code: CloseCode::Normal,
            reason: "bye".into(),
        }));
        let neutral: Message = original.into();
        let Message::Close(Some(frame)) = &neutral else {
            panic!("expected close frame");
        };
        assert_eq!(frame.code, 1000);
        assert_eq!(frame.reason, "bye");

        let back = tungstenite::Message::try_from(neutral).unwrap();
        let tungstenite::Message::Close(Some(frame)) = back else {
            panic!("expected close frame");
        };
        assert_eq!(u16::from(frame.code), 1000);
        assert_eq!(frame.reason.as_str(), "bye");
    }

    #[rstest]
    fn error_translation_closed() {
        let err: TransportError = tungstenite::Error::ConnectionClosed.into();
        assert!(matches!(err, TransportError::ConnectionClosed));
    }

    #[rstest]
    fn error_translation_utf8() {
        let err: TransportError = tungstenite::Error::Utf8(String::from("bad")).into();
        assert!(matches!(err, TransportError::InvalidUtf8));
    }
}
