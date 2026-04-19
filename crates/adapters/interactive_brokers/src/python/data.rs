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

//! Python bindings for the Interactive Brokers data client.

use std::sync::Arc;

use ibapi::contracts::{
    Contract, Currency as IBCurrency, Exchange as IBExchange, SecurityType, Symbol,
};
use nautilus_common::{clients::DataClient, live::get_runtime};
use nautilus_core::python::to_pyruntime_err;
use nautilus_model::{
    data::BarType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    python::instruments::instrument_any_to_pyobject,
};
use pyo3::{
    IntoPyObjectExt,
    prelude::*,
    types::{PyDict, PyList},
};

use crate::{
    data::InteractiveBrokersDataClient,
    python::conversion::{contract_details_to_pyobject, py_list_to_contracts, py_to_contract},
};

#[cfg(feature = "python")]
#[pymethods]
impl InteractiveBrokersDataClient {
    #[new]
    #[pyo3(signature = (_msgbus, _cache, _clock, instrument_provider, config))]
    fn py_new(
        _msgbus: Py<PyAny>,
        _cache: Py<PyAny>,
        _clock: Py<PyAny>,
        instrument_provider: crate::providers::instruments::InteractiveBrokersInstrumentProvider,
        config: crate::config::InteractiveBrokersDataClientConfig,
    ) -> PyResult<Self> {
        Self::new_for_python(config, instrument_provider).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "set_event_callback")]
    fn py_set_event_callback(&self, callback: Py<PyAny>) {
        self.register_python_event_callback(callback);
    }

    /// Returns the client ID.
    #[getter]
    pub fn client_id(&self) -> ClientId {
        DataClient::client_id(self)
    }

    /// Returns whether the client is connected.
    #[getter]
    pub fn is_connected(&self) -> bool {
        DataClient::is_connected(self)
    }

    /// Returns whether the client is disconnected.
    #[getter]
    pub fn is_disconnected(&self) -> bool {
        DataClient::is_disconnected(self)
    }

    #[pyo3(name = "connect")]
    fn py_connect(&mut self) -> PyResult<()> {
        get_runtime()
            .block_on(DataClient::connect(self))
            .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "disconnect")]
    fn py_disconnect(&mut self) -> PyResult<()> {
        get_runtime()
            .block_on(DataClient::disconnect(self))
            .map_err(to_pyruntime_err)
    }

    /// Get the instrument provider.
    ///
    /// # Errors
    ///
    /// Returns an error indicating the provider should be accessed through data client methods.
    #[getter("get_instrument_provider")]
    pub fn get_instrument_provider(
        &self,
    ) -> PyResult<crate::providers::instruments::InteractiveBrokersInstrumentProvider> {
        // The provider is wrapped in Arc, so we just need to return it
        // Since it doesn't implement Clone, we'll need to expose it differently
        // For now, return an error indicating it should be accessed through methods
        Err(to_pyruntime_err(
            "instrument_provider should be accessed through the data client's methods that use it internally",
        ))
    }

    /// Batch load multiple instrument IDs.
    ///
    /// This uses the data client's internal IB client to load instruments via the provider.
    ///
    /// # Arguments
    ///
    /// * `instrument_ids` - List of instrument IDs to load
    ///
    /// # Returns
    ///
    /// Returns the number of instruments successfully loaded.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or loading fails.
    #[pyo3(name = "batch_load")]
    fn py_batch_load<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            provider
                .batch_load(&client, instrument_ids, None)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Fetch option chain for an underlying contract with expiry filtering.
    ///
    /// This uses the data client's internal IB client to fetch options via the provider.
    ///
    /// # Arguments
    ///
    /// * `underlying_symbol` - The underlying symbol (e.g., "AAPL")
    /// * `exchange` - The exchange (defaults to "SMART")
    /// * `currency` - The currency (defaults to "USD")
    /// * `expiry_min` - Minimum expiry date string (YYYYMMDD format, optional)
    /// * `expiry_max` - Maximum expiry date string (YYYYMMDD format, optional)
    ///
    /// # Returns
    ///
    /// Returns the number of option instruments loaded.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or fetching fails.
    #[pyo3(signature = (underlying_symbol, exchange=None, currency=None, expiry_min=None, expiry_max=None))]
    fn py_fetch_option_chain_by_range<'py>(
        &self,
        py: Python<'py>,
        underlying_symbol: String,
        exchange: Option<String>,
        currency: Option<String>,
        expiry_min: Option<String>,
        expiry_max: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            let underlying = Contract {
                contract_id: 0,
                symbol: Symbol::from(underlying_symbol.clone()),
                security_type: SecurityType::Stock,
                last_trade_date_or_contract_month: String::new(),
                strike: 0.0,
                right: String::new(),
                multiplier: String::new(),
                exchange: IBExchange::from(exchange.as_deref().unwrap_or("SMART")),
                currency: IBCurrency::from(currency.as_deref().unwrap_or("USD")),
                local_symbol: String::new(),
                primary_exchange: IBExchange::default(),
                trading_class: String::new(),
                include_expired: false,
                security_id_type: String::new(),
                security_id: String::new(),
                combo_legs_description: String::new(),
                combo_legs: Vec::new(),
                delta_neutral_contract: None,
                issuer_id: String::new(),
                description: String::new(),
                last_trade_date: None,
            };

            provider
                .fetch_option_chain_by_range(
                    &client,
                    &underlying,
                    expiry_min.as_deref(),
                    expiry_max.as_deref(),
                )
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Fetch option chain for a fully specified underlying contract with expiry filtering.
    ///
    /// This variant preserves the source security type and contract fields, which is required
    /// for futures options and other non-stock underliers.
    #[pyo3(signature = (contract, expiry_min=None, expiry_max=None))]
    #[allow(clippy::needless_pass_by_value)]
    fn py_fetch_option_chain_by_range_for_contract<'py>(
        &self,
        py: Python<'py>,
        contract: Py<PyAny>,
        expiry_min: Option<String>,
        expiry_max: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);
        let rust_contract = py_to_contract(contract.bind(py))?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            provider
                .fetch_option_chain_by_range(
                    &client,
                    &rust_contract,
                    expiry_min.as_deref(),
                    expiry_max.as_deref(),
                )
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Fetch raw option chain metadata for a fully specified underlying contract.
    ///
    /// This is useful for debugging which exchanges, expiries, and strikes IB returns before
    /// expanding into full option contract details.
    #[pyo3(signature = (contract))]
    #[allow(clippy::needless_pass_by_value)]
    fn py_get_option_chain_metadata_for_contract<'py>(
        &self,
        py: Python<'py>,
        contract: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ib_client_ref = self.get_ib_client().map(Arc::clone);
        let rust_contract = py_to_contract(contract.bind(py))?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            let mut stream = client
                .option_chain(
                    rust_contract.symbol.as_str(),
                    rust_contract.exchange.as_str(),
                    rust_contract.security_type.clone(),
                    rust_contract.contract_id,
                )
                .await
                .map_err(to_pyruntime_err)?;

            let mut chains = Vec::new();

            while let Some(result) = stream.next().await {
                match result {
                    Ok(chain) => chains.push(chain),
                    Err(e) => {
                        return Err(to_pyruntime_err(format!(
                            "Failed to receive option chain metadata: {e}",
                        )));
                    }
                }
            }

            Python::attach(|py| -> PyResult<Py<PyAny>> {
                let items = PyList::empty(py);
                for chain in chains {
                    let item = PyDict::new(py);
                    item.set_item("underlying_contract_id", chain.underlying_contract_id)?;
                    item.set_item("trading_class", chain.trading_class)?;
                    item.set_item("multiplier", chain.multiplier)?;
                    item.set_item("exchange", chain.exchange)?;
                    item.set_item("expirations", chain.expirations)?;
                    item.set_item("strikes", chain.strikes)?;
                    items.append(item)?;
                }
                items.into_py_any(py)
            })
            .map_err(to_pyruntime_err)
        })
    }

    /// Fetch raw IB contract details for a fully specified contract.
    ///
    /// This is useful for debugging exact contract queries such as FOP/OPT lookups and comparing
    /// the Rust adapter output against the legacy Python adapter.
    #[pyo3(signature = (contract))]
    #[allow(clippy::needless_pass_by_value)]
    fn py_get_contract_details_for_contract<'py>(
        &self,
        py: Python<'py>,
        contract: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let ib_client_ref = self.get_ib_client().map(Arc::clone);
        let rust_contract = py_to_contract(contract.bind(py))?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            let details = client
                .contract_details(&rust_contract)
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| -> PyResult<Py<PyAny>> {
                let items = PyList::empty(py);
                for detail in details {
                    items.append(contract_details_to_pyobject(py, &detail)?)?;
                }
                items.into_py_any(py)
            })
            .map_err(to_pyruntime_err)
        })
    }

    /// Resolve a contract through the Rust provider and return the Rust instrument kind and ID.
    ///
    /// This is intended only for debugging provider resolution issues.
    #[pyo3(signature = (contract))]
    #[allow(clippy::needless_pass_by_value)]
    fn py_debug_resolve_instrument<'py>(
        &self,
        py: Python<'py>,
        contract: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider().clone();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);
        let rust_contract = py_to_contract(contract.bind(py))?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            let result = provider
                .get_instrument(&client, &rust_contract)
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| -> PyResult<Py<PyAny>> {
                let dict = PyDict::new(py);

                if let Some(instrument) = result {
                    let kind = match &instrument {
                        InstrumentAny::Betting(_) => "Betting",
                        InstrumentAny::BinaryOption(_) => "BinaryOption",
                        InstrumentAny::Cfd(_) => "Cfd",
                        InstrumentAny::Commodity(_) => "Commodity",
                        InstrumentAny::CryptoFuture(_) => "CryptoFuture",
                        InstrumentAny::CryptoOption(_) => "CryptoOption",
                        InstrumentAny::CryptoPerpetual(_) => "CryptoPerpetual",
                        InstrumentAny::CurrencyPair(_) => "CurrencyPair",
                        InstrumentAny::Equity(_) => "Equity",
                        InstrumentAny::FuturesContract(_) => "FuturesContract",
                        InstrumentAny::FuturesSpread(_) => "FuturesSpread",
                        InstrumentAny::IndexInstrument(_) => "IndexInstrument",
                        InstrumentAny::OptionContract(_) => "OptionContract",
                        InstrumentAny::OptionSpread(_) => "OptionSpread",
                        InstrumentAny::PerpetualContract(_) => "PerpetualContract",
                        InstrumentAny::TokenizedAsset(_) => "TokenizedAsset",
                    };
                    dict.set_item("kind", kind)?;
                    dict.set_item("instrument_id", instrument.id().to_string())?;
                } else {
                    dict.set_item("kind", py.None())?;
                    dict.set_item("instrument_id", py.None())?;
                }
                dict.into_py_any(py)
            })
            .map_err(to_pyruntime_err)
        })
    }

    /// Fetch futures chain for a given underlying symbol.
    ///
    /// This uses the data client's internal IB client to fetch futures via the provider.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The underlying symbol (e.g., "ES")
    /// * `exchange` - The exchange (defaults to empty string for all exchanges)
    /// * `currency` - The currency (defaults to "USD")
    ///
    /// # Returns
    ///
    /// Returns the number of futures instruments loaded.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or fetching fails.
    #[pyo3(signature = (symbol, exchange=None, currency=None, min_expiry_days=None, max_expiry_days=None))]
    fn py_fetch_futures_chain<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        exchange: Option<String>,
        currency: Option<String>,
        min_expiry_days: Option<u32>,
        max_expiry_days: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            provider
                .fetch_futures_chain(
                    &client,
                    &symbol,
                    exchange.as_deref().unwrap_or(""),
                    currency.as_deref().unwrap_or("USD"),
                    min_expiry_days,
                    max_expiry_days,
                )
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Get an instrument by IB Contract.
    ///
    /// This uses the data client's internal IB client to get instruments via the provider.
    ///
    /// # Arguments
    ///
    /// * `contract` - The IB contract (as a dict with contract fields)
    ///
    /// # Returns
    ///
    /// Returns the instrument if found, `None` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or fetching fails.
    #[pyo3(name = "get_instrument")]
    #[allow(clippy::needless_pass_by_value)]
    #[allow(deprecated)]
    fn py_get_instrument<'py>(
        &self,
        py: Python<'py>,
        contract: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider().clone();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);
        // Parse contract synchronously
        let rust_contract = py_to_contract(contract.bind(py))?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            match provider
                .get_instrument(&client, &rust_contract)
                .await
                .map_err(to_pyruntime_err)?
            {
                Some(instrument) => Python::attach(|gil| {
                    instrument_any_to_pyobject(gil, instrument).map_err(to_pyruntime_err)
                }),
                None => Python::attach(|gil| Ok(gil.None())),
            }
        })
    }

    /// Load a single instrument (does not return loaded IDs).
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to load
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or loading fails.
    #[pyo3(name = "load_async")]
    fn py_load_async<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        filters: Option<std::collections::HashMap<String, String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            provider
                .load_async(&client, instrument_id, filters)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Load a single instrument and return the loaded instrument ID.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to load
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Returns
    ///
    /// Returns the loaded instrument ID if successful, `None` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or loading fails.
    #[pyo3(name = "load_with_return_async")]
    fn py_load_with_return_async<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        filters: Option<std::collections::HashMap<String, String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            provider
                .load_with_return_async(&client, instrument_id, filters)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Load multiple instruments (does not return loaded IDs).
    ///
    /// # Arguments
    ///
    /// * `instrument_ids` - List of instrument IDs to load
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or loading fails.
    #[pyo3(name = "load_ids_async")]
    fn py_load_ids_async<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
        filters: Option<std::collections::HashMap<String, String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            provider
                .load_ids_async(&client, instrument_ids, filters)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Load multiple instruments and return the loaded instrument IDs.
    ///
    /// # Arguments
    ///
    /// * `instrument_ids` - List of instrument IDs to load
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Returns
    ///
    /// Returns a list of successfully loaded instrument IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or loading fails.
    #[pyo3(name = "load_ids_with_return_async")]
    fn py_load_ids_with_return_async<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
        filters: Option<std::collections::HashMap<String, String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            provider
                .load_ids_with_return_async(&client, instrument_ids, filters)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Get instrument ID by contract ID.
    ///
    /// # Arguments
    ///
    /// * `contract_id` - The IB contract ID
    ///
    /// # Returns
    ///
    /// Returns the instrument ID if found, `None` otherwise.
    #[pyo3(name = "get_instrument_id_by_contract_id")]
    fn py_get_instrument_id_by_contract_id(&self, contract_id: i32) -> Option<InstrumentId> {
        self.instrument_provider()
            .get_instrument_id_by_contract_id(contract_id)
    }

    /// Convert an instrument ID to IB contract details.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to convert
    ///
    /// # Returns
    ///
    /// Returns the contract details if found, `None` otherwise.
    #[pyo3(name = "instrument_id_to_ib_contract_details")]
    fn py_instrument_id_to_ib_contract_details(
        &self,
        instrument_id: InstrumentId,
    ) -> Option<Py<PyAny>> {
        // TODO: Convert ContractDetails to Python object
        // For now, return None as placeholder
        let _details = self
            .instrument_provider()
            .instrument_id_to_ib_contract_details(&instrument_id);
        // When ContractDetails PyO3 bindings are available, convert here
        None
    }

    /// Determine venue from contract using provider configuration.
    ///
    /// # Arguments
    ///
    /// * `contract` - The IB contract (as a dict with contract fields)
    ///
    /// # Returns
    ///
    /// Returns the determined venue.
    #[pyo3(name = "determine_venue")]
    #[allow(clippy::needless_pass_by_value)]
    fn py_determine_venue(&self, py: Python<'_>, contract: Py<PyAny>) -> PyResult<String> {
        let rust_contract = py_to_contract(contract.bind(py))?;
        let venue = self
            .instrument_provider()
            .determine_venue(&rust_contract, None);
        Ok(venue.to_string())
    }

    /// Load all instruments from provided IDs and contracts.
    ///
    /// This is equivalent to Python's `load_all_async` method.
    ///
    /// # Arguments
    ///
    /// * `instrument_ids` - Optional list of instrument IDs to load
    /// * `contracts` - Optional list of IB contracts (as dicts) to load
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Returns
    ///
    /// Returns a list of successfully loaded instrument IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or loading fails.
    #[pyo3(name = "load_all_async")]
    fn py_load_all_async<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Option<Vec<InstrumentId>>,
        contracts: Option<Py<PyAny>>,
        force_instrument_update: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);

        // Convert contracts synchronously
        let contracts_rust: Option<Vec<ibapi::contracts::Contract>> = if let Some(c) = contracts {
            Some(py_list_to_contracts(c.bind(py))?)
        } else {
            None
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            provider
                .load_all_async(
                    &client,
                    instrument_ids,
                    contracts_rust,
                    force_instrument_update,
                )
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Fetch a spread instrument by loading individual legs.
    ///
    /// This is equivalent to Python's `fetch_spread_instrument` method.
    ///
    /// # Arguments
    ///
    /// * `spread_instrument_id` - The spread instrument ID to fetch
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Returns
    ///
    /// Returns `true` if the spread instrument was successfully loaded, `false` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected or loading fails.
    #[pyo3(name = "fetch_spread_instrument")]
    fn py_fetch_spread_instrument<'py>(
        &self,
        py: Python<'py>,
        spread_instrument_id: InstrumentId,
        force_instrument_update: bool,
        filters: Option<std::collections::HashMap<String, String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.instrument_provider();
        let ib_client_ref = self.get_ib_client().map(Arc::clone);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = ib_client_ref
                .ok_or_else(|| anyhow::anyhow!("IB client not connected. Call connect() first"))
                .map_err(to_pyruntime_err)?;

            provider
                .fetch_spread_instrument(
                    &client,
                    spread_instrument_id,
                    force_instrument_update,
                    filters,
                )
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Subscribe to quote ticks for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to subscribe to
    /// * `params` - Optional parameters dict (e.g., {"batch_quotes": "true"})
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription fails.
    #[pyo3(name = "subscribe_quotes")]
    fn py_subscribe_quotes(
        &mut self,
        instrument_id: InstrumentId,
        params: Option<std::collections::HashMap<String, String>>,
    ) -> PyResult<()> {
        self.subscribe_quotes_for_python(instrument_id, params)
            .map_err(to_pyruntime_err)
    }

    /// Subscribe to index prices for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to subscribe to
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription fails.
    #[pyo3(name = "subscribe_index_prices")]
    fn py_subscribe_index_prices(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        self.subscribe_index_prices_for_python(instrument_id)
            .map_err(to_pyruntime_err)
    }

    /// Subscribe to option greeks for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to subscribe to
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription fails.
    #[pyo3(name = "subscribe_option_greeks")]
    fn py_subscribe_option_greeks(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        self.subscribe_option_greeks_for_python(instrument_id)
            .map_err(to_pyruntime_err)
    }

    /// Subscribe to trade ticks for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to subscribe to
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription fails.
    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        self.subscribe_trades_for_python(instrument_id)
            .map_err(to_pyruntime_err)
    }

    /// Subscribe to bars for a bar type.
    ///
    /// # Arguments
    ///
    /// * `bar_type` - The bar type to subscribe to
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription fails.
    #[pyo3(name = "subscribe_bars", signature = (bar_type, params=None))]
    fn py_subscribe_bars(
        &mut self,
        bar_type: BarType,
        params: Option<std::collections::HashMap<String, String>>,
    ) -> PyResult<()> {
        self.subscribe_bars_for_python(bar_type, params)
            .map_err(to_pyruntime_err)
    }

    /// Subscribe to order book deltas for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to subscribe to
    /// * `depth` - The depth of the order book
    /// * `params` - Optional parameters dict (e.g., {"is_smart_depth": "true"})
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription fails.
    #[pyo3(name = "subscribe_book_deltas")]
    fn py_subscribe_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
        depth: Option<u16>,
        params: Option<std::collections::HashMap<String, String>>,
    ) -> PyResult<()> {
        self.subscribe_book_deltas_for_python(instrument_id, depth, params)
            .map_err(to_pyruntime_err)
    }

    /// Unsubscribe from quote ticks for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to unsubscribe from
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription fails.
    #[pyo3(name = "unsubscribe_quotes")]
    fn py_unsubscribe_quotes(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        self.unsubscribe_quotes_for_python(instrument_id)
            .map_err(to_pyruntime_err)
    }

    /// Unsubscribe from index prices for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to unsubscribe from
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription fails.
    #[pyo3(name = "unsubscribe_index_prices")]
    fn py_unsubscribe_index_prices(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        self.unsubscribe_index_prices_for_python(instrument_id)
            .map_err(to_pyruntime_err)
    }

    /// Unsubscribe from option greeks for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to unsubscribe from
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription fails.
    #[pyo3(name = "unsubscribe_option_greeks")]
    fn py_unsubscribe_option_greeks(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        self.unsubscribe_option_greeks_for_python(instrument_id)
            .map_err(to_pyruntime_err)
    }

    /// Unsubscribe from trade ticks for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to unsubscribe from
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription fails.
    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        self.unsubscribe_trades_for_python(instrument_id)
            .map_err(to_pyruntime_err)
    }

    /// Unsubscribe from bars for a bar type.
    ///
    /// # Arguments
    ///
    /// * `bar_type` - The bar type to unsubscribe from
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription fails.
    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars(&mut self, bar_type: BarType) -> PyResult<()> {
        self.unsubscribe_bars_for_python(bar_type)
            .map_err(to_pyruntime_err)
    }

    /// Unsubscribe from order book deltas for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to unsubscribe from
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription fails.
    #[pyo3(name = "unsubscribe_book_deltas")]
    fn py_unsubscribe_book_deltas(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        self.unsubscribe_book_deltas_for_python(instrument_id)
            .map_err(to_pyruntime_err)
    }

    /// Request historical quote ticks for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to request quotes for
    /// * `limit` - Maximum number of ticks to return
    /// * `start` - Start timestamp (Unix nanoseconds, optional)
    /// * `end` - End timestamp (Unix nanoseconds, optional)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[pyo3(name = "request_quotes", signature = (instrument_id, limit=None, start=None, end=None, request_id=None))]
    fn py_request_quotes(
        &self,
        instrument_id: InstrumentId,
        limit: Option<u64>,
        start: Option<u64>,
        end: Option<u64>,
        request_id: Option<String>,
    ) -> PyResult<()> {
        self.request_quotes_for_python(instrument_id, limit, start, end, request_id)
            .map_err(to_pyruntime_err)
    }

    /// Request historical trade ticks for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to request trades for
    /// * `limit` - Maximum number of ticks to return
    /// * `start` - Start timestamp (Unix nanoseconds, optional)
    /// * `end` - End timestamp (Unix nanoseconds, optional)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[pyo3(name = "request_trades", signature = (instrument_id, limit=None, start=None, end=None, request_id=None))]
    fn py_request_trades(
        &self,
        instrument_id: InstrumentId,
        limit: Option<u64>,
        start: Option<u64>,
        end: Option<u64>,
        request_id: Option<String>,
    ) -> PyResult<()> {
        self.request_trades_for_python(instrument_id, limit, start, end, request_id)
            .map_err(to_pyruntime_err)
    }

    /// Request historical bars for a bar type.
    ///
    /// # Arguments
    ///
    /// * `bar_type` - The bar type to request
    /// * `limit` - Maximum number of bars to return
    /// * `start` - Start timestamp (Unix nanoseconds, optional)
    /// * `end` - End timestamp (Unix nanoseconds, optional)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[pyo3(name = "request_bars", signature = (bar_type, limit=None, start=None, end=None, request_id=None))]
    fn py_request_bars(
        &self,
        bar_type: BarType,
        limit: Option<u64>,
        start: Option<u64>,
        end: Option<u64>,
        request_id: Option<String>,
    ) -> PyResult<()> {
        self.request_bars_for_python(bar_type, limit, start, end, request_id)
            .map_err(to_pyruntime_err)
    }

    /// Request a single instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to request
    /// * `params` - Optional parameters dict (e.g., {"force_instrument_update": "true"})
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[pyo3(name = "request_instrument")]
    fn py_request_instrument(
        &self,
        instrument_id: InstrumentId,
        params: Option<std::collections::HashMap<String, String>>,
    ) -> PyResult<()> {
        self.request_instrument_for_python(instrument_id, params)
            .map_err(to_pyruntime_err)
    }

    /// Request multiple instruments.
    ///
    /// # Arguments
    ///
    /// * `venue` - Optional venue to filter by
    /// * `params` - Optional parameters dict (e.g., {"force_instrument_update": "true", "ib_contracts": [...]})
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[pyo3(name = "request_instruments")]
    fn py_request_instruments(
        &self,
        venue: Option<Venue>,
        params: Option<std::collections::HashMap<String, String>>,
    ) -> PyResult<()> {
        self.request_instruments_for_python(venue, params)
            .map_err(to_pyruntime_err)
    }
}
