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
use nautilus_kraken::{
    common::{
        consts::KRAKEN,
        enums::{KrakenEnvironment, KrakenProductType},
    },
    config::{KrakenDataClientConfig, KrakenExecClientConfig},
    factories::{KrakenDataClientFactory, KrakenExecutionClientFactory},
    python,
};
use nautilus_model::identifiers::{AccountId, ClientId, TraderId};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{Py, Python, types::PyModule};
use rstest::rstest;

const SMOKE_API_KEY: &str = "test_key";
const SMOKE_API_SECRET: &str = "test_secret";

#[rstest]
fn test_kraken_python_factories_extract_from_registry() {
    setup_data_event_sender();
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_kraken_python_module(py);
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

fn register_kraken_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "kraken").expect("Kraken module should be created");
    python::kraken(&module).expect("Kraken Python module should register");
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, KrakenDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        KrakenDataClientConfig {
            product_type: KrakenProductType::Futures,
            environment: KrakenEnvironment::Demo,
            ..KrakenDataClientConfig::default()
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
    let kraken_config = extracted_config
        .as_any()
        .downcast_ref::<KrakenDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "KRAKEN-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), KRAKEN);
    assert_eq!(extracted_factory.config_type(), "KrakenDataClientConfig");
    assert_eq!(kraken_config.product_type, KrakenProductType::Futures);
    assert_eq!(kraken_config.environment, KrakenEnvironment::Demo);
    assert_eq!(client.client_id(), ClientId::from("KRAKEN-DATA-EXTRACTED"));
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("KRAKEN-001");
    let factory = Py::new(py, KrakenExecutionClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        KrakenExecClientConfig {
            trader_id,
            account_id,
            api_key: SMOKE_API_KEY.to_string(),
            api_secret: SMOKE_API_SECRET.to_string(),
            product_type: KrakenProductType::Futures,
            environment: KrakenEnvironment::Demo,
            ..KrakenExecClientConfig::default()
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
    let kraken_config = extracted_config
        .as_any()
        .downcast_ref::<KrakenExecClientConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create(
            "KRAKEN-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
        )
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), KRAKEN);
    assert_eq!(extracted_factory.config_type(), "KrakenExecClientConfig");
    assert_eq!(kraken_config.trader_id, trader_id);
    assert_eq!(kraken_config.account_id, account_id);
    assert_eq!(kraken_config.product_type, KrakenProductType::Futures);
    assert_eq!(client.client_id(), ClientId::from("KRAKEN-EXEC-EXTRACTED"));
    assert_eq!(client.account_id(), account_id);
}
