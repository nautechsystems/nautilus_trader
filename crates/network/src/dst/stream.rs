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

//! Buffered-flush compatibility for Madsim network streams.

use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use madsim::net::ToSocketAddrs;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// A deterministic byte stream with no-op empty flushes.
///
/// Madsim does not model TCP half-close: shutdown flushes pending bytes, and
/// dropping the stream closes the connection. Partial writes and socket options
/// are also outside this model.
#[derive(Debug)]
pub struct TcpStream {
    inner: madsim::net::TcpStream,
    dirty: bool,
}

impl TcpStream {
    /// Connects to a peer inside the active simulation.
    ///
    /// # Errors
    /// Returns an error when the simulated peer cannot be reached.
    pub async fn connect(addr: impl ToSocketAddrs) -> io::Result<Self> {
        Ok(Self {
            inner: madsim::net::TcpStream::connect(addr).await?,
            dirty: false,
        })
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, buf);
        if matches!(result, Poll::Ready(Ok(n)) if n > 0) {
            self.dirty = true;
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Madsim 0.2.34 sends empty flushes as chunks and can fail after peer close
        if !self.dirty {
            return Poll::Ready(Ok(()));
        }
        let result = Pin::new(&mut self.inner).poll_flush(cx);
        if matches!(result, Poll::Ready(Ok(()))) {
            self.dirty = false;
        }
        result
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}

/// A listener for byte streams inside the active simulation.
#[derive(Debug)]
pub struct TcpListener(madsim::net::TcpListener);

impl TcpListener {
    /// Binds an address inside the active simulation.
    ///
    /// # Errors
    /// Returns an error when the address cannot be bound.
    pub async fn bind(addr: impl ToSocketAddrs) -> io::Result<Self> {
        Ok(Self(madsim::net::TcpListener::bind(addr).await?))
    }

    /// Accepts a simulated byte stream.
    ///
    /// # Errors
    /// Returns an error when the listener cannot accept a connection.
    pub async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        let (inner, peer) = self.0.accept().await?;
        Ok((
            TcpStream {
                inner,
                dirty: false,
            },
            peer,
        ))
    }
}
