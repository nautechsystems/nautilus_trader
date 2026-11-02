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

use nautilus_binance::{
    common::{
        consts::BINANCE,
        enums::{BinanceEnvironment, BinanceProductType},
    },
    config::{
        BinanceDataClientConfig, BinanceExecutionClientConfig, BinanceInstrumentProviderConfig,
    },
    factories::{BinanceDataClientFactory, BinanceExecutionClientFactory},
    python,
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::VirtualClock,
    factories::ExecutionClientFactory,
    live::runner::{replace_data_event_sender, replace_exec_event_sender},
    messages::{DataEvent, ExecutionEvent},
};
use nautilus_model::identifiers::{AccountId, ClientId, TraderId};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{
    Py, Python,
    types::{PyAnyMethods, PyDict, PyDictMethods, PyModule},
};
use rstest::rstest;

const SMOKE_API_KEY: &str = "test_key";
const SMOKE_API_SECRET: &str = "test_secret";

#[rstest]
fn test_binance_python_factories_extract_from_registry() {
    setup_data_event_sender();
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_binance_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
        assert_exec_factory_extracts_from_python_object(py);
    });
}

fn setup_data_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(sender);
}

pub(crate) fn create_execution_client(
    product_type: BinanceProductType,
    base_url_http: &str,
    base_url_ws: &str,
    cache: Rc<RefCell<Cache>>,
) -> Box<dyn ExecutionClient> {
    Python::initialize();
    Python::attach(|py| {
        let provider_args = PyDict::new(py);
        provider_args.set_item("load_all", false).unwrap();

        let id = if product_type == BinanceProductType::Spot {
            "BTCUSDT.BINANCE"
        } else {
            "BTCUSDT-PERP.BINANCE"
        };

        provider_args.set_item("load_ids", vec![id]).unwrap();
        let provider = py
            .get_type::<BinanceInstrumentProviderConfig>()
            .call((), Some(&provider_args))
            .unwrap();
        let args = PyDict::new(py);
        args.set_item("account_id", AccountId::from("BINANCE-001"))
            .unwrap();
        args.set_item("product_type", product_type).unwrap();
        args.set_item("api_key", SMOKE_API_KEY).unwrap();
        args.set_item("api_secret", SMOKE_API_SECRET).unwrap();
        args.set_item("use_ws_trading", false).unwrap();
        args.set_item("base_url_http", base_url_http).unwrap();
        args.set_item("base_url_ws", base_url_ws).unwrap();
        args.set_item("base_url_ws_trading", base_url_ws).unwrap();
        args.set_item("instrument_provider", provider).unwrap();
        let config = py
            .get_type::<BinanceExecutionClientConfig>()
            .call((), Some(&args))
            .unwrap()
            .extract::<BinanceExecutionClientConfig>()
            .unwrap();
        let factory = py
            .get_type::<BinanceExecutionClientFactory>()
            .call0()
            .unwrap()
            .extract::<BinanceExecutionClientFactory>()
            .unwrap();
        let clock = Rc::new(RefCell::new(VirtualClock::new()));
        factory
            .create(
                TraderId::from("TESTER-001"),
                BINANCE,
                &config,
                cache.into(),
                clock,
            )
            .unwrap()
    })
}

fn setup_exec_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    replace_exec_event_sender(sender);
}

fn register_binance_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "binance").expect("Binance module should be created");
    python::binance(py, &module).expect("Binance Python module should register");
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, BinanceDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        BinanceDataClientConfig {
            product_type: BinanceProductType::UsdM,
            environment: BinanceEnvironment::Testnet,
            ..BinanceDataClientConfig::default()
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
    let binance_config = extracted_config
        .as_any()
        .downcast_ref::<BinanceDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let client = extracted_factory
        .create(
            "BINANCE-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), BINANCE);
    assert_eq!(extracted_factory.config_type(), "BinanceDataClientConfig");
    assert_eq!(binance_config.product_type, BinanceProductType::UsdM);
    assert_eq!(binance_config.environment, BinanceEnvironment::Testnet);
    assert_eq!(client.client_id(), ClientId::from("BINANCE-DATA-EXTRACTED"));
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("BINANCE-001");
    let factory = Py::new(py, BinanceExecutionClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        BinanceExecutionClientConfig {
            account_id,
            product_type: BinanceProductType::UsdM,
            environment: BinanceEnvironment::Testnet,
            api_key: Some(SMOKE_API_KEY.into()),
            api_secret: Some(SMOKE_API_SECRET.into()),
            use_ws_trading: false,
            ..BinanceExecutionClientConfig::default()
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
    let binance_config = extracted_config
        .as_any()
        .downcast_ref::<BinanceExecutionClientConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create(
            trader_id,
            "BINANCE-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            Rc::new(RefCell::new(VirtualClock::new())),
        )
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), BINANCE);
    assert_eq!(
        extracted_factory.config_type(),
        "BinanceExecutionClientConfig"
    );
    assert_eq!(binance_config.account_id, account_id);
    assert_eq!(binance_config.product_type, BinanceProductType::UsdM);
    assert_eq!(client.client_id(), ClientId::from("BINANCE-EXEC-EXTRACTED"));
    assert_eq!(client.account_id(), account_id);
}
