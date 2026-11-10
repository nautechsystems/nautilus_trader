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

//! Instrument provider for the Kalshi adapter.

use std::{collections::HashMap, ops::Bound};

use anyhow::Context;
use nautilus_common::{
    live::sender::EventSender,
    messages::{
        DataEvent,
        data::{
            DataResponse, InstrumentResponse, InstrumentsResponse, RequestInstrument,
            RequestInstruments,
        },
    },
};
use nautilus_core::{
    UnixNanos,
    datetime::{NANOSECONDS_IN_SECOND, try_datetime_to_unix_nanos},
    params::Params,
    time::AtomicTime,
};
use nautilus_model::instruments::{BinaryOption, InstrumentAny};

use crate::{
    common::{
        consts::{KALSHI_CLIENT_ID, KALSHI_VENUE},
        enums::{MarketStatus, MarketStatusFilter, MveFilter, MveHistoricalFilter},
        parse::{parse_instrument_id, parse_ticker},
    },
    http::{
        client::KalshiHttpClient,
        error::KalshiHttpError,
        models::{Event, Field, Market, Series},
        parse::{parse_instrument, supported_market},
        query::{EventQuery, HistoricalMarketsQuery, MarketsQuery},
    },
};

const STATUS_FILTERS: &[(MarketStatusFilter, &[MarketStatus])] = &[
    (MarketStatusFilter::Unopened, &[MarketStatus::Initialized]),
    (MarketStatusFilter::Open, &[MarketStatus::Active]),
    (MarketStatusFilter::Paused, &[MarketStatus::Inactive]),
    (
        MarketStatusFilter::Closed,
        &[
            MarketStatus::Closed,
            MarketStatus::Determined,
            MarketStatus::Disputed,
            MarketStatus::Amended,
        ],
    ),
    (MarketStatusFilter::Settled, &[MarketStatus::Finalized]),
];

/// Loads current or archived definitions without serving explicit requests from a private cache.
///
/// Request parameters accept the REST market filters, `exchange_index`, `historical`, and
/// `max_pages` (default 100). An exhausted page budget fails without emitting a partial response.
/// Time bounds filter `ts_event`, using local initialization time when `updated_time` is absent.
/// They do not reconstruct historical definitions.
#[derive(Debug)]
pub struct KalshiInstrumentProvider {
    pub(crate) http: KalshiHttpClient,
    pub(crate) clock: &'static AtomicTime,
    sender: EventSender<DataEvent>,
}

impl KalshiInstrumentProvider {
    /// Creates an instrument request handler using the supplied transport and event sender.
    pub fn new(
        http: KalshiHttpClient,
        clock: &'static AtomicTime,
        sender: EventSender<DataEvent>,
    ) -> Self {
        Self {
            http,
            clock,
            sender,
        }
    }

    /// Fetches a fresh definition, falling back to the archive only after a live-market 404.
    ///
    /// # Errors
    ///
    /// Returns an error if identity, filters, admission, transport, or response dispatch fails.
    pub async fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let instrument = self.load_instrument(&request).await?;
        self.respond_instrument(request, instrument)
    }

    pub(crate) fn respond_instrument(
        &self,
        request: RequestInstrument,
        instrument: BinaryOption,
    ) -> anyhow::Result<()> {
        let (start, end) = request_bounds(request.start, request.end)?;

        let response = InstrumentResponse::new(
            request.request_id,
            request.client_id.unwrap_or(*KALSHI_CLIENT_ID),
            request.single_instrument_id()?,
            InstrumentAny::BinaryOption(instrument),
            start,
            end,
            self.clock.get_time_ns(),
            request.params,
        );

        self.sender
            .send(DataEvent::Response(DataResponse::Instrument(Box::new(
                response,
            ))))
            .map_err(|_| anyhow::anyhow!("instrument response receiver closed"))?;
        Ok(())
    }

    /// Fetches all selected live or archived markets, retaining only supported definitions.
    ///
    /// # Errors
    ///
    /// Returns an error if the venue or filters are invalid, pagination is incomplete, metadata is
    /// malformed, or transport or response dispatch fails.
    pub async fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let instruments = self.load_instruments(&request).await?;
        self.respond_instruments(request, instruments)
    }

    pub(crate) fn respond_instruments(
        &self,
        request: RequestInstruments,
        instruments: Vec<InstrumentAny>,
    ) -> anyhow::Result<()> {
        let (start, end) = request_bounds(request.start, request.end)?;

        let response = InstrumentsResponse::new(
            request.request_id,
            request.client_id.unwrap_or(*KALSHI_CLIENT_ID),
            *KALSHI_VENUE,
            instruments,
            start,
            end,
            self.clock.get_time_ns(),
            request.params,
        );

        self.sender
            .send(DataEvent::Response(DataResponse::Instruments(response)))
            .map_err(|_| anyhow::anyhow!("instruments response receiver closed"))?;
        Ok(())
    }

    pub(crate) async fn load_instruments(
        &self,
        request: &RequestInstruments,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        anyhow::ensure!(
            request.venue.is_none_or(|v| v == *KALSHI_VENUE),
            "instrument request venue must be KALSHI"
        );
        let selection = Selection::new(request.params.as_ref())?;
        let (start, end) = request_bounds(request.start, request.end)?;

        let pages = if selection.historical {
            let mut query = HistoricalMarketsQuery {
                limit: selection.query.limit,
                cursor: selection.query.cursor.clone(),
                ..Default::default()
            };

            match (
                &selection.query.tickers,
                &selection.query.event_ticker,
                &selection.query.series_ticker,
            ) {
                (Some(tickers), _, _) => query.tickers = Some(tickers.clone()),
                (_, Some(event), _) => query.event_ticker = Some(event.clone()),
                (_, _, Some(series)) => query.series_ticker = Some(series.clone()),
                _ => query.mve_filter = Some(MveHistoricalFilter::Exclude),
            }

            self.http
                .get_all_historical_markets(query, selection.max_pages)
                .await?
        } else {
            self.http
                .get_all_markets(selection.query.clone(), selection.max_pages)
                .await?
        };

        anyhow::ensure!(pages.complete, "instrument discovery page budget exhausted");
        let mut events: HashMap<String, Event> = HashMap::new();
        let mut series: HashMap<String, Series> = HashMap::new();
        let mut instruments = Vec::new();

        for market in pages.records {
            if !events.contains_key(&market.event_ticker) {
                let event = self
                    .http
                    .get_event(
                        &market.event_ticker,
                        &EventQuery {
                            exclude_markets: Some(true),
                            ..Default::default()
                        },
                    )
                    .await?
                    .event;

                anyhow::ensure!(
                    event.event_ticker == market.event_ticker,
                    "requested event identity mismatch"
                );
                events.insert(market.event_ticker.clone(), event);
            }

            let event = &events[&market.event_ticker];
            if !selection.matches(&market, event) || !supported_market(&market, event) {
                continue;
            }

            if !series.contains_key(&event.series_ticker) {
                let definition = self.http.get_series(&event.series_ticker).await?.series;
                series.insert(event.series_ticker.clone(), definition);
            }

            let instrument = Self::parse(
                &market,
                event,
                &series[&event.series_ticker],
                self.clock.get_time_ns(),
            )?;

            if start.is_some_and(|t| instrument.ts_event < t)
                || end.is_some_and(|t| instrument.ts_event > t)
            {
                continue;
            }

            instruments.push(InstrumentAny::BinaryOption(instrument));
        }

        Ok(instruments)
    }

    pub(crate) async fn load_instrument(
        &self,
        request: &RequestInstrument,
    ) -> anyhow::Result<BinaryOption> {
        let ticker = parse_ticker(request.single_instrument_id()?)?;
        let selection = Selection::new(request.params.as_ref())?;
        let (start, end) = request_bounds(request.start, request.end)?;

        let market = if selection.historical {
            self.http.get_historical_market(&ticker).await?.market
        } else {
            Self::load_market(&self.http, &ticker).await?
        };

        anyhow::ensure!(
            market.ticker == ticker,
            "requested market identity mismatch"
        );

        let event = self
            .http
            .get_event(
                &market.event_ticker,
                &EventQuery {
                    exclude_markets: Some(true),
                    ..Default::default()
                },
            )
            .await?
            .event;

        let series = self.http.get_series(&event.series_ticker).await?.series;
        anyhow::ensure!(
            selection.matches(&market, &event),
            "definition does not satisfy request filters"
        );
        let instrument = Self::parse(&market, &event, &series, self.clock.get_time_ns())?;
        anyhow::ensure!(
            start.is_none_or(|t| instrument.ts_event >= t)
                && end.is_none_or(|t| instrument.ts_event <= t),
            "definition outside requested update interval"
        );

        Ok(instrument)
    }

    pub(crate) async fn load_market(
        http: &KalshiHttpClient,
        ticker: &str,
    ) -> anyhow::Result<Market> {
        let mut attempt = 0;

        loop {
            match http.get_market(ticker).await {
                Ok(response) => return Ok(response.market),
                Err(e) if not_found(&e) => match http.get_historical_market(ticker).await {
                    Ok(response) => return Ok(response.market),
                    Err(e) if not_found(&e) && attempt < 3 => {
                        nautilus_common::live::dst::time::sleep(std::time::Duration::from_millis(
                            100 << attempt,
                        ))
                        .await;
                        attempt += 1;
                    }
                    Err(e) => return Err(e.into()),
                },
                Err(e) => return Err(e.into()),
            }
        }
    }

    pub(crate) fn parse(
        market: &Market,
        event: &Event,
        series: &Series,
        ts_init: UnixNanos,
    ) -> anyhow::Result<BinaryOption> {
        anyhow::ensure!(
            series.ticker == event.series_ticker,
            "requested series identity mismatch"
        );
        let mut instrument = parse_instrument(market, event, ts_init)?;
        // Keep exact fee metadata out of serde_json's floating-point number path
        let serialized = serde_json::to_string(series)?;
        let fields: HashMap<String, Box<serde_json::value::RawValue>> =
            serde_json::from_str(&serialized)?;

        let metadata = fields
            .into_iter()
            .map(|(key, value)| {
                let value = if key == "fee_multiplier" {
                    serde_json::Value::String(series.fee_multiplier.0.to_string())
                } else {
                    serde_json::from_str(value.get())?
                };

                Ok((key, value))
            })
            .collect::<Result<serde_json::Map<_, _>, serde_json::Error>>()?;

        let mut info = instrument
            .info
            .as_deref()
            .cloned()
            .context("missing instrument metadata")?;
        info.insert("series".into(), metadata.into());
        instrument.info = Some(Params::new(info));
        Ok(instrument)
    }
}

pub(crate) fn request_bounds(
    start: Option<jiff::Timestamp>,
    end: Option<jiff::Timestamp>,
) -> anyhow::Result<(Option<UnixNanos>, Option<UnixNanos>)> {
    let start = start
        .map(try_datetime_to_unix_nanos)
        .transpose()
        .context("invalid request start bound")?;
    let end = end
        .map(try_datetime_to_unix_nanos)
        .transpose()
        .context("invalid request end bound")?;
    anyhow::ensure!(
        start.zip(end).is_none_or(|(a, b)| a <= b),
        "invalid update interval"
    );
    Ok((start, end))
}

fn not_found(error: &KalshiHttpError) -> bool {
    matches!(
        error,
        KalshiHttpError::Http { status: 404, .. }
            | KalshiHttpError::Venue { status: 404, .. }
            | KalshiHttpError::Status(404)
    )
}

struct Selection {
    query: MarketsQuery,
    historical: bool,
    max_pages: u32,
    exchange_index: Option<i64>,
}

impl Selection {
    fn new(params: Option<&Params>) -> anyhow::Result<Self> {
        let value = params
            .map(serde_json::to_value)
            .transpose()?
            .unwrap_or_else(|| serde_json::json!({}));

        if let Some(status) = value.get("status").and_then(serde_json::Value::as_str) {
            anyhow::ensure!(
                status.parse::<MarketStatusFilter>().is_ok(),
                "unsupported market status filter"
            );
        }

        let mut query: MarketsQuery = serde_json::from_value(value.clone())?;
        anyhow::ensure!(
            query
                .mve_filter
                .as_ref()
                .is_none_or(|value| *value == MveFilter::Exclude),
            "only ordinary market discovery is supported"
        );
        query.mve_filter = Some(MveFilter::Exclude);
        anyhow::ensure!(
            query.cursor.as_deref().is_none_or(str::is_empty),
            "instrument discovery must start at the first page"
        );

        for ticker in query.tickers.iter().flat_map(|v| v.split(',')) {
            parse_instrument_id(ticker)?;
        }

        for ticker in [&query.event_ticker, &query.series_ticker]
            .into_iter()
            .flatten()
        {
            parse_instrument_id(ticker)?;
        }

        let historical = value
            .get("historical")
            .map(|v| v.as_bool().context("historical must be a boolean"))
            .transpose()?
            .unwrap_or(false);
        let max_pages = value
            .get("max_pages")
            .map(|v| v.as_u64().context("max_pages must be a positive integer"))
            .transpose()?
            .unwrap_or(100);
        let max_pages = u32::try_from(max_pages)?;
        anyhow::ensure!(max_pages > 0, "max_pages must be positive");

        let exchange_index = value
            .get("exchange_index")
            .map(|v| {
                v.as_i64()
                    .filter(|v| *v >= 0 && *v <= i64::from(i32::MAX))
                    .context("invalid exchange index")
            })
            .transpose()?;

        Ok(Self {
            query,
            historical,
            max_pages,
            exchange_index,
        })
    }

    fn matches(&self, market: &Market, event: &Event) -> bool {
        self.query
            .tickers
            .as_ref()
            .is_none_or(|v| v.split(',').any(|t| t == market.ticker))
            && self
                .query
                .event_ticker
                .as_ref()
                .is_none_or(|v| *v == market.event_ticker)
            && self
                .query
                .series_ticker
                .as_ref()
                .is_none_or(|v| *v == event.series_ticker)
            && self
                .exchange_index
                .is_none_or(|v| market.exchange_index == Field::Value(v as i32))
            && timestamp_matches(
                Some(market.created_time),
                self.query.min_created_ts,
                self.query
                    .max_created_ts
                    .map_or(Bound::Unbounded, Bound::Excluded),
            )
            && timestamp_matches(
                match market.updated_time {
                    Field::Value(t) => Some(t),
                    _ => None,
                },
                self.query.min_updated_ts,
                self.query
                    .max_updated_ts
                    .map_or(Bound::Unbounded, Bound::Included),
            )
            && timestamp_matches(
                Some(market.close_time),
                self.query.min_close_ts,
                self.query
                    .max_close_ts
                    .map_or(Bound::Unbounded, Bound::Excluded),
            )
            && timestamp_matches(
                match market.settlement_ts {
                    Field::Value(t) => Some(t),
                    _ => None,
                },
                self.query.min_settled_ts,
                self.query
                    .max_settled_ts
                    .map_or(Bound::Unbounded, Bound::Excluded),
            )
            && self.query.status.as_ref().is_none_or(|filter| {
                STATUS_FILTERS
                    .iter()
                    .find(|(name, _)| name == filter)
                    .is_some_and(|(_, statuses)| statuses.contains(&market.status))
            })
    }
}

fn timestamp_matches(
    value: Option<jiff::Timestamp>,
    minimum: Option<i64>,
    maximum: Bound<i64>,
) -> bool {
    let value = value.map(jiff::Timestamp::as_nanosecond);
    let minimum = minimum.map(|seconds| i128::from(seconds) * i128::from(NANOSECONDS_IN_SECOND));
    let maximum = maximum.map(|seconds| i128::from(seconds) * i128::from(NANOSECONDS_IN_SECOND));
    minimum.is_none_or(|min| value.is_some_and(|value| value > min))
        && match maximum {
            Bound::Unbounded => true,
            Bound::Included(max) => value.is_some_and(|value| value <= max),
            Bound::Excluded(max) => value.is_some_and(|value| value < max),
        }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    use crate::config::KalshiDataClientConfig;

    #[rstest]
    #[case("min_created_ts", [false, false, true])]
    #[case("max_created_ts", [true, false, false])]
    #[case("min_updated_ts", [false, false, true])]
    #[case("max_updated_ts", [true, true, false])]
    #[case("min_close_ts", [false, false, true])]
    #[case("max_close_ts", [true, false, false])]
    #[case("min_settled_ts", [false, false, true])]
    #[case("max_settled_ts", [true, false, false])]
    fn timestamp_filters_preserve_nanoseconds_and_boundaries(
        #[case] field: &str,
        #[case] expected: [bool; 3],
    ) {
        let (mut market, event) = definition();
        let params = Params::new([(field.into(), 100.into())].into());
        let selection = Selection::new(Some(&params)).unwrap();

        let results = [-1, 0, 1].map(|offset| {
            let timestamp = jiff::Timestamp::from_nanosecond(100_000_000_000 + offset).unwrap();
            market.created_time = timestamp;
            market.updated_time = Field::Value(timestamp);
            market.close_time = timestamp;
            market.settlement_ts = Field::Value(timestamp);
            selection.matches(&market, &event)
        });

        assert_eq!(results, expected);
    }

    #[rstest]
    #[case("unopened", "initialized", true)]
    #[case("open", "active", true)]
    #[case("paused", "inactive", true)]
    #[case("closed", "closed", true)]
    #[case("closed", "determined", true)]
    #[case("closed", "disputed", true)]
    #[case("closed", "amended", true)]
    #[case("settled", "finalized", true)]
    #[case("paused", "active", false)]
    #[case("closed", "finalized", false)]
    fn status_filters_match_lifecycle(
        #[case] filter: &str,
        #[case] status: &str,
        #[case] expected: bool,
    ) {
        let (mut market, event) = definition();
        market.status = status.parse().unwrap();
        let params = Params::new([("status".into(), filter.into())].into());
        let selection = Selection::new(Some(&params)).unwrap();
        assert_eq!(selection.matches(&market, &event), expected);
    }

    #[rstest]
    #[case("finalized")]
    #[case("open,closed")]
    #[case("not-a-status")]
    fn unknown_status_filter_is_rejected(#[case] status: &str) {
        let params = Params::new([("status".into(), status.into())].into());
        let result = Selection::new(Some(&params));
        assert_eq!(
            result.err().unwrap().to_string(),
            "unsupported market status filter"
        );
    }

    #[rstest]
    fn multivariate_only_selection_is_rejected() {
        let params = Params::new([("mve_filter".into(), "only".into())].into());
        let result = Selection::new(Some(&params));
        assert_eq!(
            result.err().unwrap().to_string(),
            "only ordinary market discovery is supported"
        );
    }

    #[rstest]
    fn series_metadata_retains_exact_fee_multiplier() {
        let (market, event) = definition();
        let values: Value =
            serde_json::from_str(include_str!("../test_data/instruments.json")).unwrap();
        let mut series: Series =
            serde_json::from_str(&serde_json::to_string(&values["series"]["series"]).unwrap())
                .unwrap();
        let exact = "0.1234567890123456789012345678";
        series.fee_multiplier.0 = rust_decimal::Decimal::from_str_exact(exact).unwrap();
        let http = KalshiHttpClient::new(&KalshiDataClientConfig::default()).unwrap();
        let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(31))));
        let (sender, _) = tokio::sync::mpsc::unbounded_channel();
        let provider = KalshiInstrumentProvider::new(http, clock, sender.into());
        let instrument =
            KalshiInstrumentProvider::parse(&market, &event, &series, provider.clock.get_time_ns())
                .unwrap();
        assert_eq!(
            instrument.info.as_ref().unwrap()["series"]["fee_multiplier"],
            exact
        );
    }

    #[rstest]
    fn ticker_selection_preserves_case_and_multiple_tickers() {
        let (market, event) = definition();

        let results = [
            market.ticker.clone(),
            format!("OTHER,{}", market.ticker),
            market.ticker.to_lowercase(),
            "OTHER".into(),
        ]
        .map(|tickers| {
            let params = Params::new([("tickers".into(), tickers.into())].into());
            Selection::new(Some(&params))
                .unwrap()
                .matches(&market, &event)
        });

        assert_eq!(results, [true, true, false, false]);
    }

    fn definition() -> (Market, Event) {
        let values: Value =
            serde_json::from_str(include_str!("../test_data/instruments.json")).unwrap();
        let value = &values["definitions"]["live"];

        (
            serde_json::from_value(value["market"].clone()).unwrap(),
            serde_json::from_value(value["event"].clone()).unwrap(),
        )
    }
}
