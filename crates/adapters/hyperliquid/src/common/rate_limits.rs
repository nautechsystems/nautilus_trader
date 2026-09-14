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

use alloy_primitives::{B256, keccak256};
use url::Url;

use super::enums::HyperliquidEnvironment;

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct HyperliquidRouteScope {
    environment: HyperliquidEnvironment,
    endpoint_origin: String,
    proxy_url_digest: Option<B256>,
}

impl HyperliquidRouteScope {
    pub(crate) fn new(
        environment: HyperliquidEnvironment,
        endpoint_url: &str,
        proxy_url: Option<&str>,
    ) -> Self {
        let endpoint_origin = Url::parse(endpoint_url).map_or_else(
            |_| endpoint_url.trim_end_matches('/').to_string(),
            |url| url.origin().ascii_serialization(),
        );

        Self {
            environment,
            endpoint_origin,
            proxy_url_digest: proxy_url.map(keccak256),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn route_scope_normalizes_endpoint_paths() {
        let info = HyperliquidRouteScope::new(
            HyperliquidEnvironment::Mainnet,
            "https://api.hyperliquid.xyz/info",
            None,
        );
        let exchange = HyperliquidRouteScope::new(
            HyperliquidEnvironment::Mainnet,
            "https://api.hyperliquid.xyz/exchange",
            None,
        );

        assert!(info == exchange);
    }

    #[rstest]
    fn route_scope_separates_environment_and_proxy() {
        let mainnet = HyperliquidRouteScope::new(
            HyperliquidEnvironment::Mainnet,
            "https://api.example.com/info",
            None,
        );
        let testnet = HyperliquidRouteScope::new(
            HyperliquidEnvironment::Testnet,
            "https://api.example.com/info",
            None,
        );
        let proxied = HyperliquidRouteScope::new(
            HyperliquidEnvironment::Mainnet,
            "https://api.example.com/info",
            Some("http://proxy.example:8080"),
        );
        let matching_proxy = HyperliquidRouteScope::new(
            HyperliquidEnvironment::Mainnet,
            "https://api.example.com/info",
            Some("http://proxy.example:8080"),
        );
        let different_proxy = HyperliquidRouteScope::new(
            HyperliquidEnvironment::Mainnet,
            "https://api.example.com/info",
            Some("http://other.example:8080"),
        );

        assert!(mainnet != testnet);
        assert!(mainnet != proxied);
        assert!(proxied == matching_proxy);
        assert!(proxied != different_proxy);
    }
}
