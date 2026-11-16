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

//! Binance REST request weights, shared with depth snapshot pacing.
//!
//! Endpoint costs follow the Spot REST API and Futures market-data, trade, and account references:
//! <https://developers.binance.com/docs/binance-spot-api-docs/rest-api>
//! <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api>
//! COIN-M history costs follow the shared architecture migration:
//! <https://developers.binance.com/docs/derivatives/coin-margined-futures/Important-CM-UM-Integration-Notice>
//! PAPI costs follow the Portfolio Margin trade and account references:
//! <https://developers.binance.com/docs/derivatives/portfolio-margin/general-info>

use nautilus_network::http::Method;

pub(crate) fn request_weight_spot(method: &Method, path: &str, query: &str) -> u32 {
    let path = path.trim_start_matches('/');

    if *method != Method::GET {
        return 1;
    }

    let has_symbol = query_value(query, "symbol").is_some();
    match (path, has_symbol) {
        ("exchangeInfo" | "account" | "account/commission" | "allOrders", _) => 20,
        ("depth", _) => depth_weight_spot(query_limit(query, 100)),
        ("trades" | "historicalTrades", _) => 25,
        ("aggTrades" | "order", _) => 4,
        ("klines" | "avgPrice", _) => 2,
        ("ticker/24hr", _) => match symbol_count(query) {
            1..=20 => 2,
            21..=100 => 40,
            _ => 80,
        },
        ("ticker/price" | "ticker/bookTicker", true) => 2,
        ("ticker/price" | "ticker/bookTicker", false) => 4,
        ("openOrders", true) => 6,
        ("openOrders", false) => 80,
        ("myTrades", _) if query_value(query, "orderId").is_some() => 5,
        ("myTrades", _) => 20,
        _ => 1,
    }
}

pub(crate) fn request_weight_futures(
    method: &Method,
    path: &str,
    query: &str,
    coin_m: bool,
) -> u32 {
    let path = path.trim_start_matches('/');
    let path = path
        .strip_prefix("fapi/v1/")
        .or_else(|| path.strip_prefix("fapi/v2/"))
        .or_else(|| path.strip_prefix("dapi/v1/"))
        .unwrap_or(path);
    if *method != Method::GET {
        return match (method, path) {
            (&Method::POST, "order" | "algoOrder") => 0,
            (&Method::PUT, "order") if !coin_m => 0,
            (&Method::POST | &Method::PUT, "batchOrders") => 5,
            _ => 1,
        };
    }

    let has_symbol = query_value(query, "symbol").is_some();
    match (path, has_symbol) {
        ("depth", _) => depth_weight_futures(query_limit(query, 500)),
        ("klines", _) => match query_limit(query, 500) {
            ..=99 => 1,
            100..=499 => 2,
            500..=1000 => 5,
            _ => 10,
        },
        ("trades" | "account" | "allOrders" | "allAlgoOrders" | "userTrades", _) => 5,
        ("positionRisk", _) if coin_m => 1,
        ("positionRisk", _) => 5,
        ("aggTrades" | "commissionRate", _) => 20,
        ("positionSide/dual", _) => 30,
        ("ticker/24hr" | "openOrders" | "openAlgoOrders", true) => 1,
        ("ticker/24hr" | "openOrders" | "openAlgoOrders", false) => 40,
        ("ticker/price", true) => 1,
        ("ticker/price", false) => 2,
        ("ticker/bookTicker", true) => 2,
        ("ticker/bookTicker", false) => 5,
        ("premiumIndex", true) if !coin_m => 1,
        ("premiumIndex", _) => 10,
        ("fundingRate" | "fundingInfo" | "futures/data/openInterestHist", _) if !coin_m => 0,
        _ => 1,
    }
}

/// Strips the PAPI version prefix from an absolute or relative endpoint path, returning the
/// route that endpoint weights and order counts are matched against.
pub(crate) fn papi_route(path: &str) -> &str {
    let path = path.trim_start_matches('/');
    path.strip_prefix("papi/v1/")
        .or_else(|| path.strip_prefix("papi/v2/"))
        .unwrap_or(path)
}

/// Returns the IP request weight for a Portfolio Margin (PAPI) endpoint.
///
/// PAPI charges route-specific IP weight for mutating requests as well as reads, including
/// order placement and amendment, so every route is matched on both method and path.
/// Parameter-dependent costs follow the official catalog: `um/openOrders`, `cm/openOrders`,
/// `um/algo/openAlgoOrders`, `cm/conditional/allOrders`, and `cm/conditional/openOrders` cost 1
/// with a `symbol` and 40 without one, `um/apiTradingStatus` costs 1 with a `symbol` and 10 without, `um/forceOrders`
/// and `cm/forceOrders` cost 20 with a `symbol` and 50 without, and `cm/allOrders` and
/// `cm/userTrades` cost 20 with a `symbol` and 40 with a `pair`.
///
/// References:
/// - <https://developers.binance.com/docs/catalog/advanced-trading-derivatives-trading-portfolio-margin/api/rest-api/trade>
/// - <https://developers.binance.com/docs/catalog/advanced-trading-derivatives-trading-portfolio-margin/api/rest-api/account>
pub(crate) fn request_weight_papi(method: &Method, path: &str, query: &str) -> u32 {
    let path = papi_route(path);

    let has_symbol = query_value(query, "symbol").is_some();

    match (method, path) {
        (&Method::GET, "ping" | "rateLimit/order") => 1,
        (&Method::GET, "account" | "balance") => 20,
        (&Method::GET, "um/commissionRate" | "cm/commissionRate") => 20,
        (&Method::GET, "um/income" | "cm/income") => 30,
        (&Method::GET, "um/feeBurn" | "repay-futures-switch") => 30,
        (&Method::GET, "um/positionSide/dual" | "cm/positionSide/dual") => 30,
        (&Method::GET, "portfolio/interest-history") => 50,
        (&Method::GET, "portfolio/negative-balance-exchange-record") => 100,
        (&Method::GET, "um/order/asyn" | "um/trade/asyn" | "um/income/asyn") => 1_500,
        (&Method::GET, "um/order/asyn/id" | "um/trade/asyn/id" | "um/income/asyn/id") => 10,
        (
            &Method::GET,
            "um/account" | "um/accountConfig" | "um/symbolConfig" | "um/positionRisk",
        ) => 5,
        (&Method::GET, "um/adlQuantile" | "cm/adlQuantile" | "um/userTrades") => 5,
        (&Method::GET, "cm/account") => 5,
        (&Method::GET, "margin/allOrderList" | "margin/allOrders") => 100,
        (&Method::GET, "margin/marginLoan" | "margin/repayLoan" | "margin/order") => 10,
        (
            &Method::GET,
            "margin/maxBorrowable"
            | "margin/maxWithdraw"
            | "margin/myTrades"
            | "margin/openOrderList"
            | "margin/openOrders"
            | "margin/orderList",
        ) => 5,
        (&Method::GET, "margin/marginInterestHistory" | "margin/forceOrders") => 1,
        (&Method::GET, "um/allOrders" | "um/algo/allAlgoOrders") => 5,
        (&Method::GET, "um/leverageBracket" | "cm/leverageBracket" | "cm/positionRisk") => 1,
        (
            &Method::GET,
            "um/algo/algoOrder"
            | "um/order"
            | "um/openOrder"
            | "um/orderAmendment"
            | "cm/order"
            | "cm/openOrder"
            | "cm/orderAmendment"
            | "cm/conditional/openOrder"
            | "cm/conditional/orderHistory",
        ) => 1,
        (
            &Method::GET,
            "um/openOrders"
            | "cm/openOrders"
            | "um/algo/openAlgoOrders"
            | "cm/conditional/allOrders"
            | "cm/conditional/openOrders",
        ) => {
            if has_symbol {
                1
            } else {
                40
            }
        }
        (&Method::GET, "um/apiTradingStatus") => {
            if has_symbol {
                1
            } else {
                10
            }
        }
        (&Method::GET, "um/forceOrders" | "cm/forceOrders") => {
            if has_symbol {
                20
            } else {
                50
            }
        }
        (&Method::GET, "cm/allOrders" | "cm/userTrades") => {
            if has_symbol {
                20
            } else {
                40
            }
        }
        (&Method::POST, "auto-collection" | "bnb-transfer" | "repay-futures-switch") => 750,
        (&Method::POST, "repay-futures-negative-balance") => 750,
        (&Method::POST, "asset-collection") => 30,
        (&Method::POST, "marginLoan" | "repayLoan") => 100,
        (&Method::POST, "margin/repay-debt") => 3_000,
        (&Method::POST, "um/stock/contract") => 5,
        (&Method::DELETE, "margin/allOpenOrders") => 5,
        (&Method::DELETE, "margin/order" | "margin/orderList") => 2,
        // Remaining order, conditional, algo, leverage, mode, fee-burn, listen-key, and
        // undocumented routes all cost one IP weight
        _ => 1,
    }
}

pub(crate) fn depth_weight_spot(limit: u32) -> u32 {
    match limit {
        ..=100 => 5,
        101..=500 => 25,
        501..=1000 => 50,
        _ => 250,
    }
}

pub(crate) fn depth_weight_futures(limit: u32) -> u32 {
    match limit {
        ..=50 => 2,
        51..=100 => 5,
        101..=500 => 10,
        _ => 20,
    }
}

fn query_limit(query: &str, default: u32) -> u32 {
    query_value(query, "limit")
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn query_value<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').find_map(|param| {
        let (key, value) = param.split_once('=')?;
        (key == name).then_some(value)
    })
}

fn symbol_count(query: &str) -> usize {
    #[derive(serde::Deserialize)]
    struct Symbols {
        symbols: String,
    }

    if query_value(query, "symbol").is_some() {
        return 1;
    }

    serde_urlencoded::from_str::<Symbols>(query)
        .ok()
        .and_then(|params| serde_json::from_str::<Vec<String>>(&params.symbols).ok())
        .map_or(0, |symbols| symbols.len())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("ping", "", 1)]
    #[case("time", "", 1)]
    #[case("exchangeInfo", "", 20)]
    #[case("depth", "", 5)]
    #[case("depth", "symbol=BTCUSDT&limit=101", 25)]
    #[case("depth", "limit=501", 50)]
    #[case("depth", "limit=1001", 250)]
    #[case("trades", "limit=1", 25)]
    #[case("aggTrades", "limit=1000", 4)]
    #[case("klines", "limit=1000", 2)]
    #[case("avgPrice", "symbol=BTCUSDT", 2)]
    #[case("ticker/24hr", "symbol=BTCUSDT", 2)]
    #[case("ticker/24hr", "", 80)]
    #[case("ticker/price", "symbol=BTCUSDT", 2)]
    #[case("ticker/price", "", 4)]
    #[case("ticker/bookTicker", "symbol=BTCUSDT", 2)]
    #[case("ticker/bookTicker", "", 4)]
    #[case("account", "omitZeroBalances=true", 20)]
    #[case("account/commission", "symbol=BTCUSDT", 20)]
    #[case("order", "symbol=BTCUSDT&orderId=42", 4)]
    #[case("openOrders", "symbol=BTCUSDT", 6)]
    #[case("openOrders", "", 80)]
    #[case("allOrders", "symbol=BTCUSDT", 20)]
    #[case("myTrades", "symbol=BTCUSDT", 20)]
    #[case("myTrades", "symbol=BTCUSDT&orderId=42", 5)]
    fn test_request_weight_spot(#[case] path: &str, #[case] query: &str, #[case] weight: u32) {
        assert_eq!(request_weight_spot(&Method::GET, path, query), weight);
    }

    #[rstest]
    #[case(1, 2)]
    #[case(20, 2)]
    #[case(21, 40)]
    #[case(100, 40)]
    #[case(101, 80)]
    fn test_spot_ticker_symbols_weight(#[case] count: usize, #[case] weight: u32) {
        let symbols = vec!["BTCUSDT"; count];
        let symbols = serde_json::to_string(&symbols).unwrap();
        let query = serde_urlencoded::to_string([("symbols", symbols)]).unwrap();

        assert_eq!(
            request_weight_spot(&Method::GET, "ticker/24hr", &query),
            weight
        );
    }

    #[rstest]
    #[case(Method::POST, "order")]
    #[case(Method::POST, "order/cancelReplace")]
    #[case(Method::POST, "orderList/oco")]
    #[case(Method::DELETE, "order")]
    #[case(Method::DELETE, "openOrders")]
    #[case(Method::PUT, "userDataStream")]
    fn test_spot_write_weight(#[case] method: Method, #[case] path: &str) {
        assert_eq!(request_weight_spot(&method, path, ""), 1);
    }

    #[rstest]
    #[case("time", "", 1, 1)]
    #[case("exchangeInfo", "", 1, 1)]
    #[case("depth", "", 10, 10)]
    #[case("depth", "limit=50", 2, 2)]
    #[case("depth", "limit=100", 5, 5)]
    #[case("depth", "limit=1000", 20, 20)]
    #[case("klines", "", 5, 5)]
    #[case("klines", "limit=99", 1, 1)]
    #[case("klines", "limit=100", 2, 2)]
    #[case("klines", "limit=499", 2, 2)]
    #[case("klines", "limit=500", 5, 5)]
    #[case("klines", "limit=1000", 5, 5)]
    #[case("klines", "limit=1001", 10, 10)]
    #[case("trades", "", 5, 5)]
    #[case("aggTrades", "", 20, 20)]
    #[case("premiumIndex", "symbol=BTCUSDT", 1, 10)]
    #[case("premiumIndex", "", 10, 10)]
    #[case("fundingRate", "", 0, 1)]
    #[case("futures/data/openInterestHist", "", 0, 1)]
    #[case("openInterest", "", 1, 1)]
    #[case("ticker/24hr", "symbol=BTCUSDT", 1, 1)]
    #[case("ticker/24hr", "", 40, 40)]
    #[case("ticker/price", "symbol=BTCUSDT", 1, 1)]
    #[case("ticker/price", "", 2, 2)]
    #[case("ticker/bookTicker", "symbol=BTCUSDT", 2, 2)]
    #[case("ticker/bookTicker", "", 5, 5)]
    #[case("account", "", 5, 5)]
    #[case("commissionRate", "", 20, 20)]
    #[case("positionRisk", "", 5, 1)]
    #[case("positionSide/dual", "", 30, 30)]
    #[case("order", "", 1, 1)]
    #[case("allOrders", "", 5, 5)]
    #[case("userTrades", "", 5, 5)]
    #[case("openOrders", "symbol=BTCUSDT", 1, 1)]
    #[case("openOrders", "", 40, 40)]
    #[case("openAlgoOrders", "symbol=BTCUSDT", 1, 1)]
    #[case("openAlgoOrders", "", 40, 40)]
    #[case("allAlgoOrders", "", 5, 5)]
    fn test_request_weight_futures(
        #[case] path: &str,
        #[case] query: &str,
        #[case] usd_weight: u32,
        #[case] coin_weight: u32,
    ) {
        assert_eq!(
            request_weight_futures(&Method::GET, path, query, false),
            usd_weight
        );
        assert_eq!(
            request_weight_futures(&Method::GET, path, query, true),
            coin_weight
        );
    }

    #[rstest]
    #[case(Method::POST, "order", 0, 0)]
    #[case(Method::POST, "algoOrder", 0, 0)]
    #[case(Method::PUT, "order", 0, 1)]
    #[case(Method::POST, "batchOrders", 5, 5)]
    #[case(Method::PUT, "batchOrders", 5, 5)]
    #[case(Method::DELETE, "batchOrders", 1, 1)]
    #[case(Method::DELETE, "order", 1, 1)]
    #[case(Method::DELETE, "algoOrder", 1, 1)]
    #[case(Method::DELETE, "allOpenOrders", 1, 1)]
    #[case(Method::POST, "leverage", 1, 1)]
    #[case(Method::POST, "marginType", 1, 1)]
    #[case(Method::POST, "listenKey", 1, 1)]
    #[case(Method::PUT, "listenKey", 1, 1)]
    #[case(Method::DELETE, "listenKey", 1, 1)]
    fn test_futures_write_weight(
        #[case] method: Method,
        #[case] path: &str,
        #[case] usd_weight: u32,
        #[case] coin_weight: u32,
    ) {
        assert_eq!(request_weight_futures(&method, path, "", false), usd_weight);
        assert_eq!(request_weight_futures(&method, path, "", true), coin_weight);
    }

    #[rstest]
    #[case("/fapi/v2/account", 5)]
    #[case("/dapi/v1/account", 5)]
    #[case("/fapi/v2/positionRisk", 5)]
    #[case("/fapi/v1/depth", 10)]
    fn test_futures_absolute_path_weight(#[case] path: &str, #[case] weight: u32) {
        assert_eq!(
            request_weight_futures(&Method::GET, path, "", false),
            weight
        );
    }

    #[rstest]
    #[case(Method::GET, "/papi/v1/ping", "", 1)]
    #[case(Method::GET, "/papi/v1/rateLimit/order", "", 1)]
    #[case(Method::GET, "/papi/v1/account", "", 20)]
    #[case(Method::GET, "/papi/v1/balance", "", 20)]
    #[case(Method::GET, "/papi/v1/um/account", "", 5)]
    #[case(Method::GET, "/papi/v2/um/account", "", 5)]
    #[case(Method::GET, "/papi/v1/um/accountConfig", "", 5)]
    #[case(Method::GET, "/papi/v1/um/symbolConfig", "", 5)]
    #[case(Method::GET, "/papi/v1/um/positionRisk", "", 5)]
    #[case(Method::GET, "/papi/v1/um/adlQuantile", "", 5)]
    #[case(Method::GET, "/papi/v1/cm/adlQuantile", "", 5)]
    #[case(Method::GET, "/papi/v1/cm/account", "", 5)]
    #[case(Method::GET, "/papi/v1/um/userTrades", "", 5)]
    #[case(Method::GET, "/papi/v1/um/commissionRate", "", 20)]
    #[case(Method::GET, "/papi/v1/cm/commissionRate", "", 20)]
    #[case(Method::GET, "/papi/v1/um/income", "", 30)]
    #[case(Method::GET, "/papi/v1/cm/income", "", 30)]
    #[case(Method::GET, "/papi/v1/um/feeBurn", "", 30)]
    #[case(Method::GET, "/papi/v1/repay-futures-switch", "", 30)]
    #[case(Method::GET, "/papi/v1/um/positionSide/dual", "", 30)]
    #[case(Method::GET, "/papi/v1/cm/positionSide/dual", "", 30)]
    #[case(Method::GET, "/papi/v1/portfolio/interest-history", "", 50)]
    #[case(
        Method::GET,
        "/papi/v1/portfolio/negative-balance-exchange-record",
        "",
        100
    )]
    #[case(Method::GET, "/papi/v1/um/order/asyn", "", 1_500)]
    #[case(Method::GET, "/papi/v1/um/trade/asyn", "", 1_500)]
    #[case(Method::GET, "/papi/v1/um/income/asyn", "", 1_500)]
    #[case(Method::GET, "/papi/v1/um/order/asyn/id", "", 10)]
    #[case(Method::GET, "/papi/v1/um/trade/asyn/id", "", 10)]
    #[case(Method::GET, "/papi/v1/um/income/asyn/id", "", 10)]
    #[case(Method::GET, "/papi/v1/margin/allOrderList", "", 100)]
    #[case(Method::GET, "/papi/v1/margin/allOrders", "", 100)]
    #[case(Method::GET, "/papi/v1/margin/marginLoan", "", 10)]
    #[case(Method::GET, "/papi/v1/margin/repayLoan", "", 10)]
    #[case(Method::GET, "/papi/v1/margin/order", "", 10)]
    #[case(Method::GET, "/papi/v1/margin/maxBorrowable", "", 5)]
    #[case(Method::GET, "/papi/v1/margin/maxWithdraw", "", 5)]
    #[case(Method::GET, "/papi/v1/margin/myTrades", "", 5)]
    #[case(Method::GET, "/papi/v1/margin/openOrderList", "", 5)]
    #[case(Method::GET, "/papi/v1/margin/openOrders", "", 5)]
    #[case(Method::GET, "/papi/v1/margin/orderList", "", 5)]
    #[case(Method::GET, "/papi/v1/margin/marginInterestHistory", "", 1)]
    #[case(Method::GET, "/papi/v1/margin/forceOrders", "", 1)]
    #[case(Method::GET, "/papi/v1/um/allOrders", "", 5)]
    #[case(Method::GET, "/papi/v1/um/algo/allAlgoOrders", "", 5)]
    #[case(Method::GET, "/papi/v1/um/leverageBracket", "", 1)]
    #[case(Method::GET, "/papi/v1/cm/leverageBracket", "", 1)]
    #[case(Method::GET, "/papi/v1/cm/positionRisk", "", 1)]
    #[case(Method::GET, "/papi/v1/um/algo/algoOrder", "", 1)]
    #[case(Method::GET, "/papi/v1/um/algo/openAlgoOrders", "symbol=BTCUSDT", 1)]
    #[case(Method::GET, "/papi/v1/um/algo/openAlgoOrders", "", 40)]
    #[case(Method::GET, "/papi/v1/um/order", "", 1)]
    #[case(Method::GET, "/papi/v1/um/openOrder", "", 1)]
    #[case(Method::GET, "/papi/v1/um/orderAmendment", "", 1)]
    #[case(Method::GET, "/papi/v1/cm/order", "", 1)]
    #[case(Method::GET, "/papi/v1/cm/openOrder", "", 1)]
    #[case(Method::GET, "/papi/v1/cm/orderAmendment", "", 1)]
    #[case(Method::GET, "/papi/v1/cm/conditional/openOrder", "", 1)]
    #[case(Method::GET, "/papi/v1/cm/conditional/orderHistory", "", 1)]
    fn test_request_weight_papi_reads(
        #[case] method: Method,
        #[case] path: &str,
        #[case] query: &str,
        #[case] weight: u32,
    ) {
        assert_eq!(request_weight_papi(&method, path, query), weight);
    }

    #[rstest]
    #[case("/papi/v1/um/openOrders", "symbol=BTCUSDT", 1)]
    #[case("/papi/v1/um/openOrders", "", 40)]
    #[case("/papi/v1/cm/openOrders", "symbol=BTCUSD_PERP", 1)]
    #[case("/papi/v1/cm/openOrders", "", 40)]
    #[case("/papi/v1/cm/conditional/allOrders", "symbol=BTCUSD_PERP", 1)]
    #[case("/papi/v1/cm/conditional/allOrders", "", 40)]
    #[case("/papi/v1/cm/conditional/openOrders", "symbol=BTCUSD_PERP", 1)]
    #[case("/papi/v1/cm/conditional/openOrders", "", 40)]
    #[case("/papi/v1/um/apiTradingStatus", "symbol=BTCUSDT", 1)]
    #[case("/papi/v1/um/apiTradingStatus", "", 10)]
    #[case("/papi/v1/um/forceOrders", "symbol=BTCUSDT", 20)]
    #[case("/papi/v1/um/forceOrders", "", 50)]
    #[case("/papi/v1/cm/forceOrders", "symbol=BTCUSD_PERP", 20)]
    #[case("/papi/v1/cm/forceOrders", "", 50)]
    #[case("/papi/v1/cm/allOrders", "symbol=BTCUSD_PERP", 20)]
    #[case("/papi/v1/cm/allOrders", "pair=BTCUSD", 40)]
    #[case("/papi/v1/cm/userTrades", "symbol=BTCUSD_PERP", 20)]
    #[case("/papi/v1/cm/userTrades", "pair=BTCUSD", 40)]
    fn test_request_weight_papi_parametrized(
        #[case] path: &str,
        #[case] query: &str,
        #[case] weight: u32,
    ) {
        assert_eq!(request_weight_papi(&Method::GET, path, query), weight);
    }

    #[rstest]
    #[case(Method::POST, "/papi/v1/auto-collection", 750)]
    #[case(Method::POST, "/papi/v1/bnb-transfer", 750)]
    #[case(Method::POST, "/papi/v1/repay-futures-switch", 750)]
    #[case(Method::POST, "/papi/v1/repay-futures-negative-balance", 750)]
    #[case(Method::POST, "/papi/v1/asset-collection", 30)]
    #[case(Method::POST, "/papi/v1/marginLoan", 100)]
    #[case(Method::POST, "/papi/v1/repayLoan", 100)]
    #[case(Method::POST, "/papi/v1/margin/repay-debt", 3_000)]
    #[case(Method::POST, "/papi/v1/um/stock/contract", 5)]
    #[case(Method::DELETE, "/papi/v1/margin/allOpenOrders", 5)]
    #[case(Method::DELETE, "/papi/v1/margin/order", 2)]
    #[case(Method::DELETE, "/papi/v1/margin/orderList", 2)]
    #[case(Method::POST, "/papi/v1/um/order", 1)]
    #[case(Method::POST, "/papi/v1/cm/conditional/order", 1)]
    #[case(Method::POST, "/papi/v1/um/algo/order", 1)]
    #[case(Method::PUT, "/papi/v1/um/order", 1)]
    #[case(Method::PUT, "/papi/v1/cm/order", 1)]
    #[case(Method::DELETE, "/papi/v1/um/order", 1)]
    #[case(Method::DELETE, "/papi/v1/um/algo/allOpenOrders", 1)]
    #[case(Method::POST, "/papi/v1/um/leverage", 1)]
    #[case(Method::POST, "/papi/v1/cm/positionSide/dual", 1)]
    #[case(Method::POST, "/papi/v1/um/feeBurn", 1)]
    #[case(Method::POST, "/papi/v1/listenKey", 1)]
    #[case(Method::PUT, "/papi/v1/listenKey", 1)]
    #[case(Method::DELETE, "/papi/v1/listenKey", 1)]
    #[case(Method::GET, "/papi/v1/undocumented", 1)]
    fn test_request_weight_papi_writes(
        #[case] method: Method,
        #[case] path: &str,
        #[case] weight: u32,
    ) {
        assert_eq!(request_weight_papi(&method, path, ""), weight);
    }
}
