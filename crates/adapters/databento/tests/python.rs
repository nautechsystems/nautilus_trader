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

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use nautilus_common::{
    cache::Cache, clock::TestClock, live::runner::replace_data_event_sender, messages::DataEvent,
};
use nautilus_databento::{
    common::DATABENTO,
    enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction},
    factories::{DatabentoDataClientFactory, DatabentoLiveClientConfig},
    python,
    types::{DatabentoImbalance, DatabentoStatistics},
};
use nautilus_model::{
    enums::OrderSide,
    identifiers::{ClientId, InstrumentId},
    types::{Price, Quantity},
};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{
    Py, Python,
    types::{PyAnyMethods, PyModule},
};
use rstest::rstest;

#[rstest]
fn test_databento_python_data_factory_extracts_from_registry() {
    setup_data_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_databento_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
    });
}

#[rstest]
fn test_databento_imbalance_to_from_dict_round_trip() {
    Python::initialize();

    Python::attach(|py| {
        let original = test_imbalance();
        let py_obj = Py::new(py, original.clone()).expect("imbalance should convert to Python");
        let dict = py_obj
            .bind(py)
            .call_method0("to_dict")
            .expect("to_dict should succeed");
        let instrument_id = dict
            .get_item("instrument_id")
            .expect("dict lookup should succeed")
            .extract::<String>()
            .expect("instrument_id should be a string");
        let restored = py_obj
            .bind(py)
            .get_type()
            .call_method1("from_dict", (dict,))
            .expect("from_dict should succeed")
            .extract::<DatabentoImbalance>()
            .expect("restored imbalance should extract");

        assert_eq!(instrument_id, "AAPL.XNAS");
        assert_eq!(restored, original);
    });
}

#[rstest]
fn test_databento_statistics_to_from_dict_round_trip() {
    Python::initialize();

    Python::attach(|py| {
        let original = test_statistics();
        let py_obj = Py::new(py, original.clone()).expect("statistics should convert to Python");
        let dict = py_obj
            .bind(py)
            .call_method0("to_dict")
            .expect("to_dict should succeed");
        let stat_type = dict
            .get_item("stat_type")
            .expect("dict lookup should succeed")
            .extract::<String>()
            .expect("stat_type should be a string");
        let restored = py_obj
            .bind(py)
            .get_type()
            .call_method1("from_dict", (dict,))
            .expect("from_dict should succeed")
            .extract::<DatabentoStatistics>()
            .expect("restored statistics should extract");

        assert_eq!(stat_type, "OPENING_PRICE");
        assert_eq!(restored, original);
    });
}

fn setup_data_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(sender);
}

fn register_databento_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "databento").expect("Databento module should be created");
    python::databento(py, &module).expect("Databento Python module should register");
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let publishers_filepath = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json");
    let factory = Py::new(py, DatabentoDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        DatabentoLiveClientConfig::new(
            "00000000000000000000000000000000",
            publishers_filepath.clone(),
            false,
            true,
        ),
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
    let databento_config = extracted_config
        .as_any()
        .downcast_ref::<DatabentoLiveClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "DATABENTO-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), DATABENTO);
    assert_eq!(extracted_factory.config_type(), "DatabentoLiveClientConfig");
    assert_eq!(databento_config.publishers_filepath, publishers_filepath);
    assert_eq!(
        client.client_id(),
        ClientId::from("DATABENTO-DATA-EXTRACTED")
    );
}

fn test_imbalance() -> DatabentoImbalance {
    DatabentoImbalance::new(
        InstrumentId::from("AAPL.XNAS"),
        Price::from("100.50"),
        Price::from("100.45"),
        Price::from("100.55"),
        Quantity::from("1000"),
        Quantity::from("500"),
        OrderSide::Buy,
        b'Y' as std::ffi::c_char,
        1.into(),
        2.into(),
        3.into(),
    )
}

fn test_statistics() -> DatabentoStatistics {
    DatabentoStatistics::new(
        InstrumentId::from("ESM4.GLBX"),
        DatabentoStatisticType::OpeningPrice,
        DatabentoStatisticUpdateAction::Added,
        Some(Price::from("5000.50")),
        Some(Quantity::from("100")),
        1,
        0,
        42,
        1_000_000_000.into(),
        500,
        2_000_000_000.into(),
        3_000_000_000.into(),
        4_000_000_000.into(),
    )
}
