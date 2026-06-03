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

use nautilus_coinbase::{
    common::{consts::COINBASE, enums::CoinbaseEnvironment},
    config::{CoinbaseDataClientConfig, CoinbaseExecClientConfig},
    factories::{CoinbaseDataClientFactory, CoinbaseExecutionClientFactory},
    python,
};
use nautilus_common::{
    cache::Cache,
    clock::TestClock,
    live::runner::{replace_exec_event_sender, set_data_event_sender},
    messages::{DataEvent, ExecutionEvent},
};
use nautilus_model::{
    enums::AccountType,
    identifiers::{AccountId, ClientId, TraderId},
};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{Py, Python, types::PyModule};
use rstest::rstest;

fn register_coinbase_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "coinbase").expect("Coinbase module should be created");
    python::coinbase(py, &module).expect("Coinbase Python module should register");
}

fn setup_data_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(sender);
}

fn setup_exec_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    replace_exec_event_sender(sender);
}

#[rstest]
fn test_coinbase_python_factories_extract_from_registry() {
    setup_data_event_sender();
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_coinbase_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
        assert_exec_factory_extracts_from_python_object(py);
    });
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, CoinbaseDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        CoinbaseDataClientConfig {
            environment: CoinbaseEnvironment::Sandbox,
            http_timeout_secs: 7,
            ..CoinbaseDataClientConfig::default()
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
    let coinbase_config = extracted_config
        .as_any()
        .downcast_ref::<CoinbaseDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "COINBASE-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), COINBASE);
    assert_eq!(extracted_factory.config_type(), "CoinbaseDataClientConfig");
    assert_eq!(coinbase_config.environment, CoinbaseEnvironment::Sandbox);
    assert_eq!(coinbase_config.http_timeout_secs, 7);
    assert_eq!(
        client.client_id(),
        ClientId::from("COINBASE-DATA-EXTRACTED")
    );
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("COINBASE-001");
    let factory = Py::new(
        py,
        CoinbaseExecutionClientFactory::new(trader_id, account_id),
    )
    .expect("factory should convert to Python object")
    .into_any();
    let config = Py::new(
        py,
        CoinbaseExecClientConfig {
            api_key: Some("organizations/test-org/apiKeys/test-key".to_string()),
            api_secret: Some("test-pem-placeholder".to_string()),
            account_type: AccountType::Cash,
            ..CoinbaseExecClientConfig::default()
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
    let coinbase_config = extracted_config
        .as_any()
        .downcast_ref::<CoinbaseExecClientConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create(
            "COINBASE-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
        )
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), COINBASE);
    assert_eq!(extracted_factory.config_type(), "CoinbaseExecClientConfig");
    assert_eq!(coinbase_config.account_type, AccountType::Cash);
    assert_eq!(
        client.client_id(),
        ClientId::from("COINBASE-EXEC-EXTRACTED")
    );
    assert_eq!(client.account_id(), account_id);
}
