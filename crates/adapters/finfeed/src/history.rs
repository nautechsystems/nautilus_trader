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

//! Bounded historical pagination without a source-completeness guarantee.

use std::{collections::HashSet, future::Future, num::NonZeroUsize, ops::RangeInclusive};

use anyhow::Context;
use jiff::Timestamp;
use nautilus_model::{
    data::{Bar, BarType, HasTsInit, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggregationSource, BarAggregation, PriceType},
    instruments::BinaryOption,
};

use super::{book, parse};
use crate::http::{
    client::FinFeedHttpClient,
    error::Error,
    models::{BookEvent, Ohlcv, OhlcvPeriod, Quote, Trade},
    query::{HOUR_SECONDS, HistoryQuery, OHLCV_LIMIT_MAX, OhlcvQuery, PAGE_LIMIT_MAX},
};

const DAY_SECONDS: i64 = 86_400;

#[derive(Debug)]
pub(crate) struct Window {
    start: Timestamp,
    end: Timestamp,
    limit: Option<NonZeroUsize>,
}

impl Window {
    pub(crate) fn new(
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<NonZeroUsize>,
        now: Timestamp,
    ) -> anyhow::Result<Self> {
        let start = start.context("FinFeed history requires a start time")?;
        let end = end.unwrap_or(now);
        anyhow::ensure!(start <= end, "FinFeed history start exceeds end");
        parse::timestamp(start)?;
        parse::timestamp(end)?;
        Ok(Self { start, end, limit })
    }

    fn partitions(&self, seconds: i64) -> RangeInclusive<i64> {
        self.start.as_second().div_euclid(seconds)..=self.end.as_second().div_euclid(seconds)
    }

    fn check_budget(&self, http: &FinFeedHttpClient, seconds: i64) -> anyhow::Result<()> {
        let partitions = self.partitions(seconds);
        let required = partitions.end() - partitions.start() + 1;
        let remaining = http.attempts_remaining();
        anyhow::ensure!(
            required as u64 <= remaining,
            "FinFeed history window [{}, {}] requires at least {required} requests, but the local attempt allowance has {remaining} remaining; shorten the window or configure a larger attempt_budget; retries and polling share this allowance",
            self.start,
            self.end
        );

        Ok(())
    }

    fn finish<T: HasTsInit>(&self, mut records: Vec<T>) -> anyhow::Result<Vec<T>> {
        let start = parse::timestamp(self.start)?;
        let end = parse::timestamp(self.end)?;
        records.retain(|record| record.ts_init() >= start && record.ts_init() <= end);
        records.sort_by_key(HasTsInit::ts_init);
        if let Some(limit) = self.limit {
            records.truncate(limit.get());
        }

        Ok(records)
    }
}

pub(crate) async fn books(
    http: &FinFeedHttpClient,
    instrument: &BinaryOption,
    window: &Window,
) -> anyhow::Result<Vec<OrderBookDeltas>> {
    window.check_budget(http, HOUR_SECONDS)?;
    let (exchange, id) = crate::common::symbol::from_instrument_id(instrument.id)?;
    let records = collect_books(window, |query| {
        let exchange = &exchange;
        let id = &id;
        async move { http.get_book_history(exchange, id, &query).await }
    })
    .await?;

    book::replay(
        &records,
        instrument,
        parse::timestamp(window.start)?,
        parse::timestamp(window.end)?,
    )
    .with_context(|| {
        format!(
            "FinFeed book replay failed for {} in [{}, {}]; retrieval starts at the containing UTC hour and does not look back for an earlier snapshot",
            instrument.id, window.start, window.end
        )
    })
}

pub(crate) async fn quotes(
    http: &FinFeedHttpClient,
    instrument: &BinaryOption,
    window: &Window,
) -> anyhow::Result<Vec<QuoteTick>> {
    window.check_budget(http, HOUR_SECONDS)?;
    let (exchange, id) = crate::common::symbol::from_instrument_id(instrument.id)?;

    let records = collect_hours(
        window,
        |row: &Quote| row.recv_time,
        |_| false,
        |query| {
            let exchange = &exchange;
            let id = &id;
            async move { http.get_quotes_history(exchange, id, &query).await }
        },
    )
    .await?;

    let parsed = records
        .into_iter()
        .map(|record| parse::quote(&record, instrument))
        .collect::<anyhow::Result<Vec<_>>>()?;
    window.finish(parsed)
}

pub(crate) async fn trades(
    http: &FinFeedHttpClient,
    instrument: &BinaryOption,
    window: &Window,
) -> anyhow::Result<Vec<TradeTick>> {
    window.check_budget(http, HOUR_SECONDS)?;
    let (exchange, id) = crate::common::symbol::from_instrument_id(instrument.id)?;

    let records = collect_hours(
        window,
        |row: &Trade| row.timestamp,
        |_| false,
        |query| {
            let exchange = &exchange;
            let id = &id;
            async move { http.get_trades_history(exchange, id, &query).await }
        },
    )
    .await?;

    let parsed = records
        .into_iter()
        .map(|record| parse::trade(&record, instrument))
        .collect::<anyhow::Result<Vec<_>>>()?;
    window.finish(parsed)
}

pub(crate) async fn bars(
    http: &FinFeedHttpClient,
    instrument: &BinaryOption,
    bar_type: BarType,
    window: &Window,
    now: Timestamp,
    period_length: (i64, i64),
) -> anyhow::Result<Vec<Bar>> {
    if window.start == window.end {
        return Ok(Vec::new());
    }

    let periods = http.get_periods().await?;
    let period = period(&periods, period_length)?;
    let (exchange, id) = crate::common::symbol::from_instrument_id(instrument.id)?;
    let records = collect_bars(window, period, |query| {
        let exchange = &exchange;
        let id = &id;
        async move { http.get_ohlcv_history(exchange, id, &query).await }
    })
    .await?;

    let mut bars = Vec::new();

    for record in records {
        if record.time_period_start < window.start || record.time_period_end > window.end.min(now) {
            continue;
        }

        if let Some(bar) = parse::bar(&record, bar_type, instrument, period)? {
            bars.push(bar);
        }
    }

    window.finish(bars)
}

pub(crate) fn period_length(bar_type: BarType) -> anyhow::Result<(i64, i64)> {
    anyhow::ensure!(
        bar_type.spec().price_type == PriceType::Last,
        "FinFeed OHLCV uses last prices"
    );
    anyhow::ensure!(
        bar_type.aggregation_source() == AggregationSource::External,
        "FinFeed provides external OHLCV bars only"
    );
    let spec = bar_type.spec();
    let step = i64::try_from(spec.step.get())?;
    let length = match spec.aggregation {
        BarAggregation::Second => (step, 0),
        BarAggregation::Minute => (step.checked_mul(60).context("bar duration overflow")?, 0),
        BarAggregation::Hour => (step.checked_mul(3600).context("bar duration overflow")?, 0),
        BarAggregation::Day => (
            step.checked_mul(DAY_SECONDS)
                .context("bar duration overflow")?,
            0,
        ),
        BarAggregation::Week => (
            step.checked_mul(7 * DAY_SECONDS)
                .context("bar duration overflow")?,
            0,
        ),
        BarAggregation::Month => (0, step),
        BarAggregation::Year => (0, step.checked_mul(12).context("bar duration overflow")?),
        _ => anyhow::bail!("unsupported FinFeed bar aggregation"),
    };

    Ok(length)
}

pub(crate) fn period(
    periods: &[OhlcvPeriod],
    (seconds, months): (i64, i64),
) -> anyhow::Result<&OhlcvPeriod> {
    periods
        .iter()
        .find(|period| {
            i64::from(period.length_seconds) == seconds && i64::from(period.length_months) == months
        })
        .filter(|period| {
            period
                .period_id
                .as_ref()
                .is_some_and(|id| !id.as_ref().trim().is_empty())
        })
        .context("FinFeed has no matching OHLCV period")
}

async fn collect_books<F, Fut>(window: &Window, fetch: F) -> anyhow::Result<Vec<BookEvent>>
where
    F: FnMut(HistoryQuery) -> Fut,
    Fut: Future<Output = Result<Vec<BookEvent>, Error>>,
{
    collect_hours(
        window,
        |row| row.time_exchange,
        |rows| rows.iter().any(|row| book::time_ordering(row) > window.end),
        fetch,
    )
    .await
}

async fn collect_hours<T, F, Fut>(
    window: &Window,
    clock: impl Fn(&T) -> Timestamp,
    stop: impl Fn(&[T]) -> bool,
    mut fetch: F,
) -> anyhow::Result<Vec<T>>
where
    T: Clone + PartialEq,
    F: FnMut(HistoryQuery) -> Fut,
    Fut: Future<Output = Result<Vec<T>, Error>>,
{
    let hours = window.partitions(HOUR_SECONDS);
    let mut start = Timestamp::from_second(hours.start() * HOUR_SECONDS)?;
    let mut overlap = Vec::new();
    let mut records = Vec::new();

    for hour in hours {
        let end = Timestamp::from_second((hour + 1) * HOUR_SECONDS)?;

        loop {
            let query = HistoryQuery {
                date: None,
                time_start: Some(start),
                time_end: Some(end),
                limit: PAGE_LIMIT_MAX,
            };

            let page = fetch(query).await?;
            anyhow::ensure!(
                page.starts_with(&overlap),
                "FinFeed history overlap changed in [{start}, {end}]; no result is published"
            );
            let full = page.len() == PAGE_LIMIT_MAX as usize;
            let matched = overlap.len();
            let added = page.len() - matched;
            if stop(&page) {
                records.extend(page.into_iter().skip(matched));
                return Ok(records);
            }

            let continuing = full || hour < *window.partitions(HOUR_SECONDS).end();
            let mut next = start;
            overlap.clear();

            if continuing && let Some(maximum) = page.iter().map(&clock).max() {
                next = start.max(Timestamp::from_second(maximum.as_second() - 1)?);
                anyhow::ensure!(
                    next < end,
                    "FinFeed history continuation clock exceeds its hourly selector; no result is published"
                );
                let offset = page
                    .iter()
                    .position(|row| clock(row) >= next)
                    .context("FinFeed history has no continuation overlap")?;
                anyhow::ensure!(
                    page[offset..].iter().all(|row| clock(row) >= next),
                    "FinFeed history overlap is not a contiguous suffix; no result is published"
                );
                overlap.extend_from_slice(&page[offset..]);
            }

            anyhow::ensure!(
                !full || (added > 0 && next > start),
                "FinFeed history cannot advance within the {PAGE_LIMIT_MAX}-record cap in [{start}, {end}]; no result is published"
            );
            records.extend(page.into_iter().skip(matched));
            start = next;

            if !full {
                break;
            }
        }
    }

    Ok(records)
}

async fn collect_bars<F, Fut>(
    window: &Window,
    period: &OhlcvPeriod,
    mut fetch: F,
) -> anyhow::Result<Vec<Ohlcv>>
where
    F: FnMut(OhlcvQuery) -> Fut,
    Fut: Future<Output = Result<Vec<Ohlcv>, Error>>,
{
    let mut start = Timestamp::from_nanosecond(window.start.as_nanosecond().div_euclid(100) * 100)?;
    let end = Timestamp::from_nanosecond((window.end.as_nanosecond() + 99).div_euclid(100) * 100)?;
    let period_id = period.period_id.context("FinFeed period ID is missing")?;
    let mut records = Vec::new();

    loop {
        let query = OhlcvQuery {
            period_id,
            time_start: Some(start),
            time_end: Some(end),
            limit: OHLCV_LIMIT_MAX,
        };

        let page = fetch(query).await?;
        if page.is_empty() {
            return Ok(records);
        }

        let mut next = start;
        let mut periods = HashSet::new();

        for row in &page {
            let period_end = parse::bar_end(row, period)?;
            anyhow::ensure!(
                periods.insert(row.time_period_start)
                    && (records.is_empty() || row.time_period_start >= start),
                "FinFeed OHLCV history contains repeated or overlapping periods; no result is published"
            );
            next = next.max(period_end);
        }

        anyhow::ensure!(
            next > start,
            "FinFeed OHLCV history cannot advance past {start}; no result is published"
        );
        records.extend(page);
        if next >= end {
            return Ok(records);
        }

        start = next;
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::BarSpecification, enums::AggregationSource, identifiers::InstrumentId,
    };
    use nautilus_network::dst::{net::TcpListener, task};
    use rstest::rstest;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::{
        common::enums::OhlcvUnit,
        http::{client::FinFeedHttpConfig, parse::parse_instrument},
    };

    #[tokio::test]
    async fn book_pages_preserve_overlap_multiplicity_and_late_rows() {
        let source: Vec<BookEvent> =
            serde_json::from_str(include_str!("../test_data/book-replay.json")).unwrap();
        let mut first = vec![source[0].clone(); PAGE_LIMIT_MAX as usize];
        for row in &mut first[9997..] {
            row.time_exchange = "2026-10-11T00:00:10.9502711Z".parse().unwrap();
        }

        let mut late = source[4].clone();
        late.time_exchange = "2026-10-11T00:00:10.9500713Z".parse().unwrap();
        let mut second = first[9997..].to_vec();
        second.extend(vec![late.clone(); 3]);
        second.push(source[11].clone());
        second.last_mut().unwrap().time_exchange = "2026-10-11T00:00:11Z".parse().unwrap();
        let mut expected = first.clone();
        expected.extend_from_slice(&second[3..]);
        let mut pages = vec![first, second].into_iter();
        let start = "2026-10-11T00:00:00Z".parse().unwrap();
        let end = "2026-10-11T00:00:30Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut queries = Vec::new();
        let actual = collect_books(&window, |query| {
            queries.push(query);
            std::future::ready(Ok(pages.next().unwrap()))
        })
        .await
        .unwrap();

        assert_eq!(actual, expected);
        assert_eq!(actual.iter().filter(|row| **row == late).count(), 3);
        assert_eq!(queries.len(), 2);

        for query in &queries {
            assert_eq!(query.date, None);
            assert_eq!(
                query.time_end,
                Some("2026-10-11T01:00:00Z".parse().unwrap())
            );
            assert_eq!(query.limit, PAGE_LIMIT_MAX);
        }

        assert_eq!(queries[0].time_start, Some(start));
        assert_eq!(
            queries[1].time_start,
            Some("2026-10-11T00:00:09Z".parse().unwrap())
        );
    }

    #[tokio::test]
    async fn book_pages_finish_a_split_snapshot_at_the_inclusive_end() {
        let source: Vec<BookEvent> =
            serde_json::from_str(include_str!("../test_data/book-replay.json")).unwrap();
        let mut first = source[..4].to_vec();
        first.extend(vec![source[4].clone(); 9995]);
        first.push(source[8].clone());
        let mut bid = source[10].clone();
        bid.time_exchange = source[8].time_exchange;
        let mut second = first[4..].to_vec();
        second.push(bid);
        let mut pages = vec![first, second].into_iter();
        let start = source[0].time_exchange;
        let end = source[8].time_exchange;
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut calls = 0;
        let rows = collect_books(&window, |_| {
            calls += 1;
            std::future::ready(Ok(pages.next().unwrap()))
        })
        .await
        .unwrap();

        let market =
            serde_json::from_str(include_str!("../test_data/polymarket-market.json")).unwrap();
        let instrument = parse_instrument(&market, 0.into()).unwrap();
        let batches = book::replay(
            &rows,
            &instrument,
            parse::timestamp(start).unwrap(),
            parse::timestamp(end).unwrap(),
        )
        .unwrap();
        let result = book::snapshot(&batches, &instrument, None).unwrap();

        assert_eq!(calls, 2);
        assert_eq!(rows.len(), 10001);
        assert_eq!(batches.len(), 3);
        assert_eq!(
            result.bids_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![("0.2".parse::<rust_decimal::Decimal>().unwrap(), 6.into())]
        );
        assert_eq!(
            result.asks_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![("0.8".parse::<rust_decimal::Decimal>().unwrap(), 9.into())]
        );
        assert_eq!(batches[2].deltas.len(), 3);

        for delta in &batches[2].deltas {
            assert_eq!(delta.ts_event, parse::timestamp(end).unwrap());
            assert_eq!(delta.ts_init, parse::timestamp(end).unwrap());
        }
    }

    #[tokio::test]
    async fn book_short_hour_rejects_a_noncontiguous_overlap() {
        let mut source: Vec<BookEvent> =
            serde_json::from_str(include_str!("../test_data/book-replay.json")).unwrap();
        source.truncate(3);
        source[0].time_exchange = "2026-10-11T00:59:58.0000077Z".parse().unwrap();
        source[1].time_exchange = "2026-10-11T00:59:57.9866770Z".parse().unwrap();
        source[2].time_exchange = "2026-10-11T00:59:59.0182848Z".parse().unwrap();
        let start = "2026-10-11T00:00:00Z".parse().unwrap();
        let end = "2026-10-11T01:30:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut calls = 0;
        let error = collect_books(&window, |_| {
            calls += 1;
            std::future::ready(Ok(source.clone()))
        })
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "FinFeed history overlap is not a contiguous suffix; no result is published"
        );
        assert_eq!(calls, 1);
    }

    #[tokio::test]
    async fn final_short_hour_retains_unordered_rows_without_a_continuation_cursor() {
        let mut source: Vec<BookEvent> =
            serde_json::from_str(include_str!("../test_data/book-replay.json")).unwrap();
        source.truncate(2);
        source[0].time_exchange = "2026-10-11T00:59:59.500Z".parse().unwrap();
        source[1].time_exchange = "2026-10-11T00:59:57Z".parse().unwrap();
        let start = "2026-10-11T00:30:00Z".parse().unwrap();
        let end = "2026-10-11T00:59:59.900Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut queries = Vec::new();

        let rows = collect_hours(
            &window,
            |row: &BookEvent| row.time_exchange,
            |_| false,
            |query| {
                queries.push(query);
                std::future::ready(Ok(source.clone()))
            },
        )
        .await
        .unwrap();

        assert_eq!(rows, source);
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].date, None);
        assert_eq!(queries[0].limit, PAGE_LIMIT_MAX);
        assert_eq!(
            queries[0].time_start,
            Some("2026-10-11T00:00:00Z".parse().unwrap())
        );
        assert_eq!(
            queries[0].time_end,
            Some("2026-10-11T01:00:00Z".parse().unwrap())
        );
    }

    #[tokio::test]
    async fn continuation_rejects_a_lower_selector_equal_to_the_hourly_upper_bound() {
        let mut source: Vec<BookEvent> =
            serde_json::from_str(include_str!("../test_data/book-replay.json")).unwrap();
        source.truncate(1);
        source[0].time_exchange = "2026-10-11T01:00:01Z".parse().unwrap();
        let start = "2026-10-11T00:00:00Z".parse().unwrap();
        let end = "2026-10-11T01:30:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut calls = 0;

        let error = collect_hours(
            &window,
            |row: &BookEvent| row.time_exchange,
            |_| false,
            |_| {
                calls += 1;
                std::future::ready(if calls == 1 {
                    Ok(source.clone())
                } else {
                    Err(Error::AttemptBudget)
                })
            },
        )
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "FinFeed history continuation clock exceeds its hourly selector; no result is published"
        );
        assert_eq!(calls, 1);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn book_pages_stop_after_the_ordering_clock_passes_end(#[case] received: bool) {
        let source: Vec<BookEvent> =
            serde_json::from_str(include_str!("../test_data/book-replay.json")).unwrap();
        let mut page = vec![source[0].clone(); PAGE_LIMIT_MAX as usize];
        let after = "2026-10-11T00:00:03Z".parse().unwrap();
        if received {
            page[9999].time_coinapi = Some(after);
        } else {
            page[9999].time_exchange = after;
        }

        let start = "2026-10-11T00:00:00Z".parse().unwrap();
        let end = "2026-10-11T00:00:02Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut calls = 0;
        let actual = collect_books(&window, |_| {
            calls += 1;
            std::future::ready(Ok(page.clone()))
        })
        .await
        .unwrap();

        assert_eq!(actual, page);
        assert_eq!(calls, 1);
    }

    #[tokio::test]
    async fn full_book_page_with_a_cut_post_end_group_replays_the_requested_prefix() {
        let source: Vec<BookEvent> =
            serde_json::from_str(include_str!("../test_data/book-replay.json")).unwrap();
        let mut page = source[..8].to_vec();
        page.resize(PAGE_LIMIT_MAX as usize - 1, source[7].clone());
        let mut tail = source[4].clone();
        tail.time_exchange = source[8].time_exchange;
        tail.price = "0.7".parse().unwrap();
        page.push(tail);
        let start = source[0].time_exchange;
        let end = source[4].time_exchange;
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut calls = 0;
        let rows = collect_books(&window, |_| {
            calls += 1;
            std::future::ready(if calls == 1 {
                Ok(page.clone())
            } else {
                Err(Error::AttemptBudget)
            })
        })
        .await
        .unwrap();

        let market =
            serde_json::from_str(include_str!("../test_data/polymarket-market.json")).unwrap();
        let instrument = parse_instrument(&market, 0.into()).unwrap();
        let batches = book::replay(
            &rows,
            &instrument,
            parse::timestamp(start).unwrap(),
            parse::timestamp(end).unwrap(),
        )
        .unwrap();
        let result = book::snapshot(&batches, &instrument, None).unwrap();

        assert_eq!(calls, 1);
        assert_eq!(rows, page);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[1].ts_init, parse::timestamp(end).unwrap());
        assert_eq!(batches[1].deltas.len(), PAGE_LIMIT_MAX as usize - 5);
        assert_eq!(
            result.bids_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![
                (
                    "0.4".parse::<rust_decimal::Decimal>().unwrap(),
                    "5.25".parse().unwrap()
                ),
                ("0.3".parse::<rust_decimal::Decimal>().unwrap(), 5.into()),
            ]
        );
        assert_eq!(
            result.asks_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![
                ("0.65".parse::<rust_decimal::Decimal>().unwrap(), 8.into()),
                ("0.7".parse::<rust_decimal::Decimal>().unwrap(), 7.into()),
            ]
        );
    }

    #[tokio::test]
    async fn empty_book_hours_do_not_infer_a_cursor() {
        let start = "2026-10-11T00:30:00Z".parse().unwrap();
        let end = "2026-10-11T02:30:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut queries = Vec::new();
        let actual = collect_books(&window, |query| {
            queries.push(query);
            std::future::ready(Ok(Vec::new()))
        })
        .await
        .unwrap();

        assert!(actual.is_empty());
        assert_eq!(queries.len(), 3);

        for query in &queries {
            assert_eq!(
                query.time_start,
                Some("2026-10-11T00:00:00Z".parse().unwrap())
            );
        }

        assert_eq!(
            queries[0].time_end,
            Some("2026-10-11T01:00:00Z".parse().unwrap())
        );
        assert_eq!(
            queries[1].time_end,
            Some("2026-10-11T02:00:00Z".parse().unwrap())
        );
        assert_eq!(
            queries[2].time_end,
            Some("2026-10-11T03:00:00Z".parse().unwrap())
        );
    }

    #[rstest]
    #[case("changed", 2, "overlap changed")]
    #[case("missing", 2, "overlap changed")]
    #[case("stalled", 2, "cannot advance")]
    #[case("budget", 2, "local attempt budget exhausted")]
    #[case("suffix", 1, "not a contiguous suffix")]
    #[case("dense", 1, "cannot advance")]
    #[tokio::test]
    async fn book_continuation_fails_without_returning_a_prefix(
        #[case] failure: &str,
        #[case] expected_calls: usize,
        #[case] expected_error: &str,
    ) {
        let source: Vec<BookEvent> =
            serde_json::from_str(include_str!("../test_data/book-replay.json")).unwrap();
        let mut first = vec![source[0].clone(); PAGE_LIMIT_MAX as usize];
        first[9999].time_exchange = "2026-10-11T00:00:10.0502711Z".parse().unwrap();
        if failure == "suffix" {
            first.swap(9998, 9999);
        } else if failure == "dense" {
            first[9999] = source[0].clone();
        }

        let mut second = vec![first[9999].clone()];
        match failure {
            "changed" => second[0].size += rust_decimal::Decimal::ONE,
            "missing" => second.clear(),
            "stalled" => second.resize(PAGE_LIMIT_MAX as usize, second[0].clone()),
            _ => {}
        }

        let mut pages = vec![
            Ok(first),
            if failure == "budget" {
                Err(Error::AttemptBudget)
            } else {
                Ok(second)
            },
        ]
        .into_iter();
        let start = "2026-10-11T00:00:00Z".parse().unwrap();
        let end = "2026-10-11T00:00:30Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut calls = 0;
        let error = collect_books(&window, |_| {
            calls += 1;
            std::future::ready(pages.next().unwrap())
        })
        .await
        .unwrap_err();

        assert!(error.to_string().contains(expected_error), "{error:#}");
        assert_eq!(calls, expected_calls);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn book_hour_expansion_checks_overlap_before_appending(#[case] changed: bool) {
        let source: Vec<BookEvent> =
            serde_json::from_str(include_str!("../test_data/book-replay.json")).unwrap();
        let mut before = source[0].clone();
        before.time_exchange = "2026-10-11T00:59:59.900Z".parse().unwrap();
        let mut after = source[4].clone();
        after.time_exchange = "2026-10-11T01:00:00.100Z".parse().unwrap();
        let mut late = source[5].clone();
        late.time_exchange = "2026-10-11T00:59:59.800Z".parse().unwrap();

        let first = if changed {
            vec![before.clone(), late.clone()]
        } else {
            vec![before.clone()]
        };

        let second = vec![before, after, late];
        let expected = second.clone();
        let mut pages = vec![first, second].into_iter();
        let start = "2026-10-11T00:30:00Z".parse().unwrap();
        let end = "2026-10-11T01:30:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut queries = Vec::new();
        let actual = collect_books(&window, |query| {
            queries.push(query);
            std::future::ready(Ok(pages.next().unwrap()))
        })
        .await;

        if changed {
            assert_eq!(
                actual.unwrap_err().to_string(),
                "FinFeed history overlap changed in [2026-10-11T00:59:58Z, 2026-10-11T02:00:00Z]; no result is published"
            );
        } else {
            assert_eq!(actual.unwrap(), expected);
        }

        assert_eq!(queries.len(), 2);
        assert_eq!(
            queries[0].time_start,
            Some("2026-10-11T00:00:00Z".parse().unwrap())
        );
        assert_eq!(
            queries[0].time_end,
            Some("2026-10-11T01:00:00Z".parse().unwrap())
        );
        assert_eq!(
            queries[1].time_start,
            Some("2026-10-11T00:59:58Z".parse().unwrap())
        );
        assert_eq!(
            queries[1].time_end,
            Some("2026-10-11T02:00:00Z".parse().unwrap())
        );
    }

    #[tokio::test]
    async fn bars_continue_short_pages_until_the_period_bound_or_an_empty_page() {
        let source: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../test_data/client-bars.json")).unwrap();
        let periods: Vec<OhlcvPeriod> =
            serde_json::from_str(include_str!("../test_data/periods.json")).unwrap();
        let period = super::period(&periods, (60, 0)).unwrap();
        let start = "2020-10-11T00:00:00Z".parse().unwrap();
        let end = "2020-10-11T00:05:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), NonZeroUsize::new(1), end).unwrap();
        let mut pages = vec![source[..2].to_vec(), source[2..].to_vec(), Vec::new()].into_iter();
        let mut queries = Vec::new();
        let actual = collect_bars(&window, period, |query| {
            queries.push(query);
            std::future::ready(Ok(pages.next().unwrap()))
        })
        .await
        .unwrap();

        assert_eq!(actual, source);
        assert_eq!(queries.len(), 3);
        assert_eq!(queries[0].time_start, Some(start));
        assert_eq!(
            queries[1].time_start,
            Some("2020-10-11T00:02:00Z".parse().unwrap())
        );
        assert_eq!(
            queries[2].time_start,
            Some("2020-10-11T00:03:00Z".parse().unwrap())
        );

        for query in queries {
            assert_eq!(query.period_id, period.period_id.unwrap());
            assert_eq!(query.time_end, Some(end));
            assert_eq!(query.limit, OHLCV_LIMIT_MAX);
        }
    }

    #[tokio::test]
    async fn bars_continue_a_valid_full_page_and_finish_at_the_exact_end() {
        let template: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../test_data/client-bars.json")).unwrap();
        let periods: Vec<OhlcvPeriod> =
            serde_json::from_str(include_str!("../test_data/periods.json")).unwrap();
        let period = super::period(&periods, (1, 0)).unwrap();
        let start: Timestamp = "2020-10-11T00:00:00Z".parse().unwrap();
        let end =
            Timestamp::from_second(start.as_second() + i64::from(OHLCV_LIMIT_MAX) + 1).unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();

        let source = (0..=OHLCV_LIMIT_MAX)
            .map(|offset| {
                let mut row = template[0].clone();
                row.time_period_start =
                    Timestamp::from_second(start.as_second() + i64::from(offset)).unwrap();
                row.time_period_end =
                    Timestamp::from_second(row.time_period_start.as_second() + 1).unwrap();
                row.time_open = Some(row.time_period_start);
                row.time_close = Some(row.time_period_end);
                row.volume_traded = (offset + 1).into();
                row.trades_count = i64::from(offset) + 1;
                row
            })
            .collect::<Vec<_>>();

        let mut pages = vec![
            source[..OHLCV_LIMIT_MAX as usize].to_vec(),
            source[OHLCV_LIMIT_MAX as usize..].to_vec(),
        ]
        .into_iter();
        let mut queries = Vec::new();
        let actual = collect_bars(&window, period, |query| {
            queries.push(query);
            std::future::ready(Ok(pages.next().unwrap()))
        })
        .await
        .unwrap();

        assert_eq!(actual, source);
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0].time_start, Some(start));
        assert_eq!(
            queries[1].time_start,
            Some(Timestamp::from_second(start.as_second() + i64::from(OHLCV_LIMIT_MAX)).unwrap())
        );

        for query in queries {
            assert_eq!(query.period_id, period.period_id.unwrap());
            assert_eq!(query.time_end, Some(end));
            assert_eq!(query.limit, OHLCV_LIMIT_MAX);
        }
    }

    #[tokio::test]
    async fn bars_reject_a_page_that_cannot_advance_the_lower_selector() {
        let source: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../test_data/client-bars.json")).unwrap();
        let periods: Vec<OhlcvPeriod> =
            serde_json::from_str(include_str!("../test_data/periods.json")).unwrap();
        let period = super::period(&periods, (60, 0)).unwrap();
        let start = "2020-10-11T00:05:00Z".parse().unwrap();
        let end = "2020-10-11T00:10:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut pages = vec![source, Vec::new()].into_iter();
        let mut calls = 0;
        let error = collect_bars(&window, period, |_| {
            calls += 1;
            std::future::ready(Ok(pages.next().unwrap()))
        })
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "FinFeed OHLCV history cannot advance past 2020-10-11T00:05:00Z; no result is published"
        );
        assert_eq!(calls, 1);
    }

    #[rstest]
    #[case(
        "overlap",
        "FinFeed OHLCV history contains repeated or overlapping periods; no result is published"
    )]
    #[case(
        "period",
        "FinFeed OHLCV period does not match requested bar specification"
    )]
    #[case("budget", "FinFeed local attempt budget exhausted")]
    #[tokio::test]
    async fn bar_continuation_failures_discard_the_complete_request(
        #[case] failure: &str,
        #[case] expected: &str,
    ) {
        let source: Vec<Ohlcv> =
            serde_json::from_str(include_str!("../test_data/client-bars.json")).unwrap();
        let periods: Vec<OhlcvPeriod> =
            serde_json::from_str(include_str!("../test_data/periods.json")).unwrap();
        let period = super::period(&periods, (60, 0)).unwrap();
        let start = "2020-10-11T00:00:00Z".parse().unwrap();
        let end = "2020-10-11T00:05:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let mut calls = 0;
        let result = collect_bars(&window, period, |_| {
            calls += 1;

            let page = if calls == 1 {
                Ok(source[..2].to_vec())
            } else if failure == "budget" {
                Err(Error::AttemptBudget)
            } else {
                let mut row = source[if failure == "overlap" { 0 } else { 2 }].clone();
                if failure == "period" {
                    row.time_period_end = end;
                    row.trades_count = 0;
                    row.volume_traded = Default::default();
                    row.price_open = None;
                    row.price_high = None;
                    row.price_low = None;
                    row.price_close = None;
                }

                Ok(vec![row])
            };

            std::future::ready(page)
        })
        .await;

        assert_eq!(result.unwrap_err().to_string(), expected);
        assert_eq!(calls, 2);
    }

    #[rstest]
    #[case("quotes", 49)]
    #[case("trades", 49)]
    #[case("books", 49)]
    #[tokio::test]
    async fn history_exceeding_budget_fails_without_spending(
        #[case] endpoint: &str,
        #[case] required: u64,
    ) {
        let http = FinFeedHttpClient::new(
            Some("test-api-key".into()),
            FinFeedHttpConfig {
                base_url: "http://127.0.0.1:9/v1/".into(),
                attempt_budget: 2,
                ..Default::default()
            },
        )
        .unwrap();

        let market =
            serde_json::from_str(include_str!("../test_data/polymarket-market.json")).unwrap();
        let instrument = parse_instrument(&market, 0.into()).unwrap();
        let start = "2026-10-11T00:00:00Z".parse().unwrap();
        let end = "2026-10-13T00:00:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), NonZeroUsize::new(1), end).unwrap();
        let error = match endpoint {
            "quotes" => quotes(&http, &instrument, &window).await.unwrap_err(),
            "trades" => trades(&http, &instrument, &window).await.unwrap_err(),
            "books" => books(&http, &instrument, &window).await.unwrap_err(),
            _ => unreachable!(),
        };

        assert_eq!(
            error.to_string(),
            format!(
                "FinFeed history window [2026-10-11T00:00:00Z, 2026-10-13T00:00:00Z] requires at least {required} requests, but the local attempt allowance has 2 remaining; shorten the window or configure a larger attempt_budget; retries and polling share this allowance"
            )
        );
        assert_eq!(http.attempts_remaining(), 2);
    }

    #[rstest]
    #[case(HOUR_SECONDS, "2026-10-11T01:00:00Z")]
    #[case(DAY_SECONDS, "2026-10-12T00:00:00Z")]
    fn history_budget_includes_end_partition_and_accepts_exact_allowance(
        #[case] seconds: i64,
        #[case] end: Timestamp,
    ) {
        let http = FinFeedHttpClient::new(
            Some("test-api-key".into()),
            FinFeedHttpConfig {
                attempt_budget: 2,
                ..Default::default()
            },
        )
        .unwrap();

        let start = "2026-10-11T00:30:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), None, end).unwrap();

        window.check_budget(&http, seconds).unwrap();
        assert_eq!(http.attempts_remaining(), 2);

        let http = FinFeedHttpClient::new(
            Some("test-api-key".into()),
            FinFeedHttpConfig {
                attempt_budget: 1,
                ..Default::default()
            },
        )
        .unwrap();

        let error = window.check_budget(&http, seconds).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "FinFeed history window [{start}, {end}] requires at least 2 requests, but the local attempt allowance has 1 remaining; shorten the window or configure a larger attempt_budget; retries and polling share this allowance"
            )
        );
        assert_eq!(http.attempts_remaining(), 1);
    }

    #[tokio::test]
    async fn history_cap_rejects_nonprogress_before_applying_a_local_limit() {
        let start = "2026-10-11T00:00:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(start), NonZeroUsize::new(1), start).unwrap();

        let error = collect_hours(
            &window,
            |row| *row,
            |_| false,
            |_| std::future::ready(Ok(vec![start; PAGE_LIMIT_MAX as usize])),
        )
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "FinFeed history cannot advance within the 10000-record cap in [2026-10-11T00:00:00Z, 2026-10-11T01:00:00Z]; no result is published"
        );
    }

    #[rstest]
    #[case("book-cap")]
    #[case("book-seed")]
    #[case("bar-cap")]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    async fn history_failures_identify_partition_and_replay_context(#[case] failure: &str) {
        let address = if cfg!(all(feature = "simulation", madsim)) {
            "127.0.0.1:18165"
        } else {
            "127.0.0.1:0"
        };

        let listener = TcpListener::bind(address).await.unwrap();
        #[cfg(not(all(feature = "simulation", madsim)))]
        let address = listener.local_addr().unwrap();

        let body = match failure {
            "book-seed" => include_str!("../test_data/empty.json").to_owned(),
            "book-cap" | "bar-cap" => {
                let (fixture, count) = if failure == "book-cap" {
                    (
                        include_str!("../test_data/book-replay.json"),
                        PAGE_LIMIT_MAX,
                    )
                } else {
                    (
                        include_str!("../test_data/client-bars.json"),
                        OHLCV_LIMIT_MAX,
                    )
                };

                let rows: serde_json::Value = serde_json::from_str(fixture).unwrap();
                serde_json::to_string(&vec![rows[0].clone(); count as usize]).unwrap()
            }
            _ => unreachable!(),
        };

        let mut bodies = Vec::new();
        if failure == "bar-cap" {
            bodies.push(include_str!("../test_data/periods.json").to_owned());
        }

        bodies.push(body);
        let peer = task::spawn(async move {
            for body in bodies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    request.push(stream.read_u8().await.unwrap());
                }

                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
            }
        });

        let http = FinFeedHttpClient::new(
            Some("test-api-key".into()),
            FinFeedHttpConfig {
                base_url: format!("http://{address}/v1/").into(),
                min_request_interval_ms: 1,
                ..Default::default()
            },
        )
        .unwrap();

        let market =
            serde_json::from_str(include_str!("../test_data/polymarket-market.json")).unwrap();
        let instrument = parse_instrument(&market, 0.into()).unwrap();
        let start = "2026-10-11T00:00:00Z".parse().unwrap();
        let end = "2026-10-11T00:30:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(end), NonZeroUsize::new(1), end).unwrap();

        let error = if failure == "bar-cap" {
            let bar_type =
                BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", instrument.id).as_str());
            bars(&http, &instrument, bar_type, &window, end, (60, 0))
                .await
                .unwrap_err()
        } else {
            books(&http, &instrument, &window).await.unwrap_err()
        };

        peer.await.unwrap();

        let expected = match failure {
            "book-cap" => "FinFeed history cannot advance within the 10000-record cap in [2026-10-11T00:00:00Z, 2026-10-11T01:00:00Z]; no result is published".to_owned(),
            "bar-cap" => "FinFeed OHLCV history contains repeated or overlapping periods; no result is published".to_owned(),
            "book-seed" => format!("FinFeed book replay failed for {} in [2026-10-11T00:00:00Z, 2026-10-11T00:30:00Z]; retrieval starts at the containing UTC hour and does not look back for an earlier snapshot: FinFeed book has no snapshot seed at or before start", instrument.id),
            _ => unreachable!(),
        };

        assert_eq!(format!("{error:#}"), expected);
        assert_eq!(
            http.attempts_remaining(),
            if failure == "bar-cap" { 98 } else { 99 }
        );
    }

    #[rstest]
    #[case("2023-02-01T00:00:00Z", "2023-03-01T00:00:00Z", 29)]
    #[case("2024-02-01T00:00:00Z", "2024-03-01T00:00:00Z", 30)]
    #[case("2024-04-01T00:00:00Z", "2024-05-01T00:00:00Z", 31)]
    #[case("2024-01-01T00:00:00Z", "2024-02-01T00:00:00Z", 32)]
    #[case("2023-01-01T00:00:00Z", "2025-01-01T00:00:00Z", 732)]
    fn hourly_partitions_cover_the_entire_inclusive_window(
        #[case] start: Timestamp,
        #[case] end: Timestamp,
        #[case] count: usize,
    ) {
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let hours = window.partitions(HOUR_SECONDS).collect::<Vec<_>>();
        let count = (count - 1) * 24 + 1;

        assert_eq!(hours.len(), count);
        assert_eq!(hours[0], start.as_second().div_euclid(HOUR_SECONDS));
        assert_eq!(hours[count - 1], end.as_second().div_euclid(HOUR_SECONDS));
        assert!(hours.windows(2).all(|pair| pair[0] + 1 == pair[1]));
    }

    #[tokio::test]
    async fn hourly_partitions_stop_at_budget_exhaustion_without_returning_a_prefix() {
        let window = Window::new(
            Some("2024-01-01T00:00:00Z".parse().unwrap()),
            Some("2025-01-01T00:00:00Z".parse().unwrap()),
            NonZeroUsize::new(1),
            Timestamp::UNIX_EPOCH,
        )
        .unwrap();
        let mut calls = 0;

        let result = collect_hours(
            &window,
            |row: &Timestamp| *row,
            |_| false,
            |_| {
                calls += 1;
                std::future::ready(if calls == 4 {
                    Err(Error::AttemptBudget)
                } else {
                    Ok(Vec::new())
                })
            },
        )
        .await;

        assert_eq!(calls, 4);
        assert_eq!(
            result.unwrap_err().to_string(),
            "FinFeed local attempt budget exhausted"
        );
    }

    #[rstest]
    fn every_claimed_catalog_period_maps_to_a_valid_specification() {
        let periods: Vec<OhlcvPeriod> =
            serde_json::from_str(include_str!("../test_data/periods.json")).unwrap();
        let mut claimed = Vec::new();

        for period in &periods {
            let aggregation = match period.unit_name.unwrap() {
                OhlcvUnit::Second => BarAggregation::Second,
                OhlcvUnit::Minute => BarAggregation::Minute,
                OhlcvUnit::Hour => BarAggregation::Hour,
                OhlcvUnit::Day => BarAggregation::Day,
                OhlcvUnit::Month => BarAggregation::Month,
                OhlcvUnit::Year => BarAggregation::Year,
                OhlcvUnit::Other(_) => panic!("unexpected catalog unit"),
            };

            let step = period.unit_count.unwrap() as usize;
            let spec = BarSpecification::new_checked(step, aggregation, PriceType::Last).unwrap();

            let bar_type = BarType::new(
                InstrumentId::from("market_NO.FINFEED_POLYMARKET"),
                spec,
                AggregationSource::External,
            );

            let length = period_length(bar_type).unwrap();
            let actual = super::period(&periods, length).unwrap();
            assert_eq!(actual, period);
            claimed.push(period.period_id.as_ref().map(AsRef::as_ref).unwrap());
        }

        assert_eq!(
            claimed,
            vec![
                "1SEC", "2SEC", "3SEC", "4SEC", "5SEC", "6SEC", "10SEC", "15SEC", "20SEC", "30SEC",
                "1MIN", "2MIN", "3MIN", "4MIN", "5MIN", "6MIN", "10MIN", "15MIN", "20MIN", "30MIN",
                "1HRS", "2HRS", "3HRS", "4HRS", "6HRS", "8HRS", "12HRS", "1DAY", "2DAY", "3DAY",
                "5DAY", "7DAY", "10DAY", "1MTH", "2MTH", "3MTH", "4MTH", "6MTH", "1YRS", "2YRS",
                "3YRS", "4YRS", "5YRS"
            ]
        );
    }

    #[rstest]
    #[case("1-MINUTE-BID-EXTERNAL", "FinFeed OHLCV uses last prices")]
    #[case("1-MINUTE-ASK-EXTERNAL", "FinFeed OHLCV uses last prices")]
    #[case("1-MINUTE-MID-EXTERNAL", "FinFeed OHLCV uses last prices")]
    #[case("1-MINUTE-LAST-INTERNAL", "FinFeed provides external OHLCV bars only")]
    #[case("1-TICK-LAST-EXTERNAL", "unsupported FinFeed bar aggregation")]
    #[case("1-MILLISECOND-LAST-EXTERNAL", "unsupported FinFeed bar aggregation")]
    fn unsupported_bar_specifications_fail_before_retrieval(
        #[case] suffix: &str,
        #[case] expected: &str,
    ) {
        let bar_type = BarType::from(format!("market_NO.FINFEED_POLYMARKET-{suffix}").as_str());
        assert_eq!(period_length(bar_type).unwrap_err().to_string(), expected);
    }

    #[rstest]
    #[case("12-SECOND-LAST-EXTERNAL")]
    #[case("4-DAY-LAST-EXTERNAL")]
    #[case("2-WEEK-LAST-EXTERNAL")]
    fn unlisted_durations_are_rejected_by_the_provider_catalog(#[case] suffix: &str) {
        let periods: Vec<OhlcvPeriod> =
            serde_json::from_str(include_str!("../test_data/periods.json")).unwrap();
        let bar_type = BarType::from(format!("market_NO.FINFEED_POLYMARKET-{suffix}").as_str());
        assert_eq!(
            period(&periods, period_length(bar_type).unwrap())
                .unwrap_err()
                .to_string(),
            "FinFeed has no matching OHLCV period"
        );
    }

    #[rstest]
    #[case("1-WEEK-LAST-EXTERNAL", "7DAY")]
    #[case("12-MONTH-LAST-EXTERNAL", "1YRS")]
    fn existing_duration_aliases_keep_their_catalog_mapping(
        #[case] suffix: &str,
        #[case] expected: &str,
    ) {
        let periods: Vec<OhlcvPeriod> =
            serde_json::from_str(include_str!("../test_data/periods.json")).unwrap();
        let bar_type = BarType::from(format!("market_NO.FINFEED_POLYMARKET-{suffix}").as_str());
        assert_eq!(
            period(&periods, period_length(bar_type).unwrap())
                .unwrap()
                .period_id
                .as_ref()
                .map(AsRef::as_ref),
            Some(expected)
        );
    }
}
