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

//! OKX API credential storage and request signing helpers.

#![allow(unused_assignments)] // Fields are accessed externally, false positive from nightly

use std::fmt::Debug;

use aws_lc_rs::hmac;
use base64::prelude::*;
use nautilus_core::{env::get_or_env_var_opt, string::secret::REDACTED};
use zeroize::ZeroizeOnDrop;

/// Returns the environment variable names for API credentials.
#[must_use]
pub fn credential_env_vars() -> (&'static str, &'static str, &'static str) {
    ("OKX_API_KEY", "OKX_API_SECRET", "OKX_API_PASSPHRASE")
}

/// OKX API credentials for signing requests.
///
/// Uses HMAC SHA256 for request signing as per OKX API specifications.
/// Secrets are automatically zeroized on drop for security.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    api_key: Box<str>,
    api_passphrase: Box<str>,
    api_secret: Box<[u8]>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field("api_key", &self.api_key)
            .field("api_passphrase", &REDACTED)
            .field("api_secret", &REDACTED)
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] instance.
    #[must_use]
    pub fn new(api_key: String, api_secret: String, api_passphrase: String) -> Self {
        Self {
            api_key: api_key.into_boxed_str(),
            api_passphrase: api_passphrase.into_boxed_str(),
            api_secret: api_secret.into_bytes().into_boxed_slice(),
        }
    }

    /// Resolves credentials from provided values or environment variables.
    #[must_use]
    pub fn resolve(
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
    ) -> Option<Self> {
        let (key_var, secret_var, passphrase_var) = credential_env_vars();
        let key = get_or_env_var_opt(api_key, key_var);
        let secret = get_or_env_var_opt(api_secret, secret_var);
        let passphrase = get_or_env_var_opt(api_passphrase, passphrase_var);

        match (key, secret, passphrase) {
            (Some(k), Some(s), Some(p)) => Some(Self::new(k, s, p)),
            _ => None,
        }
    }

    /// Returns the API key.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Returns the API passphrase.
    #[must_use]
    pub fn api_passphrase(&self) -> &str {
        &self.api_passphrase
    }

    /// Signs a request message according to the OKX authentication scheme.
    ///
    /// This string-based variant is preserved for compatibility with callers
    /// that already have a UTF-8 body string. Prefer [`Self::sign_bytes`] when you
    /// have the original body bytes to avoid any possibility of encoding drift.
    pub fn sign(&self, timestamp: &str, method: &str, endpoint: &str, body: &str) -> String {
        self.sign_bytes(timestamp, method, endpoint, Some(body.as_bytes()))
    }

    /// Signs a request message using raw body bytes to avoid any UTF-8 conversion
    /// or re-serialization differences between the signed content and the bytes sent.
    pub fn sign_bytes(
        &self,
        timestamp: &str,
        method: &str,
        endpoint: &str,
        body: Option<&[u8]>,
    ) -> String {
        let mut message = Vec::with_capacity(
            timestamp.len() + method.len() + endpoint.len() + body.map_or(0, |b| b.len()),
        );
        message.extend_from_slice(timestamp.as_bytes());
        message.extend_from_slice(method.as_bytes());
        message.extend_from_slice(endpoint.as_bytes());

        if let Some(b) = body {
            message.extend_from_slice(b);
        }

        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret[..]);
        let tag = hmac::sign(&key, &message);
        BASE64_STANDARD.encode(tag.as_ref())
    }

    /// Returns a masked version of the API key for logging purposes.
    ///
    /// Shows first 4 and last 4 characters with ellipsis in between.
    /// For keys shorter than 8 characters, shows asterisks only.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        nautilus_core::string::secret::mask_api_key(&self.api_key)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const API_KEY: &str = "985d5b66-57ce-40fb-b714-afc0b9787083";
    const API_SECRET: &str = "chNOOS4KvNXR_Xq4k4c9qsfoKWvnDecLATCRlcBwyKDYnWgO";
    const API_PASSPHRASE: &str = "1234567890";

    #[rstest]
    fn test_simple_get() {
        let credential = Credential::new(
            API_KEY.to_string(),
            API_SECRET.to_string(),
            API_PASSPHRASE.to_string(),
        );

        let signature = credential.sign(
            "2020-12-08T09:08:57.715Z",
            "GET",
            "/api/v5/account/balance",
            "",
        );

        assert_eq!(signature, "PJ61e1nb2F2Qd7D8SPiaIcx2gjdELc+o0ygzre9z33k=");
    }

    #[rstest]
    fn test_get_with_query_params() {
        let credential = Credential::new(
            API_KEY.to_string(),
            API_SECRET.to_string(),
            API_PASSPHRASE.to_string(),
        );

        let signature = credential.sign(
            "2020-12-08T09:08:57.715Z",
            "GET",
            "/api/v5/account/balance?ccy=BTC",
            "",
        );

        assert!(!signature.is_empty());
        assert!(BASE64_STANDARD.decode(&signature).is_ok());

        // Verify the message is constructed correctly
        let expected_message = "2020-12-08T09:08:57.715ZGET/api/v5/account/balance?ccy=BTC";

        // Recreate signature to verify message construction
        let key = hmac::Key::new(hmac::HMAC_SHA256, API_SECRET.as_bytes());
        let tag = hmac::sign(&key, expected_message.as_bytes());
        let expected_signature = BASE64_STANDARD.encode(tag.as_ref());
        assert_eq!(signature, expected_signature);
    }

    #[rstest]
    fn test_post_with_json_body() {
        let credential = Credential::new(
            API_KEY.to_string(),
            API_SECRET.to_string(),
            API_PASSPHRASE.to_string(),
        );

        // Test with a simple JSON body
        let body = r#"{"instId":"BTC-USD-200925","tdMode":"isolated","side":"buy","ordType":"limit","px":"432.11","sz":"2"}"#;
        let signature = credential.sign(
            "2020-12-08T09:08:57.715Z",
            "POST",
            "/api/v5/trade/order",
            body,
        );

        assert!(!signature.is_empty());
        assert!(BASE64_STANDARD.decode(&signature).is_ok());
    }

    #[rstest]
    fn test_post_algo_order() {
        let credential = Credential::new(
            API_KEY.to_string(),
            API_SECRET.to_string(),
            API_PASSPHRASE.to_string(),
        );

        // Test with an algo order JSON body (array format as OKX expects)
        let body = r#"[{"instId":"ETH-USDT-SWAP","tdMode":"isolated","side":"buy","ordType":"trigger","sz":"0.01","triggerPx":"3000","orderPx":"-1","triggerPxType":"last"}]"#;
        let signature = credential.sign(
            "2025-01-20T10:30:45.123Z",
            "POST",
            "/api/v5/trade/order-algo",
            body,
        );

        assert!(!signature.is_empty());
        assert!(BASE64_STANDARD.decode(&signature).is_ok());

        // Verify the message is constructed correctly
        let expected_message =
            format!("2025-01-20T10:30:45.123ZPOST/api/v5/trade/order-algo{body}");

        // Recreate signature to verify message construction
        let key = hmac::Key::new(hmac::HMAC_SHA256, API_SECRET.as_bytes());
        let tag = hmac::sign(&key, expected_message.as_bytes());
        let expected_signature = BASE64_STANDARD.encode(tag.as_ref());
        assert_eq!(signature, expected_signature);
    }

    #[rstest]
    fn test_debug_redacts_secrets() {
        let credential = Credential::new(
            API_KEY.to_string(),
            API_SECRET.to_string(),
            API_PASSPHRASE.to_string(),
        );
        let dbg_out = format!("{credential:?}");
        assert!(dbg_out.contains("api_secret: \"<redacted>\""));
        assert!(dbg_out.contains("api_passphrase: \"<redacted>\""));
        assert!(!dbg_out.contains("chNOO"));
        assert!(
            !dbg_out.contains(API_PASSPHRASE),
            "Debug output must not contain passphrase"
        );
    }

    #[rstest]
    fn test_api_key_masked_short() {
        let credential = Credential::new(
            "short".to_string(),
            "secret".to_string(),
            "pass".to_string(),
        );
        assert_eq!(credential.api_key_masked(), "*****");
    }

    #[rstest]
    fn test_api_key_masked_long() {
        let credential = Credential::new(
            API_KEY.to_string(),
            API_SECRET.to_string(),
            API_PASSPHRASE.to_string(),
        );
        assert_eq!(credential.api_key_masked(), "985d...7083");
    }

    #[rstest]
    fn test_resolve_with_all_args() {
        let result = Credential::resolve(
            Some("my_key".to_string()),
            Some("my_secret".to_string()),
            Some("my_pass".to_string()),
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().api_key(), "my_key");
    }

    #[rstest]
    fn test_resolve_with_no_args_no_env() {
        let (key_var, secret_var, passphrase_var) = credential_env_vars();
        if std::env::var(key_var).is_ok()
            || std::env::var(secret_var).is_ok()
            || std::env::var(passphrase_var).is_ok()
        {
            return;
        }

        let result = Credential::resolve(None, None, None);

        assert!(result.is_none());
    }

    #[rstest]
    fn test_resolve_with_partial_args_returns_none() {
        let (_, _, passphrase_var) = credential_env_vars();
        if std::env::var(passphrase_var).is_ok() {
            return;
        }

        // Key and secret provided but passphrase missing (env var not set)
        let result = Credential::resolve(
            Some("my_key".to_string()),
            Some("my_secret".to_string()),
            None,
        );

        assert!(result.is_none());
    }
}
