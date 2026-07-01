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
use nautilus_hyperliquid::{
    common::{consts::HYPERLIQUID, enums::HyperliquidEnvironment},
    config::{HyperliquidDataClientConfig, HyperliquidExecClientConfig},
    factories::{
        HyperliquidDataClientFactory, HyperliquidExecFactoryConfig,
        HyperliquidExecutionClientFactory,
    },
    python,
};
use nautilus_model::identifiers::{AccountId, ClientId, TraderId};
use nautilus_network::websocket::TransportBackend;
use nautilus_system::get_global_pyo3_registry;
use pyo3::{
    Py, Python,
    types::{PyAnyMethods, PyModule},
};
use rstest::rstest;

const SMOKE_PRIVATE_KEY: &str =
    "0x59c6995e998f97a5a0044966f094538a1da6d1310dce3f687da73cf015b05d7e";

#[rstest]
fn test_hyperliquid_python_factories_extract_from_registry() {
    setup_data_event_sender();
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_hyperliquid_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
        assert_exec_factory_extracts_from_python_object(py);
    });
}

#[rstest]
fn test_hyperliquid_data_config_python_constructor_preserves_positional_order() {
    Python::initialize();

    Python::attach(|py| {
        let module = register_hyperliquid_python_module(py);
        let config_cls = module
            .getattr("HyperliquidDataClientConfig")
            .expect("HyperliquidDataClientConfig class should exist");

        let legacy_config: HyperliquidDataClientConfig = config_cls
            .call1((
                Option::<HyperliquidEnvironment>::None,
                Option::<String>::None,
                Option::<String>::None,
                Option::<String>::None,
                Option::<String>::None,
                Option::<u64>::None,
                Option::<u64>::None,
                17_u64,
            ))
            .expect("legacy positional constructor should succeed")
            .extract()
            .expect("legacy config should extract");

        let extended_config: HyperliquidDataClientConfig = config_cls
            .call1((
                Option::<HyperliquidEnvironment>::None,
                Option::<String>::None,
                Option::<String>::None,
                Option::<String>::None,
                Option::<String>::None,
                Option::<u64>::None,
                Option::<u64>::None,
                19_u64,
                Option::<TransportBackend>::None,
                31_u64,
                7_u64,
                23_u64,
            ))
            .expect("extended positional constructor should succeed")
            .extract()
            .expect("extended config should extract");

        assert_eq!(legacy_config.update_instruments_interval_mins, 17);
        assert_eq!(legacy_config.stale_stream_receive_timeout_secs, 120);
        assert_eq!(legacy_config.stream_health_check_interval_secs, 15);
        assert_eq!(legacy_config.stale_stream_warning_cooldown_secs, 60);
        assert_eq!(extended_config.update_instruments_interval_mins, 19);
        assert_eq!(extended_config.stale_stream_receive_timeout_secs, 31);
        assert_eq!(extended_config.stream_health_check_interval_secs, 7);
        assert_eq!(extended_config.stale_stream_warning_cooldown_secs, 23);
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

fn register_hyperliquid_python_module(py: Python<'_>) -> pyo3::Bound<'_, PyModule> {
    let module = PyModule::new(py, "hyperliquid").expect("Hyperliquid module should be created");
    python::hyperliquid(&module).expect("Hyperliquid Python module should register");
    module
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, HyperliquidDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        HyperliquidDataClientConfig {
            environment: HyperliquidEnvironment::Testnet,
            ..HyperliquidDataClientConfig::default()
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
    let hyperliquid_config = extracted_config
        .as_any()
        .downcast_ref::<HyperliquidDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "HYPERLIQUID-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), HYPERLIQUID);
    assert_eq!(
        extracted_factory.config_type(),
        "HyperliquidDataClientConfig"
    );
    assert_eq!(
        hyperliquid_config.environment,
        HyperliquidEnvironment::Testnet
    );
    assert_eq!(
        client.client_id(),
        ClientId::from("HYPERLIQUID-DATA-EXTRACTED")
    );
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("HYPERLIQUID-001");
    let factory = Py::new(py, HyperliquidExecutionClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        HyperliquidExecFactoryConfig {
            trader_id,
            account_id,
            config: HyperliquidExecClientConfig {
                private_key: Some(SMOKE_PRIVATE_KEY.to_string()),
                environment: HyperliquidEnvironment::Testnet,
                ..HyperliquidExecClientConfig::default()
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
    let hyperliquid_config = extracted_config
        .as_any()
        .downcast_ref::<HyperliquidExecFactoryConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create(
            "HYPERLIQUID-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
        )
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), HYPERLIQUID);
    assert_eq!(
        extracted_factory.config_type(),
        "HyperliquidExecFactoryConfig"
    );
    assert_eq!(hyperliquid_config.trader_id, trader_id);
    assert_eq!(hyperliquid_config.account_id, account_id);
    assert_eq!(
        client.client_id(),
        ClientId::from("HYPERLIQUID-EXEC-EXTRACTED")
    );
    assert_eq!(client.account_id(), account_id);
}
