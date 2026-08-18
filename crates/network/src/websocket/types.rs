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

//! Shared transport halves, message handlers, and writer commands.
//!
//! [`MessageReader`] and the internal writer use [`BoxedWsTransport`], keeping lifecycle code
//! independent of the selected transport backend. [`MessageHandler`] receives messages without
//! connection identity, while [`EpochMessageHandler`] attaches the owning reader's connection epoch
//! to application messages and reconnect notifications.
//!
//! The channel handlers convert neutral [`Message`] values to Tungstenite messages at the adapter
//! compatibility boundary. `WriterCommand` remains internal so all sink access, connection swaps,
//! and reconnect buffering stay serialized in the writer task.

use std::{fmt::Debug, sync::Arc};

use futures_util::stream::{SplitSink, SplitStream};

use crate::{
    error::SendError,
    transport::{BoxedWsTransport, Message},
};

/// Sink half of the active WebSocket transport.
///
/// Backed by [`BoxedWsTransport`], so the writer is decoupled from the concrete
/// backend stream type. Sends are keyed off the neutral [`Message`] enum.
pub(crate) type MessageWriter = SplitSink<BoxedWsTransport, Message>;

/// Stream half of the active WebSocket transport.
///
/// Backed by [`BoxedWsTransport`], yielding neutral [`Message`] values regardless
/// of the underlying backend.
pub type MessageReader = SplitStream<BoxedWsTransport>;

/// Function type for handling WebSocket messages.
///
/// When provided, the client will spawn an internal task to read messages and pass them
/// to this handler. This enables automatic reconnection where the client can replace the
/// reader internally.
///
/// When `None`, the client returns a `MessageReader` stream (via `connect_stream`) that
/// the caller owns and reads from directly. This disables automatic reconnection because
/// the reader cannot be replaced - the caller must manually reconnect.
pub type MessageHandler = Arc<dyn Fn(Message) + Send + Sync>;

/// Function type for handling WebSocket messages with connection ownership.
///
/// The initial connection has epoch `0`. Each replacement connection increments the epoch, and
/// every application or reconnect-control message carries the epoch of its originating reader.
pub type EpochMessageHandler = Arc<dyn Fn(u64, Message) + Send + Sync>;

/// Function type for handling WebSocket ping messages.
pub type PingHandler = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

/// Function type for handling WebSocket ping messages with connection ownership.
pub type EpochPingHandler = Arc<dyn Fn(u64, Vec<u8>) + Send + Sync>;

/// Creates a channel-based message handler.
///
/// The handler accepts neutral [`Message`] values. The receiver yields
/// `tokio_tungstenite::tungstenite::Message` values for adapter compatibility.
#[must_use]
pub fn channel_message_handler() -> (
    MessageHandler,
    tokio::sync::mpsc::UnboundedReceiver<tokio_tungstenite::tungstenite::Message>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handler: MessageHandler = Arc::new(move |msg: Message| {
        match tokio_tungstenite::tungstenite::Message::try_from(msg) {
            Ok(legacy) => {
                if let Err(e) = tx.send(legacy) {
                    log::debug!("Failed to send message to channel: {e}");
                }
            }
            Err(e) => log::debug!("Dropping message that failed legacy conversion: {e}"),
        }
    });
    (handler, rx)
}

/// Creates a channel-based message handler carrying each reader's connection epoch.
///
/// The receiver preserves the handler's `(connection_epoch, message)` order.
#[must_use]
pub fn channel_epoch_message_handler() -> (
    EpochMessageHandler,
    tokio::sync::mpsc::UnboundedReceiver<(u64, tokio_tungstenite::tungstenite::Message)>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handler: EpochMessageHandler =
        Arc::new(
            move |epoch, msg: Message| match tokio_tungstenite::tungstenite::Message::try_from(msg)
            {
                Ok(legacy) => {
                    if let Err(e) = tx.send((epoch, legacy)) {
                        log::debug!("Failed to send connection message to channel: {e}");
                    }
                }
                Err(e) => log::debug!("Dropping message that failed legacy conversion: {e}"),
            },
        );
    (handler, rx)
}

/// A command processed by the WebSocket writer task.
pub(crate) enum WriterCommand {
    /// Replaces the writer after reconnection.
    Update(MessageWriter, tokio::sync::oneshot::Sender<u64>),
    /// Sends a message to the server.
    Send(Message),
    /// Sends a connection keepalive. Never replayed on a replacement connection.
    Heartbeat(Message),
    /// Sends once if the active connection still owns the message.
    SendOnConnection {
        message: Message,
        connection_epoch: u64,
        response_tx: tokio::sync::oneshot::Sender<Result<(), SendError>>,
    },
    /// Sends a pong if the active connection still owns the ping that caused it.
    SendPongOnConnection {
        data: Vec<u8>,
        connection_epoch: u64,
    },
}

impl Debug for WriterCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Update(_, _) => f.debug_tuple("Update").field(&"<writer>").finish(),
            Self::Send(msg) => f.debug_tuple("Send").field(msg).finish(),
            Self::Heartbeat(msg) => f.debug_tuple("Heartbeat").field(msg).finish(),
            Self::SendOnConnection { message, .. } => {
                f.debug_tuple("SendOnConnection").field(message).finish()
            }
            Self::SendPongOnConnection { data, .. } => {
                f.debug_tuple("SendPongOnConnection").field(data).finish()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use rstest::rstest;
    use tokio_tungstenite::tungstenite::Message as TgMessage;

    use super::*;
    use crate::RECONNECTED;

    #[rstest]
    fn channel_handler_drops_invalid_utf8_text_without_panic() {
        let (handler, mut rx) = channel_message_handler();

        // Invalid UTF-8 in a Text frame must not propagate or panic,
        // the shim logs and drops it when Utf8Bytes::try_from fails.
        handler(Message::Text(Bytes::from_static(&[0xFF, 0xFE])));
        handler(Message::Binary(Bytes::from_static(b"ok")));

        let received = rx.try_recv().expect("binary should arrive");
        assert!(matches!(received, TgMessage::Binary(ref b) if b.as_ref() == b"ok"));
        assert!(rx.try_recv().is_err(), "no further messages expected");
    }

    #[rstest]
    fn channel_handler_forwards_valid_text() {
        let (handler, mut rx) = channel_message_handler();
        handler(Message::text("hello"));

        let received = rx.try_recv().expect("text should arrive");
        match received {
            TgMessage::Text(t) => assert_eq!(t.as_str(), "hello"),
            other => panic!("expected text, was {other:?}"),
        }
    }

    #[rstest]
    fn epoch_message_handler_forwards_epoch_and_text() {
        let (handler, mut rx) = channel_epoch_message_handler();
        handler(7, Message::text("hello"));

        let (connection_epoch, received) = rx.try_recv().expect("text should arrive");
        assert_eq!(connection_epoch, 7);
        match received {
            TgMessage::Text(text) => assert_eq!(text.as_str(), "hello"),
            other => panic!("expected text, was {other:?}"),
        }
    }

    #[rstest]
    fn channel_handler_forwards_reconnected_text_to_rust_consumers() {
        let (handler, mut rx) = channel_message_handler();
        handler(Message::text(RECONNECTED));

        let received = rx.try_recv().expect("reconnect text should arrive");
        match received {
            TgMessage::Text(t) => assert_eq!(t.as_str(), RECONNECTED),
            other => panic!("expected text, was {other:?}"),
        }
    }

    #[rstest]
    fn writer_command_send_debug_includes_message() {
        let cmd = WriterCommand::Send(Message::text("hi"));
        let formatted = format!("{cmd:?}");
        assert!(
            formatted.starts_with("Send("),
            "unexpected debug output: {formatted}"
        );
        assert!(
            formatted.contains("Text"),
            "debug output should retain the message variant: {formatted}"
        );
        assert!(
            formatted.contains("hi"),
            "debug output should retain the payload bytes: {formatted}"
        );
    }
}
