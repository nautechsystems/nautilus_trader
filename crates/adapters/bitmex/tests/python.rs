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

use nautilus_bitmex::{
    common::{consts::BITMEX, enums::BitmexEnvironment},
    config::{BitmexDataClientConfig, BitmexExecClientConfig},
    factories::{BitmexDataClientFactory, BitmexExecFactoryConfig, BitmexExecutionClientFactory},
    python,
};
use nautilus_common::{
    cache::Cache,
    clock::TestClock,
    live::runner::{replace_data_event_sender, replace_exec_event_sender},
    messages::{DataEvent, ExecutionEvent},
};
use nautilus_model::identifiers::{AccountId, ClientId, TraderId};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{Py, Python, types::PyModule};
use rstest::rstest;

const SMOKE_API_KEY: &str = "test_key";
const SMOKE_API_SECRET: &str = "test_secret";

#[rstest]
fn test_bitmex_python_factories_extract_from_registry() {
    setup_data_event_sender();
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_bitmex_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
        assert_exec_factory_extracts_from_python_object(py);
    });
}

fn setup_data_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(sender);
}

fn setup_exec_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    replace_exec_event_sender(sender);
}

fn register_bitmex_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "bitmex").expect("BitMEX module should be created");
    python::bitmex(py, &module).expect("BitMEX Python module should register");
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, BitmexDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        BitmexDataClientConfig {
            environment: BitmexEnvironment::Testnet,
            ..BitmexDataClientConfig::default()
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
    let bitmex_config = extracted_config
        .as_any()
        .downcast_ref::<BitmexDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "BITMEX-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), BITMEX);
    assert_eq!(extracted_factory.config_type(), "BitmexDataClientConfig");
    assert_eq!(bitmex_config.environment, BitmexEnvironment::Testnet);
    assert_eq!(client.client_id(), ClientId::from("BITMEX-DATA-EXTRACTED"));
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("BITMEX-001");
    let factory = Py::new(py, BitmexExecutionClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        BitmexExecFactoryConfig {
            trader_id,
            account_id,
            config: BitmexExecClientConfig {
                api_key: Some(SMOKE_API_KEY.to_string()),
                api_secret: Some(SMOKE_API_SECRET.to_string()),
                environment: BitmexEnvironment::Testnet,
                ..BitmexExecClientConfig::default()
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
    let bitmex_config = extracted_config
        .as_any()
        .downcast_ref::<BitmexExecFactoryConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create(
            "BITMEX-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
        )
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), BITMEX);
    assert_eq!(extracted_factory.config_type(), "BitmexExecFactoryConfig");
    assert_eq!(bitmex_config.trader_id, trader_id);
    assert_eq!(bitmex_config.account_id, account_id);
    assert_eq!(client.client_id(), ClientId::from("BITMEX-EXEC-EXTRACTED"));
    assert_eq!(client.account_id(), account_id);
}
