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

//! Python bindings for system configuration types.

use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use pyo3::prelude::*;

use crate::config::{RotationConfig, StreamingConfig};

const NANOSECONDS_PER_DAY: u64 = 86_400_000_000_000;

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl StreamingConfig {
    /// Creates a configuration for streaming data and events to Feather files.
    #[new]
    #[pyo3(signature = (
        catalog_path,
        fs_protocol=None,
        flush_interval_ms=None,
        replace_existing=false,
        rotation_mode="NO_ROTATION",
        max_file_size=1_073_741_824,
        rotation_interval_ns=None,
        schedule_ns=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        catalog_path: String,
        fs_protocol: Option<String>,
        flush_interval_ms: Option<u64>,
        replace_existing: bool,
        rotation_mode: &str,
        max_file_size: u64,
        rotation_interval_ns: Option<u64>,
        schedule_ns: Option<u64>,
    ) -> PyResult<Self> {
        let rotation_config = match rotation_mode.to_ascii_uppercase().as_str() {
            "SIZE" => {
                if max_file_size == 0 {
                    return Err(to_pyvalue_err("max_file_size must be positive"));
                }
                RotationConfig::Size {
                    max_size: max_file_size,
                }
            }
            "INTERVAL" => RotationConfig::Interval {
                interval_ns: positive_interval(rotation_interval_ns)?,
            },
            "SCHEDULED_DATES" => RotationConfig::ScheduledDates {
                interval_ns: positive_interval(rotation_interval_ns)?,
                schedule_ns: UnixNanos::from(schedule_ns.unwrap_or(0)),
            },
            "NO_ROTATION" => RotationConfig::NoRotation,
            value => {
                return Err(to_pyvalue_err(format!("Invalid rotation_mode: '{value}'")));
            }
        };

        Self::builder()
            .catalog_path(catalog_path)
            .fs_protocol(fs_protocol.unwrap_or_else(|| "file".to_string()))
            .flush_interval_ms(flush_interval_ms.unwrap_or(1_000))
            .replace_existing(replace_existing)
            .rotation_config(rotation_config)
            .build()
            .map_err(nautilus_common::python::config_error_to_pyvalue_err)
    }

    #[getter]
    fn catalog_path(&self) -> &str {
        &self.catalog_path
    }

    #[getter]
    fn fs_protocol(&self) -> &str {
        &self.fs_protocol
    }

    #[getter]
    const fn flush_interval_ms(&self) -> u64 {
        self.flush_interval_ms
    }

    #[getter]
    const fn replace_existing(&self) -> bool {
        self.replace_existing
    }

    #[getter]
    fn rotation_mode(&self) -> &'static str {
        match self.rotation_config {
            RotationConfig::Size { .. } => "SIZE",
            RotationConfig::Interval { .. } => "INTERVAL",
            RotationConfig::ScheduledDates { .. } => "SCHEDULED_DATES",
            RotationConfig::NoRotation => "NO_ROTATION",
        }
    }

    #[getter]
    fn max_file_size(&self) -> Option<u64> {
        match self.rotation_config {
            RotationConfig::Size { max_size } => Some(max_size),
            _ => None,
        }
    }

    #[getter]
    fn rotation_interval_ns(&self) -> Option<u64> {
        match self.rotation_config {
            RotationConfig::Interval { interval_ns }
            | RotationConfig::ScheduledDates { interval_ns, .. } => Some(interval_ns),
            _ => None,
        }
    }

    #[getter]
    fn schedule_ns(&self) -> Option<u64> {
        match self.rotation_config {
            RotationConfig::ScheduledDates { schedule_ns, .. } => Some(schedule_ns.as_u64()),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

fn positive_interval(interval_ns: Option<u64>) -> PyResult<u64> {
    let interval_ns = interval_ns.unwrap_or(NANOSECONDS_PER_DAY);
    if interval_ns == 0 {
        return Err(to_pyvalue_err("rotation_interval_ns must be positive"));
    }
    Ok(interval_ns)
}
