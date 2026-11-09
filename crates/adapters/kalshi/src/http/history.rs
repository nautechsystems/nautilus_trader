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

//! Bounded live/archive market-data collection.

use std::collections::HashMap;

use super::{
    client::KalshiHttpClient,
    error::KalshiHttpError,
    models::{
        BidAskDistribution, Candlestick, Field, FixedPoint, HistoricalBidAskDistribution,
        HistoricalCandlestick, Market, PriceDistribution, Trade,
    },
    parse::Pagination,
    query::{CandlesticksQuery, TradesQuery},
};
use crate::common::enums::MarketStatus;

impl KalshiHttpClient {
    /// Collects a bounded trade window, oldest first with trade ID breaking timestamp ties.
    ///
    /// The cutoff is read once. A window reaching it reads live pages before a full-window
    /// archive probe to cover a forward handoff between the reads. The venue does not promise an
    /// atomic snapshot across pages. `limit` is a page size, not a result limit.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The query is invalid, unbounded, or resumed.
    /// - Pagination is incomplete, duplicates conflict, or records do not match.
    /// - Any source fails.
    pub async fn get_trades_range(
        &self,
        query: TradesQuery,
        max_pages: u32,
    ) -> Result<Vec<Trade>, KalshiHttpError> {
        let (Some(start), Some(end), Some(_)) =
            (query.min_ts, query.max_ts, query.ticker.as_deref())
        else {
            return Err(KalshiHttpError::Validation(
                "trade history requires ticker and both time bounds",
            ));
        };

        if start < 0
            || start > end
            || query.cursor.is_some()
            || max_pages == 0
            || query.limit.is_some_and(|v| v == 0 || v > 1000)
        {
            return Err(KalshiHttpError::Validation("invalid bounded trade query"));
        }

        let cutoff = self.get_historical_cutoff().await?;
        let mut records = HashMap::new();
        if end >= cutoff.trades_created_ts.as_second() {
            self.collect_trade_range(&query, max_pages, false, &mut records)
                .await?;
        }

        self.collect_trade_range(&query, max_pages, true, &mut records)
            .await?;
        let mut records: Vec<_> = records
            .into_values()
            .filter(|t| t.created_time.as_second() >= start && t.created_time.as_second() <= end)
            .collect();
        records.sort_by(|a, b| {
            a.created_time
                .cmp(&b.created_time)
                .then_with(|| a.trade_id.cmp(&b.trade_id))
        });

        Ok(records)
    }

    async fn collect_trade_range(
        &self,
        query: &TradesQuery,
        max_pages: u32,
        historical: bool,
        records: &mut HashMap<String, Trade>,
    ) -> Result<(), KalshiHttpError> {
        let mut query = query.clone();
        let mut pagination = Pagination::new(None);

        for _ in 0..max_pages {
            let page = if historical {
                self.get_historical_trades(&query).await?
            } else {
                self.get_trades(&query).await?
            };

            let empty = page.trades.is_empty();
            for trade in page.trades {
                merge_trade(records, trade, &query)?;
            }

            query.cursor = pagination.advance(page.cursor, empty)?;
            if query.cursor.is_none() {
                return Ok(());
            }
        }

        Err(KalshiHttpError::Pagination(
            "trade history page budget exhausted",
        ))
    }

    /// Fetches native candles using market settlement time to select the archive partition.
    ///
    /// The cutoff is read once. A settled market still in the live tier receives an archive
    /// probe after the live read to cover a forward handoff. Endpoint time bounds apply to candle end timestamps.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Bounds or periods are invalid, or a finalized market has no settlement evidence.
    /// - Market identity does not match, candles conflict, or a source fails.
    pub async fn get_candlesticks_range(
        &self,
        market: &Market,
        series: &str,
        query: &CandlesticksQuery,
    ) -> Result<Vec<Candlestick>, KalshiHttpError> {
        if query.start_ts < 0 || query.start_ts > query.end_ts {
            return Err(KalshiHttpError::Validation(
                "invalid candle window or period",
            ));
        }

        let cutoff = self.get_historical_cutoff().await?;

        let settled = match market.settlement_ts {
            Field::Value(value) => Some(value),
            _ if market.status == MarketStatus::Finalized => {
                return Err(KalshiHttpError::Parse(
                    "finalized market has no settlement timestamp".into(),
                ));
            }
            _ => None,
        };

        let archived = settled.is_some_and(|t| t < cutoff.market_settled_ts);
        let mut records = None;

        if !archived {
            match self
                .candlestick_partition(market, series, query, false)
                .await
            {
                Ok(candles) => records = Some(candles),
                Err(e) if settled.is_some() && missing(&e) => {}
                Err(e) => return Err(e),
            }
        }

        if archived || settled.is_some() {
            match self
                .candlestick_partition(market, series, query, true)
                .await
            {
                Ok(candles) => records.get_or_insert_with(Vec::new).extend(candles),
                Err(e) if records.is_some() && missing(&e) => {}
                Err(e) => return Err(e),
            }
        }

        merge_candles(records.unwrap_or_default(), query)
    }

    async fn candlestick_partition(
        &self,
        market: &Market,
        series: &str,
        query: &CandlesticksQuery,
        historical: bool,
    ) -> Result<Vec<Candlestick>, KalshiHttpError> {
        let (ticker, records) = if historical {
            let response = self
                .get_historical_candlesticks(&market.ticker, query)
                .await?;

            (
                response.ticker,
                response.candlesticks.into_iter().map(normalize).collect(),
            )
        } else {
            let response = self.get_candlesticks(series, &market.ticker, query).await?;
            (response.ticker, response.candlesticks)
        };

        if ticker != market.ticker {
            return Err(KalshiHttpError::Parse(
                "candle market identity mismatch".into(),
            ));
        }

        Ok(records)
    }
}

fn merge_trade(
    records: &mut HashMap<String, Trade>,
    trade: Trade,
    query: &TradesQuery,
) -> Result<(), KalshiHttpError> {
    if Some(&trade.ticker) != query.ticker.as_ref()
        || query
            .is_block_trade
            .is_some_and(|v| trade.is_block_trade != v)
    {
        return Err(KalshiHttpError::Parse(
            "trade does not match requested filters".into(),
        ));
    }

    if let Some(previous) = records.get(&trade.trade_id) {
        if previous != &trade {
            return Err(KalshiHttpError::Parse("conflicting duplicate trade".into()));
        }
    } else {
        records.insert(trade.trade_id.clone(), trade);
    }

    Ok(())
}

fn missing(error: &KalshiHttpError) -> bool {
    matches!(
        error,
        KalshiHttpError::Http { status: 404, .. }
            | KalshiHttpError::Venue { status: 404, .. }
            | KalshiHttpError::Status(404)
    )
}

fn merge_candles(
    mut records: Vec<Candlestick>,
    query: &CandlesticksQuery,
) -> Result<Vec<Candlestick>, KalshiHttpError> {
    records.sort_by_key(|c| c.end_period_ts);
    let mut result: Vec<Candlestick> = Vec::new();

    for candle in records {
        if candle.end_period_ts < query.start_ts || candle.end_period_ts > query.end_ts {
            continue;
        }

        if let Some(previous) = result.last()
            && previous.end_period_ts == candle.end_period_ts
        {
            if previous.yes_bid != candle.yes_bid
                || previous.yes_ask != candle.yes_ask
                || previous.price.open_dollars != candle.price.open_dollars
                || previous.price.high_dollars != candle.price.high_dollars
                || previous.price.low_dollars != candle.price.low_dollars
                || previous.price.close_dollars != candle.price.close_dollars
                || previous.volume_fp != candle.volume_fp
            {
                return Err(KalshiHttpError::Parse(
                    "conflicting duplicate candle".into(),
                ));
            }

            continue;
        }

        result.push(candle);
    }

    Ok(result)
}

fn normalize(value: HistoricalCandlestick) -> Candlestick {
    let prices = |value: HistoricalBidAskDistribution| BidAskDistribution {
        open_dollars: value.open,
        low_dollars: value.low,
        high_dollars: value.high,
        close_dollars: value.close,
    };

    let field = |value: Option<FixedPoint>| value.map_or(Field::Null, Field::Value);

    Candlestick {
        end_period_ts: value.end_period_ts,
        yes_bid: prices(value.yes_bid),
        yes_ask: prices(value.yes_ask),
        price: PriceDistribution {
            open_dollars: field(value.price.open),
            low_dollars: field(value.price.low),
            high_dollars: field(value.price.high),
            close_dollars: field(value.price.close),
            mean_dollars: field(value.price.mean),
            previous_dollars: field(value.price.previous),
            min_dollars: Field::Missing,
            max_dollars: Field::Missing,
        },
        volume_fp: value.volume,
        open_interest_fp: value.open_interest,
    }
}
