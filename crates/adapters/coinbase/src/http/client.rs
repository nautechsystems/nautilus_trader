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

//! HTTP client implementation for Coinbase Advanced Trade API.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use aws_lc_rs::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT},
    Method,
};
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::{debug, error};

use crate::{
    common::{get_user_agent, API_VERSION, BASE_URL_PROD},
    config::CoinbaseHttpConfig,
    types::*,
};

/// Convert DER-encoded ECDSA signature to raw R||S format for JWT
/// DER format: 0x30 [total-length] 0x02 [r-length] [r-bytes] 0x02 [s-length] [s-bytes]
/// Raw format: [r-bytes-padded-to-32] [s-bytes-padded-to-32]
pub fn der_to_raw_signature(der: &[u8]) -> Result<Vec<u8>> {
    if der.len() < 8 {
        anyhow::bail!("DER signature too short");
    }

    if der[0] != 0x30 {
        anyhow::bail!("Invalid DER signature: missing SEQUENCE tag");
    }

    let mut offset = 2; // Skip SEQUENCE tag and length

    // Parse R value
    if der[offset] != 0x02 {
        anyhow::bail!("Invalid DER signature: missing INTEGER tag for R");
    }
    offset += 1;

    let r_len = der[offset] as usize;
    offset += 1;

    let r_bytes = &der[offset..offset + r_len];
    offset += r_len;

    // Parse S value
    if der[offset] != 0x02 {
        anyhow::bail!("Invalid DER signature: missing INTEGER tag for S");
    }
    offset += 1;

    let s_len = der[offset] as usize;
    offset += 1;

    let s_bytes = &der[offset..offset + s_len];

    // Remove leading zero bytes (used for sign bit in DER)
    let r_trimmed = if r_bytes[0] == 0 && r_bytes.len() > 32 {
        &r_bytes[1..]
    } else {
        r_bytes
    };

    let s_trimmed = if s_bytes[0] == 0 && s_bytes.len() > 32 {
        &s_bytes[1..]
    } else {
        s_bytes
    };

    // Pad to 32 bytes if needed (for P-256)
    let mut raw = vec![0u8; 64];
    let r_start = 32 - r_trimmed.len();
    let s_start = 64 - s_trimmed.len();

    raw[r_start..32].copy_from_slice(r_trimmed);
    raw[s_start..64].copy_from_slice(s_trimmed);

    Ok(raw)
}

/// HTTP client for Coinbase Advanced Trade API
#[derive(Debug, Clone)]
pub struct CoinbaseHttpClient {
    base_url: String,
    api_key: String,
    api_secret: String,
    client: reqwest::Client,
}

impl CoinbaseHttpClient {
    /// Create a new HTTP client
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created
    pub fn new(config: CoinbaseHttpConfig) -> Result<Self> {
        let base_url = config.base_url.unwrap_or_else(|| BASE_URL_PROD.to_string());
        let timeout = std::time::Duration::from_secs(config.timeout_secs.unwrap_or(30));

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            base_url,
            api_key: config.api_key,
            api_secret: config.api_secret,
            client,
        })
    }

    /// Generate JWT for authentication using custom EC signing
    fn generate_jwt(&self, method: &str, path: &str) -> Result<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs();

        // Generate a random nonce (required by Coinbase)
        let mut nonce_bytes = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
        let nonce = nonce_bytes.iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        // Create JWT header with nonce
        let header = json!({
            "alg": "ES256",
            "kid": self.api_key,
            "nonce": nonce,
            "typ": "JWT"
        });

        // Create JWT claims
        // IMPORTANT: The uri field must include the hostname: "METHOD api.coinbase.com/path"
        let claims = json!({
            "sub": self.api_key,
            "iss": "cdp",
            "nbf": now,
            "exp": now + 120,
            "uri": format!("{} api.coinbase.com{}", method, path)
        });

        // Encode header and claims to base64url
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header)?.as_bytes());
        let claims_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims)?.as_bytes());

        // Create the message to sign
        let message = format!("{}.{}", header_b64, claims_b64);

        // Parse the EC private key from PEM format
        let key_with_newlines = self.api_secret.replace("\\n", "\n");

        debug!("Parsing EC private key (length: {} bytes)", key_with_newlines.len());

        // Parse the EC private key - handle both SEC1 and PKCS#8 formats
        let pkcs8_der = if key_with_newlines.contains("BEGIN EC PRIVATE KEY") {
            // SEC1 format - need to convert to PKCS#8
            debug!("Detected SEC1 format EC key, converting to PKCS#8");

            use pkcs8::{PrivateKeyInfo, AlgorithmIdentifierRef, ObjectIdentifier};
            use pkcs8::der::Encode;

            // Parse PEM to get SEC1 DER bytes
            let pem_data = pem::parse(key_with_newlines.as_bytes())
                .context("Failed to parse SEC1 PEM")?;

            debug!("SEC1 DER length: {} bytes", pem_data.contents().len());

            // OID for EC public key (1.2.840.10045.2.1)
            const EC_PUBLIC_KEY_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
            // OID for secp256r1/prime256v1 curve (1.2.840.10045.3.1.7)
            const SECP256R1_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");

            // Create PKCS#8 PrivateKeyInfo structure
            let algorithm = AlgorithmIdentifierRef {
                oid: EC_PUBLIC_KEY_OID,
                parameters: Some((&SECP256R1_OID).into()),
            };

            let private_key_info = PrivateKeyInfo {
                algorithm,
                private_key: pem_data.contents(),
                public_key: None,
            };

            // Encode to DER
            private_key_info.to_der()
                .context("Failed to encode PKCS#8 DER")?
        } else {
            // Already in PKCS#8 format
            debug!("Detected PKCS#8 format key");
            let pem_data = pem::parse(key_with_newlines.as_bytes())
                .context("Failed to parse PEM format")?;
            pem_data.contents().to_vec()
        };

        debug!("PKCS#8 DER length: {} bytes", pkcs8_der.len());

        // Create ECDSA key pair from PKCS#8 DER
        let key_pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &pkcs8_der)
            .context("Failed to create ECDSA key pair from PKCS#8")?;

        // Sign the message
        let rng = aws_lc_rs::rand::SystemRandom::new();
        let signature = key_pair.sign(&rng, message.as_bytes())
            .context("Failed to sign JWT")?;

        // The signature from aws-lc-rs is in ASN.1 DER format
        // JWT requires raw R||S format (64 bytes for P-256: 32 bytes R + 32 bytes S)
        // We need to convert from DER to raw format
        let signature_bytes = signature.as_ref();
        debug!("Signature DER length: {} bytes", signature_bytes.len());

        // Parse DER signature and extract R and S values
        let raw_signature = der_to_raw_signature(signature_bytes)
            .context("Failed to convert DER signature to raw format")?;

        debug!("Signature raw length: {} bytes", raw_signature.len());

        // Encode signature to base64url
        let signature_b64 = URL_SAFE_NO_PAD.encode(&raw_signature);

        // Combine to create final JWT
        let jwt = format!("{}.{}", message, signature_b64);

        debug!("Successfully generated JWT (length: {} bytes)", jwt.len());
        debug!("JWT header (decoded): {}", serde_json::to_string(&header)?);
        debug!("JWT claims (decoded): {}", serde_json::to_string(&claims)?);
        debug!("Message to sign: {}", message);
        debug!("Full JWT: {}...", &jwt[..std::cmp::min(150, jwt.len())]);

        Ok(jwt)
    }

    /// Build authenticated headers
    fn build_headers(&self, method: &str, path: &str, _body: &str) -> Result<HeaderMap> {
        // Generate JWT token
        let jwt = self.generate_jwt(method, path)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&get_user_agent()).context("Invalid user agent")?,
        );
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", jwt)).context("Invalid JWT")?,
        );

        Ok(headers)
    }

    /// Make an authenticated request
    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<&str>,
    ) -> Result<T> {
        let path = format!("/api/{API_VERSION}/{endpoint}");
        let url = format!("{}{}", self.base_url, path);
        let body_str = body.unwrap_or("");

        let headers = self.build_headers(method.as_str(), &path, body_str)?;

        debug!("Making request: {} {}", method, url);

        let mut request = self.client.request(method, &url).headers(headers.clone());

        if let Some(body_content) = body {
            request = request.body(body_content.to_string());
        }

        let response = request.send().await.context("Failed to send request")?;
        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Request failed with status {}: {}", status, error_text);
            error!("Request URL: {}", url);
            error!("Request headers: {:?}", headers);
            anyhow::bail!("Request failed with status {}: {}", status, error_text);
        }

        let response_text = response.text().await.context("Failed to read response")?;
        debug!("Response: {}", response_text);

        serde_json::from_str(&response_text)
            .with_context(|| format!("Failed to parse response: {}", response_text))
    }

    /// List all products (trading pairs)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed
    pub async fn list_products(&self) -> Result<ListProductsResponse> {
        self.request(Method::GET, "brokerage/products", None).await
    }

    /// Get a specific product
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed
    pub async fn get_product(&self, product_id: &str) -> Result<Product> {
        let endpoint = format!("brokerage/products/{product_id}");
        self.request(Method::GET, &endpoint, None).await
    }

    /// List accounts
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed
    pub async fn list_accounts(&self) -> Result<ListAccountsResponse> {
        self.request(Method::GET, "brokerage/accounts", None).await
    }

    /// Get a specific account
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed
    pub async fn get_account(&self, account_uuid: &str) -> Result<Account> {
        let endpoint = format!("brokerage/accounts/{account_uuid}");
        self.request(Method::GET, &endpoint, None).await
    }

    /// Create an order
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed
    pub async fn create_order(&self, request: &CreateOrderRequest) -> Result<CreateOrderResponse> {
        let body = serde_json::to_string(request).context("Failed to serialize order request")?;
        self.request(Method::POST, "brokerage/orders", Some(&body))
            .await
    }

    /// Cancel orders
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed
    pub async fn cancel_orders(&self, order_ids: &[String]) -> Result<CancelOrdersResponse> {
        let body = serde_json::json!({
            "order_ids": order_ids
        });
        let body_str = serde_json::to_string(&body).context("Failed to serialize cancel request")?;
        self.request(Method::POST, "brokerage/orders/batch_cancel", Some(&body_str))
            .await
    }

    /// Get order by order ID
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed
    pub async fn get_order(&self, order_id: &str) -> Result<Order> {
        let endpoint = format!("brokerage/orders/historical/{order_id}");
        #[derive(serde::Deserialize)]
        struct OrderResponse {
            order: Order,
        }
        let response: OrderResponse = self.request(Method::GET, &endpoint, None).await?;
        Ok(response.order)
    }

    /// List orders
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed
    pub async fn list_orders(&self, product_id: Option<&str>) -> Result<Vec<Order>> {
        let mut endpoint = "brokerage/orders/historical/batch".to_string();
        if let Some(pid) = product_id {
            endpoint.push_str(&format!("?product_id={pid}"));
        }

        #[derive(serde::Deserialize)]
        struct OrdersResponse {
            orders: Vec<Order>,
        }

        let response: OrdersResponse = self.request(Method::GET, &endpoint, None).await?;
        Ok(response.orders)
    }

    /// Edit an order
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed
    pub async fn edit_order(&self, request: &EditOrderRequest) -> Result<EditOrderResponse> {
        let body = serde_json::to_string(request).context("Failed to serialize edit order request")?;
        self.request(Method::POST, "brokerage/orders/edit", Some(&body))
            .await
    }

    /// Preview an order before placing it
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed
    pub async fn preview_order(&self, request: &PreviewOrderRequest) -> Result<PreviewOrderResponse> {
        let body = serde_json::to_string(request).context("Failed to serialize preview order request")?;
        self.request(Method::POST, "brokerage/orders/preview", Some(&body))
            .await
    }

    /// Close a position (market sell all holdings)
    ///
    /// # Arguments
    /// * `client_order_id` - Client-specified order ID
    /// * `product_id` - Product ID (e.g., "BTC-USD")
    /// * `size` - Optional size to close (if None, closes entire position)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response cannot be parsed
    pub async fn close_position(
        &self,
        client_order_id: &str,
        product_id: &str,
        size: Option<&str>,
    ) -> Result<CreateOrderResponse> {
        let mut body = serde_json::json!({
            "client_order_id": client_order_id,
            "product_id": product_id,
        });

        if let Some(s) = size {
            body["size"] = serde_json::json!(s);
        }

        let body_str = serde_json::to_string(&body).context("Failed to serialize close position request")?;
        self.request(Method::POST, "brokerage/orders/close_position", Some(&body_str))
            .await
    }

    /// Get product candles (OHLCV data) - PUBLIC endpoint (no auth required)
    ///
    /// # Arguments
    /// * `product_id` - Product ID (e.g., "BTC-USD")
    /// * `granularity` - Time slice in seconds (60, 300, 900, 3600, 21600, 86400)
    /// * `start` - Optional start time (Unix timestamp)
    /// * `end` - Optional end time (Unix timestamp)
    pub async fn get_candles(
        &self,
        product_id: &str,
        granularity: u32,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<GetCandlesResponse> {
        let mut endpoint = format!("market/products/{}/candles", product_id);
        let mut params = vec![format!("granularity={}", granularity)];

        if let Some(s) = start {
            params.push(format!("start={}", s));
        }
        if let Some(e) = end {
            params.push(format!("end={}", e));
        }

        if !params.is_empty() {
            endpoint.push_str("?");
            endpoint.push_str(&params.join("&"));
        }

        self.request_public(Method::GET, &endpoint).await
    }

    /// Get market trades for a product - PUBLIC endpoint (no auth required)
    ///
    /// # Arguments
    /// * `product_id` - Product ID (e.g., "BTC-USD")
    /// * `limit` - Optional limit (default 100, max 1000)
    pub async fn get_market_trades(
        &self,
        product_id: &str,
        limit: Option<u32>,
    ) -> Result<GetMarketTradesResponse> {
        let mut endpoint = format!("market/products/{}/ticker", product_id);

        if let Some(lim) = limit {
            endpoint.push_str(&format!("?limit={}", lim));
        }

        self.request_public(Method::GET, &endpoint).await
    }

    /// Get product book (order book) - PUBLIC endpoint (no auth required)
    ///
    /// # Arguments
    /// * `product_id` - Product ID (e.g., "BTC-USD")
    /// * `limit` - Optional limit for bid/ask levels (default 50, max 1000)
    pub async fn get_product_book(
        &self,
        product_id: &str,
        limit: Option<u32>,
    ) -> Result<GetProductBookResponse> {
        let mut endpoint = "market/product_book".to_string();
        let mut params = vec![format!("product_id={}", product_id)];

        if let Some(lim) = limit {
            params.push(format!("limit={}", lim));
        }

        endpoint.push_str("?");
        endpoint.push_str(&params.join("&"));

        self.request_public(Method::GET, &endpoint).await
    }

    /// Get best bid/ask for one or more products
    ///
    /// # Arguments
    /// * `product_ids` - List of product IDs (e.g., ["BTC-USD", "ETH-USD"])
    pub async fn get_best_bid_ask(
        &self,
        product_ids: &[&str],
    ) -> Result<GetBestBidAskResponse> {
        let mut endpoint = "brokerage/best_bid_ask?".to_string();

        for (i, pid) in product_ids.iter().enumerate() {
            if i > 0 {
                endpoint.push('&');
            }
            endpoint.push_str(&format!("product_ids={}", pid));
        }

        self.request(Method::GET, &endpoint, None).await
    }

    /// Make a public API request (no authentication required)
    async fn request_public<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
    ) -> Result<T> {
        let url = format!("{}/api/v3/{}", self.base_url, endpoint);

        debug!("Making public request: {} {}", method, url);

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_str(&get_user_agent())?);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let response = self
            .client
            .request(method.clone(), &url)
            .headers(headers)
            .send()
            .await
            .context("Failed to send request")?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            error!("Request failed with status {}: {}", status, error_text);
            error!("Request URL: {}", url);
            return Err(anyhow::anyhow!("Request failed with status {}: {}", status, error_text));
        }

        let body = response.text().await.context("Failed to read response body")?;
        debug!("Response body: {}", body);

        serde_json::from_str(&body).context("Failed to parse response JSON")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let config = CoinbaseHttpConfig::new(
            "test_key".to_string(),
            "test_secret".to_string(),
        );
        let client = CoinbaseHttpClient::new(config);
        assert!(client.is_ok());
    }
}

