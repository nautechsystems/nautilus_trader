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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::VirtualClock,
    factories::{ClientConfig, ExecutionClientFactory},
    live::runner::{replace_exec_event_sender, try_get_exec_event_sender},
    messages::ExecutionEvent,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{AccountId, ClientId, TraderId},
    types::{AccountBalance, Money},
};
use nautilus_okx::{
    common::{consts::OKX, enums::OKXInstrumentType},
    config::{OKXDataClientConfig, OKXExecutionClientConfig},
    factories::OKXExecutionClientFactory,
};
use rstest::rstest;

#[rstest]
fn test_okx_execution_client_factory_creation() {
    let factory = OKXExecutionClientFactory::new();
    assert_eq!(factory.name(), OKX);
    assert_eq!(factory.config_type(), "OKXExecutionClientConfig");
}

#[rstest]
fn test_okx_execution_client_factory_default() {
    let factory = OKXExecutionClientFactory::new();
    assert_eq!(factory.name(), OKX);
}

#[rstest]
#[case(false)]
#[case(true)]
fn test_okx_execution_client_factory_sender(#[case] sender_bound: bool) {
    std::thread::spawn(move || {
        assert!(try_get_exec_event_sender().is_none());

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        if sender_bound {
            replace_exec_event_sender(sender);
        }

        let account_id = AccountId::from("OKX-FACTORY-002");

        let config = OKXExecutionClientConfig {
            account_id,
            instrument_types: vec![OKXInstrumentType::Spot],
            api_key: Some("test_key".into()),
            api_secret: Some("test_secret".into()),
            api_passphrase: Some("test_pass".into()),
            ..Default::default()
        };

        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(VirtualClock::new()));
        let mut client = OKXExecutionClientFactory::new()
            .create(
                TraderId::from("TESTER-002"),
                "OKX",
                &config,
                cache.into(),
                clock,
            )
            .unwrap();
        let balances = vec![AccountBalance::new(
            Money::from("1234.50 USD"),
            Money::from("234.25 USD"),
            Money::from("1000.25 USD"),
        )];
        let ts_event = UnixNanos::from(123_456);

        client
            .generate_account_state(balances.clone(), vec![], true, ts_event, None)
            .unwrap();

        if sender_bound {
            let ExecutionEvent::Account(state) = receiver.try_recv().unwrap() else {
                panic!("Expected account state");
            };

            assert_eq!(state.account_id, account_id);
            assert_eq!(state.account_type, AccountType::Cash);
            assert_eq!(state.base_currency, None);
            assert_eq!(state.balances, balances);
            assert_eq!(state.margins, vec![]);
            assert!(state.is_reported);
            assert_eq!(state.ts_event, ts_event);
            assert_eq!(state.info, None);
        }

        assert_eq!(
            receiver.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );

        let (sender_current, mut receiver_current) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender_current);
        client.start().unwrap();
        let ts_event = UnixNanos::from(987_654);
        client
            .generate_account_state(balances.clone(), vec![], false, ts_event, None)
            .unwrap();

        let ExecutionEvent::Account(state) = receiver_current.try_recv().unwrap() else {
            panic!("Expected account state");
        };

        assert_eq!(state.account_id, account_id);
        assert_eq!(state.account_type, AccountType::Cash);
        assert_eq!(state.base_currency, None);
        assert_eq!(state.balances, balances);
        assert_eq!(state.margins, vec![]);
        assert!(!state.is_reported);
        assert_eq!(state.ts_event, ts_event);
        assert_eq!(state.info, None);
        assert_eq!(
            receiver.try_recv().unwrap_err(),
            if sender_bound {
                tokio::sync::mpsc::error::TryRecvError::Disconnected
            } else {
                tokio::sync::mpsc::error::TryRecvError::Empty
            }
        );
        assert_eq!(
            receiver_current.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
    })
    .join()
    .unwrap();
}

#[rstest]
fn test_okx_exec_client_config_implements_client_config() {
    let config = OKXExecutionClientConfig {
        account_id: AccountId::from("OKX-001"),
        instrument_types: vec![OKXInstrumentType::Spot],
        ..Default::default()
    };

    let boxed_config: Box<dyn ClientConfig> = Box::new(config);
    let downcasted = boxed_config
        .as_any()
        .downcast_ref::<OKXExecutionClientConfig>();

    assert!(downcasted.is_some());
}

#[rstest]
fn test_okx_execution_client_factory_creates_client_for_spot() {
    let factory = OKXExecutionClientFactory::new();

    let config = OKXExecutionClientConfig {
        account_id: AccountId::from("OKX-001"),
        instrument_types: vec![OKXInstrumentType::Spot],
        api_key: Some("test_key".into()),
        api_secret: Some("test_secret".into()),
        api_passphrase: Some("test_pass".into()),
        ..Default::default()
    };

    let cache = Rc::new(RefCell::new(Cache::default()));

    let result = factory.create(
        TraderId::from("TRADER-001"),
        "OKX-TEST",
        &config,
        cache.into(),
        Rc::new(RefCell::new(VirtualClock::new())),
    );
    assert!(result.is_ok());

    let client = result.unwrap();
    assert_eq!(client.client_id(), ClientId::from("OKX-TEST"));
}

#[rstest]
fn test_okx_execution_client_factory_creates_client_for_derivatives() {
    let factory = OKXExecutionClientFactory::new();

    let config = OKXExecutionClientConfig {
        account_id: AccountId::from("OKX-001"),
        instrument_types: vec![OKXInstrumentType::Swap, OKXInstrumentType::Futures],
        api_key: Some("test_key".into()),
        api_secret: Some("test_secret".into()),
        api_passphrase: Some("test_pass".into()),
        ..Default::default()
    };

    let cache = Rc::new(RefCell::new(Cache::default()));

    let result = factory.create(
        TraderId::from("TRADER-001"),
        "OKX-DERIV",
        &config,
        cache.into(),
        Rc::new(RefCell::new(VirtualClock::new())),
    );
    result.unwrap();
}

#[rstest]
fn test_okx_execution_client_factory_rejects_wrong_config_type() {
    let factory = OKXExecutionClientFactory::new();
    let wrong_config = OKXDataClientConfig::default();

    let cache = Rc::new(RefCell::new(Cache::default()));

    let result = factory.create(
        TraderId::from("TRADER-001"),
        "OKX-TEST",
        &wrong_config,
        cache.into(),
        Rc::new(RefCell::new(VirtualClock::new())),
    );
    assert!(result.is_err());
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("Invalid config type")
    );
}

#[rstest]
#[case(OKXInstrumentType::Swap, None, OmsType::Netting)]
#[case(OKXInstrumentType::Futures, Some(OmsType::Hedging), OmsType::Hedging)]
#[case(OKXInstrumentType::Spot, None, OmsType::Hedging)]
fn test_execution_oms_type(
    #[case] instrument_type: OKXInstrumentType,
    #[case] oms_type: Option<OmsType>,
    #[case] expected: OmsType,
) {
    let config = OKXExecutionClientConfig {
        instrument_types: vec![instrument_type],
        oms_type,
        api_key: Some("key".into()),
        api_secret: Some("secret".into()),
        api_passphrase: Some("pass".into()),
        ..Default::default()
    };

    let cache = Rc::new(RefCell::new(Cache::default()));
    let client = OKXExecutionClientFactory::new()
        .create(
            TraderId::from("TRADER-001"),
            "OKX-TEST",
            &config,
            cache.into(),
            Rc::new(RefCell::new(VirtualClock::new())),
        )
        .unwrap();
    assert_eq!(client.oms_type(), expected);
}

#[rstest]
#[case::spot(OKXInstrumentType::Spot, false, AccountType::Cash)]
#[case::margin(OKXInstrumentType::Margin, false, AccountType::Margin)]
#[case::spot_margin(OKXInstrumentType::Spot, true, AccountType::Margin)]
fn test_factory_configures_account_type(
    #[case] instrument_type: OKXInstrumentType,
    #[case] use_spot_margin: bool,
    #[case] expected: AccountType,
) {
    std::thread::spawn(move || {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);

        let config = OKXExecutionClientConfig {
            instrument_types: vec![instrument_type],
            use_spot_margin,
            api_key: Some("factory-key".into()),
            api_secret: Some("factory-secret".into()),
            api_passphrase: Some("factory-passphrase".into()),
            ..Default::default()
        };

        let client = OKXExecutionClientFactory::new()
            .create(
                TraderId::from("FACTORY-001"),
                "OKX",
                &config,
                Rc::new(RefCell::new(Cache::default())).into(),
                Rc::new(RefCell::new(VirtualClock::new())),
            )
            .unwrap();
        let balances = vec![AccountBalance::new(
            Money::from("71 USDT"),
            Money::from("13 USDT"),
            Money::from("58 USDT"),
        )];
        client
            .generate_account_state(balances.clone(), vec![], true, UnixNanos::from(29), None)
            .unwrap();

        let ExecutionEvent::Account(state) = receiver.try_recv().unwrap() else {
            panic!("Expected account state")
        };

        assert_eq!(state.account_id, config.account_id);
        assert_eq!(state.account_type, expected);
        assert_eq!(state.balances, balances);
        assert_eq!(state.margins, vec![]);
        assert_eq!(state.base_currency, None);
        assert_eq!(state.ts_event, UnixNanos::from(29));
        assert_eq!(state.info, None);
        assert!(state.is_reported);
    })
    .join()
    .unwrap();
}
