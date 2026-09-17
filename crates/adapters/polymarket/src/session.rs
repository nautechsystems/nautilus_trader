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

//! Owner-operated administration of Deposit Wallet session keys.

use std::time::Duration;

use alloy::sol_types::SolCall;
use alloy_primitives::{Address, U256};
use nautilus_core::{string::secret::SecretString, time::get_atomic_clock_realtime};
use nautilus_network::websocket::proxy::ProxyUrl;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    common::credential::{Credential, EvmPrivateKey},
    http::{
        clob::PolymarketClobHttpClient,
        error::{Error, Result},
        relayer::{PolymarketRelayerHttpClient, RelayerTransactionState},
    },
    signing::eip712::{DepositWalletCall, OrderSigner},
};

// The official SDK uses 4,315 hours, despite the documentation's rounded 180 days.
const SESSION_LIFETIME_SECS: u64 = 4_315 * 60 * 60;
const BATCH_DEADLINE_SECS: u64 = 600;
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(200);

/// Explicit owner and Builder credentials for session administration.
///
/// Keep this configuration outside the trading runtime. Credentials do not fall back to the environment.
#[derive(Debug, Clone)]
pub struct PolymarketSessionKeyClientConfig {
    pub private_key: SecretString,
    pub api_key: SecretString,
    pub api_secret: SecretString,
    pub passphrase: SecretString,
    pub builder_api_key: SecretString,
    pub builder_api_secret: SecretString,
    pub builder_passphrase: SecretString,
    pub funder: String,
    pub base_url_http: Option<String>,
    pub base_url_relayer: Option<String>,
    pub proxy_url: Option<SecretString>,
}

/// Active session authorization. Contains no private key or API credentials.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PolymarketSessionKey {
    pub address: String,
    pub scopes: Vec<String>,
    /// Expiration as whole Unix seconds.
    pub valid_until: u64,
}

/// Owner-operated authorization, inspection, and revocation for a Deposit Wallet.
#[derive(Debug)]
pub struct PolymarketSessionKeyClient {
    signer: OrderSigner,
    wallet: Address,
    clob: PolymarketClobHttpClient,
    relayer: PolymarketRelayerHttpClient,
    mutation: tokio::sync::Mutex<Option<SessionMutation>>,
}

impl PolymarketSessionKeyClient {
    /// Creates an administration client with explicit credentials.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid keys, wallet address, proxy, or HTTP configuration.
    pub fn new(config: PolymarketSessionKeyClientConfig) -> Result<Self> {
        let wallet = parse_address(&config.funder)?;
        let signer = OrderSigner::new(&EvmPrivateKey::new(config.private_key.expose_secret())?)?;
        let proxy = config
            .proxy_url
            .as_ref()
            .map(|url| ProxyUrl::parse(url.expose_secret().to_owned()))
            .transpose()
            .map_err(|_| Error::bad_request("Invalid session administration proxy URL"))?;
        let credential = Credential::new(config.api_key, config.api_secret, config.passphrase)?;

        let builder = Credential::new(
            config.builder_api_key,
            config.builder_api_secret,
            config.builder_passphrase,
        )?;
        let clob = PolymarketClobHttpClient::new_with_proxy(
            credential,
            format!("{:#x}", signer.address()),
            config.base_url_http,
            30,
            proxy.clone(),
        )
        .map_err(Error::from_http_client)?;
        let relayer = PolymarketRelayerHttpClient::new_with_builder(
            builder,
            config.base_url_relayer,
            300,
            proxy,
        )
        .map_err(Error::from_http_client)?;
        Ok(Self {
            signer,
            wallet,
            clob,
            relayer,
            mutation: tokio::sync::Mutex::new(None),
        })
    }

    /// Lists the wallet's active, unexpired session authorizations using owner CLOB credentials.
    ///
    /// # Errors
    ///
    /// Returns an error on authentication failure, malformed data, or a different response wallet.
    pub async fn list_session_keys(&self) -> Result<Vec<PolymarketSessionKey>> {
        let mut response = self.clob.list_session_keys().await?;
        if parse_address(&response.wallet)? != self.wallet {
            return Err(Error::decode(
                "Session registry returned a different Deposit Wallet",
            ));
        }

        for key in &response.signers {
            parse_address(&key.address)?;
            if key.scopes.is_empty() || key.scopes.iter().any(|scope| scope.trim().is_empty()) {
                return Err(Error::decode("Session registry returned empty scopes"));
            }
        }

        let now = unix_seconds();
        response.signers.retain(|key| key.valid_until > now);
        Ok(response.signers)
    }

    /// Authorizes a public session address for CLOB trading.
    ///
    /// Returns only after on-chain confirmation and matching registry visibility.
    /// The caller generates and stores the session private key separately.
    ///
    /// # Errors
    ///
    /// Returns an error on rejection, failed confirmation, timeout, or unknown submission outcome.
    pub async fn authorize_session_key(&self, address: &str) -> Result<PolymarketSessionKey> {
        self.mutate(address, true).await?.ok_or_else(|| {
            Error::decode("Session authorization completed without registry metadata")
        })
    }

    /// Revokes a session and waits for on-chain confirmation and removal from the active registry.
    ///
    /// # Errors
    ///
    /// Returns an error on rejection, timeout, or unknown submission outcome.
    /// Repeat the same operation on this client to resume an unresolved request.
    pub async fn revoke_session_key(&self, address: &str) -> Result<()> {
        self.mutate(address, false).await.map(|_| ())
    }

    async fn mutate(&self, address: &str, authorize: bool) -> Result<Option<PolymarketSessionKey>> {
        let address = self.session_address(address)?;
        let mut mutation = self.mutation.lock().await;
        if let Some(pending) = mutation.as_ref() {
            if pending.address != address || pending.valid_until.is_some() != authorize {
                return Err(pending.unresolved("Resume the previous session operation first"));
            }
        } else {
            let valid_until = authorize.then(|| unix_seconds() + SESSION_LIFETIME_SECS);

            let data = match valid_until {
                Some(valid_until) => authorizeSessionSignerCall {
                    sessionSigner: address,
                    validUntil: U256::from(valid_until),
                }
                .abi_encode(),
                None => revokeSessionSignerCall {
                    sessionSigner: address,
                }
                .abi_encode(),
            };

            let request = self.signed_request(address, data, valid_until).await?;
            *mutation = Some(SessionMutation {
                address,
                valid_until,
                body: serde_json::to_string(&request)?.into(),
                idempotency_key: Uuid::new_v4().to_string(),
                transaction_id: None,
                attempted: false,
                rejected: false,
            });
        }

        // Persist before the first POST await so cancellation cannot discard the signed request.
        let pending = mutation
            .as_mut()
            .ok_or_else(|| Error::exchange("Missing session operation"))?;
        let result = self.submit(pending).await;
        if let Err(e) = result {
            if pending.rejected {
                *mutation = None;
                return Err(e);
            }

            return Err(pending.unresolved(&e.to_string()));
        }

        let result = tokio::time::timeout(CONFIRMATION_TIMEOUT, self.confirm(pending)).await;
        match result {
            Ok(Ok(key)) => {
                *mutation = None;
                Ok(key)
            }
            Ok(Err(e)) if pending.rejected => {
                *mutation = None;
                Err(e)
            }
            Ok(Err(e)) => Err(pending.unresolved(&e.to_string())),
            Err(_) => Err(pending.unresolved("Session confirmation timed out")),
        }
    }

    async fn confirm(&self, pending: &mut SessionMutation) -> Result<Option<PolymarketSessionKey>> {
        let transaction_id = pending
            .transaction_id
            .as_deref()
            .ok_or_else(|| Error::decode("Session response omitted its transaction ID"))?;

        loop {
            match self.relayer.get_transaction(transaction_id).await {
                Ok(transaction) => match transaction.state {
                    RelayerTransactionState::Confirmed => break,
                    RelayerTransactionState::Failed | RelayerTransactionState::Invalid => {
                        pending.rejected = true;
                        return Err(Error::exchange(format!(
                            "Session transaction failed: {transaction_id}"
                        )));
                    }
                    _ => {}
                },
                Err(e) if retryable_read(&e) => {}
                Err(e) => return Err(e),
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }

        loop {
            match self.list_session_keys().await {
                Ok(keys) => {
                    let key = keys.into_iter().find(|key| {
                        key.address
                            .eq_ignore_ascii_case(&format!("{:#x}", pending.address))
                    });

                    match (pending.valid_until, key) {
                        (Some(valid_until), Some(key))
                            if key.valid_until == valid_until
                                && key.valid_until > unix_seconds()
                                && key.scopes == ["CLOB"] =>
                        {
                            return Ok(Some(key));
                        }
                        (None, None) => return Ok(None),
                        _ => {}
                    }
                }
                Err(e) if retryable_read(&e) => {}
                Err(e) => return Err(e),
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    fn session_address(&self, address: &str) -> Result<Address> {
        let address = parse_address(address)?;
        if address == self.signer.address() || address == self.wallet {
            return Err(Error::bad_request(
                "Session signer must differ from the owner and Deposit Wallet",
            ));
        }

        Ok(address)
    }

    async fn signed_request(
        &self,
        address: Address,
        data: Vec<u8>,
        valid_until: Option<u64>,
    ) -> Result<SessionRequest> {
        let nonce = self.relayer.get_wallet_nonce(self.signer.address()).await?;
        let deadline = U256::from(unix_seconds() + BATCH_DEADLINE_SECS);

        let calls = [DepositWalletCall {
            target: self.wallet,
            value: U256::ZERO,
            data: data.into(),
        }];

        let signature =
            self.signer
                .sign_deposit_wallet_batch(self.wallet, nonce, deadline, &calls)?;
        Ok(SessionRequest {
            wallet_address: format!("{:#x}", self.wallet),
            session_signer_address: format!("{address:#x}"),
            nonce: nonce.to_string(),
            deadline: deadline.to_string(),
            signature,
            valid_until: valid_until.map(|value| value.to_string()),
            scopes: valid_until.map(|_| vec!["CLOB"]),
        })
    }

    async fn submit(&self, pending: &mut SessionMutation) -> Result<()> {
        if pending.transaction_id.is_some() {
            return Ok(());
        }

        let authorize = pending.valid_until.is_some();

        let path = if authorize {
            "/v1/session-signers/authorizations"
        } else {
            "/v1/session-signers/revocations"
        };

        for attempt in 0..3 {
            let previously_attempted = pending.attempted;
            pending.attempted = true;
            let result = self
                .relayer
                .post_session::<serde_json::Value>(
                    path,
                    pending.body.expose_secret(),
                    &pending.idempotency_key,
                )
                .await;

            match result {
                Ok(response) => {
                    let (status, transaction_id) = if authorize {
                        let response: AuthorizationResponse = serde_json::from_value(response)?;

                        if !matches!(
                            response.status.as_str(),
                            "SUBMITTED" | "REGISTRY_PENDING" | "REGISTERED"
                        ) {
                            pending.rejected = matches!(
                                response.status.as_str(),
                                "FAILED" | "SUPERSEDED" | "REPAIR_REQUIRED"
                            );
                            return Err(Error::exchange(format!(
                                "Session authorization status {}",
                                response.status
                            )));
                        }

                        (response.status, response.transaction_id)
                    } else {
                        let response: RevocationResponse = serde_json::from_value(response)?;
                        if !matches!(
                            response.status.as_str(),
                            "PENDING" | "FENCED" | "SWEPT" | "CHAIN_SUBMITTED" | "CONFIRMED"
                        ) {
                            pending.rejected = response.status == "FAILED";
                            return Err(Error::exchange(format!(
                                "Session revocation status {}",
                                response.status
                            )));
                        }

                        (response.status, response.transaction_id)
                    };

                    if transaction_id.trim().is_empty() {
                        return Err(Error::decode(format!(
                            "Session {status} response omitted its transaction ID"
                        )));
                    }

                    pending.transaction_id = Some(transaction_id);
                    return Ok(());
                }
                Err(e) if e.is_retryable() && attempt < 2 => {
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
                Err(e) => {
                    pending.rejected = !previously_attempted && !e.is_submit_outcome_unknown();
                    return Err(e);
                }
            }
        }

        Err(Error::exchange("Session submission retry budget exhausted"))
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct SessionKeysResponse {
    pub wallet: String,
    pub signers: Vec<PolymarketSessionKey>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionRequest {
    wallet_address: String,
    session_signer_address: String,
    nonce: String,
    deadline: String,
    signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    valid_until: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scopes: Option<Vec<&'static str>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthorizationResponse {
    status: String,
    transaction_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RevocationResponse {
    status: String,
    transaction_id: String,
    #[serde(rename = "fenced")]
    _fenced: bool,
}

#[derive(Debug)]
struct SessionMutation {
    address: Address,
    valid_until: Option<u64>,
    body: SecretString,
    idempotency_key: String,
    transaction_id: Option<String>,
    attempted: bool,
    rejected: bool,
}

impl SessionMutation {
    fn unresolved(&self, reason: &str) -> Error {
        Error::exchange(format!(
            "{reason}; session operation unresolved (idempotency_key={}, transaction_id={}); repeat the same operation on this client",
            self.idempotency_key,
            self.transaction_id.as_deref().unwrap_or("unknown"),
        ))
    }
}

fn retryable_read(e: &Error) -> bool {
    e.is_retryable()
        || matches!(
            e,
            Error::Http {
                status: 404 | 409,
                ..
            }
        )
}

alloy::sol! {
    function authorizeSessionSigner(address sessionSigner, uint256 validUntil);
    function revokeSessionSigner(address sessionSigner);
}

fn parse_address(value: &str) -> Result<Address> {
    if !value.starts_with("0x") || value.len() != 42 {
        return Err(Error::bad_request("Expected a 0x-prefixed EVM address"));
    }

    let address = value
        .parse::<Address>()
        .map_err(|_| Error::bad_request("Invalid EVM address"))?;
    if address.is_zero() {
        return Err(Error::bad_request("EVM address must not be zero"));
    }

    Ok(address)
}

fn unix_seconds() -> u64 {
    get_atomic_clock_realtime().get_time_ns().as_u64() / 1_000_000_000
}
