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

//! Python binding tests for the Kalshi adapter.
//!
//! The bindings are what a Python `TradingNode` resolves factory and config classes through, so
//! these tests drive the same paths: the module surface, then the registry extractors.

#![cfg(feature = "python")]

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::TestClock,
    live::runner::{replace_data_event_sender, replace_exec_event_sender},
    messages::{DataEvent, ExecutionEvent},
};
use nautilus_kalshi::{
    common::{
        consts::{KALSHI, KALSHI_ACCOUNT_ID, KALSHI_CLIENT_ID, KALSHI_VENUE},
        enums::{KalshiEnvironment, KalshiSelfTradePrevention},
    },
    config::{KalshiDataClientConfig, KalshiExecClientConfig},
    factories::{KalshiDataClientFactory, KalshiExecutionClientFactory},
    python,
};
use nautilus_model::identifiers::{AccountId, ClientId, TraderId, Venue};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{Bound, Py, Python, prelude::PyAnyMethods, types::PyModule};
use rstest::rstest;

use crate::harness::{API_KEY_ID, TICKER, test_private_key_pem};

fn register_kalshi_python_module<'py>(py: Python<'py>) -> Bound<'py, PyModule> {
    let module = PyModule::new(py, "kalshi").expect("Kalshi module should be created");
    python::kalshi(&module).expect("Kalshi Python module should register");
    module
}

fn setup_event_senders() -> (
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) {
    let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(data_tx);
    let (exec_tx, exec_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    replace_exec_event_sender(exec_tx);
    (data_rx, exec_rx)
}

fn data_config() -> KalshiDataClientConfig {
    KalshiDataClientConfig::builder()
        .api_key_id(API_KEY_ID.to_string())
        .api_key_pem(test_private_key_pem().into())
        .environment(KalshiEnvironment::Prod)
        .event_tickers(vec![TICKER.to_string()])
        .build()
}

fn exec_config() -> KalshiExecClientConfig {
    KalshiExecClientConfig::builder()
        .api_key_id(API_KEY_ID.to_string())
        .api_key_pem(test_private_key_pem().into())
        .reconciliation(false)
        .self_trade_prevention(KalshiSelfTradePrevention::Maker)
        .build()
}

fn assert_module_surface(module: &Bound<'_, PyModule>) {
    let constant = |name: &str| module.getattr(name).expect("constant should be exposed");

    assert_eq!(
        constant("KALSHI").extract::<String>().unwrap(),
        KALSHI.to_string()
    );
    assert_eq!(
        constant("KALSHI_CLIENT_ID").extract::<ClientId>().unwrap(),
        ClientId::from(KALSHI_CLIENT_ID)
    );
    assert_eq!(
        constant("KALSHI_VENUE").extract::<Venue>().unwrap(),
        Venue::from(KALSHI_VENUE)
    );

    let class_name = |name: &str| {
        module
            .getattr(name)
            .expect("class should be exposed")
            .getattr("__name__")
            .unwrap()
            .extract::<String>()
            .unwrap()
    };

    assert_eq!(
        class_name("KalshiDataClientConfig"),
        "KalshiDataClientConfig"
    );
    assert_eq!(
        class_name("KalshiExecutionClientConfig"),
        "KalshiExecutionClientConfig"
    );
    assert_eq!(
        class_name("KalshiDataClientFactory"),
        "KalshiDataClientFactory"
    );
    assert_eq!(
        class_name("KalshiExecutionClientFactory"),
        "KalshiExecutionClientFactory"
    );
    assert_eq!(class_name("KalshiEnvironment"), "KalshiEnvironment");
    assert_eq!(
        class_name("KalshiSelfTradePrevention"),
        "KalshiSelfTradePrevention"
    );
}

fn assert_data_factory_extracts_from_the_registry(py: Python<'_>) {
    let factory = Py::new(py, KalshiDataClientFactory)
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(py, data_config())
        .expect("config should convert to Python object")
        .into_any();
    let registry = get_global_pyo3_registry();

    let extracted_factory = registry
        .extract_factory(py, factory)
        .expect("data factory should extract");
    let extracted_config = registry
        .extract_config(py, config)
        .expect("data config should extract");
    let extracted = extracted_config
        .as_any()
        .downcast_ref::<KalshiDataClientConfig>()
        .expect("data config should downcast");

    assert_eq!(extracted_factory.name(), KALSHI);
    assert_eq!(extracted_factory.config_type(), "KalshiDataClientConfig");
    assert_eq!(extracted.environment, KalshiEnvironment::Prod);
    assert_eq!(extracted.event_tickers, vec![TICKER.to_string()]);

    let client = extracted_factory
        .create(
            "KALSHI-DATA-EXTRACTED",
            extracted_config.as_ref(),
            Rc::new(RefCell::new(Cache::default())).into(),
            Rc::new(RefCell::new(TestClock::new())),
        )
        .expect("extracted factory should create a data client");

    assert_eq!(client.client_id(), ClientId::from("KALSHI-DATA-EXTRACTED"));
}

fn assert_exec_factory_extracts_from_the_registry(py: Python<'_>) {
    let factory = Py::new(py, KalshiExecutionClientFactory)
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(py, exec_config())
        .expect("config should convert to Python object")
        .into_any();
    let registry = get_global_pyo3_registry();

    let extracted_factory = registry
        .extract_exec_factory(py, factory)
        .expect("exec factory should extract");
    let extracted_config = registry
        .extract_config(py, config)
        .expect("exec config should extract");
    let extracted = extracted_config
        .as_any()
        .downcast_ref::<KalshiExecClientConfig>()
        .expect("exec config should downcast");

    assert_eq!(extracted_factory.name(), KALSHI);
    assert_eq!(
        extracted_factory.config_type(),
        "KalshiExecutionClientConfig"
    );
    assert!(!extracted.reconciliation);
    assert_eq!(
        extracted.self_trade_prevention,
        KalshiSelfTradePrevention::Maker
    );

    let client = extracted_factory
        .create(
            TraderId::from("TESTER-001"),
            "KALSHI-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            Rc::new(RefCell::new(Cache::default())).into(),
            Rc::new(RefCell::new(TestClock::new())),
        )
        .expect("extracted factory should create an exec client");

    assert_eq!(client.client_id(), ClientId::from("KALSHI-EXEC-EXTRACTED"));
    assert_eq!(client.account_id(), AccountId::from(KALSHI_ACCOUNT_ID));
}

/// Drives the module surface and both extractors a Python `TradingNode` resolves through.
///
/// The registration happens once per process, because `nautilus_system`'s registry rejects a
/// second registration of the same factory, so one test covers the whole surface.
#[rstest]
fn test_kalshi_python_bindings_expose_the_adapter_surface() {
    let (_data_rx, _exec_rx) = setup_event_senders();
    Python::initialize();

    Python::attach(|py| {
        let module = register_kalshi_python_module(py);

        assert_module_surface(&module);
        assert_data_factory_extracts_from_the_registry(py);
        assert_exec_factory_extracts_from_the_registry(py);
    });
}
