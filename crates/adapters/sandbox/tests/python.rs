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
    cache::Cache, live::runner::replace_exec_event_sender, messages::ExecutionEvent,
};
use nautilus_model::{
    identifiers::{AccountId, ClientId, TraderId, Venue},
    types::Money,
};
use nautilus_sandbox::{
    config::SandboxExecutionClientConfig, factory::SandboxExecutionClientFactory, python,
};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{Py, Python, types::PyModule};
use rstest::rstest;

const SANDBOX: &str = "SANDBOX";

#[rstest]
fn test_sandbox_python_sim_exec_factory_extracts_from_registry() {
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_sandbox_python_module(py);
        assert_exec_factory_extracts_from_python_object(py);
    });
}

fn setup_exec_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    replace_exec_event_sender(sender);
}

fn register_sandbox_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "sandbox").expect("Sandbox module should be created");
    if let Err(e) = python::sandbox(py, &module) {
        let message = e.to_string();
        assert!(
            message.contains("already registered"),
            "Sandbox Python module should register: {e}",
        );
    }
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("SANDBOX-001");
    let factory = Py::new(py, SandboxExecutionClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        SandboxExecutionClientConfig {
            account_id,
            venue: Venue::new(SANDBOX),
            starting_balances: vec![Money::from("100_000 USD")],
            ..SandboxExecutionClientConfig::default()
        },
    )
    .expect("config should convert to Python object")
    .into_any();
    let registry = get_global_pyo3_registry();

    let extracted_factory = registry
        .extract_sim_exec_factory(py, factory)
        .expect("simulated exec factory should extract");
    let extracted_config = registry
        .extract_config(py, config)
        .expect("exec config should extract");
    let sandbox_config = extracted_config
        .as_any()
        .downcast_ref::<SandboxExecutionClientConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create(
            trader_id,
            "SANDBOX-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            cache,
        )
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), SANDBOX);
    assert_eq!(
        extracted_factory.config_type(),
        "SandboxExecutionClientConfig"
    );
    assert_eq!(sandbox_config.account_id, account_id);
    assert_eq!(client.client_id(), ClientId::from("SANDBOX-EXEC-EXTRACTED"));
    assert_eq!(client.account_id(), account_id);
}

#[rstest]
fn test_sandbox_python_extract_preserves_matching_knobs() {
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_sandbox_python_module(py);

        let trader_id = TraderId::from("TRADER-001");
        let account_id = AccountId::from("SANDBOX-001");
        let factory = Py::new(py, SandboxExecutionClientFactory::new())
            .expect("factory should convert to Python object")
            .into_any();
        let config = Py::new(
            py,
            SandboxExecutionClientConfig {
                account_id,
                venue: Venue::new(SANDBOX),
                starting_balances: vec![Money::from("100_000 USD")],
                queue_position: true,
                liquidity_consumption: true,
                bar_adaptive_high_low_ordering: true,
                use_market_order_acks: true,
                oto_full_trigger: true,
                price_protection_points: 50,
                ..SandboxExecutionClientConfig::default()
            },
        )
        .expect("config should convert to Python object")
        .into_any();
        let registry = get_global_pyo3_registry();
        let extracted_config = registry
            .extract_config(py, config)
            .expect("exec config should extract");
        let sandbox_config = extracted_config
            .as_any()
            .downcast_ref::<SandboxExecutionClientConfig>()
            .expect("exec config should downcast");
        let engine_config = sandbox_config.to_matching_engine_config();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let extracted_factory = registry
            .extract_sim_exec_factory(py, factory)
            .expect("simulated exec factory should extract");
        extracted_factory
            .create(
                trader_id,
                "SANDBOX-EXEC-MATCHING",
                extracted_config.as_ref(),
                cache,
            )
            .expect("extracted factory should create exec client");

        assert!(sandbox_config.queue_position);
        assert!(sandbox_config.liquidity_consumption);
        assert!(sandbox_config.bar_adaptive_high_low_ordering);
        assert!(sandbox_config.use_market_order_acks);
        assert!(sandbox_config.oto_full_trigger);
        assert_eq!(sandbox_config.price_protection_points, 50);
        assert!(engine_config.queue_position);
        assert!(engine_config.liquidity_consumption);
        assert!(engine_config.bar_adaptive_high_low_ordering);
        assert!(engine_config.use_market_order_acks);
        assert!(engine_config.oto_full_trigger);
        assert_eq!(engine_config.price_protection_points, Some(50));
    });
}
