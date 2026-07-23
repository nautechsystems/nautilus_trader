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

use nautilus_common::live::get_runtime;
use nautilus_core::{python::to_pyvalue_err, time::get_atomic_clock_realtime};
use nautilus_model::python::instruments::instrument_any_to_pyobject;
use pyo3::{prelude::*, types::PyList};

use crate::{
    common::{enums::BinanceProductType, urls::get_http_base_url_with_us},
    config::BinanceDataClientConfig,
    futures::http::client::BinanceFuturesHttpClient,
    spot::http::client::BinanceSpotHttpClient,
};

/// Loads the configured Binance instrument catalogue for the Python async facade.
///
/// The public `load_binance_instruments` coroutine runs this blocking boundary in a Python worker
/// thread. The request uses the same domain-level HTTP paths and
/// [`crate::config::BinanceInstrumentProviderConfig`] as the live data client.
///
/// # Errors
///
/// Returns an error if the configuration is invalid, the product type is unsupported, the
/// catalogue request fails, or an instrument cannot be converted to Python.
#[pyfunction]
#[pyo3(name = "_load_binance_instruments")]
pub(super) fn py_load_binance_instruments<'py>(
    py: Python<'py>,
    config: BinanceDataClientConfig,
) -> PyResult<Bound<'py, PyList>> {
    config.validate().map_err(to_pyvalue_err)?;

    let instruments = py
        .detach(|| {
            get_runtime().block_on(async move {
                match config.product_type {
                    BinanceProductType::Spot => {
                        let base_url_http = config.base_url_http.clone().or_else(|| {
                            config.us.then(|| {
                                get_http_base_url_with_us(
                                    config.product_type,
                                    config.environment,
                                    true,
                                )
                                .to_string()
                            })
                        });
                        let client = BinanceSpotHttpClient::new_with_json_responses(
                            config.environment,
                            get_atomic_clock_realtime(),
                            config.api_key.clone(),
                            config.api_secret.clone(),
                            base_url_http,
                            Some(config.recv_window_ms),
                            None,
                            config.proxy_url.clone(),
                            config.us,
                        )
                        .map_err(|e| e.to_string())?;

                        client
                            .request_instruments_with_config(
                                &config.instrument_provider,
                                config.us,
                            )
                            .await
                            .map_err(|e| e.to_string())
                    }
                    BinanceProductType::UsdM | BinanceProductType::CoinM => {
                        let client = BinanceFuturesHttpClient::new(
                            config.product_type,
                            config.environment,
                            get_atomic_clock_realtime(),
                            config.api_key.clone(),
                            config.api_secret.clone(),
                            config.base_url_http.clone(),
                            Some(config.recv_window_ms),
                            None,
                            config.proxy_url.clone(),
                            false,
                        )
                        .map_err(|e| e.to_string())?;

                        client
                            .request_instruments_with_config(&config.instrument_provider)
                            .await
                            .map_err(|e| e.to_string())
                    }
                    product_type => Err(format!(
                        "Binance instrument loading supports Spot, UsdM, or CoinM, was {product_type:?}"
                    )),
                }
            })
        })
        .map_err(to_pyvalue_err)?;
    let instruments = instruments
        .into_iter()
        .map(|instrument| instrument_any_to_pyobject(py, instrument))
        .collect::<PyResult<Vec<_>>>()?;

    PyList::new(py, instruments)
}
