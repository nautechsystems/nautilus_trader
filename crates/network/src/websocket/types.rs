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

//! Type definitions for WebSocket operations.

use std::{fmt::Debug, sync::Arc};

use futures_util::stream::{SplitSink, SplitStream};

use crate::transport::{BoxedWsTransport, Message};

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

/// Function type for handling WebSocket ping messages.
pub type PingHandler = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

/// Creates a channel-based message handler.
///
/// Returns a tuple containing the message handler and a receiver for messages.
///
/// During the migration to the [`crate::transport`] abstraction the receiver still
/// yields `tokio_tungstenite::tungstenite::Message` so the 18 adapter crates keep
/// compiling unchanged. The neutral [`Message`] is converted at the channel
/// boundary; phase 3 of the migration switches both ends to the neutral type and
/// removes this shim.
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

/// Represents a command for the writer task.
pub(crate) enum WriterCommand {
    /// Update the writer reference with a new one after reconnection.
    Update(MessageWriter, tokio::sync::oneshot::Sender<bool>),
    /// Send message to the server.
    Send(Message),
}

impl Debug for WriterCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Update(_, _) => f.debug_tuple("Update").field(&"<writer>").finish(),
            Self::Send(msg) => f.debug_tuple("Send").field(msg).finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use rstest::rstest;
    use tokio_tungstenite::tungstenite::Message as TgMessage;

    use super::*;

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
