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

//! Provides the HTTP client for the Polymarket Data API.

use std::{collections::HashMap, result::Result as StdResult};

use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_model::{
    data::TradeTick,
    enums::AggressorSide,
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use nautilus_network::http::{HttpClient, HttpClientError, Method, USER_AGENT};
use ustr::Ustr;

use crate::http::{
    error::{Error, Result},
    models::{DataApiPosition, DataApiTrade},
};

const POLYMARKET_DATA_API_URL: &str = "https://data-api.polymarket.com";

/// Provides an unauthenticated HTTP client for the Polymarket Data API.
///
/// Used for fetching historical trade data from `GET /trades`.
#[derive(Debug, Clone)]
pub struct PolymarketDataApiHttpClient {
    client: HttpClient,
    base_url: String,
}

impl PolymarketDataApiHttpClient {
    /// Creates a new [`PolymarketDataApiHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(base_url: Option<String>, timeout_secs: u64) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                HashMap::from([
                    (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
                    ("Content-Type".to_string(), "application/json".to_string()),
                ]),
                vec![],
                vec![],
                None,
                Some(timeout_secs),
                None,
            )?,
            base_url: base_url
                .unwrap_or_else(|| POLYMARKET_DATA_API_URL.to_string())
                .trim_end_matches('/')
                .to_string(),
        })
    }

    /// Fetches all positions for a user from the Data API.
    ///
    /// Paginates through `GET /positions?user={address}&sizeThreshold=0`
    /// until a partial page is returned.
    pub async fn get_positions(&self, user_address: &str) -> Result<Vec<DataApiPosition>> {
        const PAGE_SIZE: u32 = 100;

        let mut all_positions: Vec<DataApiPosition> = Vec::new();
        let mut offset: u32 = 0;

        loop {
            let params = vec![
                ("user".to_string(), user_address.to_string()),
                ("limit".to_string(), PAGE_SIZE.to_string()),
                ("offset".to_string(), offset.to_string()),
                ("sizeThreshold".to_string(), "0".to_string()),
                ("sortBy".to_string(), "TOKENS".to_string()),
                ("sortDirection".to_string(), "DESC".to_string()),
            ];

            let url = format!("{}/positions", self.base_url);
            let response = self
                .client
                .request_with_params(Method::GET, url, Some(&params), None, None, None, None)
                .await
                .map_err(Error::from_http_client)?;

            if response.status.is_success() {
                let page: Vec<DataApiPosition> =
                    serde_json::from_slice(&response.body).map_err(Error::Serde)?;
                let count = page.len() as u32;
                all_positions.extend(page);

                if count < PAGE_SIZE {
                    break;
                }
                offset += count;
            } else {
                return Err(Error::from_status_code(
                    response.status.as_u16(),
                    &response.body,
                ));
            }
        }

        Ok(all_positions)
    }

    /// Fetches trades from the Data API for the given condition ID.
    pub async fn get_trades(
        &self,
        condition_id: &str,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<DataApiTrade>> {
        let mut params = vec![("market".to_string(), condition_id.to_string())];

        if let Some(l) = limit {
            params.push(("limit".to_string(), l.to_string()));
        }

        if let Some(o) = offset {
            params.push(("offset".to_string(), o.to_string()));
        }

        let url = format!("{}/trades", self.base_url);
        let response = self
            .client
            .request_with_params(Method::GET, url, Some(&params), None, None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        if response.status.is_success() {
            serde_json::from_slice(&response.body).map_err(Error::Serde)
        } else {
            Err(Error::from_status_code(
                response.status.as_u16(),
                &response.body,
            ))
        }
    }

    /// Fetches trades and converts them to [`TradeTick`] for the given instrument.
    ///
    /// Automatically paginates through all available results (up to `limit`
    /// if specified, with a safety cap of 100 pages). Filters by `token_id`
    /// (since the API returns trades for all outcomes of the condition) and
    /// returns results in chronological order.
    pub async fn request_trade_ticks(
        &self,
        instrument_id: InstrumentId,
        condition_id: &str,
        token_id: &str,
        price_precision: u8,
        size_precision: u8,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        const PAGE_SIZE: u32 = 500;
        // Polymarket Data API rejects offsets >= 3000
        const MAX_OFFSET: u32 = 3000;

        let page_size = limit.map_or(PAGE_SIZE, |l| l.min(PAGE_SIZE));
        let mut all_trades: Vec<DataApiTrade> = Vec::new();
        let mut offset: u32 = 0;

        loop {
            let page = self
                .get_trades(condition_id, Some(page_size), Some(offset))
                .await
                .map_err(|e| anyhow::anyhow!(e))?;

            let count = page.len() as u32;
            all_trades.extend(page);

            // Partial page means no more data available
            if count < page_size {
                break;
            }
            // If we've collected enough for the caller's target, stop
            if let Some(target) = limit
                && all_trades.len() as u32 >= target
            {
                break;
            }
            offset += count;
            // API hard limit on offset
            if offset >= MAX_OFFSET {
                break;
            }
        }

        // Apply final truncation to honour the caller's limit
        if let Some(target) = limit {
            all_trades.truncate(target as usize);
        }

        let mut ticks: Vec<TradeTick> = all_trades
            .into_iter()
            .filter(|t| t.asset == token_id)
            .map(|t| {
                let price = Price::new(t.price, price_precision);
                let size = Quantity::new(t.size, size_precision);
                let aggressor_side = AggressorSide::from(t.side);
                // TradeId max length is 36; tx hash is 66 chars, take last 36
                let hash = &t.transaction_hash;
                let trade_id_str = if hash.len() > 36 {
                    &hash[hash.len() - 36..]
                } else {
                    hash.as_str()
                };
                let trade_id = TradeId::new(Ustr::from(trade_id_str));
                // Data API timestamp is in epoch seconds
                let ts_event = nautilus_core::UnixNanos::from(t.timestamp as u64 * 1_000_000_000);

                TradeTick::new(
                    instrument_id,
                    price,
                    size,
                    aggressor_side,
                    trade_id,
                    ts_event,
                    ts_event,
                )
            })
            .collect();

        // API returns newest-first; reverse for chronological order
        ticks.reverse();

        Ok(ticks)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::AggressorSide,
        identifiers::{AccountId, InstrumentId},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::{
        common::consts::USDC_DECIMALS,
        execution::reconciliation::build_position_reports,
        http::models::{DataApiPosition, DataApiTrade},
    };

    fn load_positions() -> Vec<DataApiPosition> {
        let path = "test_data/data_api_positions_response.json";
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    fn load_trades() -> Vec<DataApiTrade> {
        let path = "test_data/data_api_trades_response.json";
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    #[rstest]
    fn test_data_api_position_deserialization() {
        let positions = load_positions();

        assert_eq!(positions.len(), 4);
        assert_eq!(positions[0].size, 150.5);
        assert_eq!(positions[0].avg_price, Some(0.55));
        assert_eq!(
            positions[0].condition_id,
            "0xc8f1cf5d4f26e0fd9c8fe89f2a7b3263b902cf14fde7bfccef525753bb492e47"
        );
    }

    #[rstest]
    fn test_build_position_reports_filters_dust_and_zero() {
        let positions = load_positions();
        let account_id = AccountId::from("POLYMARKET-001");
        let ts_now = nautilus_core::UnixNanos::from(1_000_000_000u64);

        let reports = build_position_reports(&positions, account_id, ts_now);

        // 4 positions: 150.5, 0.0, 42.0, 0.005 (dust)
        // Only 150.5 and 42.0 pass the DUST_SNAP_THRESHOLD (0.01)
        assert_eq!(reports.len(), 2);
        assert!(reports[0].is_long());
        assert!(reports[1].is_long());
    }

    #[rstest]
    fn test_build_position_reports_carries_avg_price() {
        let positions = load_positions();
        let account_id = AccountId::from("POLYMARKET-001");
        let ts_now = nautilus_core::UnixNanos::from(1_000_000_000u64);

        let reports = build_position_reports(&positions, account_id, ts_now);

        assert_eq!(reports.len(), 2);
        assert_eq!(
            reports[0].avg_px_open,
            Some(Decimal::try_from(0.55).unwrap())
        );
        assert_eq!(
            reports[1].avg_px_open,
            Some(Decimal::try_from(0.3).unwrap())
        );
    }

    #[rstest]
    fn test_build_position_reports_uses_usdc_precision() {
        let positions = load_positions();
        let account_id = AccountId::from("POLYMARKET-001");
        let ts_now = nautilus_core::UnixNanos::from(1_000_000_000u64);

        let reports = build_position_reports(&positions, account_id, ts_now);

        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].quantity.precision, USDC_DECIMALS as u8);
        assert_eq!(reports[1].quantity.precision, USDC_DECIMALS as u8);
    }

    #[rstest]
    fn test_build_position_reports_handles_missing_avg_price() {
        let positions = vec![DataApiPosition {
            asset: "123".to_string(),
            condition_id: "0xabc".to_string(),
            size: 10.0,
            avg_price: None,
        }];
        let account_id = AccountId::from("POLYMARKET-001");
        let ts_now = nautilus_core::UnixNanos::from(1_000_000_000u64);

        let reports = build_position_reports(&positions, account_id, ts_now);

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].avg_px_open, None);
    }

    #[rstest]
    fn test_data_api_trade_deserialization() {
        let trades = load_trades();

        assert_eq!(trades.len(), 3);

        assert_eq!(
            trades[0].asset,
            "71321045863084981365469005770620412523470745398083994982746259498689308907982"
        );
        assert_eq!(
            trades[0].condition_id,
            "0xc8f1cf5d4f26e0fd9c8fe89f2a7b3263b902cf14fde7bfccef525753bb492e47"
        );
        assert_eq!(trades[0].price, 0.55);
        assert_eq!(trades[0].size, 100.0);
        assert_eq!(trades[0].timestamp, 1710000000);
        assert_eq!(
            trades[0].transaction_hash,
            "0xabc123def456789012345678901234567890abcdef1234567890abcdef123456"
        );
    }

    #[rstest]
    fn test_data_api_trade_ignores_extra_fields() {
        let trades = load_trades();
        // proxyWallet, title, slug should be silently ignored
        assert_eq!(trades.len(), 3);
    }

    #[rstest]
    fn test_build_trade_ticks_filters_by_token_id() {
        let trades = load_trades();
        let instrument_id = InstrumentId::from(
            "0xc8f1cf5d4f26e0fd9c8fe89f2a7b3263b902cf14fde7bfccef525753bb492e47-71321045863084981365469005770620412523470745398083994982746259498689308907982.POLYMARKET",
        );
        let token_id =
            "71321045863084981365469005770620412523470745398083994982746259498689308907982";
        let price_precision = 2u8;
        let size_precision = 2u8;

        let ticks: Vec<TradeTick> = trades
            .into_iter()
            .filter(|t| t.asset == token_id)
            .map(|t| {
                let price = Price::new(t.price, price_precision);
                let size = Quantity::new(t.size, size_precision);
                let aggressor_side = AggressorSide::from(t.side);
                // TradeId max length is 36; tx hash is 66 chars, take last 36
                let hash = &t.transaction_hash;
                let trade_id_str = if hash.len() > 36 {
                    &hash[hash.len() - 36..]
                } else {
                    hash.as_str()
                };
                let trade_id = TradeId::new(Ustr::from(trade_id_str));
                let ts_event = nautilus_core::UnixNanos::from(t.timestamp as u64 * 1_000_000_000);

                TradeTick::new(
                    instrument_id,
                    price,
                    size,
                    aggressor_side,
                    trade_id,
                    ts_event,
                    ts_event,
                )
            })
            .collect();

        // Should filter out the third trade (different asset)
        assert_eq!(ticks.len(), 2);
        assert_eq!(ticks[0].aggressor_side, AggressorSide::Buyer);
        assert_eq!(ticks[1].aggressor_side, AggressorSide::Seller);
    }

    #[rstest]
    fn test_build_trade_ticks_chronological_order() {
        let trades = load_trades();
        let instrument_id = InstrumentId::from(
            "0xc8f1cf5d4f26e0fd9c8fe89f2a7b3263b902cf14fde7bfccef525753bb492e47-71321045863084981365469005770620412523470745398083994982746259498689308907982.POLYMARKET",
        );
        let token_id =
            "71321045863084981365469005770620412523470745398083994982746259498689308907982";

        let mut ticks: Vec<TradeTick> = trades
            .into_iter()
            .filter(|t| t.asset == token_id)
            .map(|t| {
                let price = Price::new(t.price, 2);
                let size = Quantity::new(t.size, 2);
                let aggressor_side = AggressorSide::from(t.side);
                // TradeId max length is 36; tx hash is 66 chars, take last 36
                let hash = &t.transaction_hash;
                let trade_id_str = if hash.len() > 36 {
                    &hash[hash.len() - 36..]
                } else {
                    hash.as_str()
                };
                let trade_id = TradeId::new(Ustr::from(trade_id_str));
                let ts_event = nautilus_core::UnixNanos::from(t.timestamp as u64 * 1_000_000_000);

                TradeTick::new(
                    instrument_id,
                    price,
                    size,
                    aggressor_side,
                    trade_id,
                    ts_event,
                    ts_event,
                )
            })
            .collect();

        // Reverse to get chronological order (API returns newest-first)
        ticks.reverse();

        assert_eq!(ticks.len(), 2);
        // First tick should be the older one (lower timestamp)
        assert!(ticks[0].ts_event < ticks[1].ts_event);
    }
}
