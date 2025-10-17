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

//! Network abstractions for dependency injection and testing.
//!
//! This module provides traits and types that allow our networking components
//! to work with both real networking (`tokio::net`) and simulated networking (`turmoil::net`)
//! through dependency injection.
//!
//! ## Conditional Compilation
//!
//! The module re-exports TCP types that are swapped at compile time:
//! - Default builds: `tokio::net::{TcpStream, TcpListener}`
//! - Builds with `--features turmoil`: `turmoil::net::{TcpStream, TcpListener}`
//!
//! This allows production code to be tested with turmoil's network simulation
//! without runtime overhead or code changes when the feature flag is enabled.

use std::{future::Future, io::Result};

use tokio::io::{AsyncRead, AsyncWrite};
// Re-export TCP types based on build configuration
// Production: use tokio networking
#[cfg(not(feature = "turmoil"))]
pub use tokio::net::{TcpListener, TcpStream};
// Testing with turmoil: use turmoil's simulated networking
#[cfg(feature = "turmoil")]
pub use turmoil::net::{TcpListener, TcpStream};

/// Trait for network types that can establish TCP connections.
pub trait TcpConnector: Send + Sync {
    type Stream: AsyncRead + AsyncWrite + Send + Unpin + 'static;

    /// Connect to the specified address.
    fn connect(&self, addr: &str) -> impl Future<Output = Result<Self::Stream>> + Send;
}

/// Production TCP connector.
///
/// Uses `tokio::net::TcpStream` in production, `turmoil::net::TcpStream` in turmoil tests.
#[derive(Default, Clone, Debug)]
pub struct RealTcpConnector;

impl TcpConnector for RealTcpConnector {
    type Stream = TcpStream;

    fn connect(&self, addr: &str) -> impl Future<Output = Result<Self::Stream>> + Send {
        TcpStream::connect(addr.to_string())
    }
}
