//! Socket types and type aliases.

use std::sync::Arc;

use bytes::Bytes;
use tokio::io::{ReadHalf, WriteHalf};
use tokio_tungstenite::MaybeTlsStream;

use crate::net::TcpStream;

pub type TcpWriter = WriteHalf<MaybeTlsStream<TcpStream>>;
pub type TcpReader = ReadHalf<MaybeTlsStream<TcpStream>>;
pub type TcpMessageHandler = Arc<dyn Fn(&[u8]) + Send + Sync>;

/// Represents a command for the writer task.
#[derive(Debug)]
pub enum WriterCommand {
    /// Update the writer reference with a new one after reconnection.
    Update(TcpWriter, tokio::sync::oneshot::Sender<bool>),
    /// Send data to the server.
    Send(Bytes),
}
