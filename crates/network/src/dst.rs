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
//! Re-exports time-related async primitives so every call site in
//! `nautilus-network` routes through one cfg-gated location. Under
//! `simulation` + `cfg(madsim)`, re-exports from `madsim::time` so waits and
//! timeouts advance with madsim's virtual clock. Otherwise re-exports from
//! `tokio::time`.
//!
//! `Instant` is routed the same way so that `now()` reads and `sleep`/`timeout`
//! waits share a single clock base. Using `tokio::time::Instant` on normal
//! builds keeps the seam compatible with `#[tokio::test(start_paused = true)]`
//! tests that drive time via `tokio::time::advance`.
//!
//! `nautilus-network` sits below `nautilus-common` in the dependency graph and
//! cannot import from `nautilus_common::live::dst`, which is why this helper
//! is crate-local.

pub mod time {
    #[cfg(not(all(feature = "simulation", madsim)))]
    pub use tokio::time::{Instant, sleep, timeout};

    #[cfg(all(feature = "simulation", madsim))]
    pub use madsim::time::{Instant, sleep, timeout};
}
