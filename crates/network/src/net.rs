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

//! Network abstractions for dependency injection and testing.
//!
//! The traits and type aliases let network clients use either real `tokio` networking or simulated
//! `turmoil` networking through dependency injection. `apply_socket_options` is the single place
//! every connect path configures its TCP socket.
//!
//! ## Conditional compilation
//!
//! The module selects TCP types at compile time:
//! - Default builds: `tokio::net::{TcpStream, TcpListener}`
//! - Builds with `--features turmoil`: `turmoil::net::{TcpStream, TcpListener}`
//!
//! Production code therefore runs against the simulator without source changes, while default
//! builds incur no runtime dispatch or simulation overhead.

#[cfg(not(feature = "turmoil"))]
use std::time::Duration;
use std::{future::Future, io::Result};

#[cfg(not(feature = "turmoil"))]
use socket2::{SockRef, TcpKeepalive};
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

    /// Connects to the specified address.
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
        TcpStream::connect(addr)
    }
}

/// Applies the standard socket options for a long-lived venue connection.
///
/// Disables Nagle so small frames leave immediately, enables TCP keepalive so a half-open peer is
/// detected in roughly a minute instead of the multi-hour platform default, and on Linux bounds
/// unacknowledged outbound data. Without keepalive, writes to a connection dropped by a NAT or load
/// balancer without a FIN or RST keep succeeding into the send buffer for as long as it takes that
/// buffer to fill.
///
/// These are a kernel-level backstop operating in tens of seconds. The application-level heartbeat
/// timeouts remain the primary detector, and are the only thing that sees a peer whose transport is
/// healthy but which has stopped sending.
///
/// A socket that rejects an option is still usable, so failures are logged and execution continues.
pub(crate) fn apply_socket_options(stream: &TcpStream) {
    if let Err(e) = stream.set_nodelay(true) {
        log::warn!("Failed to enable TCP_NODELAY: {e}");
    }

    apply_keepalive(stream);
}

/// Idle period before the kernel sends the first TCP keepalive probe.
#[cfg(not(feature = "turmoil"))]
const KEEPALIVE_TIME: Duration = Duration::from_secs(20);

/// Interval between successive TCP keepalive probes.
#[cfg(not(feature = "turmoil"))]
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

/// Unanswered TCP keepalive probes tolerated before the peer is declared gone.
#[cfg(not(feature = "turmoil"))]
const KEEPALIVE_RETRIES: u32 = 3;

/// Ceiling on how long transmitted data may stay unacknowledged before the kernel drops the
/// connection.
///
/// Must not be shorter than the full keepalive probe budget
/// (`KEEPALIVE_TIME + KEEPALIVE_INTERVAL * KEEPALIVE_RETRIES`), because Linux applies this timeout
/// to the probe sequence as well and would otherwise cut it short.
///
/// Linux additionally lets this value override `TCP_KEEPCNT`, dropping the connection once a probe
/// has been outstanding this long. Detection there is therefore governed by this timeout rather
/// than by `KEEPALIVE_RETRIES`, which only bounds the probe count on macOS and Windows.
#[cfg(all(not(feature = "turmoil"), target_os = "linux"))]
const UNACKED_DATA_TIMEOUT: Duration = Duration::from_mins(1);

#[cfg(not(feature = "turmoil"))]
fn apply_keepalive(stream: &TcpStream) {
    let socket = SockRef::from(stream);
    let keepalive = TcpKeepalive::new()
        .with_time(KEEPALIVE_TIME)
        .with_interval(KEEPALIVE_INTERVAL)
        .with_retries(KEEPALIVE_RETRIES);

    if let Err(e) = socket.set_tcp_keepalive(&keepalive) {
        log::warn!("Failed to enable TCP keepalive: {e}");
    }

    #[cfg(target_os = "linux")]
    if let Err(e) = socket.set_tcp_user_timeout(Some(UNACKED_DATA_TIMEOUT)) {
        log::warn!("Failed to set TCP_USER_TIMEOUT: {e}");
    }
}

/// The turmoil simulator models TCP without a file descriptor, so there is nothing for the
/// keepalive options to apply to.
#[cfg(feature = "turmoil")]
const fn apply_keepalive(_stream: &TcpStream) {}

#[cfg(all(test, not(feature = "turmoil")))]
mod tests {
    use rstest::rstest;
    use tokio::net::TcpListener;

    use super::*;

    #[rstest]
    #[tokio::test]
    async fn test_apply_socket_options_sets_nodelay_and_keepalive() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accept = tokio::spawn(async move { listener.accept().await.unwrap() });
        let stream = TcpStream::connect(addr).await.unwrap();
        let _accepted = accept.await.unwrap();

        apply_socket_options(&stream);

        let socket = SockRef::from(&stream);

        assert!(stream.nodelay().unwrap());
        assert!(socket.keepalive().unwrap());
        assert_eq!(socket.tcp_keepalive_time().unwrap(), KEEPALIVE_TIME);

        #[cfg(target_os = "linux")]
        assert_eq!(
            socket.tcp_user_timeout().unwrap(),
            Some(UNACKED_DATA_TIMEOUT)
        );
    }

    /// The kernel applies `TCP_USER_TIMEOUT` to the keepalive probe sequence, so a value below the
    /// full probe budget would cut detection short and silently defeat the retry count.
    #[cfg(target_os = "linux")]
    #[rstest]
    fn test_unacked_timeout_covers_keepalive_probe_budget() {
        let probe_budget = KEEPALIVE_TIME + KEEPALIVE_INTERVAL * KEEPALIVE_RETRIES;

        assert!(UNACKED_DATA_TIMEOUT >= probe_budget);
    }
}
