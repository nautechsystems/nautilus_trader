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

use nautilus_betfair::{
    common::consts::BETFAIR,
    config::{BetfairDataConfig, BetfairExecConfig},
    factories::{BetfairDataClientFactory, BetfairExecutionClientFactory},
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

const SMOKE_USERNAME: &str = "smoke_user";
const SMOKE_PASSWORD: &str = "smoke_password";
const SMOKE_APP_KEY: &str = "smoke_app_key";

#[rstest]
fn test_betfair_python_factories_extract_from_registry() {
    setup_data_event_sender();
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_betfair_python_module(py);
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

fn register_betfair_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "betfair").expect("Betfair module should be created");
    python::betfair(py, &module).expect("Betfair Python module should register");
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, BetfairDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        BetfairDataConfig {
            username: Some(SMOKE_USERNAME.to_string()),
            password: Some(SMOKE_PASSWORD.to_string()),
            app_key: Some(SMOKE_APP_KEY.to_string()),
            market_types: Some(vec!["MATCH_ODDS".to_string()]),
            ..BetfairDataConfig::default()
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
    let betfair_config = extracted_config
        .as_any()
        .downcast_ref::<BetfairDataConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "BETFAIR-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), BETFAIR);
    assert_eq!(extracted_factory.config_type(), "BetfairDataConfig");
    assert_eq!(betfair_config.username.as_deref(), Some(SMOKE_USERNAME));
    assert_eq!(
        betfair_config.market_types.as_deref(),
        Some(&["MATCH_ODDS".to_string()][..])
    );
    assert_eq!(client.client_id(), ClientId::from("BETFAIR-DATA-EXTRACTED"));
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("BETFAIR-001");
    let factory = Py::new(py, BetfairExecutionClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        BetfairExecConfig {
            trader_id,
            account_id,
            username: Some(SMOKE_USERNAME.to_string()),
            password: Some(SMOKE_PASSWORD.to_string()),
            app_key: Some(SMOKE_APP_KEY.to_string()),
            ..BetfairExecConfig::default()
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
    let betfair_config = extracted_config
        .as_any()
        .downcast_ref::<BetfairExecConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create(
            "BETFAIR-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
        )
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), BETFAIR);
    assert_eq!(extracted_factory.config_type(), "BetfairExecConfig");
    assert_eq!(betfair_config.trader_id, trader_id);
    assert_eq!(betfair_config.account_id, account_id);
    assert_eq!(client.client_id(), ClientId::from("BETFAIR-EXEC-EXTRACTED"));
    assert_eq!(client.account_id(), account_id);
}
