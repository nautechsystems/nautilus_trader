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

//! Network abstractions for dependency injection.
//!
//! This module provides traits and types that allow our networking components
//! to work with both real networking (`tokio::net`) and simulated networking (`turmoil::net`)
//! through dependency injection.

use std::{future::Future, io::Result};

use tokio::io::{AsyncRead, AsyncWrite};

/// Trait for network types that can establish TCP connections.
pub trait TcpConnector: Send + Sync {
    type Stream: AsyncRead + AsyncWrite + Send + Unpin + 'static;

    /// Connect to the specified address.
    fn connect(&self, addr: &str) -> impl Future<Output = Result<Self::Stream>> + Send;
}

/// Production TCP connector using `tokio::net`.
#[derive(Default, Clone, Debug)]
pub struct RealTcpConnector;

impl TcpConnector for RealTcpConnector {
    type Stream = tokio::net::TcpStream;

    fn connect(&self, addr: &str) -> impl Future<Output = Result<Self::Stream>> + Send {
        tokio::net::TcpStream::connect(addr.to_string())
    }
}
