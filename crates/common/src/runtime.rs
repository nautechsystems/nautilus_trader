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

//! The centralized Tokio runtime for a running Nautilus system.

#[cfg(feature = "python")]
use std::sync::Once;
use std::{sync::OnceLock, time::Duration};

#[cfg(feature = "python")]
use pyo3::Python;
use tokio::{runtime::Builder, task, time::timeout};

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
#[cfg(feature = "python")]
static PYTHON_INIT: Once = Once::new();

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
    #[cfg(feature = "python")]
    {
        // Python hosts the process when we build as an extension module. Initializing
        // here keeps the interpreter alive for the lifetime of the shared Tokio runtime
        // so every worker thread sees a prepared PyO3 environment before using it.
        PYTHON_INIT.call_once(|| {
            Python::initialize();
        });
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

/// Returns a reference to the global Nautilus Tokio runtime.
///
/// The runtime is lazily initialized on the first call and reused thereafter.
/// Intended for use cases where passing a runtime around is impractical.
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
