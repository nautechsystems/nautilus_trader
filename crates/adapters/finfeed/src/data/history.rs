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

//! Bounded retrieval without inferred timestamp continuation or completeness guarantees.

use std::{future::Future, num::NonZeroUsize, ops::RangeInclusive};

use anyhow::Context;
use jiff::{Timestamp, civil::Date, tz::TimeZone};
use nautilus_model::{
    data::{Bar, BarType, HasTsInit, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{AggregationSource, BarAggregation, PriceType},
    instruments::BinaryOption,
};

use super::{book, parse};
use crate::http::{
    client::FinFeedHttpClient,
    error::Error,
    models::{BookEvent, OhlcvPeriod},
    query::{HOUR_SECONDS, HistoryQuery, OHLCV_LIMIT_MAX, OhlcvQuery, PAGE_LIMIT_MAX},
};

const DAY_SECONDS: i64 = 86_400;

#[derive(Debug)]
pub(super) struct Window {
    start: Timestamp,
    end: Timestamp,
    limit: Option<NonZeroUsize>,
}

impl Window {
    pub(super) fn new(
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

    fn days(&self) -> impl Iterator<Item = anyhow::Result<Date>> {
        self.partitions(DAY_SECONDS).map(|day| {
            Ok(Timestamp::from_second(day * DAY_SECONDS)?
                .to_zoned(TimeZone::UTC)
                .date())
        })
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

pub(super) async fn books(
    http: &FinFeedHttpClient,
    instrument: &BinaryOption,
    window: &Window,
) -> anyhow::Result<Vec<OrderBookDeltas>> {
    window.check_budget(http, HOUR_SECONDS)?;
    let (exchange, id) = crate::common::symbol::from_instrument_id(instrument.id)?;
    let mut records: Vec<BookEvent> = Vec::new();

    for hour in window.partitions(HOUR_SECONDS) {
        let start = Timestamp::from_second(hour * HOUR_SECONDS)?;

        let query = HistoryQuery {
            date: None,
            time_start: Some(start),
            time_end: Some(Timestamp::from_second((hour + 1) * HOUR_SECONDS)?),
            limit: PAGE_LIMIT_MAX,
        };

        let page = http.get_book_history(&exchange, &id, &query).await?;
        anyhow::ensure!(
            page.len() < PAGE_LIMIT_MAX as usize,
            "FinFeed book history hour starting {start} reached the {PAGE_LIMIT_MAX}-record cap and may be truncated; no verified continuation is available; no result is published"
        );

        if let (Some(prior), Some(next)) = (records.last(), page.first()) {
            anyhow::ensure!(
                prior.time_exchange < next.time_exchange,
                "FinFeed book history has overlapping or unordered hourly responses"
            );
        }

        records.extend(page);
    }

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

pub(super) async fn quotes(
    http: &FinFeedHttpClient,
    instrument: &BinaryOption,
    window: &Window,
) -> anyhow::Result<Vec<QuoteTick>> {
    window.check_budget(http, DAY_SECONDS)?;
    let (exchange, id) = crate::common::symbol::from_instrument_id(instrument.id)?;
    let records = collect_days(window, |query| {
        let exchange = &exchange;
        let id = &id;
        async move { http.get_quotes_history(exchange, id, &query).await }
    })
    .await?;

    let parsed = records
        .into_iter()
        .map(|record| parse::quote(&record, instrument))
        .collect::<anyhow::Result<Vec<_>>>()?;
    window.finish(parsed)
}

pub(super) async fn trades(
    http: &FinFeedHttpClient,
    instrument: &BinaryOption,
    window: &Window,
) -> anyhow::Result<Vec<TradeTick>> {
    window.check_budget(http, DAY_SECONDS)?;
    let (exchange, id) = crate::common::symbol::from_instrument_id(instrument.id)?;
    let records = collect_days(window, |query| {
        let exchange = &exchange;
        let id = &id;
        async move { http.get_trades_history(exchange, id, &query).await }
    })
    .await?;

    let parsed = records
        .into_iter()
        .map(|record| parse::trade(&record, instrument))
        .collect::<anyhow::Result<Vec<_>>>()?;
    window.finish(parsed)
}

async fn collect_days<T, F, Fut>(window: &Window, mut fetch: F) -> anyhow::Result<Vec<T>>
where
    F: FnMut(HistoryQuery) -> Fut,
    Fut: Future<Output = Result<Vec<T>, Error>>,
{
    let mut records = Vec::new();

    for date in window.days() {
        let date = date?;

        let query = HistoryQuery {
            date: Some(date),
            time_start: None,
            time_end: None,
            limit: PAGE_LIMIT_MAX,
        };

        let page = fetch(query).await?;
        anyhow::ensure!(
            page.len() < PAGE_LIMIT_MAX as usize,
            "FinFeed history date {date} reached the {PAGE_LIMIT_MAX}-record cap and may be truncated; a smaller caller limit cannot avoid the cap; no verified continuation is available; no result is published"
        );
        records.extend(page);
    }

    Ok(records)
}

pub(super) async fn bars(
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

    let query = OhlcvQuery {
        period_id: period.period_id.context("FinFeed period ID is missing")?,
        time_start: Some(Timestamp::from_nanosecond(
            window.start.as_nanosecond().div_euclid(100) * 100,
        )?),
        time_end: Some(Timestamp::from_nanosecond(
            (window.end.as_nanosecond() + 99).div_euclid(100) * 100,
        )?),
        limit: OHLCV_LIMIT_MAX,
    };

    let records = http.get_ohlcv_history(&exchange, &id, &query).await?;
    anyhow::ensure!(
        records.len() < OHLCV_LIMIT_MAX as usize,
        "FinFeed OHLCV history window [{}, {}] reached the {OHLCV_LIMIT_MAX}-record cap and may be truncated; shorten the window; no verified continuation is available; no result is published",
        window.start,
        window.end
    );
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

pub(super) fn period_length(bar_type: BarType) -> anyhow::Result<(i64, i64)> {
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

pub(super) fn period(
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

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::BarSpecification, enums::AggregationSource, identifiers::InstrumentId,
    };
    use nautilus_network::dst::{net::TcpListener, task};
    use rstest::rstest;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::http::{client::FinFeedHttpConfig, parse::parse_instrument};

    #[rstest]
    #[case("quotes", 3)]
    #[case("trades", 3)]
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
            serde_json::from_str(include_str!("../../test_data/polymarket-market.json")).unwrap();
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
    async fn daily_history_cap_identifies_date_and_limit_remedy() {
        let start = "2026-10-11T00:00:00Z".parse().unwrap();
        let window = Window::new(Some(start), Some(start), NonZeroUsize::new(1), start).unwrap();

        let error = collect_days(&window, |_| {
            std::future::ready(Ok(vec![(); PAGE_LIMIT_MAX as usize]))
        })
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "FinFeed history date 2026-10-11 reached the 10000-record cap and may be truncated; a smaller caller limit cannot avoid the cap; no verified continuation is available; no result is published"
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
            "book-seed" => include_str!("../../test_data/empty.json").to_owned(),
            "book-cap" | "bar-cap" => {
                let (fixture, count) = if failure == "book-cap" {
                    (
                        include_str!("../../test_data/book-replay.json"),
                        PAGE_LIMIT_MAX,
                    )
                } else {
                    (
                        include_str!("../../test_data/client-bars.json"),
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
            bodies.push(include_str!("../../test_data/periods.json").to_owned());
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
                base_url: format!("http://{address}/v1/"),
                min_request_interval_ms: 1,
                ..Default::default()
            },
        )
        .unwrap();

        let market =
            serde_json::from_str(include_str!("../../test_data/polymarket-market.json")).unwrap();
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
            "book-cap" => "FinFeed book history hour starting 2026-10-11T00:00:00Z reached the 10000-record cap and may be truncated; no verified continuation is available; no result is published".to_owned(),
            "bar-cap" => "FinFeed OHLCV history window [2026-10-11T00:00:00Z, 2026-10-11T00:30:00Z] reached the 100000-record cap and may be truncated; shorten the window; no verified continuation is available; no result is published".to_owned(),
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
    fn daily_partitions_cover_the_entire_inclusive_window(
        #[case] start: Timestamp,
        #[case] end: Timestamp,
        #[case] count: usize,
    ) {
        let window = Window::new(Some(start), Some(end), None, end).unwrap();
        let dates = window.days().collect::<anyhow::Result<Vec<_>>>().unwrap();

        assert_eq!(dates.len(), count);
        assert_eq!(dates[0], start.to_zoned(TimeZone::UTC).date());
        assert_eq!(dates[count - 1], end.to_zoned(TimeZone::UTC).date());
        assert!(
            dates
                .windows(2)
                .all(|pair| pair[0].tomorrow().unwrap() == pair[1])
        );
    }

    #[tokio::test]
    async fn daily_partitions_stop_at_budget_exhaustion_without_returning_a_prefix() {
        let window = Window::new(
            Some("2024-01-01T00:00:00Z".parse().unwrap()),
            Some("2025-01-01T00:00:00Z".parse().unwrap()),
            NonZeroUsize::new(1),
            Timestamp::UNIX_EPOCH,
        )
        .unwrap();
        let mut calls = 0;
        let result = collect_days(&window, |_| {
            calls += 1;
            std::future::ready(if calls == 4 {
                Err(Error::AttemptBudget)
            } else {
                Ok(vec![calls])
            })
        })
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
            serde_json::from_str(include_str!("../../test_data/periods.json")).unwrap();
        let mut claimed = Vec::new();

        for period in &periods {
            let (step, aggregation) = match period.unit_name.as_ref().map(AsRef::as_ref).unwrap() {
                "second" => (period.unit_count.unwrap() as usize, BarAggregation::Second),
                "minute" => (period.unit_count.unwrap() as usize, BarAggregation::Minute),
                "hour" => (period.unit_count.unwrap() as usize, BarAggregation::Hour),
                "day" => (period.unit_count.unwrap() as usize, BarAggregation::Day),
                "month" => (period.unit_count.unwrap() as usize, BarAggregation::Month),
                "year" => (period.unit_count.unwrap() as usize, BarAggregation::Year),
                _ => unreachable!(),
            };

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
            serde_json::from_str(include_str!("../../test_data/periods.json")).unwrap();
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
            serde_json::from_str(include_str!("../../test_data/periods.json")).unwrap();
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
