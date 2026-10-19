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

//! Neutral error type for the WebSocket transport abstraction.

use std::io;

use thiserror::Error;

use super::message::CloseFrame;

/// A backend-agnostic WebSocket transport error.
///
/// Each backend translates its native error type into this enum via `From` impls
/// so the higher layers operate against a single error surface.
#[derive(Debug, Error)]
pub enum TransportError {
    /// Underlying I/O error from the socket.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// HTTP upgrade handshake failed.
    #[error("handshake failed: {0}")]
    Handshake(String),

    /// Server rejected the WebSocket HTTP upgrade.
    #[error("WebSocket upgrade rejected with status {0}")]
    UpgradeRejected(u16),

    /// Proxy rejected the HTTP CONNECT request.
    #[error("proxy CONNECT rejected with status {0}")]
    ProxyConnectRejected(u16),

    /// URL was invalid or unsupported.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// TLS-layer failure during connect or stream operation.
    #[error("TLS error: {0}")]
    Tls(String),

    /// WebSocket protocol violation reported by the peer or detected locally.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// Peer sent a close frame and the connection is closing.
    #[error("connection closed by peer")]
    ClosedByPeer(Option<CloseFrame>),

    /// Connection closed without a close frame (abnormal).
    #[error("connection closed")]
    ConnectionClosed,

    /// Connection reset by peer.
    #[error("connection reset")]
    ConnectionReset,

    /// Message exceeded the configured maximum size.
    #[error("message too large")]
    MessageTooLarge,

    /// Frame exceeded the configured maximum size.
    #[error("frame too large")]
    FrameTooLarge,

    /// UTF-8 validation failed on a text frame.
    ///
    /// Both shipped backends validate incoming text frames: tokio-tungstenite
    /// during frame assembly and sockudo-ws at parse time.
    #[error("invalid UTF-8 in text frame")]
    InvalidUtf8,

    /// Backend returned an error not covered by other variants. Carries a
    /// short description; consumers should treat as fatal.
    #[error("transport error: {0}")]
    Other(String),
}

impl TransportError {
    /// Returns an error description without URL, server, or operating-system error text.
    ///
    /// Retains the transport error category, I/O kind, and HTTP rejection status.
    #[must_use]
    pub fn redacted_message(&self) -> String {
        match self {
            Self::Io(e) => format!("I/O error: {:?}", e.kind()),
            Self::Handshake(_) => "handshake failed".to_string(),
            Self::InvalidUrl(_) => "invalid URL".to_string(),
            Self::Tls(_) => "TLS error".to_string(),
            Self::Protocol(_) => "protocol error".to_string(),
            Self::Other(_) => "transport error".to_string(),
            Self::UpgradeRejected(_)
            | Self::ProxyConnectRejected(_)
            | Self::ClosedByPeer(_)
            | Self::ConnectionClosed
            | Self::ConnectionReset
            | Self::MessageTooLarge
            | Self::FrameTooLarge
            | Self::InvalidUtf8 => self.to_string(),
        }
    }

    /// Returns `true` if the error indicates the connection is no longer usable.
    ///
    /// `InvalidUrl` is the only non-fatal variant: a bad URL is a caller-side
    /// configuration mistake that does not damage an existing connection.
    /// Everything else (including `Io` and the catch-all `Other`) implies the
    /// underlying transport cannot be reused.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        !matches!(self, Self::InvalidUrl(_))
    }

    /// Returns `true` for connection-closed style errors.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        matches!(
            self,
            Self::ConnectionClosed | Self::ConnectionReset | Self::ClosedByPeer(_)
        )
    }
}

// Keep initial-connect retry policy and handshake log severity consistent
pub(crate) const fn retryable_status(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500..=599)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        TransportError::Io(io::Error::new(io::ErrorKind::ConnectionRefused, "SYN-secret")),
        "I/O error: ConnectionRefused"
    )]
    #[case(
        TransportError::UpgradeRejected(401),
        "WebSocket upgrade rejected with status 401"
    )]
    #[case(
        TransportError::ProxyConnectRejected(407),
        "proxy CONNECT rejected with status 407"
    )]
    #[case(TransportError::Handshake("SYN-secret".to_string()), "handshake failed")]
    #[case(TransportError::InvalidUrl("SYN-secret".to_string()), "invalid URL")]
    #[case(TransportError::Tls("SYN-secret".to_string()), "TLS error")]
    #[case(TransportError::Protocol("SYN-secret".to_string()), "protocol error")]
    #[case(
        TransportError::ClosedByPeer(Some(CloseFrame::new(1000, "SYN-secret"))),
        "connection closed by peer"
    )]
    #[case(TransportError::ConnectionClosed, "connection closed")]
    #[case(TransportError::ConnectionReset, "connection reset")]
    #[case(TransportError::MessageTooLarge, "message too large")]
    #[case(TransportError::FrameTooLarge, "frame too large")]
    #[case(TransportError::InvalidUtf8, "invalid UTF-8 in text frame")]
    #[case(TransportError::Other("SYN-secret".to_string()), "transport error")]
    fn test_redacted_message_preserves_classification(
        #[case] error: TransportError,
        #[case] expected: &str,
    ) {
        assert_eq!(error.redacted_message(), expected);
    }

    #[rstest]
    fn io_error_is_fatal() {
        let err = TransportError::Io(io::Error::other("boom"));
        assert!(err.is_fatal());
        assert!(!err.is_closed());
    }

    #[rstest]
    fn other_error_is_fatal() {
        let err = TransportError::Other("unexpected".into());
        assert!(err.is_fatal());
        assert!(!err.is_closed());
    }

    #[rstest]
    fn invalid_url_is_not_fatal() {
        let err = TransportError::InvalidUrl("ws://".into());
        assert!(!err.is_fatal());
        assert!(!err.is_closed());
    }

    #[rstest]
    fn closed_variants_are_closed_and_fatal() {
        let err = TransportError::ConnectionClosed;
        assert!(err.is_fatal());
        assert!(err.is_closed());

        let err = TransportError::ConnectionReset;
        assert!(err.is_fatal());
        assert!(err.is_closed());

        let err = TransportError::ClosedByPeer(Some(CloseFrame::new(1000, "bye")));
        assert!(err.is_fatal());
        assert!(err.is_closed());
    }

    #[rstest]
    fn protocol_error_is_fatal() {
        let err = TransportError::Protocol("bad opcode".into());
        assert!(err.is_fatal());
        assert!(!err.is_closed());
    }

    #[rstest]
    fn capacity_and_handshake_variants_are_fatal() {
        for err in [
            TransportError::MessageTooLarge,
            TransportError::FrameTooLarge,
            TransportError::InvalidUtf8,
            TransportError::Tls("bad".into()),
            TransportError::Handshake("bad".into()),
            TransportError::UpgradeRejected(429),
            TransportError::ProxyConnectRejected(503),
        ] {
            assert!(err.is_fatal(), "expected fatal: {err:?}");
            assert!(!err.is_closed(), "expected not closed: {err:?}");
        }
    }
}
