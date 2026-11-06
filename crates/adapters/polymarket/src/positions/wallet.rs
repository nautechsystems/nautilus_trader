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

//! Polygon reads for Deposit Wallet identity and V2 position operations.

use std::{collections::HashMap, fmt::Debug};

use alloy::{sol, sol_types::SolCall};
use alloy_primitives::{Address, B256, Bytes, U256};
use nautilus_core::string::secret::REDACTED;
use nautilus_network::{
    http::{HttpClient, HttpRedirectPolicy, Method, create_standard_nautilus_headers},
    websocket::proxy::ProxyUrl,
};
use serde::Deserialize;
use serde_json::json;

use super::{
    convert::{ConvertIndexSet, MAX_CONVERT_QUESTIONS},
    effects::TransactionReceipt,
    metadata::{NegRiskCondition, NegRiskMarket, question_id},
};
use crate::{
    http::error::{Error, Result},
    signing::eip712::{
        CONDITIONAL_TOKENS, DEPOSIT_WALLET_FACTORY, LEGACY_NEG_RISK_ADAPTER,
        NEG_RISK_CTF_COLLATERAL_ADAPTER, POLYMARKET_COLLATERAL_TOKEN, POSITION_MANAGER, ROUTER,
    },
};

const DEFAULT_RPC_URL: &str = "https://polygon.drpc.org";

sol! {
    function id() external view returns (bytes32);
    function owner() external view returns (address);
    function nonce() external view returns (uint256);
    function predictWalletAddress(bytes32 id) external view returns (address);
    function predictLegacyWalletAddress(bytes32 id) external view returns (address);
    function balanceOf(address account) external view returns (uint256);
    function allowance(address owner, address spender) external view returns (uint256);
    function isApprovedForAll(address owner, address operator) external view returns (bool);
    interface PositionLedger {
        function balanceOf(address owner, uint256 id) external view returns (uint256);
    }
    function getPayout(uint256 positionId, uint256 amount) external view returns (uint256);
    function getQuestionCount(bytes32 marketId) external view returns (uint256);
    function getFeeBips(bytes32 marketId) external view returns (uint256);
    function getConditionId(bytes32 questionId) external view returns (bytes32);
    function getPositionId(bytes32 questionId, bool outcome) external view returns (uint256);
    function payoutNumerators(bytes32 conditionId, uint256 index) external view returns (uint256);
    function payoutDenominator(bytes32 conditionId) external view returns (uint256);
}

pub(super) struct WalletVerifier {
    client: HttpClient,
    rpc_url: String,
}

impl WalletVerifier {
    pub(super) fn new(timeout_secs: u64, proxy_url: Option<ProxyUrl>) -> Result<Self> {
        let mut headers: HashMap<String, String> =
            create_standard_nautilus_headers().into_iter().collect();
        headers.insert("Content-Type".into(), "application/json".into());

        let client = HttpClient::builder()
            .headers(headers)
            .redirect_policy(HttpRedirectPolicy::Reject)
            .redact_url(true)
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

    pub(super) async fn verify_v2_split(&self, owner: Address, amount: U256) -> Result<()> {
        let balance = self
            .read(
                POLYMARKET_COLLATERAL_TOKEN,
                balanceOfCall { account: owner },
            )
            .await?;

        if balance < amount {
            return Err(Error::bad_request(
                "Deposit Wallet has insufficient pUSD for split",
            ));
        }

        let allowance = self
            .read(
                POLYMARKET_COLLATERAL_TOKEN,
                allowanceCall {
                    owner,
                    spender: ROUTER,
                },
            )
            .await?;

        if allowance < amount {
            return Err(Error::bad_request(
                "Deposit Wallet pUSD allowance for Router is insufficient",
            ));
        }

        Ok(())
    }

    pub(super) async fn verify_v2_positions(
        &self,
        owner: Address,
        position_ids: &[U256],
        amount: U256,
    ) -> Result<()> {
        let approved = self
            .read(
                POSITION_MANAGER,
                isApprovedForAllCall {
                    owner,
                    operator: ROUTER,
                },
            )
            .await?;

        if !approved {
            return Err(Error::bad_request(
                "Deposit Wallet must approve Router as a PositionManager operator",
            ));
        }

        for &id in position_ids {
            let balance = self
                .read(
                    POSITION_MANAGER,
                    PositionLedger::balanceOfCall { owner, id },
                )
                .await?;

            if balance < amount {
                return Err(Error::bad_request(format!(
                    "Deposit Wallet has insufficient V2 position balance for {id}"
                )));
            }
        }

        Ok(())
    }

    pub(super) async fn payout(&self, position_id: U256, amount: U256) -> Result<U256> {
        self.read(
            POSITION_MANAGER,
            getPayoutCall {
                positionId: position_id,
                amount,
            },
        )
        .await
    }

    /// Loads and verifies the on-chain metadata for a negative-risk market.
    ///
    /// # Errors
    ///
    /// Returns an error if the market is unknown, its metadata is inconsistent, or a Polygon
    /// RPC read fails.
    pub(super) async fn neg_risk_market(&self, market_id: B256) -> Result<NegRiskMarket> {
        let question_count = self
            .read(
                LEGACY_NEG_RISK_ADAPTER,
                getQuestionCountCall {
                    marketId: market_id,
                },
            )
            .await?;
        let question_count = u16::try_from(question_count).map_err(|_| {
            Error::bad_request(format!(
                "neg-risk market question count is invalid: {question_count}"
            ))
        })?;

        if question_count == 0 || question_count > MAX_CONVERT_QUESTIONS {
            return Err(Error::bad_request(format!(
                "neg-risk market {market_id:#x} has invalid question count {question_count}"
            )));
        }

        let fee_bips = self
            .read(
                LEGACY_NEG_RISK_ADAPTER,
                getFeeBipsCall {
                    marketId: market_id,
                },
            )
            .await?;
        let fee_bips = u16::try_from(fee_bips).map_err(|_| {
            Error::bad_request(format!("neg-risk market fee is invalid: {fee_bips}"))
        })?;

        let mut conditions = Vec::with_capacity(usize::from(question_count));
        for index in 0..question_count {
            let index = u8::try_from(index)
                .map_err(|_| Error::bad_request("neg-risk question index exceeds uint8"))?;
            let question_id = question_id(market_id, index)?;
            let condition_id = self
                .read(
                    LEGACY_NEG_RISK_ADAPTER,
                    getConditionIdCall {
                        questionId: question_id,
                    },
                )
                .await?;
            let yes_token_id = self
                .read(
                    LEGACY_NEG_RISK_ADAPTER,
                    getPositionIdCall {
                        questionId: question_id,
                        outcome: true,
                    },
                )
                .await?;
            let no_token_id = self
                .read(
                    LEGACY_NEG_RISK_ADAPTER,
                    getPositionIdCall {
                        questionId: question_id,
                        outcome: false,
                    },
                )
                .await?;

            conditions.push(NegRiskCondition {
                index,
                question_id,
                condition_id,
                yes_token_id,
                no_token_id,
            });
        }

        NegRiskMarket::new(market_id, question_count, fee_bips, conditions)
    }

    /// Verifies the Deposit Wallet approvals and NO balances required by a conversion.
    ///
    /// # Errors
    ///
    /// Returns an error if the neg-risk adapter is not a Conditional Tokens operator or any
    /// selected NO position is below `amount`.
    pub(super) async fn verify_convert(
        &self,
        owner: Address,
        market: &NegRiskMarket,
        index_set: ConvertIndexSet,
        amount: U256,
    ) -> Result<()> {
        let approved = self
            .read(
                CONDITIONAL_TOKENS,
                isApprovedForAllCall {
                    owner,
                    operator: NEG_RISK_CTF_COLLATERAL_ADAPTER,
                },
            )
            .await?;

        if !approved {
            return Err(Error::bad_request(
                "Deposit Wallet must approve the neg-risk CTF collateral adapter as a Conditional Tokens operator",
            ));
        }

        for index in index_set.indices() {
            let condition = market.condition(index).ok_or_else(|| {
                Error::bad_request(format!("neg-risk market is missing question {index}"))
            })?;
            let balance = self
                .read(
                    CONDITIONAL_TOKENS,
                    PositionLedger::balanceOfCall {
                        owner,
                        id: condition.no_token_id,
                    },
                )
                .await?;

            if balance < amount {
                return Err(Error::bad_request(format!(
                    "Deposit Wallet has insufficient NO balance for neg-risk question {index}"
                )));
            }
        }

        Ok(())
    }

    /// Fetches a confirmed on-chain transaction receipt.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not yet mined, the receipt is malformed, or a
    /// Polygon RPC read fails.
    pub(super) async fn transaction_receipt(
        &self,
        transaction_hash: B256,
    ) -> Result<TransactionReceipt> {
        let result = self
            .rpc_value(
                "eth_getTransactionReceipt",
                json!([format!("{transaction_hash:#x}")]),
            )
            .await?;
        TransactionReceipt::from_rpc(&result)
    }

    /// Fetches the verified CTF payout denominator for a condition.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC read fails.
    pub(super) async fn ctf_payout_denominator(&self, condition_id: B256) -> Result<U256> {
        self.read(
            CONDITIONAL_TOKENS,
            payoutDenominatorCall {
                conditionId: condition_id,
            },
        )
        .await
    }

    /// Fetches the verified CTF payout numerator for one outcome index.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC read fails.
    pub(super) async fn ctf_payout_numerator(
        &self,
        condition_id: B256,
        index: U256,
    ) -> Result<U256> {
        self.read(
            CONDITIONAL_TOKENS,
            payoutNumeratorsCall {
                conditionId: condition_id,
                index,
            },
        )
        .await
    }

    async fn read<C: SolCall>(&self, target: Address, call: C) -> Result<C::Return> {
        let response = self.call(target, call.abi_encode()).await?;
        C::abi_decode_returns_validate(&response)
            .map_err(|_| Error::decode(format!("Invalid Polygon RPC result for {}", C::SIGNATURE)))
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
        let result = self.rpc_value(method, params).await?;
        serde_json::from_value(result)
            .map_err(|_| Error::decode(format!("Polygon RPC {method} returned an invalid result")))
    }

    async fn rpc_value(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let body = serde_json::to_vec(
            &json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}),
        )?;
        let response = self
            .client
            .request(
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
    result: Option<serde_json::Value>,
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
        assert!(!rendered.contains(&addr.to_string()));
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
