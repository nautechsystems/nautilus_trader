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

//! CoinAPI API key resolution and validation.
//!
//! REST and WebSocket requests send the same key in the `X-CoinAPI-Key` header.

use nautilus_core::string::secret::SecretString;

/// Environment variable read when no API key is configured.
pub const COINAPI_API_KEY: &str = "COINAPI_API_KEY";

/// Resolves the API key from `api_key`, or from `COINAPI_API_KEY` when it is `None`.
///
/// # Errors
///
/// Returns an error if neither source provides a key, or the key is not non-empty visible ASCII.
pub fn resolve_api_key(api_key: Option<SecretString>) -> anyhow::Result<SecretString> {
    let api_key = match api_key {
        Some(api_key) => api_key,
        None => std::env::var(COINAPI_API_KEY)
            .map(SecretString::from)
            .map_err(|_| {
                anyhow::anyhow!("CoinAPI API key not configured; set api_key or {COINAPI_API_KEY}")
            })?,
    };

    anyhow::ensure!(
        is_valid_api_key(api_key.expose_secret()),
        "CoinAPI API key must contain visible ASCII characters"
    );
    Ok(api_key)
}

/// Returns whether an API key is non-empty visible ASCII, as HTTP headers require.
#[must_use]
pub fn is_valid_api_key(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|b| (33..=126).contains(&b))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn resolve_prefers_explicit_key() {
        let api_key = resolve_api_key(Some(SecretString::from("explicit-key"))).unwrap();

        assert_eq!(api_key.expose_secret(), "explicit-key");
    }

    #[rstest]
    #[case("")]
    #[case("has space")]
    #[case("tab\tkey")]
    #[case("key\u{e9}")]
    fn resolve_rejects_invalid_explicit_key(#[case] api_key: &str) {
        let error = resolve_api_key(Some(SecretString::from(api_key))).unwrap_err();

        assert_eq!(
            error.to_string(),
            "CoinAPI API key must contain visible ASCII characters"
        );
    }
}
