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

use alloy::primitives::Address;
use nautilus_network::retry::RetryConfig;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Signer API compatibility mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SignerApiMode {
    /// OSS signer-server `POST /sign/eth` flat payload contract.
    #[default]
    OssV1Flat,
}

/// Optional mTLS configuration (wiring-only in this milestone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerMtlsConfig {
    pub client_cert_path: Option<String>,
    pub client_key_path: Option<String>,
    pub ca_cert_path: Option<String>,
    pub client_cert_pem: Option<String>,
    pub client_key_pem: Option<String>,
    pub ca_cert_pem: Option<String>,
}

/// Runtime configuration for [`crate::execution::signer::RemoteSignerClient`].
#[derive(Debug, Clone)]
pub struct RemoteSignerClientConfig {
    pub signer_endpoint: String,
    pub signer_route: String,
    pub signer_api_mode: SignerApiMode,
    pub signer_timeout_ms: u64,
    pub signer_require_tls: bool,
    pub signer_wallet_address: Address,
    pub signer_retry_config: RetryConfig,
    pub signer_mtls: Option<SignerMtlsConfig>,
}

impl RemoteSignerClientConfig {
    #[must_use]
    pub fn new(signer_endpoint: String, signer_wallet_address: Address) -> Self {
        Self {
            signer_endpoint,
            signer_wallet_address,
            ..Self::default()
        }
    }
}

impl Default for RemoteSignerClientConfig {
    fn default() -> Self {
        Self {
            signer_endpoint: String::new(),
            signer_route: "/sign/eth".to_string(),
            signer_api_mode: SignerApiMode::default(),
            signer_timeout_ms: 5_000,
            signer_require_tls: true,
            signer_wallet_address: Address::ZERO,
            signer_retry_config: RetryConfig {
                max_retries: 3,
                initial_delay_ms: 250,
                max_delay_ms: 2_000,
                backoff_factor: 2.0,
                jitter_ms: 100,
                operation_timeout_ms: Some(5_000),
                immediate_first: true,
                max_elapsed_ms: Some(20_000),
            },
            signer_mtls: None,
        }
    }
}

/// Canonical internal sign request before transport mapping.
#[derive(Debug, Clone)]
pub struct SignRequest {
    pub chain_id: u64,
    pub nonce: u64,
    pub to: Address,
    pub data: String,
    pub value: String,
    pub gas: u64,
    pub max_fee_per_gas: Option<u64>,
    pub max_priority_fee_per_gas: Option<u64>,
    pub gas_price: Option<u64>,
    pub deadline: i64,
    pub expected_notional: String,
    pub expected_selector: String,
}

/// Signed transaction metadata returned to execution flow.
#[derive(Debug, Clone)]
pub struct SignedTx {
    pub raw_tx_hex: String,
    pub r: String,
    pub s: String,
    pub v: u8,
    pub tx_hash: String,
    pub request_id: u64,
}

/// OSS signer-server request schema (`POST /sign/eth`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OssSignEthRequest {
    #[serde(rename = "chainId")]
    pub chain_id: u64,
    pub to: String,
    pub data: String,
    pub max_fee_per_gas: u64,
    pub max_priority_fee_per_gas: u64,
    pub gas: u64,
    pub nonce: u64,
    pub value: String,
    pub deadline: i64,
    #[serde(rename = "expected_notional")]
    pub expected_notional: String,
}

/// OSS signer-server response schema.
#[derive(Debug, Clone, Deserialize)]
pub struct OssSignEthResponse {
    pub r: String,
    pub s: String,
    pub v: u8,
    pub raw_tx_hex: String,
}

#[derive(Debug, Error)]
pub enum SignerClientError {
    #[error("Signer endpoint rejected by TLS policy: {0}")]
    InsecureEndpoint(String),

    #[error("Unsupported signer API mode: {0:?}")]
    UnsupportedApiMode(SignerApiMode),

    #[error("Signer preflight validation failed: {0}")]
    Preflight(String),

    #[error("Signer transport error: {0}")]
    Transport(String),

    #[error("Signer timeout: {0}")]
    Timeout(String),

    #[error("Signer HTTP error status={status} body={body}")]
    HttpStatus { status: u16, body: String },

    #[error("Failed to decode signer response: {0}")]
    Decode(String),

    #[error("Signer post-verify failed: {0}")]
    PostVerify(String),

    #[error("RPC tx hash mismatch: computed={computed} rpc={rpc}")]
    RpcTxHashMismatch { computed: String, rpc: String },
}

impl SignerClientError {
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) | Self::Timeout(_) => true,
            // Milestone requirement: never retry 4xx responses (including 429).
            Self::HttpStatus { status, .. } => *status >= 500,
            _ => false,
        }
    }
}
