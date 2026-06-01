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
use nautilus_model::identifiers::{AccountId, ClientId, TraderId};
use nautilus_okx::{
    common::{
        consts::OKX,
        enums::{OKXEnvironment, OKXInstrumentType, OKXMarginMode},
    },
    config::{OKXDataClientConfig, OKXExecClientConfig},
    factories::{OKXDataClientFactory, OKXExecutionClientFactory},
    python,
};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{Py, Python, types::PyModule};
use rstest::rstest;

const SMOKE_API_KEY: &str = "test_key";
const SMOKE_API_SECRET: &str = "test_secret";
const SMOKE_API_PASSPHRASE: &str = "test_passphrase";

fn register_okx_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "okx").expect("OKX module should be created");
    python::okx(py, &module).expect("OKX Python module should register");
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
fn test_okx_python_factories_extract_from_registry() {
    setup_data_event_sender();
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_okx_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
        assert_exec_factory_extracts_from_python_object(py);
    });
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, OKXDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        OKXDataClientConfig {
            environment: OKXEnvironment::Demo,
            instrument_types: vec![OKXInstrumentType::Spot],
            http_timeout_secs: 7,
            ..OKXDataClientConfig::default()
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
    let okx_config = extracted_config
        .as_any()
        .downcast_ref::<OKXDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "OKX-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), OKX);
    assert_eq!(extracted_factory.config_type(), "OKXDataClientConfig");
    assert_eq!(okx_config.environment, OKXEnvironment::Demo);
    assert_eq!(okx_config.instrument_types, [OKXInstrumentType::Spot]);
    assert_eq!(okx_config.http_timeout_secs, 7);
    assert_eq!(client.client_id(), ClientId::from("OKX-DATA-EXTRACTED"));
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("OKX-001");
    let factory = Py::new(py, OKXExecutionClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        OKXExecClientConfig {
            trader_id,
            account_id,
            environment: OKXEnvironment::Demo,
            api_key: Some(SMOKE_API_KEY.to_string()),
            api_secret: Some(SMOKE_API_SECRET.to_string()),
            api_passphrase: Some(SMOKE_API_PASSPHRASE.to_string()),
            instrument_types: vec![OKXInstrumentType::Swap],
            margin_mode: Some(OKXMarginMode::Cross),
            ..OKXExecClientConfig::default()
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
    let okx_config = extracted_config
        .as_any()
        .downcast_ref::<OKXExecClientConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create(
            "OKX-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
        )
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), OKX);
    assert_eq!(extracted_factory.config_type(), "OKXExecClientConfig");
    assert_eq!(okx_config.trader_id, trader_id);
    assert_eq!(okx_config.account_id, account_id);
    assert_eq!(okx_config.environment, OKXEnvironment::Demo);
    assert_eq!(okx_config.instrument_types, [OKXInstrumentType::Swap]);
    assert_eq!(okx_config.margin_mode, Some(OKXMarginMode::Cross));
    assert_eq!(client.client_id(), ClientId::from("OKX-EXEC-EXTRACTED"));
    assert_eq!(client.account_id(), account_id);
}
