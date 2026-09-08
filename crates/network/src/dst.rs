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

//! Deterministic simulation testing (DST) seam for network async primitives.
//!
//! Routes time primitives, owned WebSocket tasks, and supported HTTP/WebSocket
//! byte streams through a network-local boundary. With `simulation` and
//! `cfg(madsim)`, clocks, tasks, and streams use Madsim. Normal builds use
//! Tokio clocks and tasks and the existing `crate::net` streams.
//!
//! `Instant` is routed the same way so that `now()` reads and `sleep`/`timeout`
//! waits share a single clock base. Using `tokio::time::Instant` on normal
//! builds keeps the seam compatible with `#[tokio::test(start_paused = true)]`
//! tests that drive time via `tokio::time::advance`.
//!
//! `nautilus-network` sits below `nautilus-common` in the dependency graph and
//! cannot import from `nautilus_common::live::dst`, which is why this module
//! lives in the network crate.

/// Clock and timeout primitives selected for deterministic simulation.
pub mod time {
    pub use std::time::Duration;

    #[cfg(all(feature = "simulation", madsim))]
    pub use madsim::time::{Instant, sleep, sleep_until, timeout};
    #[cfg(not(all(feature = "simulation", madsim)))]
    pub use tokio::time::{Instant, sleep, sleep_until, timeout};
}

/// Owned tasks selected for deterministic simulation.
pub mod task {
    #[cfg(all(feature = "simulation", madsim))]
    pub use madsim::task::{JoinHandle, spawn};
    #[cfg(not(all(feature = "simulation", madsim)))]
    pub use tokio::task::{JoinHandle, spawn};
}

/// Network byte streams selected for deterministic simulation.
pub mod net {
    #[cfg(all(feature = "simulation", madsim))]
    pub use super::stream::{TcpListener, TcpStream};
    #[cfg(not(all(feature = "simulation", madsim)))]
    pub use crate::net::{TcpListener, TcpStream};
}

#[cfg(all(feature = "simulation", madsim))]
mod stream;

#[cfg(test)]
mod tests {
    use super::*;

    // Under madsim, `time::Instant` and `time::sleep` both run on the virtual
    // clock with sub-ms scheduling epsilon. If the cfg gate fell through to
    // real tokio, `sleep` would block on the OS scheduler with ~5-15ms of
    // jitter and the tight upper bound would fail.
    #[cfg(all(feature = "simulation", madsim))]
    #[madsim::test]
    async fn test_dst_sleep_uses_virtual_time() {
        let start = time::Instant::now();
        time::sleep(time::Duration::from_millis(100)).await;
        let elapsed = start.elapsed();
        assert!(elapsed >= time::Duration::from_millis(100));
        assert!(
            elapsed < time::Duration::from_millis(101),
            "virtual sleep showed real-tokio jitter: {elapsed:?}"
        );
    }

    // Mirror under real tokio (paused clock) to keep both routes exercised.
    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn test_dst_sleep_advances_paused_clock() {
        let start = time::Instant::now();
        time::sleep(time::Duration::from_mins(1)).await;
        assert!(start.elapsed() >= time::Duration::from_mins(1));
    }

    #[cfg(all(feature = "simulation", madsim))]
    #[madsim::test]
    async fn test_dst_timeout_fires_in_virtual_time() {
        let result = time::timeout(
            time::Duration::from_millis(10),
            std::future::pending::<()>(),
        )
        .await;
        assert!(
            result.is_err(),
            "timeout should fire on a never-completing future"
        );
    }
}
