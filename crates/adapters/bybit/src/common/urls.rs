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

//! Runtime resolution of Bybit REST and WebSocket base URLs.

use nautilus_core::string::secret::REDACTED;
use nautilus_network::url_origin;

use super::enums::{BybitEnvironment, BybitProductType};

const MIN_CONFIGURED_CREDENTIAL_LEN: usize = 8;
const MIN_SHAPED_LABEL_LEN: usize = 16;

const STREAM_MAINNET: &str = "stream";
const STREAM_TESTNET: &str = "stream-testnet";

/// Returns the base HTTP endpoint for the given environment.
#[must_use]
pub const fn bybit_http_base_url(environment: BybitEnvironment) -> &'static str {
    match environment {
        BybitEnvironment::Mainnet => "https://api.bybit.com",
        BybitEnvironment::Demo => "https://api-demo.bybit.com",
        BybitEnvironment::Testnet => "https://api-testnet.bybit.com",
    }
}

fn ws_public_subdomain(environment: BybitEnvironment) -> &'static str {
    match environment {
        BybitEnvironment::Mainnet => STREAM_MAINNET,
        // Bybit demo trading uses mainnet public market data streams
        BybitEnvironment::Demo => STREAM_MAINNET,
        BybitEnvironment::Testnet => STREAM_TESTNET,
    }
}

/// Builds the public WebSocket endpoint for the given product/environment pair.
#[must_use]
pub fn bybit_ws_public_url(
    product_type: BybitProductType,
    environment: BybitEnvironment,
) -> String {
    let subdomain = ws_public_subdomain(environment);
    format!(
        "wss://{subdomain}.bybit.com/v5/public/{}",
        product_type.as_str()
    )
}

/// Returns the private WebSocket endpoint for the given environment.
#[must_use]
pub const fn bybit_ws_private_url(environment: BybitEnvironment) -> &'static str {
    match environment {
        BybitEnvironment::Testnet => "wss://stream-testnet.bybit.com/v5/private",
        BybitEnvironment::Demo => "wss://stream-demo.bybit.com/v5/private",
        BybitEnvironment::Mainnet => "wss://stream.bybit.com/v5/private",
    }
}

/// Returns the trade WebSocket endpoint for order operations.
///
/// Note: Bybit demo environment does not support the WebSocket Trade API.
/// Demo trading must use HTTP REST API for order operations.
#[must_use]
pub const fn bybit_ws_trade_url(environment: BybitEnvironment) -> &'static str {
    match environment {
        BybitEnvironment::Testnet => "wss://stream-testnet.bybit.com/v5/trade",
        BybitEnvironment::Mainnet | BybitEnvironment::Demo => "wss://stream.bybit.com/v5/trade",
    }
}

/// Returns the public origin of `url` for diagnostics, or [`REDACTED`].
///
/// Uses [`url_origin`], then redacts the result when a host label is a configured
/// credential or has a credential-shaped form. Short configured values are ignored so
/// an empty or ordinary label such as `api` does not hide a public host.
#[must_use]
pub(crate) fn diagnostic_origin(
    url: &str,
    api_key: Option<&str>,
    api_secret: Option<&str>,
) -> String {
    let origin = url_origin(url);
    if origin == REDACTED || origin_host_is_secret(&origin, api_key, api_secret) {
        return REDACTED.to_string();
    }

    origin
}

fn origin_host_is_secret(origin: &str, api_key: Option<&str>, api_secret: Option<&str>) -> bool {
    let Some(host) = host_from_origin(origin) else {
        return true;
    };

    if credential_in_text(api_key, host) || credential_in_text(api_secret, host) {
        return true;
    }

    if host.contains(':') {
        return false;
    }

    host.split('.')
        .any(|label| label_is_secret(label, api_key, api_secret))
}

fn host_from_origin(origin: &str) -> Option<&str> {
    let rest = origin.split_once("://")?.1;
    if let Some(host) = rest.strip_prefix('[') {
        let end = host.find(']')?;
        return Some(&host[..end]);
    }

    Some(rest.split(':').next().unwrap_or(rest))
}

fn label_is_secret(label: &str, api_key: Option<&str>, api_secret: Option<&str>) -> bool {
    credential_in_text(api_key, label)
        || credential_in_text(api_secret, label)
        || label_is_credential_shaped(label)
}

fn credential_in_text(credential: Option<&str>, text: &str) -> bool {
    let Some(credential) = credential.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };

    if credential.len() < MIN_CONFIGURED_CREDENTIAL_LEN {
        return false;
    }

    contains_ignore_ascii_case(text, credential)
}

fn contains_ignore_ascii_case(text: &str, needle: &str) -> bool {
    let text = text.as_bytes();
    let needle = needle.as_bytes();
    if needle.len() > text.len() {
        return false;
    }

    text.windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

fn label_is_credential_shaped(label: &str) -> bool {
    if is_uuid(label) {
        return true;
    }

    if label.len() < MIN_SHAPED_LABEL_LEN {
        return false;
    }

    let shaped = label
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');

    if !shaped {
        return false;
    }

    let has_digit = label.bytes().any(|byte| byte.is_ascii_digit());
    let has_letter = label.bytes().any(|byte| byte.is_ascii_alphabetic());
    if has_digit && has_letter {
        return true;
    }

    if label.bytes().all(|byte| byte.is_ascii_hexdigit()) && label.len() >= 32 {
        return true;
    }

    label.len() >= 20 && label.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn is_uuid(label: &str) -> bool {
    let bytes = label.as_bytes();
    bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use nautilus_core::string::secret::REDACTED;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn resolves_public_urls() {
        assert_eq!(
            bybit_ws_public_url(BybitProductType::Linear, BybitEnvironment::Mainnet),
            "wss://stream.bybit.com/v5/public/linear"
        );
        assert_eq!(
            bybit_ws_public_url(BybitProductType::Option, BybitEnvironment::Demo),
            "wss://stream.bybit.com/v5/public/option"
        );
        assert_eq!(
            bybit_ws_public_url(BybitProductType::Inverse, BybitEnvironment::Testnet),
            "wss://stream-testnet.bybit.com/v5/public/inverse"
        );
    }

    #[rstest]
    fn resolves_private_urls() {
        assert_eq!(
            bybit_ws_private_url(BybitEnvironment::Mainnet),
            "wss://stream.bybit.com/v5/private"
        );
        assert_eq!(
            bybit_ws_private_url(BybitEnvironment::Demo),
            "wss://stream-demo.bybit.com/v5/private"
        );
        assert_eq!(
            bybit_ws_private_url(BybitEnvironment::Testnet),
            "wss://stream-testnet.bybit.com/v5/private"
        );
    }

    #[rstest]
    fn resolves_trade_urls() {
        assert_eq!(
            bybit_ws_trade_url(BybitEnvironment::Mainnet),
            "wss://stream.bybit.com/v5/trade"
        );
        assert_eq!(
            bybit_ws_trade_url(BybitEnvironment::Demo),
            "wss://stream.bybit.com/v5/trade"
        );
        assert_eq!(
            bybit_ws_trade_url(BybitEnvironment::Testnet),
            "wss://stream-testnet.bybit.com/v5/trade"
        );
    }

    #[rstest]
    fn resolves_http_urls() {
        assert_eq!(
            bybit_http_base_url(BybitEnvironment::Mainnet),
            "https://api.bybit.com"
        );
        assert_eq!(
            bybit_http_base_url(BybitEnvironment::Demo),
            "https://api-demo.bybit.com"
        );
        assert_eq!(
            bybit_http_base_url(BybitEnvironment::Testnet),
            "https://api-testnet.bybit.com"
        );
    }

    #[rstest]
    #[case(
        "https://user:password@api.bybit.com:8443/path-secret?token=query-secret#fragment-secret",
        None,
        None,
        "https://api.bybit.com:8443"
    )]
    #[case("http://127.0.0.1:9/path-secret", None, None, "http://127.0.0.1:9")]
    #[case(
        "https://my-gateway.example.com/v5",
        None,
        None,
        "https://my-gateway.example.com"
    )]
    #[case("file:///secret", None, None, REDACTED)]
    #[case("not a url", None, None, REDACTED)]
    #[case(
        "https://ab12cd34ef56gh78.example.com/path-secret",
        None,
        None,
        REDACTED
    )]
    #[case(
        "https://abcd-1234-efgh-5678.example.com/path-secret",
        None,
        None,
        REDACTED
    )]
    #[case(
        "https://AbCdEfGh12345678.example.com/path",
        Some("abcdefgh12345678"),
        None,
        REDACTED
    )]
    #[case(
        "https://SecretValue99.example.com/path",
        None,
        Some("secretvalue99"),
        REDACTED
    )]
    #[case(
        "https://gateway-SecretValue99.example.com/path",
        None,
        Some("secretvalue99"),
        REDACTED
    )]
    #[case(
        "https://api.bybit.com/path",
        Some("api"),
        Some(""),
        "https://api.bybit.com"
    )]
    fn diagnostic_origin_shows_public_origin_or_redacts_secret_host(
        #[case] url: &str,
        #[case] api_key: Option<&str>,
        #[case] api_secret: Option<&str>,
        #[case] expected: &str,
    ) {
        assert_eq!(diagnostic_origin(url, api_key, api_secret), expected);
    }
}
