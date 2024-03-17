// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

pub mod clock;
pub mod enums;
pub mod logging;
pub mod timer;
pub mod versioning;

use pyo3::prelude::*;

/// Loaded as nautilus_pyo3.common
#[pymodule]
pub fn common(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<crate::enums::ComponentState>()?;
    m.add_class::<crate::enums::ComponentTrigger>()?;
    m.add_class::<crate::enums::LogColor>()?;
    m.add_class::<crate::enums::LogLevel>()?;
    m.add_class::<crate::enums::LogFormat>()?;
    m.add_class::<crate::logging::logger::LoggerConfig>()?;
    m.add_class::<crate::logging::logger::LogGuard>()?;
    m.add_class::<crate::logging::writer::FileWriterConfig>()?;
    m.add_function(wrap_pyfunction!(logging::py_init_tracing, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_init_logging, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_logger_log, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_log_header, m)?)?;
    m.add_function(wrap_pyfunction!(logging::py_log_sysinfo, m)?)?;

    Ok(())
}
