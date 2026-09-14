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

//! Deposit Wallet identity checks against the Polygon factory.

use std::{collections::HashMap, fmt::Debug};

use alloy::{sol, sol_types::SolCall};
use alloy_primitives::{Address, Bytes, U256};
use nautilus_core::string::secret::REDACTED;
use nautilus_network::{
    http::{HttpClient, HttpRedirectPolicy, Method},
    websocket::proxy::ProxyUrl,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    http::error::{Error, Result},
    signing::eip712::DEPOSIT_WALLET_FACTORY,
};

const DEFAULT_RPC_URL: &str = "https://polygon.drpc.org";

sol! {
    function id() external view returns (bytes32);
    function owner() external view returns (address);
    function nonce() external view returns (uint256);
    function predictWalletAddress(bytes32 id) external view returns (address);
    function predictLegacyWalletAddress(bytes32 id) external view returns (address);
}

pub(super) struct WalletVerifier {
    client: HttpClient,
    rpc_url: String,
}

impl WalletVerifier {
    pub(super) fn new(timeout_secs: u64, proxy_url: Option<ProxyUrl>) -> Result<Self> {
        let client = HttpClient::builder()
            .headers(HashMap::from([(
                "Content-Type".into(),
                "application/json".into(),
            )]))
            .redirect_policy(HttpRedirectPolicy::Reject)
            .timeout_secs(timeout_secs)
            .maybe_proxy_url(proxy_url.map(|url| url.expose().to_string()))
            .build()
            .map_err(Error::from_http_client)?;
        Ok(Self {
            client,
            rpc_url: DEFAULT_RPC_URL.into(),
        })
    }

    pub(super) fn set_rpc_url(&mut self, rpc_url: String) {
        self.rpc_url = rpc_url;
    }

    pub(super) async fn verify(&self, signer: Address, wallet: Address) -> Result<U256> {
        let code = self
            .rpc("eth_getCode", json!([format!("{wallet:#x}"), "latest"]))
            .await?;

        if code.is_empty() {
            return Err(Error::bad_request("Deposit Wallet is not deployed"));
        }

        let identity = self
            .call(wallet, idCall {}.abi_encode())
            .await
            .map_err(|e| match e {
                Error::Decode(message) => Error::decode(format!(
                    "Deposit Wallet identity call failed; check wallet type and RPC: {message}"
                )),
                error => error,
            })?;

        let id = idCall::abi_decode_returns_validate(&identity).map_err(|_| {
            Error::bad_request(
                "Funder lacks Deposit Wallet identity; Safe and Proxy wallets are unsupported",
            )
        })?;

        let predicted = self
            .predict(predictWalletAddressCall { id }.abi_encode())
            .await?;

        if predicted != wallet {
            let legacy = self
                .predict(predictLegacyWalletAddressCall { id }.abi_encode())
                .await?;

            if legacy != wallet {
                return Err(Error::bad_request(
                    "Funder is not a canonical Deposit Wallet for this signer; Safe, Proxy, and EOA wallets are unsupported",
                ));
            }
        }

        let ownership = self.call(wallet, ownerCall {}.abi_encode()).await?;
        let owner = ownerCall::abi_decode_returns_validate(&ownership)
            .map_err(|e| Error::decode(format!("Invalid Deposit Wallet owner: {e}")))?;
        if owner != signer {
            return Err(Error::bad_request("Deposit Wallet signer is not its owner"));
        }

        let nonce = self.call(wallet, nonceCall {}.abi_encode()).await?;
        nonceCall::abi_decode_returns_validate(&nonce)
            .map_err(|e| Error::decode(format!("Invalid Deposit Wallet nonce: {e}")))
    }

    async fn predict(&self, data: Vec<u8>) -> Result<Address> {
        let response = self.call(DEPOSIT_WALLET_FACTORY, data).await?;
        predictWalletAddressCall::abi_decode_returns_validate(&response)
            .map_err(|e| Error::decode(format!("Invalid Deposit Wallet prediction: {e}")))
    }

    async fn call(&self, target: Address, data: Vec<u8>) -> Result<Bytes> {
        self.rpc("eth_call", json!([
            {"to": format!("{target:#x}"), "data": format!("0x{}", alloy_primitives::hex::encode(data))},
            "latest"
        ])).await
    }

    async fn rpc(&self, method: &str, params: serde_json::Value) -> Result<Bytes> {
        let body = serde_json::to_vec(
            &json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}),
        )?;
        let response = self
            .client
            .request_with_url_redacted(
                Method::POST,
                self.rpc_url.clone(),
                None,
                None,
                Some(body),
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)?;
        if !response.status.is_success() {
            return Err(Error::from_status_code(
                response.status.as_u16(),
                b"Polygon RPC request failed",
            ));
        }
        let response: RpcResponse = serde_json::from_slice(&response.body)
            .map_err(|_| Error::decode("Polygon RPC returned an invalid response"))?;
        if response.jsonrpc != "2.0" || response.id != 1 {
            return Err(Error::decode(
                "Polygon RPC did not return a successful matching response",
            ));
        }

        if let Some(error) = response.error {
            let message = match error.get("code").and_then(serde_json::Value::as_i64) {
                Some(code) => format!("Polygon RPC {method} failed (code {code})"),
                None => format!("Polygon RPC {method} failed"),
            };

            return Err(Error::decode(message));
        }

        response
            .result
            .ok_or_else(|| Error::decode("Polygon RPC omitted its result"))
    }
}

impl Debug for WalletVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(WalletVerifier))
            .field("rpc_url", &REDACTED)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct RpcResponse {
    jsonrpc: String,
    id: u64,
    result: Option<Bytes>,
    error: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use axum::{Router, http::StatusCode, routing::post};
    use rstest::rstest;

    use super::*;
    use crate::{
        common::credential::{EvmPrivateKey, RelayerApiKey},
        http::relayer::PolymarketRelayerHttpClient,
        signing::eip712::OrderSigner,
    };

    #[rstest]
    fn test_rpc_url_is_redacted_from_debug() {
        let mut verifier = WalletVerifier::new(2, None).unwrap();
        verifier.set_rpc_url("https://rpc.example/dummy-path-secret?key=dummy-query-secret".into());
        assert_eq!(
            format!("{verifier:?}"),
            "WalletVerifier { rpc_url: \"<redacted>\", .. }"
        );
    }

    #[tokio::test]
    async fn test_rpc_url_is_redacted_from_transport_errors() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            drop(stream);
        });

        let mut verifier = WalletVerifier::new(2, None).unwrap();
        verifier.set_rpc_url(format!(
            "http://{addr}/dummy-path-secret?key=dummy-query-secret"
        ));
        let error = verifier.rpc("eth_getCode", json!([])).await.unwrap_err();
        server.await.unwrap();
        assert!(matches!(error, Error::Transport(_)));
        let rendered = format!("{error}; {error:?}");
        assert!(!rendered.contains("dummy-path-secret"));
        assert!(!rendered.contains("dummy-query-secret"));
    }

    #[rstest]
    #[case(400, false)]
    #[case(429, false)]
    #[case(500, false)]
    #[case(200, true)]
    #[tokio::test]
    async fn test_rpc_response_errors_do_not_expose_provider_text(
        #[case] status: u16,
        #[case] malformed: bool,
    ) {
        let secret = "https://rpc.example/dummy-path-secret?key=dummy-query-secret";
        let body = if malformed {
            json!({"jsonrpc": "2.0", "id": secret, "result": "0x"}).to_string()
        } else {
            secret.to_string()
        };
        let app = Router::new().route(
            "/",
            post(move || async move { (StatusCode::from_u16(status).unwrap(), body) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut verifier = WalletVerifier::new(2, None).unwrap();
        verifier.set_rpc_url(format!("http://{addr}/"));

        let error = verifier.rpc("eth_getCode", json!([])).await.unwrap_err();
        server.abort();
        let rendered = format!("{error}; {error:?}");

        assert!(!rendered.contains("dummy-path-secret"));
        assert!(!rendered.contains("dummy-query-secret"));
        if malformed {
            assert!(
                matches!(error, Error::Decode(ref message) if message == "Polygon RPC returned an invalid response")
            );
        } else {
            match &error {
                Error::Http {
                    status: actual,
                    message,
                } => {
                    assert_eq!(*actual, status);
                    assert_eq!(message, "Polygon RPC request failed");
                }
                Error::RateLimit { message, .. } => {
                    assert_eq!(status, 429);
                    assert_eq!(message, "Polygon RPC request failed");
                }
                other => panic!("Unexpected error: {other:?}"),
            }
            assert_eq!(error.is_retryable(), status == 429 || status == 500);
        }
    }

    #[tokio::test]
    #[ignore = "requires Polymarket credentials and network access to Polygon and the Relayer"]
    async fn test_live_deposit_wallet_security_reads() {
        let private_key = EvmPrivateKey::new(&std::env::var("POLYMARKET_PK").unwrap()).unwrap();
        let signer = OrderSigner::new(&private_key).unwrap();
        let wallet = std::env::var("POLYMARKET_FUNDER").unwrap().parse().unwrap();
        WalletVerifier::new(20, None)
            .unwrap()
            .verify(signer.address(), wallet)
            .await
            .unwrap();
        PolymarketRelayerHttpClient::new(RelayerApiKey::from_env().unwrap(), None, 20)
            .unwrap()
            .get_wallet_nonce(signer.address())
            .await
            .unwrap();
    }
}
