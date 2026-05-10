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

//! Python bindings for risk engine configuration.

use std::{collections::HashMap, str::FromStr};

use ahash::AHashMap;
use nautilus_common::throttler::RateLimit;
use nautilus_core::{datetime::NANOSECONDS_IN_SECOND, python::to_pyvalue_err};
use nautilus_model::identifiers::InstrumentId;
use pyo3::{Py, PyAny, PyResult, Python, prelude::PyAnyMethods, pymethods};
use rust_decimal::Decimal;

use crate::engine::config::RiskEngineConfig;

fn format_rate_limit(rate: &RateLimit) -> String {
    let total_secs = rate.interval_ns / NANOSECONDS_IN_SECOND;
    let hours = total_secs / 3_600;
    let minutes = (total_secs % 3_600) / 60;
    let seconds = total_secs % 60;
    format!("{}/{:02}:{:02}:{:02}", rate.limit, hours, minutes, seconds)
}

fn parse_rate_limit(name: &str, value: &str) -> PyResult<RateLimit> {
    let (limit, interval) = value
        .split_once('/')
        .ok_or_else(|| to_pyvalue_err(format!("invalid `{name}`: expected 'limit/HH:MM:SS'")))?;

    let limit = limit
        .parse::<usize>()
        .map_err(|e| to_pyvalue_err(format!("invalid `{name}` limit: {e}")))?;

    if limit == 0 {
        return Err(to_pyvalue_err(format!(
            "invalid `{name}`: limit must be greater than zero"
        )));
    }

    let mut total_secs: u64 = 0;
    let mut parts = interval.split(':');
    for label in ["hours", "minutes", "seconds"] {
        let component = parts
            .next()
            .ok_or_else(|| {
                to_pyvalue_err(format!(
                    "invalid `{name}`: expected 'limit/HH:MM:SS' interval"
                ))
            })?
            .parse::<u64>()
            .map_err(|e| to_pyvalue_err(format!("invalid `{name}` {label}: {e}")))?;

        let multiplier: u64 = match label {
            "hours" => 3_600,
            "minutes" => 60,
            "seconds" => 1,
            _ => unreachable!(),
        };
        total_secs = total_secs.saturating_add(component.saturating_mul(multiplier));
    }

    if parts.next().is_some() {
        return Err(to_pyvalue_err(format!(
            "invalid `{name}`: expected 'limit/HH:MM:SS'"
        )));
    }

    if total_secs == 0 {
        return Err(to_pyvalue_err(format!(
            "invalid `{name}`: interval must be greater than zero"
        )));
    }

    Ok(RateLimit::new(
        limit,
        total_secs.saturating_mul(NANOSECONDS_IN_SECOND),
    ))
}

fn coerce_max_notional_per_order(
    raw: HashMap<String, Py<PyAny>>,
) -> PyResult<AHashMap<InstrumentId, Decimal>> {
    Python::attach(|py| -> PyResult<AHashMap<InstrumentId, Decimal>> {
        let mut result = AHashMap::with_capacity(raw.len());
        for (instrument_id, value) in raw {
            let parsed_id = InstrumentId::from_str(&instrument_id).map_err(|e| {
                to_pyvalue_err(format!(
                    "invalid `max_notional_per_order` instrument ID {instrument_id:?}: {e}"
                ))
            })?;
            let value_str: String = value.bind(py).str()?.extract()?;
            let notional = Decimal::from_str(&value_str).map_err(|e| {
                to_pyvalue_err(format!(
                    "invalid `max_notional_per_order` notional {value_str:?}: {e}"
                ))
            })?;
            result.insert(parsed_id, notional);
        }
        Ok(result)
    })
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl RiskEngineConfig {
    /// Configuration for `RiskEngine` instances.
    #[new]
    #[pyo3(signature = (
        bypass = None,
        max_order_submit_rate = None,
        max_order_modify_rate = None,
        max_notional_per_order = None,
        debug = None,
    ))]
    fn py_new(
        bypass: Option<bool>,
        max_order_submit_rate: Option<String>,
        max_order_modify_rate: Option<String>,
        max_notional_per_order: Option<HashMap<String, Py<PyAny>>>,
        debug: Option<bool>,
    ) -> PyResult<Self> {
        let default = Self::default();

        let max_order_submit = match max_order_submit_rate {
            Some(value) => parse_rate_limit("max_order_submit_rate", &value)?,
            None => default.max_order_submit,
        };
        let max_order_modify = match max_order_modify_rate {
            Some(value) => parse_rate_limit("max_order_modify_rate", &value)?,
            None => default.max_order_modify,
        };
        let max_notional_per_order = match max_notional_per_order {
            Some(raw) => coerce_max_notional_per_order(raw)?,
            None => default.max_notional_per_order,
        };

        Ok(Self {
            bypass: bypass.unwrap_or(default.bypass),
            max_order_submit,
            max_order_modify,
            max_notional_per_order,
            debug: debug.unwrap_or(default.debug),
        })
    }

    #[getter]
    #[pyo3(name = "bypass")]
    const fn py_bypass(&self) -> bool {
        self.bypass
    }

    #[getter]
    #[pyo3(name = "max_order_submit_rate")]
    fn py_max_order_submit_rate(&self) -> String {
        format_rate_limit(&self.max_order_submit)
    }

    #[getter]
    #[pyo3(name = "max_order_modify_rate")]
    fn py_max_order_modify_rate(&self) -> String {
        format_rate_limit(&self.max_order_modify)
    }

    #[getter]
    #[pyo3(name = "max_notional_per_order")]
    fn py_max_notional_per_order(&self) -> HashMap<String, String> {
        self.max_notional_per_order
            .iter()
            .map(|(id, notional)| (id.to_string(), notional.to_string()))
            .collect()
    }

    #[getter]
    #[pyo3(name = "debug")]
    const fn py_debug(&self) -> bool {
        self.debug
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}
