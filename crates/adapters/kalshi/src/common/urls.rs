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

//! Base URLs and endpoint paths for the Kalshi Trade API.

/// Production REST base URL.
pub const PROD_REST_URL: &str = "https://external-api.kalshi.com/trade-api/v2";

/// Production shared REST base URL, also supported by the exchange.
pub const PROD_SHARED_REST_URL: &str = "https://api.elections.kalshi.com/trade-api/v2";

/// Demo REST base URL.
pub const DEMO_REST_URL: &str = "https://external-api.demo.kalshi.co/trade-api/v2";

/// Demo shared REST base URL, also supported by the exchange.
pub const DEMO_SHARED_REST_URL: &str = "https://demo-api.kalshi.co/trade-api/v2";

/// Path prefix of every Trade API route.
///
/// Request signatures cover the full path from the API root, including this prefix and excluding
/// any query parameters.
pub const API_PATH_PREFIX: &str = "/trade-api/v2";

/// Markets list endpoint.
pub const PATH_MARKETS: &str = "/markets";

/// Trades endpoint, unfiltered by market.
pub const PATH_TRADES: &str = "/markets/trades";

/// Events list endpoint.
pub const PATH_EVENTS: &str = "/events";

/// Exchange status endpoint.
pub const PATH_EXCHANGE_STATUS: &str = "/exchange/status";

/// Historical data cutoff endpoint.
pub const PATH_HISTORICAL_CUTOFF: &str = "/historical/cutoff";

/// Member balance endpoint.
pub const PATH_BALANCE: &str = "/portfolio/balance";

/// Member positions endpoint.
pub const PATH_POSITIONS: &str = "/portfolio/positions";

/// Member settlements endpoint.
pub const PATH_SETTLEMENTS: &str = "/portfolio/settlements";

/// Member fills endpoint.
pub const PATH_FILLS: &str = "/portfolio/fills";

/// Member orders endpoint.
pub const PATH_ORDERS: &str = "/portfolio/orders";

/// Member order entry endpoint for the version 2 order API.
///
/// Order creation, amendment, cancellation, and the cancel-all operation all live under this path;
/// the version 1 `/portfolio/orders` mutations are deprecated by the exchange.
pub const PATH_EVENT_ORDERS: &str = "/portfolio/events/orders";

/// Member order entry endpoint for submitting several orders in one request.
pub const PATH_EVENT_ORDERS_BATCHED: &str = "/portfolio/events/orders/batched";

/// Returns the market endpoint path for the given market `ticker`.
#[must_use]
pub fn market_path(ticker: &str) -> String {
    format!("{PATH_MARKETS}/{ticker}")
}

/// Returns the order book endpoint path for the given market `ticker`.
#[must_use]
pub fn market_orderbook_path(ticker: &str) -> String {
    format!("{PATH_MARKETS}/{ticker}/orderbook")
}

/// Returns the event endpoint path for the given `event_ticker`.
#[must_use]
pub fn event_path(event_ticker: &str) -> String {
    format!("{PATH_EVENTS}/{event_ticker}")
}

/// Returns the single order path for the given `order_id`.
#[must_use]
pub fn order_path(order_id: &str) -> String {
    format!("{PATH_ORDERS}/{order_id}")
}

/// Returns the amend endpoint path for the given `order_id`.
#[must_use]
pub fn amend_order_path(order_id: &str) -> String {
    format!("{PATH_EVENT_ORDERS}/{order_id}/amend")
}

/// Returns the decrease endpoint path for the given `order_id`.
#[must_use]
pub fn decrease_order_path(order_id: &str) -> String {
    format!("{PATH_EVENT_ORDERS}/{order_id}/decrease")
}

/// Returns the cancel endpoint path for the given `order_id`.
#[must_use]
pub fn cancel_order_path(order_id: &str) -> String {
    format!("{PATH_EVENT_ORDERS}/{order_id}")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_market_path() {
        assert_eq!(
            market_path("KXHIGHNY-25JAN01-T50"),
            "/markets/KXHIGHNY-25JAN01-T50"
        );
        assert_eq!(
            market_orderbook_path("KXHIGHNY-25JAN01-T50"),
            "/markets/KXHIGHNY-25JAN01-T50/orderbook"
        );
    }

    #[rstest]
    fn test_event_and_order_paths() {
        assert_eq!(event_path("KXHIGHNY-25JAN01"), "/events/KXHIGHNY-25JAN01");
        assert_eq!(order_path("abcd-1234"), "/portfolio/orders/abcd-1234");
        assert_eq!(
            cancel_order_path("abcd-1234"),
            "/portfolio/events/orders/abcd-1234"
        );
        assert_eq!(
            amend_order_path("abcd-1234"),
            "/portfolio/events/orders/abcd-1234/amend"
        );
        assert_eq!(
            decrease_order_path("abcd-1234"),
            "/portfolio/events/orders/abcd-1234/decrease"
        );
        assert_eq!(PATH_EVENT_ORDERS, "/portfolio/events/orders");
        assert_eq!(
            PATH_EVENT_ORDERS_BATCHED,
            "/portfolio/events/orders/batched"
        );
    }

    #[rstest]
    fn test_environment_base_urls_are_versioned_api_roots() {
        for url in [
            PROD_REST_URL,
            PROD_SHARED_REST_URL,
            DEMO_REST_URL,
            DEMO_SHARED_REST_URL,
        ] {
            assert!(url.starts_with("https://"), "{url}");
            assert!(url.ends_with(API_PATH_PREFIX), "{url}");
        }
    }
}
