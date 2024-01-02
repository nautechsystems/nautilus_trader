// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::{time::AtomicTime, uuid::UUID4};
use nautilus_model::identifiers::trader_id::TraderId;
use pyo3::prelude::*;

use crate::logging::{FileWriterConfig, Logger, LoggerConfig};

/// Initialize tracing.
///
/// Tracing is meant to be used to trace/debug async Rust code. It can be
/// configured to filter modules and write up to a specific level only using
/// by passing a configuration using the `RUST_LOG` environment variable.
///
/// # Safety
///
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
#[pyfunction]
pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init()
        .unwrap_or_else(|e| eprintln!("Cannot set tracing subscriber because of error: {}", e));
}

/// Initialize logging.
///
/// Logging should be used for Python and sync Rust logic which is most of
/// the components in the main `nautilus_trader` package.
/// Logging can be configured to filter components and write up to a specific level only
/// by passing a configuration using the `NAUTILUS_LOG` environment variable.
///
/// # Safety
///
/// Should only be called once during an applications run, ideally at the
/// beginning of the run.
#[pyfunction]
pub fn init_logging(
    clock: AtomicTime,
    trader_id: TraderId,
    instance_id: UUID4,
    file_writer_config: FileWriterConfig,
    config: LoggerConfig,
) {
    Logger::init_with_config(clock, trader_id, instance_id, file_writer_config, config);
}

#[pymethods]
impl LoggerConfig {
    #[staticmethod]
    #[pyo3(name = "from_spec")]
    pub fn py_from_spec(spec: String) -> Self {
        Self::from_spec(&spec)
    }

    #[staticmethod]
    #[pyo3(name = "from_env")]
    pub fn py_from_env() -> Self {
        Self::from_env()
    }
}

#[pymethods]
impl FileWriterConfig {
    #[new]
    pub fn py_new(
        directory: Option<String>,
        file_name: Option<String>,
        file_format: Option<String>,
    ) -> Self {
        Self::new(directory, file_name, file_format)
    }
}
