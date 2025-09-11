// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Provides the HTTP client integration for the [Hyperliquid](https://hyperliquid.xyz/) REST API.
//!
//! This module defines and implements a [`HyperliquidHttpClient`] for sending requests to various
//! Hyperliquid endpoints. It handles request signing (when credentials are provided), constructs
//! valid HTTP requests using the [`HttpClient`], and parses the responses back into structured
//! data or an [`Error`].

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, LazyLock},
};

use anyhow::Context;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_network::{http::HttpClient, ratelimiter::quota::Quota};
use reqwest::{Method, header::USER_AGENT};
use serde_json::Value;

use crate::{
    common::{
        consts::{HyperliquidNetwork, exchange_url, info_url},
        credential::{Secrets, VaultAddress},
    },
    http::{
        error::{Error, Result},
        models::{
            HyperliquidExchangeRequest, HyperliquidExchangeResponse, HyperliquidFills,
            HyperliquidL2Book, HyperliquidMeta, HyperliquidOrderStatus,
        },
        query::{ExchangeAction, InfoRequest},
    },
    signing::{
        HyperliquidActionType, HyperliquidEip712Signer, NonceManager, SignRequest, types::SignerId,
    },
};

// https://hyperliquid.xyz/docs/api#rate-limits
pub static HYPERLIQUID_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_minute(NonZeroU32::new(1200).unwrap()));

/// Provides a lower-level HTTP client for connecting to the [Hyperliquid](https://hyperliquid.xyz/) REST API.
///
/// This client wraps the underlying `HttpClient` to handle functionality
/// specific to Hyperliquid, such as request signing (for authenticated endpoints),
/// forming request URLs, and deserializing responses into specific data models.
#[derive(Debug)]
pub struct HyperliquidHttpClient {
    client: HttpClient,
    #[allow(dead_code)] // May be used for future network-specific logic
    network: HyperliquidNetwork,
    base_info: String,
    base_exchange: String,
    signer: Option<HyperliquidEip712Signer>,
    nonce_manager: Option<Arc<NonceManager>>,
    vault_address: Option<VaultAddress>,
}

impl Default for HyperliquidHttpClient {
    fn default() -> Self {
        Self::new(HyperliquidNetwork::Testnet, None)
    }
}

impl HyperliquidHttpClient {
    /// Creates a new [`HyperliquidHttpClient`] using the default Hyperliquid HTTP URL,
    /// optionally overridden with a custom timeout.
    ///
    /// This version of the client has **no credentials**, so it can only
    /// call publicly accessible endpoints.
    #[must_use]
    pub fn new(network: HyperliquidNetwork, timeout_secs: Option<u64>) -> Self {
        Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*HYPERLIQUID_REST_QUOTA),
                timeout_secs,
            ),
            network,
            base_info: info_url(network).to_string(),
            base_exchange: exchange_url(network).to_string(),
            signer: None,
            nonce_manager: None,
            vault_address: None,
        }
    }

    /// Creates a new [`HyperliquidHttpClient`] configured with credentials
    /// for authenticated requests.
    #[must_use]
    pub fn with_credentials(secrets: &Secrets, timeout_secs: Option<u64>) -> Self {
        let signer = HyperliquidEip712Signer::new(secrets.private_key.clone());
        let nonce_manager = Arc::new(NonceManager::new());

        Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*HYPERLIQUID_REST_QUOTA),
                timeout_secs,
            ),
            network: secrets.network,
            base_info: info_url(secrets.network).to_string(),
            base_exchange: exchange_url(secrets.network).to_string(),
            signer: Some(signer),
            nonce_manager: Some(nonce_manager),
            vault_address: secrets.vault_address,
        }
    }

    /// Creates an authenticated client from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if required environment variables
    /// are not set.
    pub fn from_env() -> Result<Self> {
        let secrets =
            Secrets::from_env().map_err(|_| Error::auth("missing credentials in environment"))?;
        Ok(Self::with_credentials(&secrets, None))
    }

    /// Builds the default headers to include with each request (e.g., `User-Agent`).
    fn default_headers() -> HashMap<String, String> {
        HashMap::from([(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())])
    }

    // ---------------- INFO ENDPOINTS --------------------------------------------

    /// Get metadata about available markets.
    pub async fn info_meta(&self) -> Result<HyperliquidMeta> {
        let request = InfoRequest::meta();
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Get L2 order book for a coin.
    pub async fn info_l2_book(&self, coin: &str) -> Result<HyperliquidL2Book> {
        let request = InfoRequest::l2_book(coin);
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Get user fills (trading history).
    pub async fn info_user_fills(&self, user: &str) -> Result<HyperliquidFills> {
        let request = InfoRequest::user_fills(user);
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Get order status for a user.
    pub async fn info_order_status(&self, user: &str, oid: u64) -> Result<HyperliquidOrderStatus> {
        let request = InfoRequest::order_status(user, oid);
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Send a raw info request and return the JSON response.
    async fn send_info_request(&self, request: &InfoRequest) -> Result<Value> {
        let url = &self.base_info;
        let body = serde_json::to_value(request).map_err(Error::Serde)?;
        let body_bytes = serde_json::to_string(&body)
            .map_err(Error::Serde)?
            .into_bytes();

        let response = self
            .client
            .request(
                Method::POST,
                url.clone(),
                None,
                Some(body_bytes),
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)?;

        if response.status.is_success() {
            serde_json::from_slice(&response.body).map_err(Error::Serde)
        } else {
            let error_body = String::from_utf8_lossy(&response.body);
            Err(Error::http(
                response.status.as_u16(),
                error_body.to_string(),
            ))
        }
    }

    // ---------------- EXCHANGE ENDPOINTS ---------------------------------------

    /// Send a signed action to the exchange.
    pub async fn post_action(
        &self,
        action: &ExchangeAction,
    ) -> Result<HyperliquidExchangeResponse> {
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| Error::auth("credentials required for exchange operations"))?;

        let nonce_manager = self
            .nonce_manager
            .as_ref()
            .ok_or_else(|| Error::auth("nonce manager missing"))?;

        let signer_id = self.signer_id()?;
        let time_nonce = nonce_manager.next(signer_id.clone())?;
        nonce_manager.validate_local(signer_id, time_nonce)?;

        let action_value = serde_json::to_value(action)
            .context("serialize exchange action")
            .map_err(|e| Error::bad_request(e.to_string()))?;

        let sig = signer
            .sign(&SignRequest {
                action: action_value.clone(),
                time_nonce,
                action_type: HyperliquidActionType::UserSigned,
            })?
            .signature;

        let request = if let Some(vault) = self.vault_address {
            HyperliquidExchangeRequest::with_vault(
                action.clone(),
                time_nonce.as_millis() as u64,
                sig,
                vault.to_string(),
            )
        } else {
            HyperliquidExchangeRequest::new(action.clone(), time_nonce.as_millis() as u64, sig)
        };

        let url = &self.base_exchange;
        let body = serde_json::to_string(&request).map_err(Error::Serde)?;
        let body_bytes = body.into_bytes();

        let response = self
            .client
            .request(
                Method::POST,
                url.clone(),
                None,
                Some(body_bytes),
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)?;

        if response.status.is_success() {
            serde_json::from_slice(&response.body).map_err(Error::Serde)
        } else {
            let error_body = String::from_utf8_lossy(&response.body);
            Err(Error::http(
                response.status.as_u16(),
                error_body.to_string(),
            ))
        }
    }

    // ---------------- INTERNALS -----------------------------------------------

    fn signer_id(&self) -> Result<SignerId> {
        Ok(SignerId("hyperliquid:default".into()))
    }
}

#[cfg(test)]
mod tests {
    use crate::http::query::InfoRequest;

    #[test]
    fn stable_json_roundtrips() {
        let v = serde_json::json!({"type":"l2Book","coin":"BTC"});
        let s = serde_json::to_string(&v).unwrap();
        // Parse back to ensure JSON structure is correct, regardless of field order
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed["type"], "l2Book");
        assert_eq!(parsed["coin"], "BTC");
        assert_eq!(parsed, v);
    }

    #[test]
    fn info_pretty_shape() {
        let r = InfoRequest::l2_book("BTC");
        let val = serde_json::to_value(&r).unwrap();
        let pretty = serde_json::to_string_pretty(&val).unwrap();
        assert!(pretty.contains("\"type\": \"l2Book\""));
        assert!(pretty.contains("\"coin\": \"BTC\""));
    }
}
