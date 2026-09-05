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
use nautilus_core::UnixNanos;
use nautilus_execution::{
    models::fee::FeeModel,
    python::fee::{PyFeeModel, pyobject_to_fee_model_handle},
};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType},
    identifiers::{AccountId, ClientId, InstrumentId, TraderId},
    instruments::{Instrument, InstrumentAny},
    orders::{builder::OrderTestBuilder, stubs::TestOrderStubs},
    types::{Price, Quantity},
};
use nautilus_polymarket::{
    common::consts::POLYMARKET,
    config::{
        PolymarketDataClientConfig, PolymarketExecutionClientConfig,
        PolymarketInstrumentProviderConfig,
    },
    data_types::PolymarketRtdsCryptoTwap,
    factories::{PolymarketDataClientFactory, PolymarketExecutionClientFactory},
    http::{
        models::GammaMarket,
        parse::{create_instrument_from_def, parse_gamma_market},
    },
    python,
};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{
    Py, Python,
    types::{PyAnyMethods, PyModule, PyString},
};
use rstest::rstest;
use rust_decimal_macros::dec;

const SMOKE_PRIVATE_KEY: &str =
    "0x59c6995e998f97a5a0044966f094538a1da6d1310dce3f687da73cf015b05d7e";
const SMOKE_API_KEY: &str = "test_key";
const SMOKE_API_SECRET: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const SMOKE_PASSPHRASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[rstest]
fn test_polymarket_python_factories_extract_from_registry() {
    setup_data_event_sender();
    setup_exec_event_sender();
    Python::initialize();

    Python::attach(|py| {
        register_polymarket_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
        assert_exec_factory_extracts_from_python_object(py);
    });
}

#[rstest]
fn test_polymarket_python_module_registers_data_loader() {
    Python::initialize();

    Python::attach(|py| {
        let module = PyModule::new(py, "polymarket").expect("Polymarket module should be created");
        python::polymarket(py, &module).expect("Polymarket Python module should register");
        let loader = module
            .getattr("PolymarketDataLoader")
            .expect("PolymarketDataLoader should be registered");

        assert_eq!(
            loader
                .getattr("__name__")
                .expect("loader name")
                .extract::<String>()
                .expect("string loader name"),
            "PolymarketDataLoader",
        );
        assert!(loader.getattr("from_market_slug").is_ok());
        assert!(loader.getattr("query_events").is_ok());
    });
}

#[rstest]
fn test_polymarket_crypto_twap_python_value_is_exact_decimal_string() {
    Python::initialize();

    Python::attach(|py| {
        let twap = Py::new(
            py,
            PolymarketRtdsCryptoTwap::new(
                "alpha/usd".to_string(),
                60,
                dec!(123.456789012345678901),
                1_772_752_581_815,
                1_772_752_582_004,
                UnixNanos::from_millis(1_772_752_581_815),
                UnixNanos::from_millis(1_772_752_582_005),
            ),
        )
        .expect("PolymarketRtdsCryptoTwap should construct");
        let value = twap
            .bind(py)
            .getattr("value")
            .expect("TWAP value should be exposed");

        assert!(value.is_instance_of::<PyString>());
        assert_eq!(
            value.extract::<String>().expect("exact decimal string"),
            "123.456789012345678901"
        );
    });
}

#[rstest]
fn test_polymarket_python_fee_model_uses_runtime_handle() {
    Python::initialize();

    Python::attach(|py| {
        let module = PyModule::new(py, "polymarket").expect("Polymarket module should be created");
        python::polymarket(py, &module).expect("Polymarket Python module should register");
        let model = module
            .getattr("PolymarketFeeModel")
            .expect("PolymarketFeeModel should be registered")
            .call0()
            .expect("PolymarketFeeModel should construct");
        let handle = pyobject_to_fee_model_handle(&model).expect("fee model should extract");
        let instrument = fee_instrument();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .price(Price::from("0.50"))
            .quantity(Quantity::from("100"))
            .build();
        let order = TestOrderStubs::make_filled_order(&order, &instrument, LiquiditySide::Maker);

        let commission = handle
            .get_commission(
                &order,
                Quantity::from("100"),
                Price::from("0.50"),
                &instrument,
            )
            .unwrap();

        assert!(model.is_instance_of::<PyFeeModel>());
        assert_eq!(commission.as_decimal(), dec!(-0.18750));
        assert_eq!(commission.currency, instrument.quote_currency());
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

fn register_polymarket_python_module(py: Python<'_>) {
    let module = PyModule::new(py, "polymarket").expect("Polymarket module should be created");
    python::polymarket(py, &module).expect("Polymarket Python module should register");
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, PolymarketDataClientFactory)
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(py, PolymarketDataClientConfig::default())
        .expect("config should convert to Python object")
        .into_any();
    let registry = get_global_pyo3_registry();

    let extracted_factory = registry
        .extract_factory(py, factory)
        .expect("data factory should extract");
    let extracted_config = registry
        .extract_config(py, config)
        .expect("data config should extract");
    let polymarket_config = extracted_config
        .as_any()
        .downcast_ref::<PolymarketDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "POLYMARKET-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), POLYMARKET);
    assert_eq!(
        extracted_factory.config_type(),
        "PolymarketDataClientConfig"
    );
    assert!(polymarket_config.auto_load_missing_instruments);
    assert_eq!(
        client.client_id(),
        ClientId::from("POLYMARKET-DATA-EXTRACTED")
    );
}

fn assert_exec_factory_extracts_from_python_object(py: Python<'_>) {
    let trader_id = TraderId::from("TRADER-001");
    let account_id = AccountId::from("POLYMARKET-001");
    let scoped = InstrumentId::from("0xabc-123.POLYMARKET");
    let factory = Py::new(py, PolymarketExecutionClientFactory)
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        PolymarketExecutionClientConfig {
            account_id,
            private_key: Some(SMOKE_PRIVATE_KEY.into()),
            api_key: Some(SMOKE_API_KEY.into()),
            api_secret: Some(SMOKE_API_SECRET.into()),
            passphrase: Some(SMOKE_PASSPHRASE.into()),
            heartbeat_enabled: true,
            instrument_config: Some(PolymarketInstrumentProviderConfig {
                load_ids: Some(vec![scoped]),
                ..Default::default()
            }),
            ..PolymarketExecutionClientConfig::default()
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
    let polymarket_config = extracted_config
        .as_any()
        .downcast_ref::<PolymarketExecutionClientConfig>()
        .expect("exec config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = extracted_factory
        .create(
            trader_id,
            "POLYMARKET-EXEC-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
        )
        .expect("extracted factory should create exec client");

    assert_eq!(extracted_factory.name(), POLYMARKET);
    assert_eq!(
        extracted_factory.config_type(),
        "PolymarketExecutionClientConfig"
    );
    assert_eq!(polymarket_config.account_id, account_id);
    assert!(polymarket_config.heartbeat_enabled);
    assert_eq!(
        polymarket_config.reconciliation_load_ids(),
        Some([scoped].as_slice())
    );
    assert_eq!(
        client.client_id(),
        ClientId::from("POLYMARKET-EXEC-EXTRACTED")
    );
    assert_eq!(client.account_id(), account_id);
}

fn fee_instrument() -> InstrumentAny {
    let market: GammaMarket = serde_json::from_str(include_str!(
        "../../test_data/gamma_market_sports_market_money_line.json"
    ))
    .unwrap();
    let def = parse_gamma_market(&market).unwrap().remove(0);
    create_instrument_from_def(&def, UnixNanos::default()).unwrap()
}
