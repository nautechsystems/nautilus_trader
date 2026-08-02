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

#![cfg(all(feature = "python", feature = "hypersync"))]

use std::{cell::RefCell, rc::Rc, sync::Arc};

use nautilus_blockchain::{
    config::BlockchainDataClientConfig, constants::BLOCKCHAIN,
    factories::BlockchainDataClientFactory, python,
};
use nautilus_common::{
    cache::Cache, clock::TestClock, live::runner::replace_data_event_sender, messages::DataEvent,
};
use nautilus_model::{
    defi::{DexType, chain::chains},
    identifiers::ClientId,
};
use nautilus_network::{python as network_python, websocket::TransportBackend};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{
    Bound, Py, Python,
    types::{PyAnyMethods, PyDict, PyDictMethods, PyModule},
};
use rstest::rstest;

#[rstest]
fn test_blockchain_python_data_factory_extracts_from_registry() {
    setup_data_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_blockchain_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
    });
}

#[rstest]
fn test_blockchain_python_config_accepts_transport_backend() {
    setup_data_event_sender();
    Python::initialize();

    Python::attach(|py| {
        let blockchain_module = register_blockchain_python_module(py);
        let network_module = register_network_python_module(py);
        assert_data_config_extracts_transport_backend_from_python_constructor(
            py,
            &blockchain_module,
            &network_module,
        );
    });
}

fn setup_data_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(sender);
}

fn register_blockchain_python_module(py: Python<'_>) -> Bound<'_, PyModule> {
    let module = PyModule::new(py, "blockchain").expect("Blockchain module should be created");
    python::blockchain(py, &module).expect("Blockchain Python module should register");
    module
}

fn register_network_python_module(py: Python<'_>) -> Bound<'_, PyModule> {
    let module = PyModule::new(py, "network").expect("Network module should be created");
    network_python::network(py, &module).expect("Network Python module should register");
    module
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, BlockchainDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        BlockchainDataClientConfig::builder()
            .chain(Arc::new(chains::ETHEREUM.clone()))
            .http_rpc_url("https://eth-mainnet.example.com".to_string())
            .build(),
    )
    .expect("config should convert to Python object")
    .into_any();
    let registry = get_global_pyo3_registry();

    let extracted_factory = registry
        .extract_factory(py, factory)
        .expect("data factory should extract");
    let extracted_config = registry
        .extract_config(py, config)
        .expect("data config should extract");
    let blockchain_config = extracted_config
        .as_any()
        .downcast_ref::<BlockchainDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "BLOCKCHAIN-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), BLOCKCHAIN);
    assert_eq!(
        extracted_factory.config_type(),
        "BlockchainDataClientConfig"
    );
    assert_eq!(
        blockchain_config.http_rpc_url,
        "https://eth-mainnet.example.com"
    );
    assert_eq!(
        client.client_id(),
        ClientId::from("BLOCKCHAIN-DATA-EXTRACTED")
    );
}

fn assert_data_config_extracts_transport_backend_from_python_constructor(
    py: Python<'_>,
    blockchain_module: &Bound<'_, PyModule>,
    network_module: &Bound<'_, PyModule>,
) {
    let config_type = blockchain_module
        .getattr("BlockchainDataClientConfig")
        .expect("BlockchainDataClientConfig should be available");
    let transport_backend = network_module
        .getattr("TransportBackend")
        .expect("TransportBackend should be available")
        .getattr("TUNGSTENITE")
        .expect("TransportBackend.TUNGSTENITE should be available");
    let kwargs = PyDict::new(py);
    kwargs
        .set_item("transport_backend", transport_backend)
        .expect("transport_backend kwarg should be set");
    let config = config_type
        .call(
            (
                chains::ETHEREUM.clone(),
                vec![DexType::UniswapV3],
                "https://eth-mainnet.example.com",
            ),
            Some(&kwargs),
        )
        .expect("BlockchainDataClientConfig should construct from Python");
    let registry = get_global_pyo3_registry();
    let extracted_config = registry
        .extract_config(py, config.into())
        .expect("data config should extract");
    let blockchain_config = extracted_config
        .as_any()
        .downcast_ref::<BlockchainDataClientConfig>()
        .expect("data config should downcast");

    assert_eq!(
        blockchain_config.transport_backend,
        TransportBackend::Tungstenite,
    );
}
