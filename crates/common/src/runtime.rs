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

use std::sync::OnceLock;

use tokio::runtime::Builder;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Environment variable name to configure the number of OS threads for the common runtime.
/// If not set or if the value cannot be parsed as a positive integer, the default value is used.
const NAUTILUS_WORKER_THREADS: &str = "NAUTILUS_WORKER_THREADS";

/// The default number of OS threads to use if the environment variable is not set.
///
/// 0 means Tokio will use the default (number of logical CPUs).
const DEFAULT_OS_THREADS: usize = 0;

/// Retrieves a reference to a globally shared Tokio runtime.
/// The runtime is lazily initialized on the first call and reused thereafter.
///
/// This global runtime is intended for use cases where passing a runtime
/// around is impractical. The number of OS threads is configured using the
/// `NAUTILUS_WORKER_THREADS` environment variable. If not set, all available
/// logical CPUs will be used.
///
/// # Panics
///
/// Panics if the runtime could not be created, which typically indicates
/// an inability to spawn threads or allocate necessary resources.
pub fn get_runtime() -> &'static tokio::runtime::Runtime {
    let worker_threads = std::env::var(NAUTILUS_WORKER_THREADS)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_OS_THREADS);

    RUNTIME.get_or_init(|| {
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
    })
}
