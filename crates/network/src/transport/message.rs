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

//! Neutral WebSocket message types shared by all transport backends.

use bytes::Bytes;

/// A WebSocket message.
///
/// Backend-agnostic representation handed to consumers of the
/// `nautilus-network` transport layer. Each backend provides `From` impls
/// between its native `Message` type and this enum.
///
/// `Text` is documented to carry UTF-8 by contract but the type does not
/// enforce it. Backends that already validate (such as `tokio-tungstenite`)
/// produce valid bytes; an in-house HFT backend may skip validation for
/// performance and rely on the consumer's parser to catch malformed bytes.
/// Use [`Self::as_text`] to view the payload as `&str`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Text message. Payload is UTF-8 by contract, not by type guarantee.
    Text(Bytes),
    /// Binary message.
    Binary(Bytes),
    /// Ping control frame. Payload bounded to 125 bytes by RFC 6455.
    Ping(Bytes),
    /// Pong control frame. Payload bounded to 125 bytes by RFC 6455.
    Pong(Bytes),
    /// Close control frame with optional close frame payload.
    Close(Option<CloseFrame>),
}

impl Message {
    /// Construct a text message from any string-like value.
    #[inline]
    #[must_use]
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(Bytes::from(s.into()))
    }

    /// Borrow a text message as `&str` if the payload is valid UTF-8.
    ///
    /// Validates on each call; for hot paths where the producer is trusted,
    /// callers can read the bytes directly via [`Self::as_bytes`] and feed
    /// them to a parser that catches malformed input.
    #[inline]
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(b) => std::str::from_utf8(b).ok(),
            _ => None,
        }
    }

    /// Construct a binary message.
    #[inline]
    #[must_use]
    pub fn binary(data: impl Into<Bytes>) -> Self {
        Self::Binary(data.into())
    }

    /// Construct a ping message.
    #[inline]
    #[must_use]
    pub fn ping(data: impl Into<Bytes>) -> Self {
        Self::Ping(data.into())
    }

    /// Construct a pong message.
    #[inline]
    #[must_use]
    pub fn pong(data: impl Into<Bytes>) -> Self {
        Self::Pong(data.into())
    }

    /// Returns `true` for text messages.
    #[inline]
    #[must_use]
    pub const fn is_text(&self) -> bool {
        matches!(self, Self::Text(_))
    }

    /// Returns `true` for binary messages.
    #[inline]
    #[must_use]
    pub const fn is_binary(&self) -> bool {
        matches!(self, Self::Binary(_))
    }

    /// Returns `true` for ping messages.
    #[inline]
    #[must_use]
    pub const fn is_ping(&self) -> bool {
        matches!(self, Self::Ping(_))
    }

    /// Returns `true` for pong messages.
    #[inline]
    #[must_use]
    pub const fn is_pong(&self) -> bool {
        matches!(self, Self::Pong(_))
    }

    /// Returns `true` for close messages.
    #[inline]
    #[must_use]
    pub const fn is_close(&self) -> bool {
        matches!(self, Self::Close(_))
    }

    /// Returns `true` for control frames (ping, pong, close).
    #[inline]
    #[must_use]
    pub const fn is_control(&self) -> bool {
        matches!(self, Self::Ping(_) | Self::Pong(_) | Self::Close(_))
    }

    /// Returns the message payload as a byte slice.
    ///
    /// For close frames, returns the reason payload as bytes.
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Text(b) | Self::Binary(b) | Self::Ping(b) | Self::Pong(b) => b,
            Self::Close(_) => &[],
        }
    }

    /// Consumes the message and returns its payload as `Bytes`.
    ///
    /// For close frames, returns an empty `Bytes`.
    #[inline]
    #[must_use]
    pub fn into_bytes(self) -> Bytes {
        match self {
            Self::Text(b) | Self::Binary(b) | Self::Ping(b) | Self::Pong(b) => b,
            Self::Close(_) => Bytes::new(),
        }
    }
}

impl From<String> for Message {
    #[inline]
    fn from(s: String) -> Self {
        Self::Text(Bytes::from(s))
    }
}

impl From<&str> for Message {
    #[inline]
    fn from(s: &str) -> Self {
        Self::Text(Bytes::copy_from_slice(s.as_bytes()))
    }
}

impl From<Vec<u8>> for Message {
    #[inline]
    fn from(v: Vec<u8>) -> Self {
        Self::Binary(Bytes::from(v))
    }
}

impl From<Bytes> for Message {
    #[inline]
    fn from(b: Bytes) -> Self {
        Self::Binary(b)
    }
}

/// A WebSocket close frame.
///
/// Mirrors the RFC 6455 close payload: a 16-bit status code followed by an
/// optional UTF-8 reason string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseFrame {
    /// RFC 6455 close status code.
    pub code: u16,
    /// Human-readable reason string. Empty if no reason was provided.
    pub reason: String,
}

impl CloseFrame {
    /// Normal closure (1000).
    pub const NORMAL: u16 = 1000;
    /// Going away (1001).
    pub const GOING_AWAY: u16 = 1001;
    /// Protocol error (1002).
    pub const PROTOCOL_ERROR: u16 = 1002;
    /// Unsupported data (1003).
    pub const UNSUPPORTED: u16 = 1003;
    /// Abnormal closure, no close frame received (1006).
    pub const ABNORMAL: u16 = 1006;
    /// Policy violation (1008).
    pub const POLICY_VIOLATION: u16 = 1008;
    /// Message too large (1009).
    pub const MESSAGE_TOO_LARGE: u16 = 1009;
    /// Internal server error (1011).
    pub const INTERNAL_ERROR: u16 = 1011;

    /// Construct a close frame.
    #[inline]
    #[must_use]
    pub fn new(code: u16, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn text_constructor_round_trips() {
        let msg = Message::text("hello");
        assert!(msg.is_text());
        assert_eq!(msg.as_bytes(), b"hello");
    }

    #[rstest]
    fn binary_constructor_round_trips() {
        let msg = Message::binary(vec![1, 2, 3]);
        assert!(msg.is_binary());
        assert_eq!(msg.as_bytes(), &[1, 2, 3]);
    }

    #[rstest]
    fn ping_pong_classify_as_control() {
        assert!(Message::ping(Bytes::new()).is_control());
        assert!(Message::pong(Bytes::new()).is_control());
        assert!(Message::Close(None).is_control());
        assert!(!Message::text("x").is_control());
        assert!(!Message::binary(vec![]).is_control());
    }

    #[rstest]
    fn close_frame_carries_code_and_reason() {
        let frame = CloseFrame::new(CloseFrame::GOING_AWAY, "shutdown");
        assert_eq!(frame.code, 1001);
        assert_eq!(frame.reason, "shutdown");
    }

    #[rstest]
    fn into_bytes_consumes_payload() {
        let msg = Message::binary(vec![9, 8, 7]);
        let bytes = msg.into_bytes();
        assert_eq!(&bytes[..], &[9, 8, 7]);
    }

    #[rstest]
    fn into_bytes_close_returns_empty() {
        let msg = Message::Close(Some(CloseFrame::new(1000, "bye")));
        assert!(msg.into_bytes().is_empty());
    }

    #[rstest]
    fn as_text_returns_str_for_valid_utf8() {
        let msg = Message::text("café");
        assert_eq!(msg.as_text(), Some("café"));
    }

    #[rstest]
    fn as_text_returns_none_for_invalid_utf8() {
        let msg = Message::Text(Bytes::from_static(&[0xFF, 0xFE]));
        assert!(msg.as_text().is_none());
    }

    #[rstest]
    fn as_text_returns_none_for_non_text() {
        assert!(Message::binary(vec![1u8]).as_text().is_none());
        assert!(Message::ping(Bytes::new()).as_text().is_none());
        assert!(Message::Close(None).as_text().is_none());
    }
}
