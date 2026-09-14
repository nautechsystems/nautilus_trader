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

//! HTTP client for the Polymarket Relayer v2 API.

use std::{collections::HashMap, result::Result as StdResult, str::from_utf8};

use alloy_primitives::{Address, U256};
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_network::{
    http::{HttpClient, HttpClientError, HttpRedirectPolicy, HttpResponse, Method, USER_AGENT},
    websocket::proxy::ProxyUrl,
};
use serde::{Deserialize, Serialize};

use crate::{
    common::{credential::RelayerApiKey, urls::relayer_http_url},
    http::error::{Error, Result, decode_response},
    signing::eip712::{DEPOSIT_WALLET_FACTORY, DepositWalletCall},
};

const PATH_NONCE: &str = "/v1/account/transactions/params";
const PATH_SUBMIT: &str = "/submit";
const PATH_TRANSACTION: &str = "/v1/account/transactions";

/// Relayer transaction lifecycle state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayerTransactionState {
    /// Transaction accepted and not yet terminal.
    New,
    /// Transaction mined successfully.
    Confirmed,
    /// Transaction failed on chain or in the relayer.
    Failed,
    /// Transaction was marked invalid.
    Invalid,
    /// A non-terminal or unrecognized relayer state.
    Other(String),
}

impl RelayerTransactionState {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::New => "STATE_NEW",
            Self::Confirmed => "STATE_CONFIRMED",
            Self::Failed => "STATE_FAILED",
            Self::Invalid => "STATE_INVALID",
            Self::Other(value) => value.as_str(),
        }
    }

    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Confirmed | Self::Failed | Self::Invalid)
    }

    fn from_wire(value: &str) -> Self {
        match value {
            "STATE_NEW" => Self::New,
            "STATE_CONFIRMED" => Self::Confirmed,
            "STATE_FAILED" => Self::Failed,
            "STATE_INVALID" => Self::Invalid,
            other => Self::Other(other.to_string()),
        }
    }
}

/// Response from Relayer submit or transaction polling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayerTransaction {
    /// Relayer transaction identifier when the venue supplied one.
    pub transaction_id: Option<String>,
    /// On-chain transaction hash when the venue supplied one.
    pub transaction_hash: Option<String>,
    /// Relayer lifecycle state.
    pub state: RelayerTransactionState,
    /// Relayer error detail when present.
    pub error_msg: Option<String>,
}

/// HTTP client for Relayer nonce, submit, and transaction polling.
#[derive(Debug, Clone)]
pub struct PolymarketRelayerHttpClient {
    client: HttpClient,
    base_url: String,
    credential: RelayerApiKey,
}

impl PolymarketRelayerHttpClient {
    /// Creates a Relayer HTTP client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        credential: RelayerApiKey,
        base_url: Option<String>,
        timeout_secs: u64,
    ) -> StdResult<Self, HttpClientError> {
        Self::new_with_proxy(credential, base_url, timeout_secs, None)
    }

    /// Creates a Relayer HTTP client with an optional proxy.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new_with_proxy(
        credential: RelayerApiKey,
        base_url: Option<String>,
        timeout_secs: u64,
        proxy_url: Option<ProxyUrl>,
    ) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::builder()
                .headers(Self::default_headers())
                .redirect_policy(HttpRedirectPolicy::Reject)
                .timeout_secs(timeout_secs)
                .maybe_proxy_url(proxy_url.map(|url| url.expose().to_string()))
                .build()?,
            base_url: base_url
                .unwrap_or_else(|| relayer_http_url().to_string())
                .trim_end_matches('/')
                .to_string(),
            credential,
        })
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ])
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    fn auth_headers(&self) -> HashMap<String, String> {
        HashMap::from([
            (
                "RELAYER_API_KEY".to_string(),
                self.credential.key().to_string(),
            ),
            (
                "RELAYER_API_KEY_ADDRESS".to_string(),
                self.credential.address().to_string(),
            ),
        ])
    }

    /// Fetches a fresh `WALLET` nonce for `signer`.
    ///
    /// # Errors
    ///
    /// Returns an error if the Relayer rejects the request or the nonce is missing.
    pub async fn get_wallet_nonce(&self, signer: Address) -> Result<U256> {
        let address = format!("{signer:#x}");
        let params = [("address", address.as_str()), ("type", "WALLET")];
        let response: RelayerNonceResponse = self.send_get(PATH_NONCE, Some(&params)).await?;
        parse_u256(&response.nonce, "nonce")
    }

    /// Submits a signed Deposit Wallet batch.
    ///
    /// This method does not retry. A lost or timed-out submit remains an
    /// explicit ambiguous outcome.
    ///
    /// # Errors
    ///
    /// Returns an error if the Relayer rejects the request, the HTTP client
    /// times out, or the response cannot be decoded.
    pub async fn submit_wallet_batch(
        &self,
        request: RelayerWalletSubmit<'_>,
    ) -> Result<RelayerTransaction> {
        if request.calls.is_empty() {
            return Err(Error::bad_request(
                "Deposit Wallet batch must contain at least one call",
            ));
        }

        let body = RelayerSubmitRequest {
            tx_type: "WALLET",
            from: format!("{:#x}", request.signer),
            to: format!("{DEPOSIT_WALLET_FACTORY:#x}"),
            nonce: request.nonce.to_string(),
            signature: request.signature.to_string(),
            metadata: request.metadata.to_string(),
            deposit_wallet_params: DepositWalletParams {
                deposit_wallet: format!("{:#x}", request.deposit_wallet),
                deadline: request.deadline.to_string(),
                calls: request.calls.iter().map(WireCall::from).collect(),
            },
        };

        let body_bytes = serde_json::to_vec(&body)?;
        let headers = Some(self.auth_headers());
        let url = self.url(PATH_SUBMIT);
        let response = self
            .client
            .request(
                Method::POST,
                url,
                None,
                headers,
                Some(body_bytes),
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)?;
        decode_relayer_transaction(&response)
    }

    /// Polls a Relayer transaction by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the Relayer rejects the request or the body cannot
    /// be decoded.
    pub async fn get_transaction(&self, transaction_id: &str) -> Result<RelayerTransaction> {
        if transaction_id.trim().is_empty() {
            return Err(Error::bad_request("transaction_id must not be empty"));
        }

        let path = format!("{PATH_TRANSACTION}/{transaction_id}");
        let url = self.url(&path);
        let response = self
            .client
            .request_with_params(
                Method::GET,
                url,
                None::<&[(&str, &str); 0]>,
                Some(self.auth_headers()),
                None,
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)?;
        let transaction = decode_relayer_transaction(&response)?;
        if transaction.transaction_id.as_deref() != Some(transaction_id) {
            return Err(Error::decode(format!(
                "Relayer response did not match transaction {transaction_id}; transaction outcome is unknown"
            )));
        }

        Ok(transaction)
    }

    async fn send_get<P: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> Result<T> {
        let url = self.url(path);
        let response = self
            .client
            .request_with_params(
                Method::GET,
                url,
                params,
                Some(self.auth_headers()),
                None,
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)?;
        decode_response(&response)
    }
}

/// Signed Deposit Wallet batch submitted to the Relayer.
#[derive(Clone, Debug)]
pub struct RelayerWalletSubmit<'a> {
    /// Signer address authorizing the batch.
    pub signer: Address,
    /// Deposit Wallet executing the batch.
    pub deposit_wallet: Address,
    /// Fresh Relayer wallet nonce.
    pub nonce: U256,
    /// Unix-second signature deadline.
    pub deadline: U256,
    /// EIP-712 batch signature.
    pub signature: &'a str,
    /// Relayer metadata string.
    pub metadata: &'a str,
    /// Ordered contract calls.
    pub calls: &'a [DepositWalletCall],
}

#[derive(Deserialize)]
struct RelayerNonceResponse {
    nonce: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RelayerSubmitRequest {
    #[serde(rename = "type")]
    tx_type: &'static str,
    from: String,
    to: String,
    nonce: String,
    signature: String,
    metadata: String,
    deposit_wallet_params: DepositWalletParams,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DepositWalletParams {
    deposit_wallet: String,
    deadline: String,
    calls: Vec<WireCall>,
}

#[derive(Serialize)]
struct WireCall {
    target: String,
    value: String,
    data: String,
}

impl From<&DepositWalletCall> for WireCall {
    fn from(call: &DepositWalletCall) -> Self {
        Self {
            target: format!("{:#x}", call.target),
            value: call.value.to_string(),
            data: format!("0x{}", alloy_primitives::hex::encode(&call.data)),
        }
    }
}

#[derive(Deserialize)]
struct RelayerTransactionWire {
    #[serde(alias = "transactionID", alias = "transaction_id")]
    transaction_id: Option<String>,
    #[serde(alias = "transactionHash", alias = "transaction_hash")]
    transaction_hash: Option<String>,
    state: String,
    #[serde(alias = "errorMsg", alias = "error_msg")]
    error_msg: Option<String>,
}

impl From<RelayerTransactionWire> for RelayerTransaction {
    fn from(wire: RelayerTransactionWire) -> Self {
        Self {
            transaction_id: empty_to_none(wire.transaction_id),
            transaction_hash: empty_to_none(wire.transaction_hash),
            state: RelayerTransactionState::from_wire(&wire.state),
            error_msg: empty_to_none(wire.error_msg),
        }
    }
}

fn empty_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn parse_u256(value: &str, field: &str) -> Result<U256> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::decode(format!("{field} was empty")));
    }

    if let Some(hex) = trimmed.strip_prefix("0x") {
        return U256::from_str_radix(hex, 16)
            .map_err(|e| Error::decode(format!("Invalid {field}: {e}")));
    }

    trimmed
        .parse()
        .map_err(|e| Error::decode(format!("Invalid {field}: {e}")))
}

fn decode_relayer_transaction(response: &HttpResponse) -> Result<RelayerTransaction> {
    if !response.status.is_success() {
        return Err(Error::from_status_code(
            response.status.as_u16(),
            &response.body,
        ));
    }

    let body = from_utf8(&response.body)
        .map_err(|e| Error::decode(format!("UTF-8 error: {e}")))?
        .trim();

    if body.is_empty() || body == "null" {
        return Err(Error::decode(
            "Relayer submit response was empty; transaction outcome is unknown",
        ));
    }

    if let Ok(wires) = serde_json::from_str::<Vec<RelayerTransactionWire>>(body) {
        let wire = wires.into_iter().next().ok_or_else(|| {
            Error::decode(
                "Relayer transaction response was an empty array; transaction outcome is unknown",
            )
        })?;

        return Ok(wire.into());
    }

    let wire = serde_json::from_str::<RelayerTransactionWire>(body)?;
    Ok(wire.into())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_relayer_transaction_state_terminal() {
        assert!(!RelayerTransactionState::New.is_terminal());
        assert!(RelayerTransactionState::Confirmed.is_terminal());
        assert!(RelayerTransactionState::Failed.is_terminal());
        assert!(RelayerTransactionState::Invalid.is_terminal());
        assert!(!RelayerTransactionState::Other("STATE_EXECUTED".into()).is_terminal());
    }

    #[rstest]
    fn test_decode_submit_response_aliases() {
        let body = serde_json::to_vec(&json!({
            "transactionID": "tx-1",
            "state": "STATE_NEW"
        }))
        .unwrap();
        let wire: RelayerTransactionWire = serde_json::from_slice(&body).unwrap();
        let tx = RelayerTransaction::from(wire);
        assert_eq!(tx.transaction_id.as_deref(), Some("tx-1"));
        assert_eq!(tx.state, RelayerTransactionState::New);
        assert!(tx.transaction_hash.is_none());
    }

    #[rstest]
    fn test_decode_poll_response_snake_case() {
        let body = serde_json::to_vec(&json!({
            "transaction_id": "tx-2",
            "transaction_hash": "0xabc",
            "state": "STATE_CONFIRMED",
            "error_msg": null
        }))
        .unwrap();
        let wire: RelayerTransactionWire = serde_json::from_slice(&body).unwrap();
        let tx = RelayerTransaction::from(wire);
        assert_eq!(tx.transaction_id.as_deref(), Some("tx-2"));
        assert_eq!(tx.transaction_hash.as_deref(), Some("0xabc"));
        assert_eq!(tx.state, RelayerTransactionState::Confirmed);
        assert!(tx.error_msg.is_none());
    }

    #[rstest]
    fn test_parse_u256_decimal_and_hex() {
        assert_eq!(parse_u256("12", "nonce").unwrap(), U256::from(12u64));
        assert_eq!(parse_u256("0x0a", "nonce").unwrap(), U256::from(10u64));
        assert!(parse_u256("", "nonce").is_err());
    }
}
