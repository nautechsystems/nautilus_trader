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

//! Python bindings from [PyO3](https://pyo3.rs).

pub mod config;
pub mod factories;
pub mod http;
pub mod loaders;
pub mod models;

mod query;

use nautilus_common::factories::{ClientConfig, DataClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::identifiers::InstrumentId;
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::{
        consts::{FINFEED, FINFEED_CLIENT_ID},
        symbol::to_instrument_id,
    },
    config::FinFeedDataClientConfig,
    factories::FinFeedDataClientFactory,
    instruments::FinFeedInstrumentSelection,
};

/// FinFeed Python module exposed as `nautilus_trader._libnautilus.finfeed`.
///
/// # Errors
///
/// Returns an error if adding module attributes or registering an extractor fails.
#[pymodule]
pub fn finfeed(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(FINFEED), FINFEED)?;
    m.add(stringify!(FINFEED_CLIENT_ID), *FINFEED_CLIENT_ID)?;
    m.add_class::<FinFeedDataClientConfig>()?;
    m.add_class::<FinFeedDataClientFactory>()?;
    m.add_class::<loaders::PyFinFeedDataLoader>()?;
    m.add_class::<FinFeedInstrumentSelection>()?;
    m.add_function(wrap_pyfunction!(py_finfeed_instrument_id, m)?)?;
    http::register(m)?;
    models::register(m)?;
    query::register(m)?;
    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor(FINFEED.to_string(), extract_finfeed_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register FinFeed data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "FinFeedDataClientConfig".to_string(),
        extract_finfeed_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register FinFeed data config extractor: {e}"
        )));
    }

    Ok(())
}

/// Returns the FinFeed instrument ID for an exchange and an exact outcome ID.
///
/// The venue is `FINFEED_<exchange_id>`, for example `FINFEED_POLYMARKET`. The inputs are checked
/// locally, without a request.
///
/// # Errors
///
/// Returns an error if the exchange is unknown or the outcome ID is invalid.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.adapters.finfeed")]
#[pyo3(name = "finfeed_instrument_id")]
fn py_finfeed_instrument_id(exchange_id: &str, market_id: &str) -> PyResult<InstrumentId> {
    to_instrument_id(exchange_id, market_id).map_err(to_pyvalue_err)
}

#[expect(clippy::needless_pass_by_value)]
fn extract_finfeed_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<FinFeedDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract FinFeedDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_finfeed_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<FinFeedDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract FinFeedDataClientConfig: {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::ClientId;
    use pyo3::prelude::*;
    use rstest::rstest;

    #[rstest]
    fn test_module_registration() {
        Python::initialize();
        Python::attach(|py| {
            let module = pyo3::wrap_pymodule!(super::finfeed)(py);
            let module = module.bind(py);

            assert_eq!(module.name().unwrap(), "finfeed");
            assert_eq!(
                module
                    .getattr("FINFEED")
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "FINFEED",
            );
            assert_eq!(
                module
                    .getattr("FINFEED_CLIENT_ID")
                    .unwrap()
                    .extract::<ClientId>()
                    .unwrap(),
                ClientId::from("FINFEED"),
            );
        });
    }
}
