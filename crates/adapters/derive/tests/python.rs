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

#![cfg(feature = "python")]

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::TestClock,
    live::runner::{replace_data_event_sender, replace_exec_event_sender},
    messages::{DataEvent, ExecutionEvent},
};
use nautilus_derive::{
    common::{consts::DERIVE, enums::DeriveEnvironment},
    config::{DeriveDataClientConfig, DeriveExecClientConfig},
    factories::{DeriveDataClientFactory, DeriveExecFactoryConfig, DeriveExecutionClientFactory},
    python,
};
use nautilus_model::identifiers::{AccountId, ClientId, TraderId};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{Py, Python, types::PyModule};
use rstest::rstest;

const TEST_WALLET_ADDRESS: &str = "0x0000000000000000000000000000000000001234";
const TEST_SESSION_KEY: &str = "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
const TEST_SUBACCOUNT_ID: u64 = 30769;

fn register_derive_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "derive").expect("Derive module should be created");
    python::derive(py, &module).expect("Derive Python module should register");
}

fn setup_data_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(sender);
}

fn setup_exec_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    replace_exec_event_sender(sender);
}

#[rstest]
fn test_derive_python_factories_extract_from_registry() {
    setup_data_event_sender();
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_derive_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
        assert_exec_factory_extracts_from_python_object(py);
    });
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, DeriveDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        DeriveDataClientConfig {
            environment: DeriveEnvironment::Testnet,
            http_timeout_secs: 7,
            currencies: vec!["ETH".to_string()],
            ..DeriveDataClientConfig::default()
        },
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
    let derive_config = extracted_config
        .as_any()
        .downcast_ref::<DeriveDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "DERIVE-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), DERIVE);
    assert_eq!(extracted_factory.config_type(), "DeriveDataClientConfig");
    assert_eq!(derive_config.environment, DeriveEnvironment::Testnet);
    assert_eq!(derive_config.http_timeout_secs, 7);
    assert_eq!(derive_config.currencies, ["ETH".to_string()]);
    assert_eq!(client.client_id(), ClientId::from("DERIVE-DATA-EXTRACTED"));
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("DERIVE-001");
    let factory = Py::new(py, DeriveExecutionClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        DeriveExecFactoryConfig {
            trader_id,
            account_id,
            config: DeriveExecClientConfig {
                wallet_address: Some(TEST_WALLET_ADDRESS.to_string()),
                session_key: Some(TEST_SESSION_KEY.to_string()),
                subaccount_id: Some(TEST_SUBACCOUNT_ID),
                environment: DeriveEnvironment::Testnet,
                ..DeriveExecClientConfig::default()
            },
        },
    )
    .expect("config should convert to Python object")
    .into_any();
    let registry = get_global_pyo3_registry();

    let extracted_factory = registry
        .extract_exec_factory(py, factory)
        .expect("exec factory should extract");
    let extracted_config = registry
        .extract_config(py, config)
        .expect("exec config should extract");
    let derive_config = extracted_config
        .as_any()
        .downcast_ref::<DeriveExecFactoryConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create(
            "DERIVE-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
        )
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), DERIVE);
    assert_eq!(extracted_factory.config_type(), "DeriveExecFactoryConfig");
    assert_eq!(derive_config.trader_id, trader_id);
    assert_eq!(derive_config.account_id, account_id);
    assert_eq!(derive_config.config.environment, DeriveEnvironment::Testnet);
    assert_eq!(client.client_id(), ClientId::from("DERIVE-EXEC-EXTRACTED"));
    assert_eq!(client.account_id(), account_id);
}
