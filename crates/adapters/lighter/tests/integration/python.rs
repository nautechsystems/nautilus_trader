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
use nautilus_lighter::{
    common::{
        consts::LIGHTER,
        enums::{LighterDeployment, LighterEnvironment},
    },
    config::{LighterDataClientConfig, LighterExecutionClientConfig},
    factories::{LighterDataClientFactory, LighterExecutionClientFactory},
    python,
};
use nautilus_model::identifiers::{AccountId, ClientId, TraderId, Venue};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{
    Py, Python,
    types::{PyAnyMethods, PyModule, PyModuleMethods},
};
use rstest::rstest;

const PRIVATE_KEY_HEX: &str =
    "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";

fn register_lighter_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "lighter").expect("Lighter module should be created");
    python::lighter(&module).expect("Lighter Python module should register");
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
fn test_lighter_python_configs_default_to_lighter_mainnet() {
    Python::initialize();

    Python::attach(|py| {
        let module = PyModule::new(py, "lighter").expect("Lighter module should be created");
        module
            .add_class::<LighterDataClientConfig>()
            .expect("data config class should register");
        module
            .add_class::<LighterExecutionClientConfig>()
            .expect("execution config class should register");

        let data_config = module
            .getattr("LighterDataClientConfig")
            .expect("data config class should be registered")
            .call0()
            .expect("data config should construct")
            .extract::<LighterDataClientConfig>()
            .expect("data config should extract");

        let account_id = Py::new(py, AccountId::from("LIGHTER-001"))
            .expect("account ID should convert to Python");

        let exec_config = module
            .getattr("LighterExecutionClientConfig")
            .expect("execution config class should be registered")
            .call1((account_id,))
            .expect("execution config should construct")
            .extract::<LighterExecutionClientConfig>()
            .expect("execution config should extract");

        assert_eq!(data_config.environment, LighterEnvironment::Mainnet);
        assert_eq!(data_config.deployment, LighterDeployment::Lighter);
        assert_eq!(data_config.venue, None);
        assert_eq!(exec_config.environment, LighterEnvironment::Mainnet);
        assert_eq!(exec_config.deployment, LighterDeployment::Lighter);
        assert_eq!(exec_config.venue, None);
    });
}

#[rstest]
fn test_lighter_python_factories_extract_from_registry() {
    setup_data_event_sender();
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_lighter_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
        assert_exec_factory_extracts_from_python_object(py);
    });
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, LighterDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();

    let config = Py::new(
        py,
        LighterDataClientConfig {
            deployment: LighterDeployment::Robinhood,
            venue: Some(Venue::from("LIGHTER_CUSTOM")),
            environment: LighterEnvironment::Testnet,
            account_index: Some(12_345),
            api_key_index: Some(5),
            private_key: Some(PRIVATE_KEY_HEX.into()),
            http_timeout_secs: 7,
            rest_quota_per_min: Some(24_000),
            ..LighterDataClientConfig::default()
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
    let lighter_config = extracted_config
        .as_any()
        .downcast_ref::<LighterDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "LIGHTER-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), LIGHTER);
    assert_eq!(extracted_factory.config_type(), "LighterDataClientConfig");
    assert_eq!(lighter_config.environment, LighterEnvironment::Testnet);
    assert_eq!(lighter_config.deployment, LighterDeployment::Robinhood);
    assert_eq!(lighter_config.venue, Some(Venue::from("LIGHTER_CUSTOM")));
    assert_eq!(lighter_config.http_timeout_secs, 7);
    assert_eq!(lighter_config.rest_quota_per_min, Some(24_000));
    assert_eq!(client.client_id(), ClientId::from("LIGHTER-DATA-EXTRACTED"));
    assert_eq!(client.venue(), Some(Venue::from("LIGHTER_CUSTOM")));
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("LIGHTER_ROBINHOOD-001");
    let factory = Py::new(py, LighterExecutionClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();

    let config = Py::new(
        py,
        LighterExecutionClientConfig::builder()
            .account_id(account_id)
            .deployment(LighterDeployment::Robinhood)
            .environment(LighterEnvironment::Testnet)
            .account_index(12_345)
            .api_key_index(5)
            .private_key(PRIVATE_KEY_HEX.into())
            .rest_quota_per_min(24_000)
            .sendtx_quota_per_min(4_000)
            .build(),
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
    let lighter_config = extracted_config
        .as_any()
        .downcast_ref::<LighterExecutionClientConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create(
            trader_id,
            "LIGHTER-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
        )
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), LIGHTER);
    assert_eq!(
        extracted_factory.config_type(),
        "LighterExecutionClientConfig"
    );
    assert_eq!(lighter_config.account_id, account_id);
    assert_eq!(lighter_config.environment, LighterEnvironment::Testnet);
    assert_eq!(lighter_config.deployment, LighterDeployment::Robinhood);
    assert_eq!(lighter_config.venue, None);
    assert_eq!(lighter_config.rest_quota_per_min, Some(24_000));
    assert_eq!(lighter_config.sendtx_quota_per_min, Some(4_000));
    assert_eq!(client.client_id(), ClientId::from("LIGHTER-EXEC-EXTRACTED"));
    assert_eq!(client.account_id(), account_id);
    assert_eq!(client.venue(), Venue::from("LIGHTER_ROBINHOOD"));
}
