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

//! The centralized Tokio runtime for a running Nautilus system.
//!
//! # Design Rationale
//!
//! NautilusTrader uses a single global Tokio runtime because:
//! - A single long-lived runtime avoids repeated startup/shutdown overhead.
//! - The runtime is lazily initialized on first call to `get_runtime()` via `OnceLock`.
//! - Worker thread count is configurable via the `NAUTILUS_WORKER_THREADS` environment variable.
//! - Rust-native hosts can install a pre-built runtime via [`set_runtime`] before first use.
//!
//! # Custom Runtime Injection
//!
//! Callers who use [`set_runtime`] must supply a multi-threaded runtime built with
//! `tokio::runtime::Builder::new_multi_thread()` and `enable_all()`. Adapters assume I/O,
//! timers, spawning, and `tokio::task::block_in_place()` are available.
//!
//! # Python Support
//!
//! When the `python` feature is enabled, the runtime initializes the Python interpreter
//! before starting worker threads. The PyO3 module registers an `atexit` handler via
//! `shutdown_runtime()` to cleanly shut down when Python exits.
//!
//! A runtime passed to [`set_runtime`] is already built, so this module cannot run the default
//! Python initialization hook before its worker threads start. Hosts using custom runtimes with
//! Python support must prepare Python before building the runtime.
//!
//! # Testing Considerations
//!
//! The global runtime pattern makes it harder to inject test doubles. For testing:
//! - Unit tests can use `#[tokio::test]` which creates its own runtime.
//! - Integration tests should be aware they share the global runtime state.

use std::{sync::OnceLock, time::Duration};

use tokio::{runtime::Builder, task, time::timeout};

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Environment variable name to configure the number of OS threads for the common runtime.
/// If not set or if the value cannot be parsed as a positive integer, the default value is used.
const NAUTILUS_WORKER_THREADS: &str = "NAUTILUS_WORKER_THREADS";

/// The default number of OS threads to use if the environment variable is not set.
///
/// 0 means Tokio will use the default (number of logical CPUs).
const DEFAULT_OS_THREADS: usize = 0;

/// Creates and configures a new multi-threaded Tokio runtime.
///
/// The number of OS threads is configured using the `NAUTILUS_WORKER_THREADS`
/// environment variable. If not set, all available logical CPUs will be used.
///
/// # Panics
///
/// Panics if the runtime could not be created, which typically indicates
/// an inability to spawn threads or allocate necessary resources.
fn initialize_runtime() -> tokio::runtime::Runtime {
    // Initialize Python if running as a Python extension module
    #[cfg(feature = "python")]
    {
        crate::python::runtime::initialize_python();
    }

    let worker_threads = std::env::var(NAUTILUS_WORKER_THREADS)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_OS_THREADS);

    let mut builder = Builder::new_multi_thread();

    let builder = if worker_threads > 0 {
        builder.worker_threads(worker_threads)
    } else {
        &mut builder
    };

    builder
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime")
}

/// Sets a custom pre-built Tokio runtime as the global Nautilus runtime.
///
/// Must be called before the first [`get_runtime`] invocation (i.e. before
/// `LiveNode::build()` or any adapter/client usage). This gives callers who
/// own `main()` full control over worker threads, blocking threads, thread
/// names, stack sizes, and any other [`tokio::runtime::Builder`] options.
///
/// # Runtime Requirements
///
/// The supplied runtime must be multi-threaded and have all Tokio drivers
/// enabled with `tokio::runtime::Builder::enable_all()`.
///
/// # Errors
///
/// Returns `Err(runtime)` if a runtime was already initialized.
pub fn set_runtime(runtime: tokio::runtime::Runtime) -> Result<(), tokio::runtime::Runtime> {
    RUNTIME.set(runtime)
}

/// Returns a reference to the global Nautilus Tokio runtime.
///
/// The runtime is lazily initialized on the first call and reused thereafter.
/// If a custom runtime was previously installed via [`set_runtime`], that
/// runtime is returned instead.
pub fn get_runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(initialize_runtime)
}

/// Provides a best-effort flush for runtime tasks during shutdown.
///
/// The function yields once to the Tokio scheduler and gives outstanding tasks a chance
/// to observe shutdown signals before Python finalizes the interpreter, which calls this via
/// an `atexit` hook.
pub fn shutdown_runtime(wait: Duration) {
    if let Some(runtime) = RUNTIME.get() {
        runtime.block_on(async {
            let _ = timeout(wait, async {
                task::yield_now().await;
            })
            .await;
        });
    }
}
