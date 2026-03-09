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

use std::{
    collections::HashMap,
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use alloy::{
    consensus::{Transaction, TxEnvelope, transaction::SignerRecoverable},
    eips::eip2718::Decodable2718,
    primitives::{B256, U256},
};
use nautilus_model::defi::tx_hash::{decode_raw_tx_hex, tx_hash_hex_from_raw_tx_hex};
use nautilus_network::{
    http::{HttpClient, HttpClientError, Method},
    retry::RetryManager,
};
use rust_decimal::Decimal;

use crate::execution::signer::types::{
    OssSignEthRequest, OssSignEthResponse, RemoteSignerClientConfig, SignRequest, SignedTx,
    SignerApiMode, SignerClientError,
};

/// Signer payload mapper for API-mode specific transport contracts.
#[derive(Debug, Default)]
pub struct SignerPayloadMapper;

impl SignerPayloadMapper {
    /// Maps canonical sign request to OSS signer-server flat payload.
    ///
    /// # Errors
    ///
    /// Returns an error if mandatory EIP-1559 fee fields are missing.
    pub fn to_oss_v1_flat(request: &SignRequest) -> Result<OssSignEthRequest, SignerClientError> {
        let max_fee_per_gas = request.max_fee_per_gas.ok_or_else(|| {
            SignerClientError::Preflight(
                "EIP-1559 max_fee_per_gas is required in oss_v1_flat mode".to_string(),
            )
        })?;
        let max_priority_fee_per_gas = request.max_priority_fee_per_gas.ok_or_else(|| {
            SignerClientError::Preflight(
                "EIP-1559 max_priority_fee_per_gas is required in oss_v1_flat mode".to_string(),
            )
        })?;

        Ok(OssSignEthRequest {
            chain_id: request.chain_id,
            to: request.to.to_string(),
            data: request.data.clone(),
            max_fee_per_gas,
            max_priority_fee_per_gas,
            gas: request.gas,
            nonce: request.nonce,
            value: request.value.clone(),
            deadline: request.deadline,
            expected_notional: request.expected_notional.clone(),
        })
    }
}

/// Client for remote signer-server transaction signing.
#[derive(Debug)]
pub struct RemoteSignerClient {
    signer_url: String,
    config: RemoteSignerClientConfig,
    http_client: HttpClient,
    retry_manager: RetryManager<SignerClientError>,
    request_id: AtomicU64,
}

impl RemoteSignerClient {
    /// Creates a new remote signer client.
    ///
    /// # Errors
    ///
    /// Returns an error when TLS policy is violated or HTTP client initialization fails.
    pub fn new(config: RemoteSignerClientConfig) -> Result<Self, SignerClientError> {
        if config.signer_require_tls
            && !config
                .signer_endpoint
                .to_ascii_lowercase()
                .starts_with("https://")
        {
            return Err(SignerClientError::InsecureEndpoint(
                "signer_require_tls=true requires https:// signer_endpoint".to_string(),
            ));
        }

        let route = normalize_route(config.signer_route.as_str());
        let signer_url = format!("{}{}", config.signer_endpoint.trim_end_matches('/'), route);
        let retry_manager = RetryManager::new(config.signer_retry_config.clone());

        if config.signer_mtls.is_some() {
            // mTLS in this milestone is config wiring only; HTTP stack certificate loading is deferred.
            log::info!("Remote signer mTLS config detected (wiring-only in current milestone)");
        }

        let http_client = HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, None, None)
            .map_err(|e| {
                SignerClientError::Transport(format!(
                    "failed to initialize signer HTTP client: {e}"
                ))
            })?;

        Ok(Self {
            signer_url,
            config,
            http_client,
            retry_manager,
            request_id: AtomicU64::new(1),
        })
    }

    /// Signs an EVM transaction intent through remote signer-server.
    ///
    /// # Errors
    ///
    /// Returns an error for preflight failures, non-retryable signer rejections,
    /// transport failures after retry budget, or post-sign verification mismatches.
    pub async fn sign_evm_tx(&self, request: SignRequest) -> Result<SignedTx, SignerClientError> {
        let request_id = self.request_id.fetch_add(1, Ordering::Relaxed);
        log::info!(
            "Remote signer submit request_id={} mode={:?} route={}",
            request_id,
            self.config.signer_api_mode,
            self.signer_url
        );

        self.preflight_validate(&request)?;

        let payload = match self.config.signer_api_mode {
            SignerApiMode::OssV1Flat => SignerPayloadMapper::to_oss_v1_flat(&request)?,
        };

        let payload_bytes = serde_json::to_vec(&payload)
            .map_err(|e| SignerClientError::Decode(format!("failed to encode payload: {e}")))?;

        let signer_url = self.signer_url.clone();
        let http_client = self.http_client.clone();
        let timeout_secs = timeout_secs_from_ms(self.config.signer_timeout_ms);
        let operation_name = format!("remote_signer.sign_evm_tx.{request_id}");
        let should_retry = move |error: &SignerClientError| {
            let decision = error.is_retryable();
            log::warn!(
                "Remote signer decision request_id={} retry={} error={}",
                request_id,
                decision,
                error
            );
            decision
        };
        let create_error = move |message: String| {
            SignerClientError::Timeout(format!("request_id={request_id} {message}"))
        };

        let signer_response = self
            .retry_manager
            .execute_with_retry(
                operation_name.as_str(),
                move || {
                    let client = http_client.clone();
                    let url = signer_url.clone();
                    let body = payload_bytes.clone();
                    async move {
                        let mut headers = HashMap::new();
                        headers.insert("content-type".to_string(), "application/json".to_string());
                        headers.insert("x-request-id".to_string(), request_id.to_string());

                        let response = client
                            .request(
                                Method::POST,
                                url,
                                None,
                                Some(headers),
                                Some(body),
                                timeout_secs,
                                None,
                            )
                            .await
                            .map_err(map_http_client_error)?;

                        parse_signer_response(response.status.as_u16(), response.body.as_ref())
                    }
                },
                should_retry,
                create_error,
            )
            .await?;

        let signed = self.post_verify_sign_response(&request, signer_response, request_id)?;
        log::info!(
            "Remote signer success request_id={} tx_hash={}",
            request_id,
            signed.tx_hash
        );
        Ok(signed)
    }

    fn preflight_validate(&self, request: &SignRequest) -> Result<(), SignerClientError> {
        let max_fee = request.max_fee_per_gas.ok_or_else(|| {
            if request.gas_price.is_some() {
                SignerClientError::Preflight(
                    "EIP-1559 fee fields are required; gasPrice-only path is unsafe".to_string(),
                )
            } else {
                SignerClientError::Preflight(
                    "EIP-1559 max_fee_per_gas is required in signer request".to_string(),
                )
            }
        })?;
        let max_priority = request.max_priority_fee_per_gas.ok_or_else(|| {
            if request.gas_price.is_some() {
                SignerClientError::Preflight(
                    "EIP-1559 fee fields are required; gasPrice-only path is unsafe".to_string(),
                )
            } else {
                SignerClientError::Preflight(
                    "EIP-1559 max_priority_fee_per_gas is required in signer request".to_string(),
                )
            }
        })?;

        if max_fee == 0 || max_priority == 0 {
            return Err(SignerClientError::Preflight(
                "EIP-1559 fee fields must be positive".to_string(),
            ));
        }
        if max_priority > max_fee {
            return Err(SignerClientError::Preflight(
                "max_priority_fee_per_gas cannot exceed max_fee_per_gas".to_string(),
            ));
        }

        if request.deadline <= 0 {
            return Err(SignerClientError::Preflight(
                "deadline must be a positive unix timestamp".to_string(),
            ));
        }
        let now = current_unix_seconds()?;
        if request.deadline <= now {
            return Err(SignerClientError::Preflight(format!(
                "deadline must be in the future (deadline={} now={now})",
                request.deadline
            )));
        }

        validate_canonical_hex_bytes(request.value.as_str(), "value")?;
        validate_canonical_hex_bytes(request.data.as_str(), "data")?;

        let expected_selector = normalize_selector(request.expected_selector.as_str())?;
        let actual_selector = extract_selector(request.data.as_str())?;
        if actual_selector != expected_selector {
            return Err(SignerClientError::Preflight(format!(
                "selector mismatch expected={} actual={}",
                expected_selector, actual_selector
            )));
        }

        validate_expected_notional(request.expected_notional.as_str())?;

        Ok(())
    }

    fn post_verify_sign_response(
        &self,
        request: &SignRequest,
        response: OssSignEthResponse,
        request_id: u64,
    ) -> Result<SignedTx, SignerClientError> {
        let raw_tx_bytes = decode_raw_tx_hex(response.raw_tx_hex.as_str())
            .map_err(|e| SignerClientError::PostVerify(format!("invalid raw_tx_hex: {e}")))?;
        let envelope = TxEnvelope::decode_2718_exact(raw_tx_bytes.as_slice()).map_err(|e| {
            SignerClientError::PostVerify(format!("failed to decode EIP-2718 signed tx: {e}"))
        })?;

        if envelope.chain_id() != Some(request.chain_id) {
            return Err(SignerClientError::PostVerify(format!(
                "chain_id mismatch expected={} actual={:?}",
                request.chain_id,
                envelope.chain_id()
            )));
        }

        if envelope.nonce() != request.nonce {
            return Err(SignerClientError::PostVerify(format!(
                "nonce mismatch expected={} actual={}",
                request.nonce,
                envelope.nonce()
            )));
        }

        if envelope.to() != Some(request.to) {
            return Err(SignerClientError::PostVerify(format!(
                "to mismatch expected={} actual={:?}",
                request.to,
                envelope.to()
            )));
        }

        let tx_data_hex = format!("0x{}", hex::encode(envelope.input()));
        if tx_data_hex != request.data {
            return Err(SignerClientError::PostVerify(
                "data mismatch between signer response and request".to_string(),
            ));
        }

        let request_value = parse_u256_hex(request.value.as_str())
            .map_err(|e| SignerClientError::PostVerify(format!("invalid request value: {e}")))?;
        if envelope.value() != request_value {
            return Err(SignerClientError::PostVerify(format!(
                "value mismatch expected={} actual={}",
                request.value,
                envelope.value()
            )));
        }

        if envelope.gas_limit() != request.gas {
            return Err(SignerClientError::PostVerify(format!(
                "gas mismatch expected={} actual={}",
                request.gas,
                envelope.gas_limit()
            )));
        }

        let expected_max_fee = u128::from(request.max_fee_per_gas.ok_or_else(|| {
            SignerClientError::PostVerify("missing expected max_fee_per_gas".to_string())
        })?);
        if envelope.max_fee_per_gas() != expected_max_fee {
            return Err(SignerClientError::PostVerify(format!(
                "max_fee_per_gas mismatch expected={} actual={}",
                expected_max_fee,
                envelope.max_fee_per_gas()
            )));
        }

        let expected_max_priority =
            u128::from(request.max_priority_fee_per_gas.ok_or_else(|| {
                SignerClientError::PostVerify(
                    "missing expected max_priority_fee_per_gas".to_string(),
                )
            })?);
        if envelope.max_priority_fee_per_gas() != Some(expected_max_priority) {
            return Err(SignerClientError::PostVerify(format!(
                "max_priority_fee_per_gas mismatch expected={} actual={:?}",
                expected_max_priority,
                envelope.max_priority_fee_per_gas()
            )));
        }

        let recovered_sender = envelope.recover_signer().map_err(|e| {
            SignerClientError::PostVerify(format!("failed recovering sender from signed tx: {e}"))
        })?;
        if recovered_sender != self.config.signer_wallet_address {
            return Err(SignerClientError::PostVerify(format!(
                "sender mismatch expected={} actual={}",
                self.config.signer_wallet_address, recovered_sender
            )));
        }

        let tx_hash = tx_hash_hex_from_signer_raw_tx(response.raw_tx_hex.as_str())?;

        Ok(SignedTx {
            raw_tx_hex: response.raw_tx_hex,
            r: response.r,
            s: response.s,
            v: response.v,
            tx_hash,
            request_id,
        })
    }
}

/// Computes canonical tx hash from signer-returned raw transaction hex.
///
/// # Errors
///
/// Returns an error when the raw transaction hex cannot be decoded.
pub fn tx_hash_hex_from_signer_raw_tx(raw_tx_hex: &str) -> Result<String, SignerClientError> {
    tx_hash_hex_from_raw_tx_hex(raw_tx_hex).map_err(|e| {
        SignerClientError::PostVerify(format!("failed computing tx hash from raw tx bytes: {e}"))
    })
}

/// Fails closed if RPC-reported hash differs from computed hash.
///
/// # Errors
///
/// Returns [`SignerClientError::RpcTxHashMismatch`] when hashes differ.
pub fn assert_rpc_tx_hash_matches_computed(
    rpc_tx_hash: &str,
    computed_tx_hash: &str,
) -> Result<(), SignerClientError> {
    let normalized_rpc = normalize_tx_hash(rpc_tx_hash)?;
    let normalized_computed = normalize_tx_hash(computed_tx_hash)?;

    if normalized_rpc != normalized_computed {
        return Err(SignerClientError::RpcTxHashMismatch {
            computed: normalized_computed,
            rpc: normalized_rpc,
        });
    }

    Ok(())
}

fn parse_signer_response(
    status: u16,
    body: &[u8],
) -> Result<OssSignEthResponse, SignerClientError> {
    if !(200..300).contains(&status) {
        return Err(SignerClientError::HttpStatus {
            status,
            body: format_signer_error_body(body),
        });
    }

    serde_json::from_slice::<OssSignEthResponse>(body).map_err(|e| {
        SignerClientError::Decode(format!(
            "invalid signer success response: {e}; body={}",
            body_preview(body)
        ))
    })
}

fn map_http_client_error(error: HttpClientError) -> SignerClientError {
    match error {
        HttpClientError::TimeoutError(msg) => SignerClientError::Timeout(msg),
        HttpClientError::Error(msg) => SignerClientError::Transport(msg),
        HttpClientError::InvalidProxy(msg) => SignerClientError::Transport(msg),
        HttpClientError::ClientBuildError(msg) => SignerClientError::Transport(msg),
    }
}

fn normalize_route(route: &str) -> String {
    if route.starts_with('/') {
        route.to_string()
    } else {
        format!("/{route}")
    }
}

fn timeout_secs_from_ms(timeout_ms: u64) -> Option<u64> {
    if timeout_ms == 0 {
        None
    } else {
        Some(timeout_ms.div_ceil(1000))
    }
}

fn parse_u256_hex(value: &str) -> Result<U256, SignerClientError> {
    let digits = value
        .strip_prefix("0x")
        .ok_or_else(|| SignerClientError::Preflight("hex value must start with 0x".to_string()))?;
    U256::from_str_radix(digits, 16)
        .map_err(|e| SignerClientError::Preflight(format!("invalid hex value '{value}': {e}")))
}

fn current_unix_seconds() -> Result<i64, SignerClientError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| SignerClientError::Preflight(format!("system clock error: {e}")))?;
    i64::try_from(now.as_secs()).map_err(|_| {
        SignerClientError::Preflight("unix timestamp is out of i64 bounds".to_string())
    })
}

fn validate_expected_notional(expected_notional: &str) -> Result<(), SignerClientError> {
    let trimmed = expected_notional.trim();
    if trimmed.is_empty() {
        return Err(SignerClientError::Preflight(
            "expected_notional cannot be empty".to_string(),
        ));
    }

    let value = Decimal::from_str(trimmed).map_err(|e| {
        SignerClientError::Preflight(format!("expected_notional must be a decimal string: {e}"))
    })?;
    if value <= Decimal::ZERO {
        return Err(SignerClientError::Preflight(
            "expected_notional must be positive".to_string(),
        ));
    }

    Ok(())
}

fn validate_canonical_hex_bytes(value: &str, field: &str) -> Result<(), SignerClientError> {
    let digits = value
        .strip_prefix("0x")
        .ok_or_else(|| SignerClientError::Preflight(format!("{field} must start with 0x")))?;
    if digits.is_empty() {
        return Err(SignerClientError::Preflight(format!(
            "{field} cannot be empty"
        )));
    }
    if digits.len() % 2 != 0 {
        return Err(SignerClientError::Preflight(format!(
            "{field} must be even-length hex bytes"
        )));
    }
    if !digits
        .as_bytes()
        .iter()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(SignerClientError::Preflight(format!(
            "{field} must use lowercase hexadecimal bytes"
        )));
    }

    Ok(())
}

fn normalize_selector(selector: &str) -> Result<String, SignerClientError> {
    let selector = selector.to_ascii_lowercase();
    let digits = selector.strip_prefix("0x").ok_or_else(|| {
        SignerClientError::Preflight("expected_selector must start with 0x".to_string())
    })?;
    if digits.len() != 8
        || !digits
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(SignerClientError::Preflight(
            "expected_selector must be 4 bytes (0x + 8 hex chars)".to_string(),
        ));
    }

    Ok(selector)
}

fn extract_selector(data: &str) -> Result<String, SignerClientError> {
    let digits = data
        .strip_prefix("0x")
        .ok_or_else(|| SignerClientError::Preflight("data must start with 0x".to_string()))?;
    if digits.len() < 8 {
        return Err(SignerClientError::Preflight(
            "data must include at least a 4-byte selector".to_string(),
        ));
    }
    Ok(format!("0x{}", &digits[..8]))
}

fn normalize_tx_hash(hash: &str) -> Result<String, SignerClientError> {
    let parsed: B256 = hash.parse().map_err(|e| {
        SignerClientError::PostVerify(format!("invalid tx hash '{hash}' for comparison: {e}"))
    })?;
    Ok(format!("{parsed:#x}"))
}

fn format_signer_error_body(body: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) {
        let code = value.get("code").cloned();
        let message = value
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            });
        let data = value.get("data").cloned();

        if code.is_some() || message.is_some() || data.is_some() {
            let mut parts = Vec::new();
            if let Some(code) = code {
                parts.push(format!("code={code}"));
            }
            if let Some(message) = message {
                parts.push(format!("message={message}"));
            }
            if let Some(data) = data {
                parts.push(format!("data={data}"));
            }
            return parts.join(" ");
        }

        return value.to_string();
    }

    body_preview(body)
}

fn body_preview(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    if text.chars().count() > 300 {
        let preview: String = text.chars().take(300).collect();
        format!("{}... (truncated, {} bytes total)", preview, body.len())
    } else {
        text.to_string()
    }
}
