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
pub mod csv;
pub mod factories;

use nautilus_common::factories::{ClientConfig, DataClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::consts::{COINAPI, COINAPI_CLIENT_ID},
    config::{CoinApiDataClientConfig, CoinApiInstrumentSelection},
    factories::CoinApiDataClientFactory,
};

#[expect(clippy::needless_pass_by_value)]
fn extract_coinapi_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<CoinApiDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract CoinApiDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_coinapi_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<CoinApiDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract CoinApiDataClientConfig: {e}"
        ))),
    }
}

/// CoinAPI Python module exposed as `nautilus_trader._libnautilus.coinapi`.
///
/// # Errors
///
/// Returns an error if adding module attributes or registering an extractor fails.
#[pymodule]
pub fn coinapi(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(COINAPI), COINAPI)?;
    m.add(stringify!(COINAPI_CLIENT_ID), *COINAPI_CLIENT_ID)?;
    m.add_class::<CoinApiDataClientConfig>()?;
    m.add_class::<CoinApiDataClientFactory>()?;
    m.add_class::<CoinApiInstrumentSelection>()?;
    m.add_class::<crate::csv::CoinApiOhlcvLoader>()?;
    m.add_class::<csv::CoinApiDeltaStreamIterator>()?;
    m.add_class::<csv::CoinApiQuoteStreamIterator>()?;
    m.add_class::<csv::CoinApiTradeStreamIterator>()?;
    m.add_function(wrap_pyfunction!(csv::py_load_coinapi_deltas, m)?)?;
    m.add_function(wrap_pyfunction!(csv::py_load_coinapi_quotes, m)?)?;
    m.add_function(wrap_pyfunction!(csv::py_load_coinapi_trades, m)?)?;
    m.add_function(wrap_pyfunction!(csv::py_stream_coinapi_deltas, m)?)?;
    m.add_function(wrap_pyfunction!(csv::py_stream_coinapi_quotes, m)?)?;
    m.add_function(wrap_pyfunction!(csv::py_stream_coinapi_trades, m)?)?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor(COINAPI.to_string(), extract_coinapi_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register CoinAPI data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "CoinApiDataClientConfig".to_string(),
        extract_coinapi_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register CoinAPI data config extractor: {e}"
        )));
    }

    Ok(())
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
            let module = pyo3::wrap_pymodule!(super::coinapi)(py);
            let module = module.bind(py);

            assert_eq!(module.name().unwrap(), "coinapi");
            assert_eq!(
                module
                    .getattr("COINAPI")
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "COINAPI",
            );
            assert_eq!(
                module
                    .getattr("COINAPI_CLIENT_ID")
                    .unwrap()
                    .extract::<ClientId>()
                    .unwrap(),
                ClientId::from("COINAPI"),
            );

            for name in [
                "CoinApiOhlcvLoader",
                "load_coinapi_deltas",
                "load_coinapi_quotes",
                "load_coinapi_trades",
                "stream_coinapi_deltas",
                "stream_coinapi_quotes",
                "stream_coinapi_trades",
            ] {
                assert!(module.hasattr(name).unwrap(), "missing {name}");
            }
        });
    }
}
