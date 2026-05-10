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

//! API credential utilities for signing BitMEX requests.

#![allow(unused_assignments)] // Fields are accessed externally, false positive from nightly

use std::fmt::Debug;

use aws_lc_rs::hmac;
use nautilus_core::{
    env::resolve_env_var_pair,
    string::{REDACTED, mask_api_key},
};
use zeroize::ZeroizeOnDrop;

/// Returns the environment variable names for API credentials,
/// based on the network.
#[must_use]
pub fn credential_env_vars(testnet: bool) -> (&'static str, &'static str) {
    if testnet {
        ("BITMEX_TESTNET_API_KEY", "BITMEX_TESTNET_API_SECRET")
    } else {
        ("BITMEX_API_KEY", "BITMEX_API_SECRET")
    }
}

/// BitMEX API credentials for signing requests.
///
/// Uses HMAC SHA256 for request signing as per BitMEX API specifications.
/// Secrets are automatically zeroized on drop for security.
#[derive(Clone, ZeroizeOnDrop)]
pub struct Credential {
    api_key: Box<str>,
    api_secret: Box<[u8]>,
}

impl Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credential))
            .field("api_key", &self.api_key)
            .field("api_secret", &REDACTED)
            .finish()
    }
}

impl Credential {
    /// Creates a new [`Credential`] instance.
    #[must_use]
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self {
            api_key: api_key.into_boxed_str(),
            api_secret: api_secret.into_bytes().into_boxed_slice(),
        }
    }

    /// Resolves credentials from provided values or environment variables.
    #[must_use]
    pub fn resolve(
        api_key: Option<String>,
        api_secret: Option<String>,
        testnet: bool,
    ) -> Option<Self> {
        let (key_var, secret_var) = credential_env_vars(testnet);
        let (k, s) = resolve_env_var_pair(api_key, api_secret, key_var, secret_var)?;
        Some(Self::new(k, s))
    }

    /// Returns the API key.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Signs a request message according to the BitMEX authentication scheme.
    #[must_use]
    pub fn sign(&self, verb: &str, endpoint: &str, expires: i64, data: &str) -> String {
        let sign_message = format!("{verb}{endpoint}{expires}{data}");
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.api_secret[..]);
        let signature = hmac::sign(&key, sign_message.as_bytes());
        hex::encode(signature.as_ref())
    }

    /// Returns a masked version of the API key for logging purposes.
    ///
    /// Shows first 4 and last 4 characters with ellipsis in between.
    /// For keys shorter than 8 characters, shows asterisks only.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        mask_api_key(&self.api_key)
    }
}

/// Tests use examples from <https://www.bitmex.com/app/apiKeysUsage>.
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    const API_KEY: &str = "LAqUlngMIQkIUjXMUreyu3qn";
    const API_SECRET: &str = "chNOOS4KvNXR_Xq4k4c9qsfoKWvnDecLATCRlcBwyKDYnWgO";

    #[rstest]
    fn test_simple_get() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string());

        let signature = credential.sign("GET", "/api/v1/instrument", 1518064236, "");

        assert_eq!(
            signature,
            "c7682d435d0cfe87c16098df34ef2eb5a549d4c5a3c2b1f0f77b8af73423bf00"
        );
    }

    #[rstest]
    fn test_get_with_query() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string());

        let signature = credential.sign(
            "GET",
            "/api/v1/instrument?filter=%7B%22symbol%22%3A+%22XBTM15%22%7D",
            1518064237,
            "",
        );

        assert_eq!(
            signature,
            "e2f422547eecb5b3cb29ade2127e21b858b235b386bfa45e1c1756eb3383919f"
        );
    }

    #[rstest]
    fn test_post_with_data() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string());

        let data = load_test_json("credential_post_order.json");

        let signature = credential.sign("POST", "/api/v1/order", 1518064238, data.trim_end());

        assert_eq!(
            signature,
            "1749cd2ccae4aa49048ae09f0b95110cee706e0944e6a14ad0b3a8cb45bd336b"
        );
    }

    #[rstest]
    fn test_debug_redacts_secret() {
        let credential = Credential::new(API_KEY.to_string(), API_SECRET.to_string());
        let dbg_out = format!("{credential:?}");
        assert!(dbg_out.contains("api_secret: \"<redacted>\""));
        assert!(!dbg_out.contains("chNOO"));
        let secret_bytes_dbg = format!("{:?}", API_SECRET.as_bytes());
        assert!(
            !dbg_out.contains(&secret_bytes_dbg),
            "Debug output must not contain raw secret bytes"
        );
    }

    #[rstest]
    fn test_resolve_with_both_args() {
        let result = Credential::resolve(
            Some("my_key".to_string()),
            Some("my_secret".to_string()),
            false,
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().api_key(), "my_key");
    }

    #[rstest]
    fn test_resolve_with_no_args_no_env() {
        let (key_var, secret_var) = credential_env_vars(false);
        if std::env::var(key_var).is_ok() || std::env::var(secret_var).is_ok() {
            return;
        }

        let result = Credential::resolve(None, None, false);

        assert!(result.is_none());
    }
}
