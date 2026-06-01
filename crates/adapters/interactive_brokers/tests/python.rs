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
use nautilus_interactive_brokers::{
    common::consts::IB,
    config::{InteractiveBrokersDataClientConfig, InteractiveBrokersExecClientConfig},
    factories::{InteractiveBrokersDataClientFactory, InteractiveBrokersExecutionClientFactory},
    python,
};
use nautilus_model::identifiers::{AccountId, ClientId, TraderId};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{Py, Python, types::PyModule};
use rstest::rstest;

#[rstest]
fn test_interactive_brokers_python_factories_extract_from_registry() {
    setup_data_event_sender();
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_interactive_brokers_python_module(py);
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

fn register_interactive_brokers_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "interactive_brokers")
        .expect("Interactive Brokers module should be created");
    python::interactive_brokers(py, &module)
        .expect("Interactive Brokers Python module should register");
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, InteractiveBrokersDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        InteractiveBrokersDataClientConfig {
            client_id: 101,
            ..InteractiveBrokersDataClientConfig::default()
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
    let ib_config = extracted_config
        .as_any()
        .downcast_ref::<InteractiveBrokersDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "IB-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), IB);
    assert_eq!(
        extracted_factory.config_type(),
        "InteractiveBrokersDataClientConfig",
    );
    assert_eq!(ib_config.client_id, 101);
    assert_eq!(client.client_id(), ClientId::from("IB-DATA-EXTRACTED"));
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("IB-001");
    let factory = Py::new(
        py,
        InteractiveBrokersExecutionClientFactory::new(trader_id, account_id),
    )
    .expect("factory should convert to Python object")
    .into_any();
    let config = Py::new(
        py,
        InteractiveBrokersExecClientConfig {
            client_id: 101,
            ..InteractiveBrokersExecClientConfig::default()
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
    let ib_config = extracted_config
        .as_any()
        .downcast_ref::<InteractiveBrokersExecClientConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create("IB-EXEC-EXTRACTED", extracted_config.as_ref(), cache.into())
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), IB);
    assert_eq!(
        extracted_factory.config_type(),
        "InteractiveBrokersExecClientConfig",
    );
    assert_eq!(ib_config.client_id, 101);
    assert_eq!(client.client_id(), ClientId::from("IB-EXEC-EXTRACTED"));
    assert_eq!(client.account_id(), account_id);
}
