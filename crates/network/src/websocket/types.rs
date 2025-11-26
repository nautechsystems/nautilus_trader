// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::sync::Arc;

use futures_util::stream::{SplitSink, SplitStream};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

// Type aliases for different build configurations
#[cfg(not(feature = "turmoil"))]
pub(crate) type MessageWriter =
    SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, Message>;

#[cfg(not(feature = "turmoil"))]
pub type MessageReader = SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>;

#[cfg(feature = "turmoil")]
pub(crate) type MessageWriter =
    SplitSink<WebSocketStream<MaybeTlsStream<crate::net::TcpStream>>, Message>;

#[cfg(feature = "turmoil")]
pub type MessageReader = SplitStream<WebSocketStream<MaybeTlsStream<crate::net::TcpStream>>>;

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
#[must_use]
pub fn channel_message_handler() -> (
    MessageHandler,
    tokio::sync::mpsc::UnboundedReceiver<Message>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handler = Arc::new(move |msg: Message| {
        if let Err(e) = tx.send(msg) {
            tracing::debug!("Failed to send message to channel: {e}");
        }
    });
    (handler, rx)
}

/// Represents a command for the writer task.
#[derive(Debug)]
pub(crate) enum WriterCommand {
    /// Update the writer reference with a new one after reconnection.
    Update(MessageWriter, tokio::sync::oneshot::Sender<bool>),
    /// Send message to the server.
    Send(Message),
}
