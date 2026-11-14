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

use std::{cell::RefCell, rc::Rc, sync::atomic::Ordering, time::Duration};

use nautilus_common::{
    cache::Cache,
    clock::{Clock, VirtualClock},
    live::runner::{replace_data_event_sender, replace_exec_event_sender},
    messages::{
        DataEvent, ExecutionEvent,
        data::{DataResponse, RequestInstrument},
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_kalshi::{
    common::{consts::KALSHI, enums::KalshiEnvironment},
    config::{KalshiDataClientConfig, KalshiExecutionClientConfig},
    factories::{KalshiDataClientFactory, KalshiExecutionClientFactory},
    python,
};
use nautilus_model::{
    enums::OmsType,
    identifiers::{AccountId, ClientId, InstrumentId, TraderId},
};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{Py, Python, types::PyModule};
use rstest::rstest;

#[allow(
    dead_code,
    reason = "shared key fixtures serve factory credential setup"
)]
mod keys {
    include!("keys.rs");
}

fn register_kalshi(py: Python<'_>) {
    let module = PyModule::new(py, "kalshi").unwrap();
    python::kalshi(&module).unwrap();
}

#[rstest]
fn test_kalshi_python_factories_extract_and_construct() {
    let (data_sender, _data_receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(data_sender);
    let (exec_sender, _exec_receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    replace_exec_event_sender(exec_sender);
    Python::initialize();

    Python::attach(|py| {
        register_kalshi(py);
        let registry = get_global_pyo3_registry();
        let instrument_id = InstrumentId::from("KXHIGHNY-26OCT31-T70.KALSHI");
        let data_factory = Py::new(py, KalshiDataClientFactory::new())
            .unwrap()
            .into_any();

        let data_config = Py::new(
            py,
            KalshiDataClientConfig {
                environment: KalshiEnvironment::Demo,
                http_timeout_secs: 17,
                instrument_ids: vec![instrument_id],
                ..Default::default()
            },
        )
        .unwrap()
        .into_any();

        let extracted_factory = registry.extract_factory(py, data_factory).unwrap();
        let extracted_config = registry.extract_config(py, data_config).unwrap();
        let config = extracted_config
            .as_any()
            .downcast_ref::<KalshiDataClientConfig>()
            .unwrap();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
        let client = extracted_factory
            .create(
                "KALSHI-DATA-EXTRACTED",
                extracted_config.as_ref(),
                cache.clone().into(),
                Rc::clone(&clock),
            )
            .unwrap();

        assert_eq!(extracted_factory.name(), KALSHI);
        assert_eq!(extracted_factory.config_type(), "KalshiDataClientConfig");
        assert_eq!(config.environment, KalshiEnvironment::Demo);
        assert_eq!(config.http_timeout_secs, 17);
        assert_eq!(config.instrument_ids, vec![instrument_id]);
        assert_eq!(client.client_id(), ClientId::from("KALSHI-DATA-EXTRACTED"));

        let trader_id = TraderId::from("TRADER-001");
        let account_id = AccountId::from("KALSHI-001");
        let exec_factory = Py::new(py, KalshiExecutionClientFactory::new())
            .unwrap()
            .into_any();

        let exec_config = Py::new(
            py,
            KalshiExecutionClientConfig {
                api_key_id: Some("python-factory-key".into()),
                private_key: Some(keys::rsa_pkcs8().into()),
                ..Default::default()
            },
        )
        .unwrap()
        .into_any();

        let extracted_exec = registry.extract_exec_factory(py, exec_factory).unwrap();
        let extracted_exec_config = registry.extract_config(py, exec_config).unwrap();
        let exec_client = extracted_exec
            .create(
                trader_id,
                "KALSHI-EXEC-EXTRACTED",
                extracted_exec_config.as_ref(),
                cache.into(),
                clock,
            )
            .unwrap();

        assert_eq!(extracted_exec.name(), KALSHI);
        assert_eq!(extracted_exec.config_type(), "KalshiExecutionClientConfig");
        assert_eq!(
            exec_client.client_id(),
            ClientId::from("KALSHI-EXEC-EXTRACTED")
        );
        assert_eq!(exec_client.account_id(), account_id);
        assert_eq!(exec_client.oms_type(), OmsType::Netting);
    });
}

#[tokio::test]
async fn python_extracted_data_client_requests_the_configured_peer() {
    tokio::time::timeout(Duration::from_secs(8), async {
        let peer = crate::data_peer::Peer::new().await;
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(sender);
        Python::initialize();
        let instrument_id = InstrumentId::from("KXBTC15M-26OCT162345-45.KALSHI");

        let mut client = Python::attach(|py| {
            register_kalshi(py);
            let registry = get_global_pyo3_registry();
            let factory = Py::new(py, KalshiDataClientFactory::new())
                .unwrap()
                .into_any();

            let config = Py::new(
                py,
                KalshiDataClientConfig {
                    base_url_http: peer.config.base_url_http.clone(),
                    instrument_ids: vec![instrument_id],
                    http_timeout_secs: 2,
                    ..Default::default()
                },
            )
            .unwrap()
            .into_any();

            let extracted_factory = registry.extract_factory(py, factory).unwrap();
            let extracted_config = registry.extract_config(py, config).unwrap();
            let cache = Rc::new(RefCell::new(Cache::default()));
            let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
            extracted_factory
                .create(
                    "KALSHI-PYTHON-PEER",
                    extracted_config.as_ref(),
                    cache.into(),
                    clock,
                )
                .unwrap()
        });

        client
            .request_instrument(RequestInstrument::new(
                instrument_id,
                None,
                None,
                Some(ClientId::from("KALSHI-PYTHON-PEER")),
                UUID4::new(),
                UnixNanos::from(4),
                None,
            ))
            .unwrap();
        let mut response = None;

        while response.is_none() {
            match events.recv().await.unwrap() {
                DataEvent::Response(DataResponse::Instrument(report)) => response = Some(report),
                DataEvent::Data(_) => {}
                other => panic!("unexpected data event {other:?}"),
            }
        }

        let response = response.unwrap();
        assert_eq!(response.instrument_id, instrument_id);
        assert!(
            peer.request_paths
                .lock()
                .unwrap()
                .iter()
                .any(|path| path.contains("KXBTC15M-26OCT162345-45"))
        );
        let requests = peer.requests.load(Ordering::SeqCst);
        client.disconnect().await.unwrap();
        assert_eq!(peer.requests.load(Ordering::SeqCst), requests);
    })
    .await
    .unwrap();
}
