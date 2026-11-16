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

use std::num::NonZeroUsize;

use anyhow::Context;
use nautilus_core::{UnixNanos, datetime::try_datetime_to_unix_nanos, params::Params};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
    identifiers::TradeId,
    instruments::BinaryOption,
    types::Quantity,
};
use rust_decimal::Decimal;

use super::{parse, request_bounds};
use crate::{
    common::enums::CandlestickInterval,
    http::{
        models::{BookSide, Candlestick, Field, FixedPoint, OutcomeSide, Trade},
        query::{CandlesticksQuery, TradesQuery},
    },
};

pub(super) struct HistoryRange {
    start: UnixNanos,
    end: UnixNanos,
    limit: Option<NonZeroUsize>,
    pub(super) max_pages: u32,
}

impl HistoryRange {
    pub(super) fn new(
        start: Option<jiff::Timestamp>,
        end: Option<jiff::Timestamp>,
        limit: Option<NonZeroUsize>,
        params: Option<&Params>,
        now: UnixNanos,
    ) -> anyhow::Result<Self> {
        let (start, end) = request_bounds(start, end)?;
        let start = start.context("Kalshi history requires a start bound")?;
        let end = end.unwrap_or(now);
        anyhow::ensure!(start <= end, "invalid history interval");
        let mut max_pages = 100;

        if let Some(params) = params {
            for (key, value) in params {
                match key.as_str() {
                    "max_pages" => {
                        max_pages = u32::try_from(
                            value
                                .as_u64()
                                .context("max_pages must be a positive integer")?,
                        )?;
                        anyhow::ensure!(max_pages > 0, "max_pages must be positive");
                    }
                    "bar_types"
                    | "update_subscriptions"
                    | "skip_first_non_full_bar"
                    | "time_zone"
                    | "durations_seconds"
                    | "point_data"
                    | "data_count"
                    | "subscription_name"
                    | "update_catalog"
                    | "skip_catalog_data"
                    | "filter_expr"
                    | "only_last"
                    | "from_day_start" => {}
                    _ => anyhow::bail!("unsupported Kalshi history parameter: {key}"),
                }
            }
        }

        Ok(Self {
            start,
            end,
            limit,
            max_pages,
        })
    }

    pub(super) fn trades_query(&self, ticker: String) -> TradesQuery {
        TradesQuery {
            ticker: Some(ticker),
            min_ts: Some((self.start.as_u64() / 1_000_000_000).saturating_sub(1) as i64),
            max_ts: Some((self.end.as_u64() / 1_000_000_000 + 1) as i64),
            limit: Some(1000),
            is_block_trade: Some(false),
            ..Default::default()
        }
    }

    pub(super) fn candles_query(&self, period_interval: CandlestickInterval) -> CandlesticksQuery {
        CandlesticksQuery {
            start_ts: (self.start.as_u64() / 1_000_000_000) as i64,
            end_ts: (self.end.as_u64() / 1_000_000_000) as i64,
            period_interval,
        }
    }

    pub(super) fn contains(&self, time: UnixNanos) -> bool {
        time >= self.start && time <= self.end
    }

    pub(super) fn truncate<T>(&self, records: &mut Vec<T>) {
        if let Some(limit) = self.limit {
            records.truncate(limit.get());
        }
    }
}

pub(super) fn period(bar_type: BarType) -> anyhow::Result<CandlestickInterval> {
    anyhow::ensure!(
        bar_type.is_standard() && bar_type.aggregation_source() == AggregationSource::External,
        "Kalshi history requires standard external bars"
    );
    let spec = bar_type.spec();
    anyhow::ensure!(
        matches!(
            spec.price_type,
            PriceType::Last | PriceType::Bid | PriceType::Ask
        ),
        "unsupported Kalshi candle price type"
    );
    anyhow::ensure!(spec.step.get() == 1, "unsupported Kalshi candle interval");

    match spec.aggregation {
        BarAggregation::Minute => Ok(CandlestickInterval::Minute),
        BarAggregation::Hour => Ok(CandlestickInterval::Hour),
        BarAggregation::Day => Ok(CandlestickInterval::Day),
        _ => anyhow::bail!("unsupported Kalshi candle interval"),
    }
}

pub(super) fn trade(value: Trade, instrument: &BinaryOption) -> anyhow::Result<TradeTick> {
    anyhow::ensure!(
        value.ticker == instrument.raw_symbol.as_str(),
        "trade instrument mismatch"
    );
    anyhow::ensure!(
        !value.is_block_trade,
        "block trades are excluded from ordinary market history"
    );
    anyhow::ensure!(
        value
            .yes_price_dollars
            .0
            .checked_add(value.no_price_dollars.0)
            == Some(Decimal::ONE),
        "inconsistent trade prices"
    );

    let aggressor = match (value.taker_book_side, value.taker_outcome_side) {
        (BookSide::Bid, OutcomeSide::Yes) => AggressorSide::Buy,
        (BookSide::Ask, OutcomeSide::No) => AggressorSide::Sell,
        _ => anyhow::bail!("inconsistent canonical taker direction"),
    };

    let price = parse::price(value.yes_price_dollars, instrument.price_precision)?;
    let size = parse::size(value.count_fp, instrument.size_precision)?;
    let trade_id = TradeId::new_checked(value.trade_id)?;
    let ts_event = try_datetime_to_unix_nanos(value.created_time)?;
    TradeTick::new_checked(
        instrument.id,
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_event,
    )
}

pub(super) fn bar(
    value: Candlestick,
    bar_type: BarType,
    instrument: &BinaryOption,
) -> anyhow::Result<Option<Bar>> {
    let fields = match bar_type.spec().price_type {
        PriceType::Last => [
            value.price.open_dollars,
            value.price.high_dollars,
            value.price.low_dollars,
            value.price.close_dollars,
        ],
        PriceType::Bid => [
            Field::Value(value.yes_bid.open_dollars),
            Field::Value(value.yes_bid.high_dollars),
            Field::Value(value.yes_bid.low_dollars),
            Field::Value(value.yes_bid.close_dollars),
        ],
        PriceType::Ask => [
            Field::Value(value.yes_ask.open_dollars),
            Field::Value(value.yes_ask.high_dollars),
            Field::Value(value.yes_ask.low_dollars),
            Field::Value(value.yes_ask.close_dollars),
        ],
        _ => anyhow::bail!("unsupported Kalshi candle price type"),
    };

    let volume = volume(value.volume_fp, instrument.size_precision)?;

    if fields
        .iter()
        .all(|f| matches!(f, Field::Null | Field::Missing))
        && volume.is_zero()
    {
        return Ok(None);
    }

    let [open, high, low, close] = fields.map(|field| match field {
        Field::Value(v) => parse::price(v, instrument.price_precision),
        Field::Null => anyhow::bail!("null candle price with nonempty OHLCV"),
        Field::Missing => anyhow::bail!("missing candle price"),
    });

    let (open, high, low, close) = (open?, high?, low?, close?);
    let ts_event = candle_time(value.end_period_ts)?;
    let bar = Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_event)?;
    Ok(Some(bar))
}

pub(super) fn candle_time(seconds: i64) -> anyhow::Result<UnixNanos> {
    let timestamp = jiff::Timestamp::from_second(seconds)?;
    try_datetime_to_unix_nanos(timestamp)
}

fn volume(value: FixedPoint, precision: u8) -> anyhow::Result<Quantity> {
    anyhow::ensure!(value.0 >= Decimal::ZERO, "negative candle volume");
    let quantity = Quantity::from_decimal_dp(value.0, precision)?;
    anyhow::ensure!(
        quantity.as_decimal() == value.0,
        "volume exceeds instrument precision"
    );
    Ok(quantity)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    #[rstest]
    #[case("1-MINUTE-LAST-EXTERNAL", Some(1))]
    #[case("1-HOUR-BID-EXTERNAL", Some(60))]
    #[case("1-DAY-ASK-EXTERNAL", Some(1440))]
    #[case("5-MINUTE-LAST-EXTERNAL", None)]
    #[case("1-MINUTE-MID-EXTERNAL", None)]
    #[case("1-MINUTE-LAST-INTERNAL", None)]
    fn native_periods_reject_unsupported_specs(#[case] spec: &str, #[case] expected: Option<u32>) {
        let bar_type = format!("TEST.KALSHI-{spec}").parse().unwrap();
        let result = period(bar_type);

        match expected {
            Some(expected) => assert_eq!(u32::from(result.unwrap()), expected),
            None => assert!(result.is_err()),
        }
    }

    #[rstest]
    #[case("all_null", None)]
    #[case("all_missing", None)]
    #[case("mixed_empty", None)]
    #[case("null_with_volume", Some("null candle price with nonempty OHLCV"))]
    #[case("one_null", Some("null candle price with nonempty OHLCV"))]
    #[case("missing", Some("missing candle price"))]
    #[case("negative_volume", Some("negative candle volume"))]
    #[case("precision", Some("volume exceeds instrument precision"))]
    fn candle_null_missing_and_volume_cases(
        #[case] scenario: &str,
        #[case] expected: Option<&str>,
    ) {
        let f: Value = serde_json::from_str(include_str!("../../test_data/history.json")).unwrap();
        let market = serde_json::from_value(f["market"]["market"].clone()).unwrap();
        let event = serde_json::from_value(f["event"]["event"].clone()).unwrap();
        let now = UnixNanos::from(1700000100000000000);
        let instrument = crate::http::parse::parse_instrument(&market, &event, now).unwrap();
        let mut candle = f["candles"]["candlesticks"][0].clone();

        match scenario {
            "all_null" | "all_missing" | "mixed_empty" | "null_with_volume" => {
                for key in [
                    "open_dollars",
                    "high_dollars",
                    "low_dollars",
                    "close_dollars",
                ] {
                    if scenario == "all_missing"
                        || (scenario == "mixed_empty"
                            && matches!(key, "open_dollars" | "close_dollars"))
                    {
                        candle["price"].as_object_mut().unwrap().remove(key);
                    } else {
                        candle["price"][key] = Value::Null;
                    }
                }

                if scenario == "null_with_volume" {
                    candle["volume_fp"] = "1.00".into();
                }
            }
            "one_null" => candle["price"]["open_dollars"] = Value::Null,
            "missing" => {
                candle["price"]
                    .as_object_mut()
                    .unwrap()
                    .remove("open_dollars");
            }
            "negative_volume" => candle["volume_fp"] = "-0.01".into(),
            "precision" => candle["volume_fp"] = "0.001".into(),
            _ => unreachable!(),
        }

        let bar_type = format!("{}-1-MINUTE-LAST-EXTERNAL", instrument.id)
            .parse()
            .unwrap();
        let result = bar(
            serde_json::from_value(candle).unwrap(),
            bar_type,
            &instrument,
        );

        match expected {
            Some(message) => assert_eq!(result.unwrap_err().to_string(), message),
            None => assert_eq!(result.unwrap(), None),
        }
    }

    #[rstest]
    #[case("start")]
    #[case("reversed")]
    #[case("params")]
    #[case("budget")]
    fn invalid_history_ranges_fail(#[case] scenario: &str) {
        let start = Some("2023-11-14T22:14:00Z".parse().unwrap());
        let end = Some("2023-11-14T22:13:00Z".parse().unwrap());
        let now = UnixNanos::from(1700000100000000000);
        let result = match scenario {
            "start" => HistoryRange::new(None, None, None, None, now),
            "reversed" => HistoryRange::new(start, end, None, None, now),
            "params" => {
                let params = Params::new([("historical".into(), true.into())].into());
                HistoryRange::new(start, None, None, Some(&params), now)
            }
            "budget" => {
                let params = Params::new([("max_pages".into(), 0.into())].into());
                HistoryRange::new(start, None, None, Some(&params), now)
            }
            _ => unreachable!(),
        };

        let message = match scenario {
            "start" => "Kalshi history requires a start bound",
            "reversed" => "invalid update interval",
            "params" => "unsupported Kalshi history parameter: historical",
            "budget" => "max_pages must be positive",
            _ => unreachable!(),
        };

        assert_eq!(result.err().unwrap().to_string(), message);
    }
}
