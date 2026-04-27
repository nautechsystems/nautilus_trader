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

//! Deterministic simulation testing (DST) re-export module.
//!
//! Feature-switched re-exports of the tokio primitives that need deterministic
//! behavior under simulation: `time`, `task`, `runtime`, and `signal`. Code on
//! the DST path imports async primitives from here rather than directly from
//! `tokio`, so that toggling the `simulation` feature switches the whole
//! runtime in one place.
//!
//! # Feature-switched behavior
//!
//! The switch requires both the `simulation` Cargo feature (which enables the
//! `madsim` dependency) and `RUSTFLAGS="--cfg madsim"` (which activates the
//! simulation re-exports). Without both, all paths route to real tokio.
//!
//! Only four submodules switch: `time`, `task`, `runtime`, and `signal`. All
//! other tokio modules (`sync`, `io`, `select!`, `fs`, `net`) use real tokio
//! unconditionally. Transitive crates (`tokio-tungstenite`, `reqwest`, ...)
//! are unaffected.
//!
//! # Surface
//!
//! The submodules re-export the following items. A compile-time probe at the
//! bottom of this file keeps the list and the signatures of the non-generic
//! free functions consistent across the tokio and madsim routes, and fires on
//! `cargo build` (not only `cargo test`) so breakage surfaces upstream.
//!
//! - `time`: `Duration`, `Instant`, `Interval`, `MissedTickBehavior`, `Sleep`,
//!   `error` (submodule), `interval`, `interval_at`, `sleep`, `sleep_until`,
//!   `timeout`
//! - `task`: `JoinHandle`, `spawn`, `spawn_local`, `yield_now`
//! - `runtime`: `Builder`, `Handle`, `Runtime`
//! - `signal`: `ctrl_c`
//!
//! # Related seam
//!
//! Monotonic time (`Instant`) goes through this module. Wall-clock time
//! (`SystemTime` / Unix epoch) is a separate seam: see
//! `nautilus_core::time::duration_since_unix_epoch`. Collapsing the two would
//! lose epoch information and break order and fill timestamps.

/// Deterministic time: virtual time under simulation, real time in production.
///
/// Under simulation (`simulation` + `cfg(madsim)`), `Instant` is
/// `std::time::Instant` (madsim intercepts the system clock calls). `sleep`,
/// `timeout`, `interval` advance in virtual time controlled by the
/// deterministic scheduler.
pub mod time {
    pub use std::time::Duration;

    #[cfg(all(feature = "simulation", madsim))]
    pub use madsim::time::{
        Instant, Interval, MissedTickBehavior, Sleep, error, interval, interval_at, sleep,
        sleep_until, timeout,
    };
    #[cfg(not(all(feature = "simulation", madsim)))]
    pub use tokio::time::{
        Instant, Interval, MissedTickBehavior, Sleep, error, interval, interval_at, sleep,
        sleep_until, timeout,
    };
}

/// Deterministic task spawning: fixed-order scheduler under simulation.
pub mod task {
    #[cfg(all(feature = "simulation", madsim))]
    pub use madsim::task::{JoinHandle, spawn, spawn_local, yield_now};
    #[cfg(not(all(feature = "simulation", madsim)))]
    pub use tokio::task::{JoinHandle, spawn, spawn_local, yield_now};
}

/// Deterministic runtime: single-threaded sim runtime under simulation.
///
/// Under simulation (`simulation` + `cfg(madsim)`),
/// `Builder::new_multi_thread()` returns a single-threaded deterministic
/// runtime. `worker_threads()` and `enable_all()` are no-ops.
pub mod runtime {
    #[cfg(not(all(feature = "simulation", madsim)))]
    pub use tokio::runtime::{Builder, Handle, Runtime};

    #[cfg(all(feature = "simulation", madsim))]
    mod sim {
        use std::io;

        /// Tokio-compatible runtime builder that produces a madsim runtime.
        #[derive(Debug)]
        pub struct Builder {
            _inner: (),
        }

        impl Builder {
            #[must_use]
            pub fn new_current_thread() -> Self {
                Self { _inner: () }
            }

            #[must_use]
            pub fn new_multi_thread() -> Self {
                Self { _inner: () }
            }

            #[must_use]
            pub fn worker_threads(&mut self, _val: usize) -> &mut Self {
                self
            }

            #[must_use]
            pub fn thread_name(&mut self, _val: impl Into<String>) -> &mut Self {
                self
            }

            #[must_use]
            pub fn enable_all(&mut self) -> &mut Self {
                self
            }

            /// # Errors
            ///
            /// Returns an error if the runtime cannot be created.
            pub fn build(&mut self) -> io::Result<madsim::runtime::Runtime> {
                Ok(madsim::runtime::Runtime::new())
            }
        }

        pub use madsim::runtime::Handle;
        pub type Runtime = madsim::runtime::Runtime;
    }

    #[cfg(all(feature = "simulation", madsim))]
    pub use sim::{Builder, Handle, Runtime};
}

/// Deterministic signal handling: injectable signals under simulation.
///
/// Under simulation (`simulation` + `cfg(madsim)`), `ctrl_c()` responds to
/// `madsim::runtime::Handle::send_ctrl_c(node_id)` from test code.
pub mod signal {
    #[cfg(all(feature = "simulation", madsim))]
    pub use madsim::signal::ctrl_c;
    #[cfg(not(all(feature = "simulation", madsim)))]
    pub use tokio::signal::ctrl_c;
}

/// Compile-time probe of the DST re-export surface.
///
/// Names every item listed in the module-level "Surface" section and pins the
/// signatures of the non-generic free functions via function pointer coercion.
/// Removing, renaming, cfg-gating, or changing the signature of any of them on
/// either the tokio or madsim route causes this module to fail to compile, so
/// both routes stay in sync. Compiled unconditionally so plain `cargo build`
/// fires it, not only `cargo test`.
///
/// Generic free functions (`timeout`, `spawn`, `spawn_local`) and `async fn`s
/// (`yield_now`, `ctrl_c`) cannot be written as concrete function pointers
/// here, so their `use` binding only locks the name, not the signature.
///
/// Visibility narrowing (e.g. `pub` -> `pub(crate)`) cannot be detected from
/// inside the defining crate; that gap is structurally outside this probe.
///
/// Shape check only, not a behavior test.
#[allow(unused_imports)]
mod surface {
    use super::{
        runtime::{Builder, Handle, Runtime},
        signal::ctrl_c,
        task::{JoinHandle, spawn, spawn_local, yield_now},
        time::{
            Duration, Instant, Interval, MissedTickBehavior, Sleep, error, interval, interval_at,
            sleep, sleep_until, timeout,
        },
    };

    const _: fn(Duration) -> Sleep = sleep;
    const _: fn(Instant) -> Sleep = sleep_until;
    const _: fn(Duration) -> Interval = interval;
    const _: fn(Instant, Duration) -> Interval = interval_at;
}

#[cfg(test)]
mod tests {
    #[cfg(all(feature = "simulation", madsim))]
    use nautilus_core::{datetime::NANOSECONDS_IN_SECOND, time::nanos_since_unix_epoch};
    use rstest::rstest;

    use super::*;

    // -- Normal build tests (real tokio) --

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn test_dst_sleep() {
        let start = time::Instant::now();
        time::sleep(time::Duration::from_millis(10)).await;
        let elapsed = start.elapsed();
        assert!(elapsed >= time::Duration::from_millis(5));
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn test_dst_task_spawn() {
        let handle = task::spawn(async { 42 });
        let result = handle.await.unwrap();
        assert_eq!(result, 42);
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn test_real_tokio_sync_alongside_dst() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(7).unwrap();
        let result = time::timeout(time::Duration::from_secs(1), rx.recv()).await;
        assert_eq!(result.unwrap(), Some(7));
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    fn test_dst_runtime_builder() {
        let rt = runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(async { 99 });
        assert_eq!(result, 99);
    }

    // -- Simulation build tests (madsim) --

    #[cfg(all(feature = "simulation", madsim))]
    #[madsim::test]
    async fn test_dst_sleep() {
        let start = time::Instant::now();
        time::sleep(time::Duration::from_millis(100)).await;
        let elapsed = start.elapsed();
        // Virtual time: ~100ms with sub-ms scheduling epsilon.
        // Real tokio would show 100-115ms from OS jitter.
        assert!(elapsed >= time::Duration::from_millis(100));
        assert!(elapsed < time::Duration::from_millis(101));
    }

    #[cfg(all(feature = "simulation", madsim))]
    #[madsim::test]
    async fn test_dst_task_spawn() {
        let handle = task::spawn(async { 42 });
        let result = handle.await.unwrap();
        assert_eq!(result, 42);
    }

    #[cfg(all(feature = "simulation", madsim))]
    #[madsim::test]
    async fn test_real_tokio_sync_alongside_dst() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(7).unwrap();
        let result = time::timeout(time::Duration::from_secs(1), rx.recv()).await;
        assert_eq!(result.unwrap(), Some(7));
    }

    #[cfg(all(feature = "simulation", madsim))]
    #[rstest]
    fn test_dst_runtime_builder() {
        let rt = runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(async { 99 });
        assert_eq!(result, 99);
    }

    // Pins the wall-clock seam end-to-end on the common leg: the `simulation`
    // feature on `nautilus-common` propagates `nautilus-core/simulation`, so
    // `nautilus_core::time::nanos_since_unix_epoch` routes through
    // `madsim::time::TimeHandle::current().now_time()`. Sleeping for 60
    // virtual seconds via the common DST `time::sleep` re-export must advance
    // the wall-clock value by 60s. If either the cfg gate or the propagation
    // regressed, the elapsed value would only reflect real wall-clock time
    // (~0ms) and this assertion would fail.
    #[cfg(all(feature = "simulation", madsim))]
    #[madsim::test]
    async fn test_dst_wall_clock_advances_with_virtual_time() {
        let before = nanos_since_unix_epoch();
        time::sleep(time::Duration::from_secs(60)).await;
        let after = nanos_since_unix_epoch();

        let elapsed_ns = after.saturating_sub(before);
        let sixty_seconds_ns = 60 * NANOSECONDS_IN_SECOND;
        assert!(
            elapsed_ns >= sixty_seconds_ns,
            "wall clock did not advance by full virtual sleep: elapsed={elapsed_ns}ns"
        );
    }
}
