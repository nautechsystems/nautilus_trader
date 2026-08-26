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

//! Python bindings from `pyo3`.

pub mod config;
pub mod enums;
pub mod factories;

use nautilus_common::factories::{ClientConfig, DataClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::consts::{MASSIVE, MASSIVE_CLIENT_ID, MASSIVE_VENUE},
    config::MassiveDataClientConfig,
    factories::MassiveDataClientFactory,
};

#[expect(clippy::needless_pass_by_value)]
fn extract_massive_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<MassiveDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract MassiveDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_massive_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<MassiveDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract MassiveDataClientConfig: {e}"
        ))),
    }
}

/// Exposed through `nautilus_trader.adapters.massive`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
pub fn massive(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(MASSIVE), MASSIVE)?;
    m.add(stringify!(MASSIVE_CLIENT_ID), *MASSIVE_CLIENT_ID)?;
    m.add(stringify!(MASSIVE_VENUE), *MASSIVE_VENUE)?;
    m.add_class::<crate::common::enums::MassiveDataFeed>()?;
    m.add_class::<MassiveDataClientConfig>()?;
    m.add_class::<MassiveDataClientFactory>()?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor(MASSIVE.to_string(), extract_massive_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Massive data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "MassiveDataClientConfig".to_string(),
        extract_massive_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Massive data config extractor: {e}"
        )));
    }

    Ok(())
}
