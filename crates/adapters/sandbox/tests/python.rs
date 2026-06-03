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
    python::sandbox(py, &module).expect("Sandbox Python module should register");
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
            trader_id,
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
        .create("SANDBOX-EXEC-EXTRACTED", extracted_config.as_ref(), cache)
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), SANDBOX);
    assert_eq!(
        extracted_factory.config_type(),
        "SandboxExecutionClientConfig"
    );
    assert_eq!(sandbox_config.trader_id, trader_id);
    assert_eq!(sandbox_config.account_id, account_id);
    assert_eq!(client.client_id(), ClientId::from("SANDBOX-EXEC-EXTRACTED"));
    assert_eq!(client.account_id(), account_id);
}
