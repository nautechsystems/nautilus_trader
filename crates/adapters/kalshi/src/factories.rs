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

//! Factories for Kalshi data and execution clients.

use std::{any::Any, cell::RefCell, rc::Rc};

use anyhow::Context;
use nautilus_common::{
    cache::CacheView,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    live::runner::try_get_data_event_sender,
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{ClientId, TraderId},
    types::Currency,
};

use crate::{
    common::consts::{KALSHI, KALSHI_VENUE},
    config::{KalshiDataClientConfig, KalshiExecutionClientConfig},
    data::KalshiDataClient,
    execution::KalshiExecutionClient,
};

impl ClientConfig for KalshiDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClientConfig for KalshiExecutionClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Kalshi data clients.
///
/// Streaming is limited to `instrument_ids` on the data config. Construction requires the
/// calling thread's data-event sender because the client cannot install one later.
#[derive(Debug, Clone, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader._libnautilus.kalshi", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.kalshi")
)]
pub struct KalshiDataClientFactory;

impl KalshiDataClientFactory {
    /// Creates a new [`KalshiDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl DataClientFactory for KalshiDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let config = config
            .as_any()
            .downcast_ref::<KalshiDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for KalshiDataClientFactory. Expected KalshiDataClientConfig, was {config:?}"
                )
            })?;

        Ok(Box::new(Self::build(name, config, cache)?))
    }

    fn name(&self) -> &'static str {
        KALSHI
    }

    fn config_type(&self) -> &'static str {
        "KalshiDataClientConfig"
    }
}

impl KalshiDataClientFactory {
    fn build(
        name: &str,
        config: &KalshiDataClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<KalshiDataClient> {
        config.validate()?;
        anyhow::ensure!(
            !config.instrument_ids.is_empty(),
            "KalshiDataClientFactory requires at least one instrument_id"
        );

        let sender = try_get_data_event_sender().context("data event sender is not installed")?;
        KalshiDataClient::with_runtime(ClientId::from(name), config.clone(), sender, cache)
    }
}

/// Factory for creating Kalshi execution clients.
///
/// The node supplies `trader_id`. The client always uses a USD cash account and netting OMS.
/// A bound execution sender is installed during construction; `start` installs the sender again.
#[derive(Debug, Clone, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader._libnautilus.kalshi", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.kalshi")
)]
pub struct KalshiExecutionClientFactory;

impl KalshiExecutionClientFactory {
    /// Creates a new [`KalshiExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ExecutionClientFactory for KalshiExecutionClientFactory {
    fn create(
        &self,
        trader_id: TraderId,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let config = config
            .as_any()
            .downcast_ref::<KalshiExecutionClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for KalshiExecutionClientFactory. Expected KalshiExecutionClientConfig, was {config:?}"
                )
            })?;

        Ok(Box::new(Self::build(
            trader_id, name, config, cache, clock,
        )?))
    }

    fn name(&self) -> &'static str {
        KALSHI
    }

    fn config_type(&self) -> &'static str {
        "KalshiExecutionClientConfig"
    }
}

impl KalshiExecutionClientFactory {
    fn build(
        trader_id: TraderId,
        name: &str,
        config: &KalshiExecutionClientConfig,
        cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<KalshiExecutionClient> {
        config.validate()?;

        let core = ExecutionClientCore::new(
            trader_id,
            ClientId::from(name),
            *KALSHI_VENUE,
            OmsType::Netting,
            config.account_id,
            AccountType::Cash,
            Some(Currency::USD()),
            cache,
        );

        KalshiExecutionClient::new(core, config.clone())
    }
}

#[cfg(test)]
#[cfg(not(all(feature = "simulation", madsim)))] // Factory fixtures require native proxies and threads
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clock::VirtualClock,
        enums::Environment,
        live::runner::{replace_data_event_sender, replace_exec_event_sender},
        messages::{DataEvent, ExecutionEvent},
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_live::node::LiveNode;
    use nautilus_model::{
        accounts::AccountAny,
        enums::AccountType,
        events::AccountState,
        identifiers::{AccountId, ClientId, InstrumentId, Symbol, TraderId},
        instruments::{BinaryOption, CurrencyPair, Instrument, InstrumentAny},
        types::{AccountBalance, Currency, Money, Price, Quantity},
    };
    use nautilus_network::websocket::TransportBackend;
    use rstest::rstest;

    use super::*;
    use crate::{
        common::enums::KalshiEnvironment,
        http::{
            models::{Event, Market},
            parse::parse_instrument,
        },
    };

    #[allow(
        dead_code,
        reason = "shared key fixtures serve factory credential setup"
    )]
    mod keys {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/integration/keys.rs"
        ));
    }

    fn data_config() -> KalshiDataClientConfig {
        KalshiDataClientConfig {
            api_key_id: Some("factory-data-key".into()),
            private_key: Some("factory-data-private".into()),
            environment: KalshiEnvironment::Demo,
            base_url_http: Some("http://data-host/path".into()),
            base_url_ws: Some("ws://data-ws/path".into()),
            proxy_url: Some("http://user:data-proxy@localhost:12000".into()),
            http_timeout_secs: 17,
            ws_timeout_secs: 19,
            book_snapshot_timeout_secs: 31,
            update_instruments_interval_mins: 23,
            transport_backend: TransportBackend::Sockudo,
            instrument_ids: vec![InstrumentId::from("KXHIGHNY-26OCT31-T70.KALSHI")],
        }
    }

    fn exec_config() -> KalshiExecutionClientConfig {
        KalshiExecutionClientConfig {
            account_id: AccountId::from("KALSHI-FACTORY"),
            api_key_id: Some("factory-exec-key".into()),
            private_key: Some(keys::rsa_pkcs8().into()),
            environment: KalshiEnvironment::Demo,
            base_url_http: Some("http://exec-host/path".into()),
            base_url_ws: Some("ws://exec-ws/path".into()),
            proxy_url: Some("http://user:exec-proxy@localhost:12001".into()),
            http_timeout_secs: 41,
            ws_timeout_secs: 43,
            exchange_index: 3,
            transport_backend: TransportBackend::Sockudo,
        }
    }

    fn live_binary_option() -> BinaryOption {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../test_data/instruments.json")).unwrap();
        let market: Market =
            serde_json::from_value(fixtures["definitions"]["live"]["market"].clone()).unwrap();
        let event: Event =
            serde_json::from_value(fixtures["definitions"]["live"]["event"].clone()).unwrap();
        parse_instrument(&market, &event, UnixNanos::from(1)).unwrap()
    }

    fn foreign_instrument(id: InstrumentId) -> InstrumentAny {
        let pair = CurrencyPair::builder()
            .instrument_id(id)
            .raw_symbol(Symbol::new(id.symbol.as_str()))
            .base_currency(Currency::from("BTC"))
            .quote_currency(Currency::from("USDT"))
            .price_precision(2)
            .size_precision(0)
            .price_increment(Price::from("0.01"))
            .size_increment(Quantity::from("1"))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap();
        InstrumentAny::CurrencyPair(pair)
    }

    fn bind_data_sender() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(sender);
    }

    fn bind_exec_sender() -> tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(sender);
        receiver
    }

    #[rstest]
    fn test_factories_name_their_configs() {
        let data_factory = KalshiDataClientFactory::new();
        let exec_factory = KalshiExecutionClientFactory::new();
        assert_eq!(data_factory.name(), KALSHI);
        assert_eq!(data_factory.config_type(), "KalshiDataClientConfig");
        assert_eq!(exec_factory.name(), KALSHI);
        assert_eq!(exec_factory.config_type(), "KalshiExecutionClientConfig");
    }

    #[rstest]
    fn test_configs_downcast_through_client_config() {
        let data = data_config();
        let execution = exec_config();
        let data_boxed: Box<dyn ClientConfig> = Box::new(data.clone());
        let exec_boxed: Box<dyn ClientConfig> = Box::new(execution.clone());

        assert_eq!(
            data_boxed
                .as_any()
                .downcast_ref::<KalshiDataClientConfig>()
                .unwrap(),
            &data
        );
        assert_eq!(
            exec_boxed
                .as_any()
                .downcast_ref::<KalshiExecutionClientConfig>()
                .unwrap(),
            &execution
        );
        assert!(
            data_boxed
                .as_any()
                .downcast_ref::<KalshiExecutionClientConfig>()
                .is_none()
        );
    }

    #[rstest]
    fn test_data_factory_propagates_fields_and_cache() {
        std::thread::spawn(|| {
            bind_data_sender();
            let config = data_config();
            let cache = Rc::new(RefCell::new(Cache::default()));
            let marker = ClientId::from("CACHE-MARKER");
            cache.borrow_mut().add_client_route(marker, *KALSHI_VENUE);
            let client =
                KalshiDataClientFactory::build("KALSHI-DATA", &config, cache.into()).unwrap();

            assert_eq!(client.client_id(), ClientId::from("KALSHI-DATA"));
            assert_eq!(client.venue(), Some(*KALSHI_VENUE));
            assert_eq!(client.config(), &config);
            assert_eq!(client.instruments(), config.instrument_ids.as_slice());
            assert_eq!(
                client.cache().borrow().client_id_for_venue(&KALSHI_VENUE),
                Some(&marker)
            );
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_data_factory_rejects_wrong_config_empty_selection_and_missing_sender() {
        std::thread::spawn(|| {
            let cache = Rc::new(RefCell::new(Cache::default()));
            let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
            let wrong = KalshiDataClientFactory.create(
                "KALSHI",
                &exec_config(),
                cache.clone().into(),
                Rc::clone(&clock),
            );

            let Err(e) = wrong else {
                panic!("wrong config type should be rejected");
            };

            let message = e.to_string();
            assert!(message.contains("KalshiDataClientFactory"), "{message}");
            assert!(message.contains("KalshiDataClientConfig"), "{message}");

            bind_data_sender();
            let empty = KalshiDataClientConfig::default();
            let message = KalshiDataClientFactory::build("KALSHI", &empty, cache.into())
                .unwrap_err()
                .to_string();
            assert!(message.contains("instrument_id"), "{message}");

            let unbound = std::thread::spawn(|| {
                let cache = Rc::new(RefCell::new(Cache::default()));
                KalshiDataClientFactory::build("KALSHI", &data_config(), cache.into())
                    .unwrap_err()
                    .to_string()
            })
            .join()
            .unwrap();

            assert!(unbound.contains("data event sender"), "{unbound}");
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_data_factory_uses_cached_instrument_type() {
        std::thread::spawn(|| {
            bind_data_sender();
            let instrument = live_binary_option();
            let id = instrument.id();
            let mut config = data_config();
            config.instrument_ids = vec![id];
            let accepted = Rc::new(RefCell::new(Cache::default()));
            accepted
                .borrow_mut()
                .add_instrument(InstrumentAny::BinaryOption(instrument))
                .unwrap();

            let client =
                KalshiDataClientFactory::build("KALSHI", &config, accepted.into()).unwrap();

            assert_eq!(client.instruments(), &[id]);

            let rejected = Rc::new(RefCell::new(Cache::default()));
            rejected
                .borrow_mut()
                .add_instrument(foreign_instrument(id))
                .unwrap();
            let message = KalshiDataClientFactory::build("KALSHI", &config, rejected.into())
                .unwrap_err()
                .to_string();
            assert!(message.contains("not a Kalshi binary option"), "{message}");
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_execution_factory_propagates_fields_and_initializes_emitter() {
        std::thread::spawn(|| {
            let mut output = bind_exec_sender();
            let trader_id = TraderId::from("TRADER-009");
            let config = exec_config();
            let cache = Rc::new(RefCell::new(Cache::default()));
            let zero = Money::zero(Currency::USD());

            let event = AccountState::new(
                config.account_id,
                AccountType::Cash,
                vec![AccountBalance::new(zero, zero, zero)],
                Vec::new(),
                true,
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
                Some(Currency::USD()),
            );

            cache
                .borrow_mut()
                .add_account(AccountAny::from(event))
                .unwrap();
            let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));

            let client = KalshiExecutionClientFactory::build(
                trader_id,
                "KALSHI-EXEC",
                &config,
                cache.into(),
                Rc::clone(&clock),
            )
            .unwrap();

            assert_eq!(client.client_id(), ClientId::from("KALSHI-EXEC"));
            assert_eq!(client.account_id(), config.account_id);
            assert_eq!(client.venue(), *KALSHI_VENUE);
            assert_eq!(client.oms_type(), OmsType::Netting);
            assert_eq!(client.config(), &config);
            assert_eq!(client.core().trader_id, trader_id);
            assert_eq!(client.core().account_type, AccountType::Cash);
            assert_eq!(client.core().oms_type, OmsType::Netting);
            assert_eq!(client.core().base_currency, Some(Currency::USD()));
            assert_eq!(client.get_account().unwrap().id(), config.account_id);

            let balances = vec![AccountBalance::new(
                Money::from("10 USD"),
                Money::from("1 USD"),
                Money::from("9 USD"),
            )];
            client
                .generate_account_state(balances.clone(), vec![], true, UnixNanos::from(9), None)
                .unwrap();

            let ExecutionEvent::Account(state) = output.try_recv().unwrap() else {
                panic!("expected account state");
            };

            assert_eq!(state.account_id, config.account_id);
            assert_eq!(state.account_type, AccountType::Cash);
            assert_eq!(state.base_currency, Some(Currency::USD()));
            assert_eq!(state.balances, balances);
            assert!(state.is_reported);
            assert_eq!(state.ts_event, UnixNanos::from(9));
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_execution_factory_rejects_wrong_config_and_defers_missing_sender() {
        std::thread::spawn(|| {
            let cache = Rc::new(RefCell::new(Cache::default()));
            let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
            let wrong = KalshiExecutionClientFactory.create(
                TraderId::from("TRADER-001"),
                "KALSHI",
                &data_config(),
                cache.clone().into(),
                Rc::clone(&clock),
            );

            let Err(e) = wrong else {
                panic!("wrong config type should be rejected");
            };

            let message = e.to_string();
            assert!(
                message.contains("KalshiExecutionClientFactory"),
                "{message}"
            );
            assert!(message.contains("KalshiExecutionClientConfig"), "{message}");

            let mut client = KalshiExecutionClientFactory::build(
                TraderId::from("TRADER-001"),
                "KALSHI",
                &exec_config(),
                cache.into(),
                clock,
            )
            .unwrap();
            let message = client.start().unwrap_err().to_string();
            assert!(message.contains("execution sender"), "{message}");
            bind_exec_sender();
            client.start().unwrap();
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_execution_start_installs_rebound_sender() {
        std::thread::spawn(|| {
            let mut first = bind_exec_sender();
            let cache = Rc::new(RefCell::new(Cache::default()));
            let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
            let mut client = KalshiExecutionClientFactory::build(
                TraderId::from("TRADER-001"),
                "KALSHI",
                &exec_config(),
                cache.into(),
                clock,
            )
            .unwrap();
            let mut second = bind_exec_sender();
            client.start().unwrap();
            let balances = vec![AccountBalance::new(
                Money::from("10 USD"),
                Money::from("1 USD"),
                Money::from("9 USD"),
            )];
            client
                .generate_account_state(balances, vec![], true, UnixNanos::from(9), None)
                .unwrap();

            assert!(first.try_recv().is_err());

            let ExecutionEvent::Account(state) = second.try_recv().unwrap() else {
                panic!("expected account state on the rebound sender");
            };

            assert_eq!(state.account_id, exec_config().account_id);
            assert_eq!(state.ts_event, UnixNanos::from(9));
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_live_node_constructs_kalshi_clients() {
        std::thread::spawn(|| {
            let trader_id = TraderId::from("TRADER-001");
            let node = LiveNode::builder(trader_id, Environment::Sandbox)
                .unwrap()
                .with_name("KALSHI-FACTORY-NODE".to_string())
                .add_data_client(
                    None,
                    Box::new(KalshiDataClientFactory::new()),
                    Box::new(data_config()),
                )
                .unwrap()
                .add_exec_client(
                    None,
                    Box::new(KalshiExecutionClientFactory::new()),
                    Box::new(exec_config()),
                )
                .unwrap()
                .build()
                .unwrap();

            assert_eq!(node.trader_id(), trader_id);
        })
        .join()
        .unwrap();
    }
}
