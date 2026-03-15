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

//! L1 API credential creation and derivation for the Polymarket CLOB.

use std::collections::HashMap;

use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_network::http::{HttpClient, Method};
use serde::Deserialize;

use crate::{
    common::{credential::EvmPrivateKey, urls::clob_http_url},
    http::error::{Error, Result},
    signing::eip712::sign_clob_auth,
};

/// API credentials returned by the Polymarket CLOB auth endpoints.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCredentials {
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

/// Creates new API credentials via `POST /auth/api-key` using L1 authentication.
///
/// Fails if credentials already exist for this `(address, nonce)` pair.
/// Use [`derive_api_key`] to retrieve existing credentials, or
/// [`create_or_derive_api_key`] for idempotent behavior.
pub async fn create_api_key(
    private_key: &EvmPrivateKey,
    nonce: u64,
    base_url: Option<&str>,
) -> Result<ApiCredentials> {
    let (client, headers, base) = prepare_l1_request(private_key, nonce, base_url)?;

    let url = format!("{base}/auth/api-key");
    let response = client
        .request(Method::POST, url, None, Some(headers), None, None, None)
        .await
        .map_err(Error::from_http_client)?;

    if response.status.is_success() {
        serde_json::from_slice(&response.body).map_err(Error::Serde)
    } else {
        Err(Error::from_status_code(
            response.status.as_u16(),
            &response.body,
        ))
    }
}

/// Derives existing API credentials via `GET /auth/derive-api-key` using L1 authentication.
///
/// Fails if no credentials exist for this `(address, nonce)` pair.
/// Use [`create_api_key`] to create new credentials, or
/// [`create_or_derive_api_key`] for idempotent behavior.
pub async fn derive_api_key(
    private_key: &EvmPrivateKey,
    nonce: u64,
    base_url: Option<&str>,
) -> Result<ApiCredentials> {
    let (client, headers, base) = prepare_l1_request(private_key, nonce, base_url)?;

    let url = format!("{base}/auth/derive-api-key");
    let response = client
        .request(Method::GET, url, None, Some(headers), None, None, None)
        .await
        .map_err(Error::from_http_client)?;

    if response.status.is_success() {
        serde_json::from_slice(&response.body).map_err(Error::Serde)
    } else {
        Err(Error::from_status_code(
            response.status.as_u16(),
            &response.body,
        ))
    }
}

/// Creates or derives API credentials using L1 (EIP-712) authentication.
///
/// First attempts `POST /auth/api-key` (create). On HTTP-level errors
/// (e.g. nonce already used), falls back to `GET /auth/derive-api-key`
/// (derive). Transport and network errors are propagated immediately
/// without attempting the fallback.
pub async fn create_or_derive_api_key(
    private_key: &EvmPrivateKey,
    nonce: u64,
    base_url: Option<&str>,
) -> Result<ApiCredentials> {
    match create_api_key(private_key, nonce, base_url).await {
        Ok(creds) => Ok(creds),
        Err(e) if e.is_http_status_error() => derive_api_key(private_key, nonce, base_url).await,
        Err(e) => Err(e),
    }
}

fn prepare_l1_request(
    private_key: &EvmPrivateKey,
    nonce: u64,
    base_url: Option<&str>,
) -> Result<(HttpClient, HashMap<String, String>, String)> {
    let base = base_url
        .unwrap_or_else(|| clob_http_url())
        .trim_end_matches('/')
        .to_string();
    let timestamp =
        (get_atomic_clock_realtime().get_time_ns().as_u64() / 1_000_000_000).to_string();
    let (address, signature) = sign_clob_auth(private_key, &timestamp, nonce)?;
    let headers = l1_headers(&address, &signature, &timestamp, nonce);
    let client = HttpClient::new(HashMap::new(), vec![], vec![], None, None, None)
        .map_err(Error::from_http_client)?;
    Ok((client, headers, base))
}

fn l1_headers(
    address: &str,
    signature: &str,
    timestamp: &str,
    nonce: u64,
) -> HashMap<String, String> {
    HashMap::from([
        ("POLY_ADDRESS".to_string(), address.to_string()),
        ("POLY_SIGNATURE".to_string(), signature.to_string()),
        ("POLY_TIMESTAMP".to_string(), timestamp.to_string()),
        ("POLY_NONCE".to_string(), nonce.to_string()),
    ])
}
