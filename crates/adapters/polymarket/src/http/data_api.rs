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

//! Provides the HTTP client for the Polymarket Data API v2.

use std::{collections::HashMap, convert::Infallible, result::Result as StdResult};

use anyhow::Context;
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::TradeTick,
    enums::AggressorSide,
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::{HttpClient, HttpClientError, Method, create_standard_nautilus_headers},
    websocket::proxy::ProxyUrl,
};
use rust_decimal::Decimal;

use crate::{
    common::{enums::PolymarketOrderSide, urls::data_api_url},
    http::{
        error::{Error, Result, decode_response},
        models::{DataApiPage, DataApiPosition, DataApiTrade},
        pagination::{
            CollectAll, Completion, CursorProtocol, FetchOutcome, PageReducer, Paginator,
        },
    },
};

const PATH_POSITIONS: &str = "/v2/positions";
const PATH_TRADES: &str = "/v2/trades";

// Bounds retained trades when neither `start` nor `limit` is supplied; matches the
// 10,000 rows the v1 offset ceiling served for the same request shape.
const MAX_UNBOUNDED_WALK_ROWS: usize = 10_000;

// Approximate venue retention horizon for diagnostics only
const TRADE_RETENTION_SECONDS: i64 = 3 * 365 * 86_400;

// Composite key for stabilizing same-second trades across paginated responses
fn data_api_trade_sort_key(t: &DataApiTrade) -> (i64, &str, &str, &'static str, Decimal, Decimal) {
    (
        t.timestamp,
        t.transaction_hash.as_str(),
        t.asset.as_str(),
        match t.side {
            PolymarketOrderSide::Buy => "BUY",
            PolymarketOrderSide::Sell => "SELL",
        },
        t.price,
        t.size,
    )
}

// Composite TradeId: tx hashes recur across multi-fill swaps, so a per-(tx,
// asset) sequence is appended to disambiguate fills that would otherwise
// collide on the last 36 chars of the transaction hash.
pub(crate) fn build_polymarket_trade_id(transaction_hash: &str, asset: &str, seq: u32) -> String {
    let hash_suffix = if transaction_hash.len() > 24 {
        &transaction_hash[transaction_hash.len() - 24..]
    } else {
        transaction_hash
    };
    let asset_suffix = if asset.len() > 4 {
        &asset[asset.len() - 4..]
    } else {
        asset
    };
    format!("{hash_suffix}-{asset_suffix}-{seq:06}")
}

fn validate_trade_page_scope(
    rows: Vec<DataApiTrade>,
    expected_condition_id: &str,
) -> anyhow::Result<Vec<DataApiTrade>> {
    match rows.iter().find(|trade| {
        !trade
            .condition_id
            .eq_ignore_ascii_case(expected_condition_id)
    }) {
        Some(trade) => anyhow::bail!(
            "Polymarket Data API returned trade for condition {} while requesting {expected_condition_id}",
            trade.condition_id
        ),
        None => Ok(rows),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TradeTickStop {
    CallerCapped,
    OlderThanStart,
    PageCapReached,
}

struct TradeTickReducer {
    rows: Vec<DataApiTrade>,
    instrument_id: InstrumentId,
    condition_id: String,
    token_id: String,
    price_precision: u8,
    size_precision: u8,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    limit: Option<usize>,
}

impl PageReducer<DataApiTrade, anyhow::Error> for TradeTickReducer {
    type Output = Vec<TradeTick>;
    type Stop = TradeTickStop;

    fn consume(&mut self, rows: Vec<DataApiTrade>) -> anyhow::Result<Option<Self::Stop>> {
        // The v2 condition feed ignores start/end bounds and is served
        // newest-first, so once an entire page precedes the requested start
        // the remaining pages cannot contain matching rows.
        let older_than_start = self.start.is_some_and(|start| {
            let start_secs = (start.as_u64() / 1_000_000_000) as i64;
            !rows.is_empty() && rows.iter().all(|trade| trade.timestamp < start_secs)
        });

        let end_secs = self.end.map(|end| (end.as_u64() / 1_000_000_000) as i64);
        self.rows.extend(rows.into_iter().filter(|trade| {
            trade.asset == self.token_id && end_secs.is_none_or(|end| trade.timestamp <= end)
        }));

        if older_than_start {
            return Ok(Some(TradeTickStop::OlderThanStart));
        }

        let capped =
            self.start.is_none() && self.limit.is_some_and(|target| self.rows.len() >= target);
        if capped {
            return Ok(Some(TradeTickStop::CallerCapped));
        }

        // End-only requests must traverse newer pages first, so this bounds
        // retained history, not the number of requests.
        let unbounded_capped = self.start.is_none()
            && self.limit.is_none()
            && self.rows.len() >= MAX_UNBOUNDED_WALK_ROWS;
        Ok(unbounded_capped.then_some(TradeTickStop::PageCapReached))
    }

    fn finish(self, completion: &Completion<Self::Stop>) -> anyhow::Result<Self::Output> {
        if let Some(start) = self.start
            && matches!(completion, Completion::WireExhausted)
            && start_predates_retention_window(
                (start.as_u64() / 1_000_000_000) as i64,
                (get_atomic_clock_realtime().get_time_ns().as_u64() / 1_000_000_000) as i64,
            )
        {
            log::warn!(
                "Polymarket Data API trades start predates the approximate three-year retention window for condition {}; results may be incomplete",
                self.condition_id
            );
        }

        let start_secs = self
            .start
            .map(|value| (value.as_u64() / 1_000_000_000) as i64);
        let end_secs = self
            .end
            .map(|value| (value.as_u64() / 1_000_000_000) as i64);
        let mut trades = parse_trade_ticks(
            self.rows,
            self.instrument_id,
            &self.token_id,
            self.price_precision,
            self.size_precision,
        )?;
        trades.retain(|trade| {
            let event_secs = trade.ts_event.as_u64() / 1_000_000_000;
            start_secs.is_none_or(|start| event_secs >= start as u64)
                && end_secs.is_none_or(|end| event_secs <= end as u64)
        });

        if let Some(target) = self.limit
            && trades.len() > target
        {
            if self.start.is_some() {
                trades.truncate(target);
            } else {
                trades.drain(..trades.len() - target);
            }
        }

        Ok(trades)
    }
}

/// Provides an unauthenticated HTTP client for the Polymarket Data API.
///
/// Used for fetching historical trade data from `GET /v2/trades`.
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
        Self::new_with_proxy(base_url, timeout_secs, None)
    }

    /// Creates a new client with an optional validated proxy URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new_with_proxy(
        base_url: Option<String>,
        timeout_secs: u64,
        proxy_url: Option<ProxyUrl>,
    ) -> StdResult<Self, HttpClientError> {
        let mut headers: HashMap<String, String> =
            create_standard_nautilus_headers().into_iter().collect();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        Ok(Self {
            client: HttpClient::builder()
                .headers(headers)
                .timeout_secs(timeout_secs)
                .maybe_proxy_url(proxy_url.map(|url| url.expose().to_string()))
                .build()?,
            base_url: base_url
                .unwrap_or_else(|| data_api_url().to_string())
                .trim_end_matches('/')
                .to_string(),
        })
    }

    /// Fetches all positions for a user from the Data API v2.
    ///
    /// Walks `GET /v2/positions?user={address}` by cursor until the venue
    /// reports no further pages. A short page never ends the walk; only a
    /// `null` `next_cursor` does.
    pub async fn get_positions(&self, user_address: &str) -> Result<Vec<DataApiPosition>> {
        // v2 caps `limit` at 1000 and it only sizes the first page; the cursor
        // carries the page size onward.
        const PAGE_SIZE: u32 = 500;

        let protocol = CursorProtocol::<Infallible>::gamma(PATH_POSITIONS);
        let paginator = Paginator::new(PATH_POSITIONS, protocol, CollectAll::new());
        let completed = paginator
            .run(
                |position| async move {
                    let page = self
                        .get_positions_page(
                            user_address,
                            PAGE_SIZE,
                            position.as_ref().map(|cursor| cursor.as_ref()),
                        )
                        .await?;
                    Ok(FetchOutcome::Page {
                        rows: page.data,
                        wire: page.pagination.next_cursor,
                    })
                },
                |e| Error::decode(e.to_string()),
            )
            .await?;

        match completed.completion {
            Completion::WireExhausted => Ok(completed.output),
            Completion::Stopped(never) => match never {},
        }
    }

    async fn get_positions_page(
        &self,
        user_address: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<DataApiPage<DataApiPosition>> {
        let mut params = vec![
            ("user".to_string(), user_address.to_string()),
            ("limit".to_string(), limit.to_string()),
            ("filter_type".to_string(), "TOKENS".to_string()),
            ("filter_amount".to_string(), "0".to_string()),
            ("sort_by".to_string(), "TOKENS".to_string()),
            ("sort_direction".to_string(), "DESC".to_string()),
        ];

        if let Some(cursor) = cursor {
            params.push(("cursor".to_string(), cursor.to_string()));
        }

        let url = format!("{}{PATH_POSITIONS}", self.base_url);
        let response = self
            .client
            .request_with_params(Method::GET, url, Some(&params), None, None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        decode_response(&response)
    }

    /// Fetches a single page of trades from the Data API v2 for the given
    /// condition ID.
    pub async fn get_trades(
        &self,
        condition_id: &str,
        limit: Option<u32>,
    ) -> Result<Vec<DataApiTrade>> {
        Ok(self.get_trades_page(condition_id, limit, None).await?.data)
    }

    /// Fetches trades and converts them to [`TradeTick`] for the given instrument.
    ///
    /// Automatically walks all pages by cursor (up to `limit` if specified).
    /// Filters by `token_id` (since the API returns trades for all outcomes of
    /// the condition) and returns results in chronological order.
    ///
    /// The v2 condition feed serves a fixed three-year window and ignores
    /// `start`/`end` bounds, so window filtering happens locally and the walk
    /// stops as soon as an entire page precedes `start` (the feed is served
    /// newest-first).
    #[expect(clippy::too_many_arguments)]
    pub async fn request_trade_ticks(
        &self,
        instrument_id: InstrumentId,
        condition_id: &str,
        token_id: &str,
        price_precision: u8,
        size_precision: u8,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        // v2 caps `limit` at 1000; it sizes the first page and the cursor
        // carries that size onward.
        const PAGE_SIZE: u32 = 1000;

        if let (Some(start), Some(end)) = (start, end)
            && start > end
        {
            anyhow::bail!("start must not be later than end");
        }

        if limit == Some(0) {
            anyhow::bail!("limit must be greater than zero");
        }

        let protocol = CursorProtocol::<TradeTickStop>::gamma(PATH_TRADES);
        let reducer = TradeTickReducer {
            rows: Vec::new(),
            instrument_id,
            condition_id: condition_id.to_string(),
            token_id: token_id.to_string(),
            price_precision,
            size_precision,
            start,
            end,
            limit: limit.map(|value| value as usize),
        };
        let paginator = Paginator::new(PATH_TRADES, protocol, reducer);
        let completed = paginator
            .run(
                |position| async move {
                    let page = self
                        .get_trades_page(
                            condition_id,
                            Some(PAGE_SIZE),
                            position.as_ref().map(|cursor| cursor.as_ref()),
                        )
                        .await
                        .map_err(anyhow::Error::new)?;
                    let rows = validate_trade_page_scope(page.data, condition_id)?;
                    Ok::<_, anyhow::Error>(FetchOutcome::Page {
                        rows,
                        wire: page.pagination.next_cursor,
                    })
                },
                anyhow::Error::new,
            )
            .await?;

        match completed.completion {
            Completion::WireExhausted
            | Completion::Stopped(TradeTickStop::CallerCapped | TradeTickStop::OlderThanStart) => {
                Ok(completed.output)
            }
            Completion::Stopped(TradeTickStop::PageCapReached) => {
                log::warn!(
                    "Polymarket Data API trades walk for condition {condition_id} capped at {MAX_UNBOUNDED_WALK_ROWS} rows; returning newest partial results, bound the request with start or limit for more",
                );
                Ok(completed.output)
            }
        }
    }

    async fn get_trades_page(
        &self,
        condition_id: &str,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> Result<DataApiPage<DataApiTrade>> {
        let mut params = vec![("condition".to_string(), condition_id.to_string())];

        if let Some(limit) = limit {
            params.push(("limit".to_string(), limit.to_string()));
        }

        if let Some(cursor) = cursor {
            params.push(("cursor".to_string(), cursor.to_string()));
        }

        let url = format!("{}{PATH_TRADES}", self.base_url);
        let response = self
            .client
            .request_with_params(Method::GET, url, Some(&params), None, None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        decode_response(&response)
    }
}

fn start_predates_retention_window(start_secs: i64, now_secs: i64) -> bool {
    start_secs < now_secs - TRADE_RETENTION_SECONDS
}

// Extracted from `request_trade_ticks` so the parse behavior can be
// unit-tested without HTTP
fn parse_trade_ticks(
    mut data_api_trades: Vec<DataApiTrade>,
    instrument_id: InstrumentId,
    token_id: &str,
    price_precision: u8,
    size_precision: u8,
) -> anyhow::Result<Vec<TradeTick>> {
    // Composite sort to stabilize same-second trades across pages
    data_api_trades.sort_by(|a, b| data_api_trade_sort_key(a).cmp(&data_api_trade_sort_key(b)));

    let mut timestamp_counts: HashMap<u64, u32> = HashMap::new();
    let mut tx_asset_counts: HashMap<(String, String), u32> = HashMap::new();
    let mut trades: Vec<TradeTick> = Vec::new();

    for t in data_api_trades {
        if t.asset != token_id {
            continue;
        }

        let price = Price::from_decimal_dp(t.price, price_precision).with_context(|| {
            format!(
                "failed to convert Data API trade price {} with precision {price_precision}",
                t.price
            )
        })?;
        let size = Quantity::from_decimal_dp(t.size, size_precision).with_context(|| {
            format!(
                "failed to convert Data API trade size {} with precision {size_precision}",
                t.size
            )
        })?;
        let aggressor_side = AggressorSide::from(t.side);

        let base_ns = (t.timestamp as u64) * 1_000_000_000;
        let occurrence = timestamp_counts.entry(base_ns).or_insert(0);
        let tiebreaker = (*occurrence).min(999_999_999) as u64;
        *occurrence += 1;
        let ts_event = nautilus_core::UnixNanos::from(base_ns + tiebreaker);

        let key = (t.transaction_hash.clone(), t.asset.clone());
        let seq = *tx_asset_counts
            .entry(key)
            .and_modify(|n| *n += 1)
            .or_insert(0);
        let trade_id = TradeId::new(build_polymarket_trade_id(
            &t.transaction_hash,
            &t.asset,
            seq,
        ));

        trades.push(TradeTick::new(
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_event,
        ));
    }

    Ok(trades)
}

#[cfg(test)]
mod tests {
    use nautilus_model::{enums::AggressorSide, identifiers::InstrumentId};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::http::models::{DataApiPosition, DataApiTrade};

    fn load_positions() -> Vec<DataApiPosition> {
        let path = "test_data/data_api_positions_response.json";
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        let page: DataApiPage<DataApiPosition> =
            serde_json::from_str(&content).expect("Failed to parse test data");
        page.data
    }

    fn load_trades() -> Vec<DataApiTrade> {
        // Constructed fixture retained for conversion, filtering, and ordering tests
        let path = "test_data/data_api_trades_response.json";
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        let page: DataApiPage<DataApiTrade> =
            serde_json::from_str(&content).expect("Failed to parse test data");
        page.data
    }

    #[rstest]
    #[case::before(99, true)]
    #[case::at(100, false)]
    #[case::after(101, false)]
    fn test_start_predates_retention_window(#[case] start: i64, #[case] expected: bool) {
        assert_eq!(
            start_predates_retention_window(start, TRADE_RETENTION_SECONDS + 100),
            expected,
        );
    }

    #[rstest]
    #[case::caller_limit(Some(2), TradeTickStop::CallerCapped)]
    #[case::retained_cap(None, TradeTickStop::PageCapReached)]
    fn test_trade_reducer_discards_irrelevant_rows(
        #[case] limit: Option<usize>,
        #[case] expected_stop: TradeTickStop,
    ) {
        let mut reducer = TradeTickReducer {
            rows: Vec::new(),
            instrument_id: test_instrument_id(),
            condition_id: "0xcond".to_string(),
            token_id: "token_aaa".to_string(),
            price_precision: 2,
            size_precision: 2,
            start: None,
            end: Some(UnixNanos::from(100_000_000_000_u64)),
            limit,
        };
        let matching = make_trade(
            100,
            "0xmatch",
            "token_aaa",
            PolymarketOrderSide::Buy,
            0.5,
            2.0,
        );
        let newer = make_trade(
            101,
            "0xnew",
            "token_aaa",
            PolymarketOrderSide::Buy,
            0.6,
            3.0,
        );
        let other = make_trade(
            99,
            "0xother",
            "token_bbb",
            PolymarketOrderSide::Sell,
            0.4,
            4.0,
        );

        for _ in 0..12 {
            let stop = reducer.consume(vec![newer.clone(); 1_000]).unwrap();
            assert_eq!(stop, None);
            assert_eq!(reducer.rows.len(), 0);
        }

        let stop = reducer
            .consume(vec![newer, other, matching.clone()])
            .unwrap();
        assert_eq!(stop, None);
        assert_eq!(reducer.rows.len(), 1);
        assert_eq!(reducer.rows[0].transaction_hash, "0xmatch");

        let target = limit.unwrap_or(MAX_UNBOUNDED_WALK_ROWS);
        let stop = reducer.consume(vec![matching; target - 1]).unwrap();
        assert_eq!(stop, Some(expected_stop));
        assert_eq!(reducer.rows.len(), target);
    }

    #[rstest]
    #[case::start_after_end(
        Some(nautilus_core::UnixNanos::from(2_u64)),
        Some(nautilus_core::UnixNanos::from(1_u64)),
        None,
        "start must not be later than end"
    )]
    #[case::zero_limit(None, None, Some(0), "limit must be greater than zero")]
    #[tokio::test]
    async fn test_request_trade_ticks_rejects_invalid_arguments(
        #[case] start: Option<UnixNanos>,
        #[case] end: Option<UnixNanos>,
        #[case] limit: Option<u32>,
        #[case] expected_error: &str,
    ) {
        let client = PolymarketDataApiHttpClient::new(None, 5).unwrap();

        let error = client
            .request_trade_ticks(
                test_instrument_id(),
                "0xcondition_test",
                "token_aaa",
                2,
                2,
                start,
                end,
                limit,
            )
            .await
            .expect_err("invalid arguments must fail before any request");

        assert_eq!(error.to_string(), expected_error);
    }

    #[rstest]
    fn test_data_api_position_deserialization() {
        let positions = load_positions();

        assert_eq!(positions.len(), 4);
        assert_eq!(positions[0].size, dec!(150.5));
        assert_eq!(positions[0].avg_price, Some(dec!(0.55)));
        assert_eq!(
            positions[0].condition_id,
            "0xc8f1cf5d4f26e0fd9c8fe89f2a7b3263b902cf14fde7bfccef525753bb492e47"
        );
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
        assert_eq!(trades[0].price, dec!(0.55));
        assert_eq!(trades[0].size, dec!(100.0));
        assert_eq!(trades[0].timestamp, 1710000000);
        assert_eq!(
            trades[0].transaction_hash,
            "0xabc123def456789012345678901234567890abcdef1234567890abcdef123456"
        );
    }

    #[rstest]
    fn test_data_api_trade_ignores_extra_fields() {
        let trades = load_trades();
        // proxy_wallet, title, slug should be silently ignored
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
                let price = Price::from_decimal_dp(t.price, price_precision).unwrap();
                let size = Quantity::from_decimal_dp(t.size, size_precision).unwrap();
                let aggressor_side = AggressorSide::from(t.side);
                // TradeId max length is 36; tx hash is 66 chars, take last 36
                let hash = &t.transaction_hash;
                let trade_id_str = if hash.len() > 36 {
                    &hash[hash.len() - 36..]
                } else {
                    hash.as_str()
                };
                let trade_id = TradeId::new(trade_id_str);
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
        assert_eq!(ticks[0].aggressor_side, AggressorSide::Buy);
        assert_eq!(ticks[1].aggressor_side, AggressorSide::Sell);
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
                let price = Price::from_decimal_dp(t.price, 2).unwrap();
                let size = Quantity::from_decimal_dp(t.size, 2).unwrap();
                let aggressor_side = AggressorSide::from(t.side);
                // TradeId max length is 36; tx hash is 66 chars, take last 36
                let hash = &t.transaction_hash;
                let trade_id_str = if hash.len() > 36 {
                    &hash[hash.len() - 36..]
                } else {
                    hash.as_str()
                };
                let trade_id = TradeId::new(trade_id_str);
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

    fn make_trade(
        timestamp: i64,
        transaction_hash: &str,
        asset: &str,
        side: PolymarketOrderSide,
        price: f64,
        size: f64,
    ) -> DataApiTrade {
        DataApiTrade {
            proxy_wallet: None,
            asset: asset.to_string(),
            condition_id: "0xcond".to_string(),
            side,
            price: Decimal::from_str_exact(&price.to_string()).unwrap(),
            size: Decimal::from_str_exact(&size.to_string()).unwrap(),
            timestamp,
            title: None,
            slug: None,
            icon: None,
            event_slug: None,
            outcome: None,
            outcome_index: None,
            name: None,
            pseudonym: None,
            bio: None,
            profile_image: None,
            profile_image_optimized: None,
            transaction_hash: transaction_hash.to_string(),
        }
    }

    fn test_instrument_id() -> InstrumentId {
        InstrumentId::from(
            "0xc8f1cf5d4f26e0fd9c8fe89f2a7b3263b902cf14fde7bfccef525753bb492e47-71321045863084981365469005770620412523470745398083994982746259498689308907982.POLYMARKET",
        )
    }

    #[rstest]
    fn test_data_api_trade_sort_key_orders_pages_deterministically() {
        let mut trades = [
            make_trade(1729000005, "0xZ", "T", PolymarketOrderSide::Buy, 0.5, 1.0),
            make_trade(1729000000, "0xC", "T", PolymarketOrderSide::Buy, 0.5, 1.0),
            make_trade(1729000000, "0xA", "T", PolymarketOrderSide::Sell, 0.5, 1.0),
            make_trade(1729000000, "0xB", "T", PolymarketOrderSide::Buy, 0.5, 1.0),
        ];

        trades.sort_by(|a, b| data_api_trade_sort_key(a).cmp(&data_api_trade_sort_key(b)));

        let order: Vec<&str> = trades.iter().map(|t| t.transaction_hash.as_str()).collect();
        assert_eq!(order, ["0xA", "0xB", "0xC", "0xZ"]);
    }

    #[rstest]
    fn test_data_api_trade_sort_key_uses_full_composite_for_inner_ties() {
        // Locks ordering on the (asset, side, price, size) tail of the key
        let mut trades = [
            // (ts, hash) all equal; tail differs across asset/side/price/size
            make_trade(1, "0xH", "Tb", PolymarketOrderSide::Buy, 0.5, 1.0),
            make_trade(1, "0xH", "Ta", PolymarketOrderSide::Sell, 0.5, 1.0),
            make_trade(1, "0xH", "Ta", PolymarketOrderSide::Buy, 0.6, 1.0),
            make_trade(1, "0xH", "Ta", PolymarketOrderSide::Buy, 0.5, 2.0),
            make_trade(1, "0xH", "Ta", PolymarketOrderSide::Buy, 0.5, 1.0),
        ];

        trades.sort_by(|a, b| data_api_trade_sort_key(a).cmp(&data_api_trade_sort_key(b)));

        // Sort key composite: (ts, hash, asset, side, price, size)
        // Expected ordering across the five trades:
        //   1. asset=Ta side=BUY  price=0.5 size=1.0 (lex-min on side first)
        //   2. asset=Ta side=BUY  price=0.5 size=2.0 (size breaks tie)
        //   3. asset=Ta side=BUY  price=0.6 size=1.0 (price breaks tie)
        //   4. asset=Ta side=SELL price=0.5 size=1.0 (side breaks tie)
        //   5. asset=Tb side=BUY  price=0.5 size=1.0 (asset breaks tie)
        let key: Vec<(String, String, Decimal, Decimal)> = trades
            .iter()
            .map(|t| (t.asset.clone(), t.side.to_string(), t.price, t.size))
            .collect();
        assert_eq!(key[0], ("Ta".into(), "BUY".into(), dec!(0.5), dec!(1.0)));
        assert_eq!(key[1], ("Ta".into(), "BUY".into(), dec!(0.5), dec!(2.0)));
        assert_eq!(key[2], ("Ta".into(), "BUY".into(), dec!(0.6), dec!(1.0)));
        assert_eq!(key[3], ("Ta".into(), "SELL".into(), dec!(0.5), dec!(1.0)));
        assert_eq!(key[4], ("Tb".into(), "BUY".into(), dec!(0.5), dec!(1.0)));
    }

    #[rstest]
    fn test_parse_trade_ticks_filters_other_tokens() {
        let token_id = "T_KEEP";
        let trades = vec![
            make_trade(
                1729000000,
                "0xa",
                token_id,
                PolymarketOrderSide::Buy,
                0.5,
                1.0,
            ),
            make_trade(
                1729000000,
                "0xb",
                "T_DROP",
                PolymarketOrderSide::Sell,
                0.5,
                1.0,
            ),
        ];

        let trades = parse_trade_ticks(trades, test_instrument_id(), token_id, 2, 2).unwrap();

        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].aggressor_side, AggressorSide::Buy);
    }

    #[rstest]
    fn test_parse_trade_ticks_disambiguates_multi_fill_transaction() {
        // Two fills sharing tx + asset must produce distinct TradeIds
        let token_id = "12345token";
        let same_hash = "0x000000000000000000000000000000000000000000000000000000000000abcdef";
        let trades = vec![
            make_trade(
                1729000000,
                same_hash,
                token_id,
                PolymarketOrderSide::Buy,
                0.5,
                1.0,
            ),
            make_trade(
                1729000000,
                same_hash,
                token_id,
                PolymarketOrderSide::Sell,
                0.5,
                1.0,
            ),
        ];

        let trades = parse_trade_ticks(trades, test_instrument_id(), token_id, 2, 2).unwrap();

        assert_eq!(trades.len(), 2);
        assert_ne!(trades[0].trade_id, trades[1].trade_id);
        // ts_event monotonic: same epoch second + nanosecond tiebreaker
        assert!(trades[0].ts_event < trades[1].ts_event);
        assert_eq!(
            u64::from(trades[1].ts_event) - u64::from(trades[0].ts_event),
            1
        );
        // ID format ends with the per-(tx, asset) sequence
        assert!(trades[0].trade_id.to_string().ends_with("-000000"));
        assert!(trades[1].trade_id.to_string().ends_with("-000001"));
    }

    #[rstest]
    fn test_parse_trade_ticks_distinct_tx_share_timestamp() {
        // Different transactions in the same epoch second still get distinct
        // ts_event values (the tiebreaker is per-second, not per-transaction).
        let token_id = "T";
        let trades = vec![
            make_trade(
                1729000000,
                "0xtx1",
                token_id,
                PolymarketOrderSide::Buy,
                0.5,
                1.0,
            ),
            make_trade(
                1729000000,
                "0xtx2",
                token_id,
                PolymarketOrderSide::Buy,
                0.5,
                1.0,
            ),
            make_trade(
                1729000000,
                "0xtx3",
                token_id,
                PolymarketOrderSide::Buy,
                0.5,
                1.0,
            ),
        ];

        let trades = parse_trade_ticks(trades, test_instrument_id(), token_id, 2, 2).unwrap();

        assert_eq!(trades.len(), 3);
        // Strictly increasing ts_event
        assert!(trades[0].ts_event < trades[1].ts_event);
        assert!(trades[1].ts_event < trades[2].ts_event);
        // Each trade is the first fill on its (tx, asset) so all have seq 0
        for trade in &trades {
            assert!(trade.trade_id.to_string().ends_with("-000000"));
        }
    }

    #[rstest]
    fn test_parse_trade_ticks_assigns_per_second_tiebreakers() {
        // Same-second fills get strictly increasing nanosecond tiebreakers
        // starting at zero, all bounded below 1 second.
        let token_id = "T";
        let mut trades = Vec::new();

        for i in 0..3 {
            let hash = format!("0x{i:064x}");
            trades.push(make_trade(
                1729000000,
                &hash,
                token_id,
                PolymarketOrderSide::Buy,
                0.5,
                1.0,
            ));
        }

        let trades = parse_trade_ticks(trades, test_instrument_id(), token_id, 2, 2).unwrap();

        assert_eq!(trades.len(), 3);
        let base_ns = 1_729_000_000u64 * 1_000_000_000;

        for (i, trade) in trades.iter().enumerate() {
            assert!(u64::from(trade.ts_event) - base_ns < 1_000_000_000);
            assert_eq!(u64::from(trade.ts_event) - base_ns, i as u64);
        }
    }

    #[rstest]
    fn test_parse_trade_ticks_sorts_inputs_by_composite_key() {
        // Mirror what the API may return: same-second fills delivered out
        // of order. parse_trade_ticks must produce a deterministic stream.
        let token_id = "T";
        let trades = vec![
            make_trade(
                1729000005,
                "0xZ",
                token_id,
                PolymarketOrderSide::Buy,
                0.5,
                1.0,
            ),
            make_trade(
                1729000000,
                "0xC",
                token_id,
                PolymarketOrderSide::Buy,
                0.5,
                1.0,
            ),
            make_trade(
                1729000000,
                "0xA",
                token_id,
                PolymarketOrderSide::Sell,
                0.5,
                1.0,
            ),
            make_trade(
                1729000000,
                "0xB",
                token_id,
                PolymarketOrderSide::Buy,
                0.5,
                1.0,
            ),
        ];

        let trades = parse_trade_ticks(trades, test_instrument_id(), token_id, 2, 2).unwrap();

        assert_eq!(trades.len(), 4);

        // Strictly non-decreasing ts_event
        for i in 1..trades.len() {
            assert!(trades[i - 1].ts_event <= trades[i].ts_event);
        }

        // Composite tiebreaker: same-second trades order by transaction_hash
        let trade_ids: Vec<String> = trades.iter().map(|t| t.trade_id.to_string()).collect();
        assert!(trade_ids[0].contains("0xA"));
        assert!(trade_ids[1].contains("0xB"));
        assert!(trade_ids[2].contains("0xC"));
        assert!(trade_ids[3].contains("0xZ"));
    }

    #[rstest]
    fn test_parse_trade_ticks_propagates_invalid_price() {
        let token_id = "T";
        let mut trade = make_trade(
            1729000000,
            "0xtx",
            token_id,
            PolymarketOrderSide::Buy,
            0.5,
            1.0,
        );
        trade.price = Decimal::from_str_exact("99999999999999999999.99").unwrap();

        let error = parse_trade_ticks(vec![trade], test_instrument_id(), token_id, 2, 2)
            .expect_err("out-of-range price should fail");

        assert_eq!(
            error.to_string(),
            "failed to convert Data API trade price 99999999999999999999.99 with precision 2"
        );
        assert_eq!(error.chain().count(), 2);
    }

    #[rstest]
    fn test_parse_trade_ticks_propagates_invalid_size() {
        let token_id = "T";
        let trade = make_trade(
            1729000000,
            "0xtx",
            token_id,
            PolymarketOrderSide::Buy,
            0.5,
            -1.5,
        );

        let error = parse_trade_ticks(vec![trade], test_instrument_id(), token_id, 2, 2)
            .expect_err("negative size should fail");

        assert_eq!(
            error.to_string(),
            "failed to convert Data API trade size -1.5 with precision 2"
        );
        assert_eq!(error.chain().count(), 2);
    }
}
