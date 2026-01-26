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

const OKX_HTTP_URL: &str = "https://www.okx.com";
const OKX_WS_PUBLIC_URL: &str = "wss://ws.okx.com:8443/ws/v5/public";
const OKX_WS_PRIVATE_URL: &str = "wss://ws.okx.com:8443/ws/v5/private";
const OKX_WS_BUSINESS_URL: &str = "wss://ws.okx.com:8443/ws/v5/business";
const OKX_DEMO_WS_PUBLIC_URL: &str = "wss://wspap.okx.com:8443/ws/v5/public";
const OKX_DEMO_WS_PRIVATE_URL: &str = "wss://wspap.okx.com:8443/ws/v5/private";
const OKX_DEMO_WS_BUSINESS_URL: &str = "wss://wspap.okx.com:8443/ws/v5/business";

/// OKX endpoint types for determining URL and authentication requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "python", pyo3::pyclass)]
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
pub const fn get_ws_base_url_public(is_demo: bool) -> &'static str {
    if is_demo {
        OKX_DEMO_WS_PUBLIC_URL
    } else {
        OKX_WS_PUBLIC_URL
    }
}

/// Returns the WebSocket base URL for private data (account/order management).
#[must_use]
pub const fn get_ws_base_url_private(is_demo: bool) -> &'static str {
    if is_demo {
        OKX_DEMO_WS_PRIVATE_URL
    } else {
        OKX_WS_PRIVATE_URL
    }
}

/// Returns the WebSocket base URL for business data (bars/candlesticks).
#[must_use]
pub const fn get_ws_base_url_business(is_demo: bool) -> &'static str {
    if is_demo {
        OKX_DEMO_WS_BUSINESS_URL
    } else {
        OKX_WS_BUSINESS_URL
    }
}

/// Returns WebSocket URL by endpoint type.
#[must_use]
pub const fn get_ws_url(endpoint_type: OKXEndpointType, is_demo: bool) -> &'static str {
    match endpoint_type {
        OKXEndpointType::Public => get_ws_base_url_public(is_demo),
        OKXEndpointType::Private => get_ws_base_url_private(is_demo),
        OKXEndpointType::Business => get_ws_base_url_business(is_demo),
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
    fn test_ws_urls_production() {
        assert_eq!(get_ws_base_url_public(false), OKX_WS_PUBLIC_URL);
        assert_eq!(get_ws_base_url_private(false), OKX_WS_PRIVATE_URL);
        assert_eq!(get_ws_base_url_business(false), OKX_WS_BUSINESS_URL);
    }

    #[rstest]
    fn test_ws_urls_demo() {
        assert_eq!(get_ws_base_url_public(true), OKX_DEMO_WS_PUBLIC_URL);
        assert_eq!(get_ws_base_url_private(true), OKX_DEMO_WS_PRIVATE_URL);
        assert_eq!(get_ws_base_url_business(true), OKX_DEMO_WS_BUSINESS_URL);
    }

    #[rstest]
    fn test_get_ws_url_by_type() {
        assert_eq!(
            get_ws_url(OKXEndpointType::Public, false),
            get_ws_base_url_public(false)
        );
        assert_eq!(
            get_ws_url(OKXEndpointType::Private, false),
            get_ws_base_url_private(false)
        );
        assert_eq!(
            get_ws_url(OKXEndpointType::Business, false),
            get_ws_base_url_business(false)
        );
    }
}
