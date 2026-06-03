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
    cache::Cache, clock::TestClock, live::runner::replace_data_event_sender, messages::DataEvent,
};
use nautilus_model::identifiers::ClientId;
use nautilus_system::get_global_pyo3_registry;
use nautilus_tardis::{
    common::consts::TARDIS, config::TardisDataClientConfig, factories::TardisDataClientFactory,
    python,
};
use pyo3::{Py, Python, types::PyModule};
use rstest::rstest;

#[rstest]
fn test_tardis_python_data_factory_extracts_from_registry() {
    setup_data_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_tardis_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
    });
}

fn setup_data_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(sender);
}

fn register_tardis_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "tardis").expect("Tardis module should be created");
    python::tardis(py, &module).expect("Tardis Python module should register");
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, TardisDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        TardisDataClientConfig {
            tardis_ws_url: Some("ws://localhost:8001".to_string()),
            ..TardisDataClientConfig::default()
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
    let tardis_config = extracted_config
        .as_any()
        .downcast_ref::<TardisDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "TARDIS-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), TARDIS);
    assert_eq!(extracted_factory.config_type(), "TardisDataClientConfig");
    assert_eq!(
        tardis_config.tardis_ws_url.as_deref(),
        Some("ws://localhost:8001")
    );
    assert_eq!(client.client_id(), ClientId::from("TARDIS-DATA-EXTRACTED"));
}
