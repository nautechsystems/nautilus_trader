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

use crate::common::enums::{OKXEnvironment, OKXRegion};

// Global region (accounts registered on www.okx.com).
const OKX_HTTP_URL: &str = "https://www.okx.com";
const OKX_WS_PUBLIC_URL: &str = "wss://ws.okx.com:8443/ws/v5/public";
const OKX_WS_PRIVATE_URL: &str = "wss://ws.okx.com:8443/ws/v5/private";
const OKX_WS_BUSINESS_URL: &str = "wss://ws.okx.com:8443/ws/v5/business";
const OKX_DEMO_WS_PUBLIC_URL: &str = "wss://wspap.okx.com:8443/ws/v5/public";
const OKX_DEMO_WS_PRIVATE_URL: &str = "wss://wspap.okx.com:8443/ws/v5/private";
const OKX_DEMO_WS_BUSINESS_URL: &str = "wss://wspap.okx.com:8443/ws/v5/business";

// EEA region (European Economic Area, accounts registered on my.okx.com).
const OKX_EEA_HTTP_URL: &str = "https://eea.okx.com";
const OKX_EEA_WS_PUBLIC_URL: &str = "wss://wseea.okx.com:8443/ws/v5/public";
const OKX_EEA_WS_PRIVATE_URL: &str = "wss://wseea.okx.com:8443/ws/v5/private";
const OKX_EEA_WS_BUSINESS_URL: &str = "wss://wseea.okx.com:8443/ws/v5/business";
const OKX_EEA_DEMO_WS_PUBLIC_URL: &str = "wss://wseeapap.okx.com:8443/ws/v5/public";
const OKX_EEA_DEMO_WS_PRIVATE_URL: &str = "wss://wseeapap.okx.com:8443/ws/v5/private";
const OKX_EEA_DEMO_WS_BUSINESS_URL: &str = "wss://wseeapap.okx.com:8443/ws/v5/business";

// US region (United States and Australia, accounts registered on app.okx.com).
const OKX_US_HTTP_URL: &str = "https://us.okx.com";
const OKX_US_WS_PUBLIC_URL: &str = "wss://wsus.okx.com:8443/ws/v5/public";
const OKX_US_WS_PRIVATE_URL: &str = "wss://wsus.okx.com:8443/ws/v5/private";
const OKX_US_WS_BUSINESS_URL: &str = "wss://wsus.okx.com:8443/ws/v5/business";
const OKX_US_DEMO_WS_PUBLIC_URL: &str = "wss://wsuspap.okx.com:8443/ws/v5/public";
const OKX_US_DEMO_WS_PRIVATE_URL: &str = "wss://wsuspap.okx.com:8443/ws/v5/private";
const OKX_US_DEMO_WS_BUSINESS_URL: &str = "wss://wsuspap.okx.com:8443/ws/v5/business";

/// OKX endpoint types for determining URL and authentication requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "python", pyo3::pyclass(from_py_object))]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.okx")
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

/// Returns the HTTP base URL for the given region.
///
/// The REST host is region-only; demo trading reuses the live host together
/// with the `x-simulated-trading` header.
#[must_use]
pub const fn get_http_base_url(region: OKXRegion) -> &'static str {
    match region {
        OKXRegion::Global => OKX_HTTP_URL,
        OKXRegion::Eea => OKX_EEA_HTTP_URL,
        OKXRegion::Us => OKX_US_HTTP_URL,
    }
}

/// Returns the WebSocket base URL for public data (market data).
#[must_use]
pub fn get_ws_base_url_public(region: OKXRegion, environment: OKXEnvironment) -> &'static str {
    match (region, environment) {
        (OKXRegion::Global, OKXEnvironment::Live) => OKX_WS_PUBLIC_URL,
        (OKXRegion::Global, OKXEnvironment::Demo) => OKX_DEMO_WS_PUBLIC_URL,
        (OKXRegion::Eea, OKXEnvironment::Live) => OKX_EEA_WS_PUBLIC_URL,
        (OKXRegion::Eea, OKXEnvironment::Demo) => OKX_EEA_DEMO_WS_PUBLIC_URL,
        (OKXRegion::Us, OKXEnvironment::Live) => OKX_US_WS_PUBLIC_URL,
        (OKXRegion::Us, OKXEnvironment::Demo) => OKX_US_DEMO_WS_PUBLIC_URL,
    }
}

/// Returns the WebSocket base URL for private data (account/order management).
#[must_use]
pub fn get_ws_base_url_private(region: OKXRegion, environment: OKXEnvironment) -> &'static str {
    match (region, environment) {
        (OKXRegion::Global, OKXEnvironment::Live) => OKX_WS_PRIVATE_URL,
        (OKXRegion::Global, OKXEnvironment::Demo) => OKX_DEMO_WS_PRIVATE_URL,
        (OKXRegion::Eea, OKXEnvironment::Live) => OKX_EEA_WS_PRIVATE_URL,
        (OKXRegion::Eea, OKXEnvironment::Demo) => OKX_EEA_DEMO_WS_PRIVATE_URL,
        (OKXRegion::Us, OKXEnvironment::Live) => OKX_US_WS_PRIVATE_URL,
        (OKXRegion::Us, OKXEnvironment::Demo) => OKX_US_DEMO_WS_PRIVATE_URL,
    }
}

/// Returns the WebSocket base URL for business data (bars/candlesticks).
#[must_use]
pub fn get_ws_base_url_business(region: OKXRegion, environment: OKXEnvironment) -> &'static str {
    match (region, environment) {
        (OKXRegion::Global, OKXEnvironment::Live) => OKX_WS_BUSINESS_URL,
        (OKXRegion::Global, OKXEnvironment::Demo) => OKX_DEMO_WS_BUSINESS_URL,
        (OKXRegion::Eea, OKXEnvironment::Live) => OKX_EEA_WS_BUSINESS_URL,
        (OKXRegion::Eea, OKXEnvironment::Demo) => OKX_EEA_DEMO_WS_BUSINESS_URL,
        (OKXRegion::Us, OKXEnvironment::Live) => OKX_US_WS_BUSINESS_URL,
        (OKXRegion::Us, OKXEnvironment::Demo) => OKX_US_DEMO_WS_BUSINESS_URL,
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
pub fn get_ws_url(
    endpoint_type: OKXEndpointType,
    region: OKXRegion,
    environment: OKXEnvironment,
) -> &'static str {
    match endpoint_type {
        OKXEndpointType::Public => get_ws_base_url_public(region, environment),
        OKXEndpointType::Private => get_ws_base_url_private(region, environment),
        OKXEndpointType::Business => get_ws_base_url_business(region, environment),
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
    #[case(OKXRegion::Global, "https://www.okx.com")]
    #[case(OKXRegion::Eea, "https://eea.okx.com")]
    #[case(OKXRegion::Us, "https://us.okx.com")]
    fn test_http_base_url(#[case] region: OKXRegion, #[case] expected: &str) {
        assert_eq!(get_http_base_url(region), expected);
    }

    #[rstest]
    #[case(
        OKXRegion::Global,
        "wss://ws.okx.com:8443/ws/v5/public",
        "wss://ws.okx.com:8443/ws/v5/private",
        "wss://ws.okx.com:8443/ws/v5/business"
    )]
    #[case(
        OKXRegion::Eea,
        "wss://wseea.okx.com:8443/ws/v5/public",
        "wss://wseea.okx.com:8443/ws/v5/private",
        "wss://wseea.okx.com:8443/ws/v5/business"
    )]
    #[case(
        OKXRegion::Us,
        "wss://wsus.okx.com:8443/ws/v5/public",
        "wss://wsus.okx.com:8443/ws/v5/private",
        "wss://wsus.okx.com:8443/ws/v5/business"
    )]
    fn test_ws_urls_live(
        #[case] region: OKXRegion,
        #[case] public: &str,
        #[case] private: &str,
        #[case] business: &str,
    ) {
        assert_eq!(get_ws_base_url_public(region, OKXEnvironment::Live), public);
        assert_eq!(
            get_ws_base_url_private(region, OKXEnvironment::Live),
            private
        );
        assert_eq!(
            get_ws_base_url_business(region, OKXEnvironment::Live),
            business
        );
    }

    #[rstest]
    #[case(
        OKXRegion::Global,
        "wss://wspap.okx.com:8443/ws/v5/public",
        "wss://wspap.okx.com:8443/ws/v5/private",
        "wss://wspap.okx.com:8443/ws/v5/business"
    )]
    #[case(
        OKXRegion::Eea,
        "wss://wseeapap.okx.com:8443/ws/v5/public",
        "wss://wseeapap.okx.com:8443/ws/v5/private",
        "wss://wseeapap.okx.com:8443/ws/v5/business"
    )]
    #[case(
        OKXRegion::Us,
        "wss://wsuspap.okx.com:8443/ws/v5/public",
        "wss://wsuspap.okx.com:8443/ws/v5/private",
        "wss://wsuspap.okx.com:8443/ws/v5/business"
    )]
    fn test_ws_urls_demo(
        #[case] region: OKXRegion,
        #[case] public: &str,
        #[case] private: &str,
        #[case] business: &str,
    ) {
        assert_eq!(get_ws_base_url_public(region, OKXEnvironment::Demo), public);
        assert_eq!(
            get_ws_base_url_private(region, OKXEnvironment::Demo),
            private
        );
        assert_eq!(
            get_ws_base_url_business(region, OKXEnvironment::Demo),
            business
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
    #[case(OKXRegion::Global)]
    #[case(OKXRegion::Eea)]
    #[case(OKXRegion::Us)]
    fn test_get_ws_url_by_type(#[case] region: OKXRegion) {
        assert_eq!(
            get_ws_url(OKXEndpointType::Public, region, OKXEnvironment::Live),
            get_ws_base_url_public(region, OKXEnvironment::Live)
        );
        assert_eq!(
            get_ws_url(OKXEndpointType::Private, region, OKXEnvironment::Live),
            get_ws_base_url_private(region, OKXEnvironment::Live)
        );
        assert_eq!(
            get_ws_url(OKXEndpointType::Business, region, OKXEnvironment::Live),
            get_ws_base_url_business(region, OKXEnvironment::Live)
        );
    }
}
