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

//! Socket types and type aliases.

use std::sync::Arc;

use bytes::Bytes;
use tokio::io::{ReadHalf, WriteHalf};
use tokio_tungstenite::MaybeTlsStream;

use crate::net::TcpStream;

/// The write half of a plain or TLS TCP stream.
pub type TcpWriter = WriteHalf<MaybeTlsStream<TcpStream>>;

/// The read half of a plain or TLS TCP stream.
pub type TcpReader = ReadHalf<MaybeTlsStream<TcpStream>>;

/// A thread‑safe callback for complete suffix‑framed messages.
pub type TcpMessageHandler = Arc<dyn Fn(&[u8]) + Send + Sync>;

/// A command processed by the socket writer task.
#[derive(Debug)]
pub enum WriterCommand<W = TcpWriter> {
    /// Replaces the writer after reconnection and reports whether buffered messages were drained.
    Update(W, tokio::sync::oneshot::Sender<bool>),
    /// Sends data to the server.
    Send(Bytes),
}
