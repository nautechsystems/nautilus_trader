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

use std::{sync::Arc, time::Duration};

use futures_util::StreamExt;
use nautilus_common::{config::ConfigError, testing::wait_until_async};
use nautilus_kalshi::{config::KalshiDataClientConfig, websocket::client::connect};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use tokio_tungstenite::{accept_hdr_async, tungstenite::handshake::server};

use super::{
    http::{assert_auth_request, assert_no_connection, bind_peer},
    keys,
};

#[rstest]
#[case::rsa(&keys::rsa_pkcs8(), true)]
#[case::ed25519(&keys::ed25519_pkcs8(), false)]
#[tokio::test]
async fn fresh_clients_sign_and_disable_transparent_reconnect(
    #[case] pem: &str,
    #[case] rsa: bool,
) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let (listener, address) = bind_peer(18187).await;

        let config = KalshiDataClientConfig {
            api_key_id: Some("auth-test-id".into()),
            private_key: Some(pem.into()),
            base_url_ws: Some(format!("ws://{address}/custom/extra/../ws/v3").into()),
            ..Default::default()
        };

        let first_peer = async {
            let (stream, _) = listener.accept().await.unwrap();
            let mut timestamp = None;

            #[allow(
                clippy::result_large_err,
                reason = "Tungstenite requires an unboxed HTTP response error in the callback"
            )]
            let callback = |request: &server::Request, response: server::Response| {
                assert_eq!(request.uri().path(), "/custom/ws/v3");
                let raw = request
                    .headers()
                    .iter()
                    .map(|(name, value)| format!("{name}: {}\r\n", value.to_str().unwrap()))
                    .collect::<String>();
                timestamp = Some(assert_auth_request(&raw, "/custom/ws/v3", rsa));
                Ok(response)
            };

            let mut ws_client = accept_hdr_async(stream, callback).await.unwrap();
            let timestamp = timestamp.unwrap();
            while nautilus_core::time::get_atomic_clock_realtime().get_time_ms() <= timestamp {
                tokio::task::yield_now().await;
            }

            ws_client.close(None).await.unwrap();
            timestamp
        };

        let (first, first_timestamp) = tokio::join!(connect(&config, Arc::new(|_| {})), first_peer);
        let first = first.unwrap();
        assert_no_connection(&listener, address, async {
            wait_until_async(|| async { first.is_disconnected() }, Duration::from_secs(2)).await;
        })
        .await;

        assert!(first.is_closed());

        let second_peer = async {
            let (stream, _) = listener.accept().await.unwrap();
            let mut timestamp = None;

            #[allow(
                clippy::result_large_err,
                reason = "Tungstenite requires an unboxed HTTP response error in the callback"
            )]
            let callback = |request: &server::Request, response: server::Response| {
                assert_eq!(request.uri().path(), "/custom/ws/v3");
                let raw = request
                    .headers()
                    .iter()
                    .map(|(name, value)| format!("{name}: {}\r\n", value.to_str().unwrap()))
                    .collect::<String>();
                timestamp = Some(assert_auth_request(&raw, "/custom/ws/v3", rsa));
                Ok(response)
            };

            let mut ws_client = accept_hdr_async(stream, callback).await.unwrap();
            while ws_client.next().await.is_some() {}
            timestamp.unwrap()
        };

        let second = async {
            let ws_client = connect(&config, Arc::new(|_| {})).await.unwrap();
            ws_client.disconnect().await;
        };

        let ((), second_timestamp) = tokio::join!(second, second_peer);

        assert!(second_timestamp > first_timestamp);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn connection_errors_redact_secret_endpoint_and_proxy() {
    let config = KalshiDataClientConfig {
        api_key_id: Some("secret-id".into()),
        private_key: Some(keys::ed25519_pkcs8().into()),
        base_url_ws: Some("ws://secret-target.invalid/secret-path".into()),
        proxy_url: Some("http://secret-user:secret-password@127.0.0.1:1".into()),
        ..Default::default()
    };

    let error = connect(&config, Arc::new(|_| {})).await.unwrap_err();
    for output in [error.to_string(), format!("{error:?}")] {
        for secret in [
            "secret-id",
            "secret-target",
            "secret-path",
            "secret-user",
            "secret-password",
            "127.0.0.1",
        ] {
            assert!(!output.contains(secret));
        }
    }
}

#[rstest]
#[case(&keys::rsa_pkcs8())]
#[case(&keys::ed25519_pkcs8())]
#[tokio::test]
async fn invalid_credentials_prevent_websocket_traffic(#[case] pem: &str) {
    let (listener, address) = bind_peer(18191).await;

    let config = KalshiDataClientConfig {
        api_key_id: Some("secret-invalid-id".into()),
        private_key: Some(format!("unwanted-prefix{pem}").into()),
        base_url_ws: Some(format!("ws://{address}/ws").into()),
        ws_timeout_secs: 1,
        ..Default::default()
    };

    let error = connect(&config, Arc::new(|_| {})).await.unwrap_err();
    assert_eq!(
        error.downcast_ref::<ConfigError>(),
        Some(&ConfigError::invalid_value(
            "private_key",
            "expected one unencrypted RSA or Ed25519 private key in PEM format",
        ))
    );
    assert!(!format!("{error:?}").contains("secret-invalid-id"));
    assert_no_connection(&listener, address, std::future::ready(())).await;
}

#[tokio::test]
async fn tungstenite_backend_is_rejected_before_network_traffic() {
    let (listener, address) = bind_peer(18192).await;

    let config = KalshiDataClientConfig {
        api_key_id: Some("auth-test-id".into()),
        private_key: Some(keys::ed25519_pkcs8().into()),
        base_url_ws: Some(format!("ws://{address}/ws").into()),
        transport_backend: TransportBackend::Tungstenite,
        ..Default::default()
    };

    let error = connect(&config, Arc::new(|_| {})).await.unwrap_err();
    assert_eq!(
        error.downcast_ref::<ConfigError>(),
        Some(&ConfigError::invalid_value(
            "transport_backend",
            "Kalshi requires the Sockudo WebSocket backend",
        ))
    );
    assert_no_connection(&listener, address, std::future::ready(())).await;
}
