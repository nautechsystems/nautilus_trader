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

use std::{future::Future, net::SocketAddr, time::Duration};

use aws_lc_rs::signature::{ED25519, RSA_PSS_2048_8192_SHA256, UnparsedPublicKey};
use base64::prelude::*;
use nautilus_common::config::ConfigError;
use nautilus_kalshi::{
    config::KalshiDataClientConfig,
    http::{client::KalshiHttpClient, error::KalshiHttpError},
};
use nautilus_network::{
    dst::{
        net::{TcpListener, TcpStream},
        task, time,
    },
    http::HttpClientError,
};
use rstest::rstest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) use super::http_request::read_headers;
use super::keys;

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn http_status_uses_configured_url_and_preserves_response() {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = bind_peer(18180).await;
        let peer = task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_headers(&mut stream).await;
            let body = include_str!("../../test_data/exchange_status.json");
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
            request
        });

        let config = KalshiDataClientConfig {
            api_key_id: Some("explicit-fake-key".into()),
            private_key: Some("explicit-fake-private-key".into()),
            base_url_http: Some(format!("http://{address}/custom/v2/").into()),
            http_timeout_secs: 2,
            ..Default::default()
        };

        let client = KalshiHttpClient::new(&config).unwrap();
        let response = client.get_exchange_status().await.unwrap();
        let request = peer.await.unwrap();
        assert!(request.starts_with("GET /custom/v2/exchange/status HTTP/1.1\r\n"));
        assert!(
            request
                .to_lowercase()
                .contains(&format!("host: {address}\r\n"))
        );
        assert!(!request.contains("explicit-fake"));
        assert!(!request.to_lowercase().contains("kalshi-access"));
        assert_eq!(response.status.as_u16(), 200);
        assert_eq!(
            response.body.as_ref(),
            include_bytes!("../../test_data/exchange_status.json")
        );
        assert!(!format!("{client:?}").contains("/custom/v2"));
    }))
    .await
    .unwrap();
}

#[cfg(not(all(feature = "simulation", madsim)))]
#[tokio::test]
async fn http_status_uses_explicit_proxy() {
    time::timeout(Duration::from_secs(5), async {
        let (listener, address) = bind_peer(18181).await;
        let peer = task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_headers(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();
            request
        });

        let config = KalshiDataClientConfig {
            base_url_http: Some("http://kalshi-peer.invalid/custom/v2".into()),
            proxy_url: Some(format!("http://proxy-user:proxy-password@{address}").into()),
            http_timeout_secs: 2,
            ..Default::default()
        };

        let client = KalshiHttpClient::new(&config).unwrap();
        let response = client.get_exchange_status().await.unwrap();
        let request = peer.await.unwrap();
        assert!(
            request.starts_with(
                "GET http://kalshi-peer.invalid/custom/v2/exchange/status HTTP/1.1\r\n"
            )
        );

        let authorization = request.lines().find_map(|line| {
            let (name, value) = line.split_once(": ")?;
            name.eq_ignore_ascii_case("proxy-authorization")
                .then_some(value)
        });

        let expected = format!(
            "Basic {}",
            BASE64_STANDARD.encode("proxy-user:proxy-password")
        );
        assert_eq!(authorization, Some(expected.as_str()));
        assert_eq!(response.status.as_u16(), 204);
        let formatted = format!("{client:?}");
        assert!(!formatted.contains("proxy-user"));
        assert!(!formatted.contains("proxy-password"));
        assert!(!formatted.contains("kalshi-peer.invalid"));
        assert!(!formatted.contains(&address.to_string()));
    })
    .await
    .unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn http_status_rejects_redirects() {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = bind_peer(18182).await;
        let peer = task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_headers(&mut stream).await;
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://redirect.invalid/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            stream.flush().await.unwrap();
            request
        });

        let config = KalshiDataClientConfig { base_url_http: Some(format!("http://{address}/v2").into()), ..Default::default() };
        let client = KalshiHttpClient::new(&config).unwrap();
        let response = client.get_exchange_status().await.unwrap();
        peer.await.unwrap();
        assert_eq!(response.status.as_u16(), 302);
    })).await.unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn http_status_uses_configured_timeout() {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = bind_peer(18183).await;

        let peer = task::spawn(async move {
            let mut streams = Vec::new();

            for _ in 0..3 {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_headers(&mut stream).await;
                streams.push(stream);
            }

            std::future::pending::<()>().await;
            drop(streams);
        });

        let config = KalshiDataClientConfig {
            base_url_http: Some(format!("http://{address}/v2").into()),
            http_timeout_secs: 1,
            ..Default::default()
        };

        let client = KalshiHttpClient::new(&config).unwrap();
        let error = client.get_exchange_status().await.unwrap_err();
        peer.abort();
        assert!(matches!(error, HttpClientError::TimeoutError(_)));
    }))
    .await
    .unwrap();
}

#[rstest]
fn constructor_rejects_invalid_config() {
    let config = KalshiDataClientConfig {
        http_timeout_secs: 0,
        ..Default::default()
    };

    let error = KalshiHttpClient::new(&config).unwrap_err();
    assert_eq!(
        error.downcast_ref::<ConfigError>(),
        Some(&ConfigError::invalid_value(
            "http_timeout_secs",
            "must be positive"
        ))
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn http_status_transport_error_redacts_endpoint() {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = bind_peer(18184).await;
        let peer = task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_headers(&mut stream).await;
        });

        let config = KalshiDataClientConfig {
            base_url_http: Some(format!("http://{address}/secret-path").into()),
            ..Default::default()
        };

        let client = KalshiHttpClient::new(&config).unwrap();
        let error = client.get_exchange_status().await.unwrap_err();
        peer.await.unwrap();

        for output in [error.to_string(), format!("{error:?}")] {
            assert!(!output.contains("secret-path"));
            assert!(!output.contains(&address.to_string()));
        }
    }))
    .await
    .unwrap();
}

#[rstest]
#[case::rsa(&keys::rsa_pkcs8(), true)]
#[case::ed25519(&keys::ed25519_pkcs8(), false)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn authenticated_read_refreshes_headers_on_retry(#[case] pem: &str, #[case] rsa: bool) {
    time::timeout(Duration::from_secs(10), async {
        let (listener, address) = bind_peer(18185).await;
        let peer = task::spawn(async move {
            super::rest::quota_discovery(&listener).await;
            let mut timestamps = Vec::new();

            for status in [503, 200] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_headers(&mut stream).await;
                assert!(
                    request.starts_with(
                        "GET /custom/v3/portfolio/balance?exchange_index=3 HTTP/1.1\r\n"
                    )
                );
                timestamps.push(assert_auth_request(
                    &request,
                    "/custom/v3/portfolio/balance",
                    rsa,
                ));

                if status == 503 {
                    let started = std::time::Instant::now();

                    while nautilus_core::time::get_atomic_clock_realtime().get_time_ms()
                        <= timestamps[0]
                    {
                        assert!(started.elapsed() < Duration::from_secs(1));
                        std::thread::yield_now();
                    }
                }

                let response = format!(
                    "HTTP/1.1 {status} Response\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
            }

            timestamps
        });

        let config = KalshiDataClientConfig {
            api_key_id: Some("auth-test-id".into()),
            private_key: Some(pem.into()),
            base_url_http: Some(format!("http://{address}/custom/prefix/../v3").into()),
            ..Default::default()
        };

        let client = KalshiHttpClient::new_authenticated(&config).unwrap();
        let response = Box::pin(client.get_balance(3)).await.unwrap();
        let timestamps = peer.await.unwrap();
        assert_eq!(response.status.as_u16(), 200);
        assert!(timestamps[1] > timestamps[0]);
        assert!(!format!("{client:?}").contains("auth-test-id"));
    })
    .await
    .unwrap();
}

#[rstest]
#[case(302)]
#[case(401)]
#[case(429)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn authenticated_read_does_not_retry_permanent_responses(#[case] status: u16) {
    time::timeout(Duration::from_secs(5), async {
        let (listener, address) = bind_peer(18186).await;
        let (completed, completion) = tokio::sync::oneshot::channel();

        let peer = task::spawn(async move {
            super::rest::quota_discovery(&listener).await;
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_headers(&mut stream).await;
            assert_auth_request(&request,"/trade-api/v2/portfolio/balance",false);
            let response = format!("HTTP/1.1 {status} Response\r\nLocation: http://secret-redirect.invalid/path\r\nContent-Length: 16\r\nConnection: close\r\n\r\nsensitive-secret");
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
            assert_no_connection(&listener, address, async { completion.await.unwrap(); }).await;
        });

        let config = KalshiDataClientConfig {
            api_key_id: Some("auth-test-id".into()),
            private_key: Some(keys::ed25519_pkcs8().into()),
            base_url_http: Some(format!("http://{address}/trade-api/v2").into()), ..Default::default()
        };

        let client = KalshiHttpClient::new_authenticated(&config).unwrap();
        let error = Box::pin(client.get_balance(0)).await.unwrap_err();
        completed.send(()).unwrap();
        peer.await.unwrap();
        assert!(matches!(error, KalshiHttpError::Status(actual) if actual == status));
        assert_eq!(error.to_string(),format!("HTTP {status}"));
        assert!(!format!("{error:?}").contains("secret"));
    }).await.unwrap();
}

#[rstest]
fn authenticated_constructor_rejects_malformed_credentials() {
    let config = KalshiDataClientConfig {
        api_key_id: Some("secret-key-id".into()),
        private_key: Some("secret-malformed-pem".into()),
        base_url_http: Some("http://secret-host.invalid/secret-path".into()),
        ..Default::default()
    };

    let error = KalshiHttpClient::new_authenticated(&config).unwrap_err();
    assert!(!format!("{error:?}").contains("secret"));
    assert!(error.to_string().starts_with("invalid private_key:"));
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn authenticated_read_stops_after_two_retries() {
    time::timeout(Duration::from_secs(10), async {
        let (listener,address) = bind_peer(18190).await;
        let peer = task::spawn(async move {
            super::rest::quota_discovery(&listener).await;
            for _ in 0..3 {
                let (mut stream,_) = listener.accept().await.unwrap();
                let request = read_headers(&mut stream).await;
                assert_auth_request(&request,"/v2/portfolio/balance",false);
                stream.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                stream.flush().await.unwrap();
            }
        });

        let config = KalshiDataClientConfig {
            api_key_id:Some("auth-test-id".into()),private_key:Some(keys::ed25519_pkcs8().into()),
            base_url_http:Some(format!("http://{address}/v2").into()),..Default::default()
        };

        let client = KalshiHttpClient::new_authenticated(&config).unwrap();
        let error = Box::pin(client.get_balance(0)).await.unwrap_err();
        peer.await.unwrap();
        assert!(matches!(error,KalshiHttpError::Status(503)), "unexpected failure: {error:?}");
        assert!(error.is_retryable());
    }).await.unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn public_client_rejects_authenticated_read_as_authentication_error() {
    let client = KalshiHttpClient::new(&KalshiDataClientConfig::default()).unwrap();
    let error = client.get_balance(0).await.unwrap_err();
    assert!(matches!(error, KalshiHttpError::Authentication(ref source)
        if source == &ConfigError::invalid_value("credentials", "authenticated client required")));
}

pub(super) fn assert_auth_request(request: &str, path: &str, rsa: bool) -> u64 {
    let header = |name: &str| {
        request
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(": ")?;
                key.eq_ignore_ascii_case(name).then_some(value)
            })
            .unwrap()
    };

    assert_eq!(header("KALSHI-ACCESS-KEY"), "auth-test-id");
    let timestamp = header("KALSHI-ACCESS-TIMESTAMP").parse::<u64>().unwrap();
    assert!(timestamp >= 1_000_000_000_000);
    assert!(timestamp <= nautilus_core::time::get_atomic_clock_realtime().get_time_ms());
    let signature = BASE64_STANDARD
        .decode(header("KALSHI-ACCESS-SIGNATURE"))
        .unwrap();
    let payload = format!("{timestamp}GET{path}");

    if rsa {
        UnparsedPublicKey::new(
            &RSA_PSS_2048_8192_SHA256,
            include_bytes!("../../test_data/auth_rsa_public.der"),
        )
        .verify(payload.as_bytes(), &signature)
        .unwrap();
    } else {
        let public = include_bytes!("../../test_data/auth_ed25519_public.der");
        UnparsedPublicKey::new(&ED25519, &public[public.len() - 32..])
            .verify(payload.as_bytes(), &signature)
            .unwrap();
    }

    timestamp
}

pub(super) async fn bind_peer(port: u16) -> (TcpListener, SocketAddr) {
    #[cfg(all(feature = "simulation", madsim))]
    let address = {
        // Fresh venues must not inherit process-wide quota owners from earlier simulations
        static NEXT_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(30_000);
        let _ = port;
        let port = NEXT_PORT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        SocketAddr::from(([127, 0, 0, 1], port))
    };

    #[cfg(not(all(feature = "simulation", madsim)))]
    let address = {
        let _ = port;
        SocketAddr::from(([127, 0, 0, 1], 0))
    };

    let listener = TcpListener::bind(address).await.unwrap();
    #[cfg(not(all(feature = "simulation", madsim)))]
    let address = listener.local_addr().unwrap();
    (listener, address)
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
#[should_panic(expected = "unexpected connection")]
async fn no_connection_check_rejects_queued_traffic() {
    let (listener, address) = bind_peer(18230).await;
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /unexpected HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    stream.flush().await.unwrap();

    assert_no_connection(&listener, address, std::future::ready(())).await;
}

pub(super) async fn assert_no_connection(
    listener: &TcpListener,
    address: SocketAddr,
    completed: impl Future<Output = ()>,
) {
    tokio::select! {
        biased;
        connection = listener.accept() => panic!("unexpected connection: {connection:?}"),
        () = completed => {},
    }

    // A known connection fences requests still waiting for reactor readiness
    time::timeout(Duration::from_secs(2), async {
        let marker = 0;

        let probe = async {
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream.write_all(&[marker]).await.unwrap();
            stream.flush().await.unwrap();
        };

        let accept = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert_eq!(
                stream.read_u8().await.unwrap(),
                marker,
                "unexpected connection before completion probe"
            );
        };

        tokio::join!(probe, accept);
    })
    .await
    .unwrap();
}
