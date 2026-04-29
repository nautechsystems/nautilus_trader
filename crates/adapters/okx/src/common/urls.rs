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

//! URL helpers and endpoint metadata for OKX services.

use crate::common::enums::OKXEnvironment;

const OKX_HTTP_URL: &str = "https://www.okx.com";
const OKX_WS_PUBLIC_URL: &str = "wss://ws.okx.com:8443/ws/v5/public";
const OKX_WS_PRIVATE_URL: &str = "wss://ws.okx.com:8443/ws/v5/private";
const OKX_WS_BUSINESS_URL: &str = "wss://ws.okx.com:8443/ws/v5/business";
const OKX_DEMO_WS_PUBLIC_URL: &str = "wss://wspap.okx.com:8443/ws/v5/public";
const OKX_DEMO_WS_PRIVATE_URL: &str = "wss://wspap.okx.com:8443/ws/v5/private";
const OKX_DEMO_WS_BUSINESS_URL: &str = "wss://wspap.okx.com:8443/ws/v5/business";

/// OKX endpoint types for determining URL and authentication requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "python", pyo3::pyclass(from_py_object))]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.okx")
)]
pub enum OKXEndpointType {
    Public,
    Private,
    Business,
}

/// Checks if endpoint requires authentication.
pub fn requires_authentication(endpoint_type: OKXEndpointType) -> bool {
    matches!(
        endpoint_type,
        OKXEndpointType::Private | OKXEndpointType::Business
    )
}

/// Returns the HTTP base URL.
#[must_use]
pub const fn get_http_base_url() -> &'static str {
    OKX_HTTP_URL
}

/// Returns the WebSocket base URL for public data (market data).
#[must_use]
pub fn get_ws_base_url_public(environment: OKXEnvironment) -> &'static str {
    match environment {
        OKXEnvironment::Demo => OKX_DEMO_WS_PUBLIC_URL,
        OKXEnvironment::Live => OKX_WS_PUBLIC_URL,
    }
}

/// Returns the WebSocket base URL for private data (account/order management).
#[must_use]
pub fn get_ws_base_url_private(environment: OKXEnvironment) -> &'static str {
    match environment {
        OKXEnvironment::Demo => OKX_DEMO_WS_PRIVATE_URL,
        OKXEnvironment::Live => OKX_WS_PRIVATE_URL,
    }
}

/// Returns the WebSocket base URL for business data (bars/candlesticks).
#[must_use]
pub fn get_ws_base_url_business(environment: OKXEnvironment) -> &'static str {
    match environment {
        OKXEnvironment::Demo => OKX_DEMO_WS_BUSINESS_URL,
        OKXEnvironment::Live => OKX_WS_BUSINESS_URL,
    }
}

/// Derives a WebSocket URL for a given channel from a base URL.
///
/// Replaces the last path segment (`/public`, `/private`, or `/business`)
/// with the target channel. If no recognized segment is found, appends
/// `/{channel}` to the path.
#[must_use]
pub fn derive_ws_url(base_url: &str, channel: &str) -> String {
    let url = base_url.trim_end_matches('/');
    for suffix in ["/public", "/private", "/business"] {
        if let Some(base) = url.strip_suffix(suffix) {
            return format!("{base}/{channel}");
        }
    }
    format!("{url}/{channel}")
}

/// Returns WebSocket URL by endpoint type.
#[must_use]
pub fn get_ws_url(endpoint_type: OKXEndpointType, environment: OKXEnvironment) -> &'static str {
    match endpoint_type {
        OKXEndpointType::Public => get_ws_base_url_public(environment),
        OKXEndpointType::Private => get_ws_base_url_private(environment),
        OKXEndpointType::Business => get_ws_base_url_business(environment),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_endpoint_authentication() {
        assert!(!requires_authentication(OKXEndpointType::Public));
        assert!(requires_authentication(OKXEndpointType::Private));
        assert!(requires_authentication(OKXEndpointType::Business));
    }

    #[rstest]
    fn test_http_base_url() {
        assert_eq!(get_http_base_url(), OKX_HTTP_URL);
    }

    #[rstest]
    fn test_ws_urls_live() {
        assert_eq!(
            get_ws_base_url_public(OKXEnvironment::Live),
            OKX_WS_PUBLIC_URL
        );
        assert_eq!(
            get_ws_base_url_private(OKXEnvironment::Live),
            OKX_WS_PRIVATE_URL
        );
        assert_eq!(
            get_ws_base_url_business(OKXEnvironment::Live),
            OKX_WS_BUSINESS_URL
        );
    }

    #[rstest]
    fn test_ws_urls_demo() {
        assert_eq!(
            get_ws_base_url_public(OKXEnvironment::Demo),
            OKX_DEMO_WS_PUBLIC_URL
        );
        assert_eq!(
            get_ws_base_url_private(OKXEnvironment::Demo),
            OKX_DEMO_WS_PRIVATE_URL
        );
        assert_eq!(
            get_ws_base_url_business(OKXEnvironment::Demo),
            OKX_DEMO_WS_BUSINESS_URL
        );
    }

    #[rstest]
    #[case(
        "wss://ws.okx.com:8443/ws/v5/public",
        "business",
        "wss://ws.okx.com:8443/ws/v5/business"
    )]
    #[case(
        "wss://wseea.okx.com:8443/ws/v5/public",
        "private",
        "wss://wseea.okx.com:8443/ws/v5/private"
    )]
    #[case(
        "wss://wseea.okx.com:8443/ws/v5/private",
        "business",
        "wss://wseea.okx.com:8443/ws/v5/business"
    )]
    #[case(
        "wss://wseea.okx.com:8443/ws/v5/private/",
        "business",
        "wss://wseea.okx.com:8443/ws/v5/business"
    )]
    #[case(
        "wss://custom.proxy:8443/ws/v5",
        "business",
        "wss://custom.proxy:8443/ws/v5/business"
    )]
    fn test_derive_ws_url(#[case] base_url: &str, #[case] channel: &str, #[case] expected: &str) {
        assert_eq!(derive_ws_url(base_url, channel), expected);
    }

    #[rstest]
    fn test_get_ws_url_by_type() {
        assert_eq!(
            get_ws_url(OKXEndpointType::Public, OKXEnvironment::Live),
            get_ws_base_url_public(OKXEnvironment::Live)
        );
        assert_eq!(
            get_ws_url(OKXEndpointType::Private, OKXEnvironment::Live),
            get_ws_base_url_private(OKXEnvironment::Live)
        );
        assert_eq!(
            get_ws_url(OKXEndpointType::Business, OKXEnvironment::Live),
            get_ws_base_url_business(OKXEnvironment::Live)
        );
    }
}
