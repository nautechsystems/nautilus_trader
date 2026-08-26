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
    cache::Cache, clock::TestClock, live::runner::set_data_event_sender, messages::DataEvent,
};
use nautilus_massive::{
    common::{consts::MASSIVE, enums::MassiveDataFeed},
    config::MassiveDataClientConfig,
    factories::MassiveDataClientFactory,
    python,
};
use nautilus_model::identifiers::ClientId;
use nautilus_system::get_global_pyo3_registry;
use pyo3::{Py, Python, types::PyModule};
use rstest::rstest;

fn register_massive_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "massive").expect("Massive module should be created");
    python::massive(py, &module).expect("Massive Python module should register");
}

fn setup_data_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(sender);
}

#[rstest]
fn test_massive_python_factory_extracts_from_registry() {
    setup_data_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_massive_python_module(py);

        let factory = Py::new(py, MassiveDataClientFactory::new())
            .expect("factory should convert to Python object")
            .into_any();
        let config = Py::new(
            py,
            MassiveDataClientConfig {
                feed: MassiveDataFeed::Delayed,
                http_timeout_secs: 7,
                symbols: vec!["AAPL".to_string()],
                ..MassiveDataClientConfig::default()
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
        let massive_config = extracted_config
            .as_any()
            .downcast_ref::<MassiveDataClientConfig>()
            .expect("data config should downcast");
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let client = extracted_factory
            .create(
                "MASSIVE-DATA-EXTRACTED",
                extracted_config.as_ref(),
                cache.into(),
                clock,
            )
            .expect("extracted factory should create data client");

        assert_eq!(extracted_factory.name(), MASSIVE);
        assert_eq!(extracted_factory.config_type(), "MassiveDataClientConfig");
        assert_eq!(massive_config.feed, MassiveDataFeed::Delayed);
        assert_eq!(massive_config.http_timeout_secs, 7);
        assert_eq!(massive_config.symbols, vec!["AAPL".to_string()]);
        assert_eq!(client.client_id(), ClientId::from("MASSIVE-DATA-EXTRACTED"));
    });
}
