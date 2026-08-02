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

//! Factory functions for creating Polymarket clients and components.

use std::{any::Any, cell::RefCell, rc::Rc, sync::Arc};

use nautilus_common::{
    cache::CacheView,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::ClientId,
};
use nautilus_network::retry::RetryConfig;
#[cfg(test)]
use nautilus_network::websocket::proxy::ProxyUrl;

use crate::{
    common::consts::{POLYMARKET, POLYMARKET_VENUE},
    config::{PolymarketDataClientConfig, PolymarketExecClientConfig},
    data::PolymarketDataClient,
    execution::PolymarketExecutionClient,
    http::{
        clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
        gamma::PolymarketGammaHttpClient,
    },
    websocket::pool::PolymarketMarketConnectionPool,
};

impl ClientConfig for PolymarketDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Polymarket data clients.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.polymarket",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")
)]
#[derive(Debug, Clone)]
pub struct PolymarketDataClientFactory;

impl DataClientFactory for PolymarketDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let polymarket_config = config
            .as_any()
            .downcast_ref::<PolymarketDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for PolymarketDataClientFactory. Expected PolymarketDataClientConfig, was {config:?}",
                )
            })?;

        Ok(Box::new(Self::create_client(name, polymarket_config)?))
    }

    fn name(&self) -> &'static str {
        POLYMARKET
    }

    fn config_type(&self) -> &'static str {
        "PolymarketDataClientConfig"
    }
}

impl PolymarketDataClientFactory {
    fn create_client(
        name: &str,
        polymarket_config: &PolymarketDataClientConfig,
    ) -> anyhow::Result<PolymarketDataClient> {
        let client_id = ClientId::from(name);
        let proxy_url = polymarket_config.validated_proxy_url()?;

        let gamma_client = PolymarketGammaHttpClient::new_with_proxy(
            Some(polymarket_config.gamma_url()),
            polymarket_config.http_timeout_secs,
            RetryConfig {
                max_retries: 10,
                initial_delay_ms: 5_000,
                max_delay_ms: 30_000,
                backoff_factor: 1.5,
                jitter_ms: 2_000,
                operation_timeout_ms: Some(30_000),
                immediate_first: true,
                max_elapsed_ms: Some(300_000),
            },
            proxy_url.clone(),
        )?;

        let clob_public_client = PolymarketClobPublicClient::new_with_proxy(
            polymarket_config.base_url_http.clone(),
            polymarket_config.http_timeout_secs,
            proxy_url.clone(),
        )?;

        let data_api_client = PolymarketDataApiHttpClient::new_with_proxy(
            Some(polymarket_config.data_api_url()),
            polymarket_config.http_timeout_secs,
            proxy_url.clone(),
        )?;

        let ws_client = PolymarketMarketConnectionPool::new_with_proxy(
            polymarket_config.base_url_ws.clone(),
            polymarket_config.subscribe_new_markets,
            polymarket_config.transport_backend,
            polymarket_config.ws_max_subscriptions,
            proxy_url.clone(),
        );

        let mut client = PolymarketDataClient::new_with_proxy(
            client_id,
            polymarket_config.clone(),
            gamma_client,
            clob_public_client,
            data_api_client,
            ws_client,
            proxy_url,
        );

        for filter in &polymarket_config.filters {
            client.add_instrument_filter(Arc::clone(filter));
        }

        Ok(client)
    }
}

impl ClientConfig for PolymarketExecClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Polymarket execution clients.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.polymarket",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")
)]
#[derive(Debug, Clone)]
pub struct PolymarketExecutionClientFactory;

impl ExecutionClientFactory for PolymarketExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let polymarket_config = config
            .as_any()
            .downcast_ref::<PolymarketExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for PolymarketExecutionClientFactory. Expected PolymarketExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        let oms_type = OmsType::Netting;
        let account_type = AccountType::Cash;

        let client_id = ClientId::from(name);
        let core = ExecutionClientCore::new(
            polymarket_config.trader_id,
            client_id,
            *POLYMARKET_VENUE,
            oms_type,
            polymarket_config.account_id,
            account_type,
            None, // base_currency
            cache,
        );

        let client = PolymarketExecutionClient::new(core, polymarket_config)?;

        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        POLYMARKET
    }

    fn config_type(&self) -> &'static str {
        "PolymarketExecClientConfig"
    }
}

#[cfg(test)]
pub(crate) async fn spawn_rejecting_proxy(
    connection_count: usize,
) -> (
    std::net::SocketAddr,
    std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let captured = std::sync::Arc::clone(&requests);

    tokio::spawn(async move {
        for _ in 0..connection_count {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                let read = stream.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            captured
                .lock()
                .await
                .push(String::from_utf8(request).unwrap());
            stream
                .write_all(
                    b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n",
                )
                .await
                .unwrap();
        }
    });

    (addr, requests)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
        live::runner::replace_data_event_sender,
        messages::DataEvent,
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        common::credential::Credential,
        config::{PolymarketDataClientConfig, PolymarketExecClientConfig},
        http::clob::PolymarketClobHttpClient,
    };

    #[derive(Debug)]
    struct WrongConfig;

    impl ClientConfig for WrongConfig {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[rstest]
    fn test_polymarket_data_client_factory_creation() {
        let factory = PolymarketDataClientFactory;
        assert_eq!(factory.name(), POLYMARKET);
        assert_eq!(factory.config_type(), "PolymarketDataClientConfig");
    }

    #[rstest]
    fn test_polymarket_data_client_config_implements_client_config() {
        let config = PolymarketDataClientConfig::default();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<PolymarketDataClientConfig>();
        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_polymarket_data_client_factory_rejects_wrong_config_type() {
        let factory = PolymarketDataClientFactory;
        let wrong_config = WrongConfig;
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));

        let result = factory.create(POLYMARKET, &wrong_config, cache.into(), clock);
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
    fn test_polymarket_execution_client_factory_creation() {
        let factory = PolymarketExecutionClientFactory;
        assert_eq!(factory.name(), POLYMARKET);
        assert_eq!(factory.config_type(), "PolymarketExecClientConfig");
    }

    #[rstest]
    fn test_polymarket_exec_client_config_implements_client_config() {
        let config = PolymarketExecClientConfig::default();
        let boxed_config: Box<dyn ClientConfig> = Box::new(config);
        let downcasted = boxed_config
            .as_any()
            .downcast_ref::<PolymarketExecClientConfig>();
        assert!(downcasted.is_some());
    }

    #[rstest]
    fn test_polymarket_execution_client_factory_rejects_wrong_config_type() {
        let factory = PolymarketExecutionClientFactory;
        let wrong_config = WrongConfig;
        let cache = Rc::new(RefCell::new(Cache::default()));

        let result = factory.create(POLYMARKET, &wrong_config, cache.into());
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
    fn data_factory_invalid_proxy_error_redacts_credentials() {
        const SECRET: &str = "data-factory-proxy-secret";
        let factory = PolymarketDataClientFactory;
        let config = PolymarketDataClientConfig {
            proxy_url: Some(format!("http://proxy-user:{SECRET}@[::1")),
            ..PolymarketDataClientConfig::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let Err(e) = factory.create(POLYMARKET, &config, cache.into(), clock) else {
            panic!("malformed proxy URL should fail");
        };

        assert!(!e.to_string().contains(SECRET));
    }

    #[rstest]
    fn execution_factory_invalid_proxy_error_redacts_credentials() {
        const SECRET: &str = "execution-factory-proxy-secret";
        let factory = PolymarketExecutionClientFactory;
        let config = PolymarketExecClientConfig {
            proxy_url: Some(format!("http://proxy-user:{SECRET}@[::1")),
            ..PolymarketExecClientConfig::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));
        let Err(e) = factory.create(POLYMARKET, &config, cache.into()) else {
            panic!("malformed proxy URL should fail");
        };

        assert!(!e.to_string().contains(SECRET));
    }

    #[rstest]
    #[tokio::test]
    async fn data_factory_propagates_configured_proxy() {
        const USERNAME: &str = "proxytest";
        const SECRET: &str = "http-client-proxy-secret";
        let (proxy_addr, requests) = spawn_rejecting_proxy(3).await;
        let proxy_url = format!("http://{USERNAME}:{SECRET}@{proxy_addr}");
        let (data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(data_tx);
        let config = PolymarketDataClientConfig {
            base_url_http: Some("https://clob-public.fixture".to_string()),
            base_url_ws: Some("wss://market.fixture/ws".to_string()),
            base_url_gamma: Some("https://gamma.fixture".to_string()),
            base_url_data_api: Some("https://data.fixture".to_string()),
            base_url_rtds: Some("wss://rtds.fixture/ws".to_string()),
            proxy_url: Some(proxy_url.clone()),
            http_timeout_secs: 2,
            ..PolymarketDataClientConfig::default()
        };
        let client = PolymarketDataClientFactory::create_client(POLYMARKET, &config).unwrap();

        let errors = [
            client
                .provider()
                .http_client()
                .request_tags()
                .await
                .unwrap_err()
                .to_string(),
            client
                .clob_public_client()
                .get_book("public-token")
                .await
                .unwrap_err()
                .to_string(),
            client
                .data_api_client()
                .get_positions("0x0000000000000000000000000000000000000002")
                .await
                .unwrap_err()
                .to_string(),
        ];
        let configured_proxy = client.config().validated_proxy_url().unwrap().unwrap();

        assert_eq!(client.ws_client().proxy_url().unwrap().expose(), proxy_url);
        assert_eq!(client.rtds_feed().proxy_url().unwrap().expose(), proxy_url);
        assert_eq!(configured_proxy.expose(), proxy_url);

        let requests = requests.lock().await;
        let request_lines = requests
            .iter()
            .map(|request| request.lines().next().unwrap().to_string())
            .collect::<Vec<_>>();
        let expected_auth = format!("Basic {}", BASE64.encode(format!("{USERNAME}:{SECRET}")));

        assert_eq!(
            request_lines,
            [
                "CONNECT gamma.fixture:443 HTTP/1.1",
                "CONNECT clob-public.fixture:443 HTTP/1.1",
                "CONNECT data.fixture:443 HTTP/1.1",
            ]
        );

        for request in requests.iter() {
            let auth = request
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("proxy-authorization")
                        .then_some(value.trim())
                })
                .expect("Proxy-Authorization header");
            assert_eq!(auth, expected_auth);
        }

        for error in errors {
            assert!(!error.contains(SECRET));
            assert!(!error.contains(&BASE64.encode(SECRET)));
            assert!(!error.contains(&expected_auth));
        }
    }

    #[rstest]
    #[tokio::test]
    async fn authenticated_clob_http_client_uses_configured_proxy() {
        const USERNAME: &str = "proxytest";
        const SECRET: &str = "authenticated-clob-proxy-secret";
        let (proxy_addr, requests) = spawn_rejecting_proxy(1).await;
        let proxy_url =
            ProxyUrl::parse(format!("http://{USERNAME}:{SECRET}@{proxy_addr}")).unwrap();
        let credential = Credential::new(
            "fixture-key",
            "Zml4dHVyZQ==",
            "fixture-passphrase".to_string(),
        )
        .unwrap();
        let clob_auth = PolymarketClobHttpClient::new_with_proxy(
            credential,
            "0x0000000000000000000000000000000000000001".to_string(),
            Some("https://clob-auth.fixture".to_string()),
            2,
            Some(proxy_url),
        )
        .unwrap();

        let error = clob_auth
            .get_book("auth-token")
            .await
            .unwrap_err()
            .to_string();
        let requests = requests.lock().await;
        let request = requests.first().expect("captured CONNECT request");
        let request_line = request.lines().next().unwrap();
        let expected_auth = format!("Basic {}", BASE64.encode(format!("{USERNAME}:{SECRET}")));
        let auth = request
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("proxy-authorization")
                    .then_some(value.trim())
            })
            .expect("Proxy-Authorization header");

        assert_eq!(request_line, "CONNECT clob-auth.fixture:443 HTTP/1.1");
        assert_eq!(auth, expected_auth);
        assert!(!error.contains(SECRET));
        assert!(!error.contains(&BASE64.encode(SECRET)));
        assert!(!error.contains(&expected_auth));
    }
}
