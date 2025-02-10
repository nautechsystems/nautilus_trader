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

use tokio::runtime::Runtime;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Retrieves a reference to a globally shared Tokio runtime.
/// The runtime is lazily initialized on the first call and reused thereafter.
///
/// This global runtime is intended for use cases where passing a runtime
/// around is impractical. It uses default configuration values.
///
/// # Panics
///
/// Panics if the runtime could not be created, which typically indicates
/// an inability to spawn threads or allocate necessary resources.
pub fn get_runtime() -> &'static tokio::runtime::Runtime {
    // Using default configuration values for now
    RUNTIME.get_or_init(|| Runtime::new().expect("Failed to create tokio runtime"))
}
