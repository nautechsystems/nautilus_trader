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

use anyhow::Context;
use nautilus_common::messages::data::{
    BarsResponse, BookDeltasResponse, BookDepthResponse, DataResponse, FundingRatesResponse,
    QuotesResponse, RequestBars, RequestBookDeltas, RequestBookDepth, RequestCommand,
    RequestFundingRates, RequestJoin, RequestQuotes, RequestTrades, TradesResponse,
};
use nautilus_core::{Params, UUID4, UnixNanos, datetime::NANOSECONDS_IN_SECOND};
use nautilus_model::identifiers::ClientId;
use serde_json::Value;

use super::{
    DataEngine, datetime_to_unix_nanos, log_if_empty_response,
    requests::{remove_request_bar_aggregation_params, request_params, response_params},
};

const TIME_RANGE_GENERATOR: &str = "time_range_generator";
const TIME_RANGE_DURATIONS_SECONDS: &str = "durations_seconds";
const TIME_RANGE_POINT_DATA: &str = "point_data";

#[derive(Debug, Clone)]
pub(super) struct TimeRangePipelineState {
    parent: RequestCommand,
    generator: DefaultTimeRangeGenerator,
    start_ns: UnixNanos,
    end_ns: UnixNanos,
    response_client_id: ClientId,
    data_count: u64,
    last_response: Option<DataResponse>,
}

#[derive(Debug, Clone)]
struct DefaultTimeRangeGenerator {
    prev_request_end_ns: u64,
    last_end_ns: u64,
    durations_ns: Vec<Option<u64>>,
    point_data: bool,
    iteration_index: usize,
    duration_index: usize,
    last_duration_ns: u64,
    stopped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimeRangeWindow {
    start_ns: u64,
    end_ns: u64,
}

pub(super) fn has_time_range_pipeline_params(params: Option<&Params>) -> bool {
    params.is_some_and(|params| params.contains_key(TIME_RANGE_GENERATOR))
}

pub(super) fn is_time_range_pipeline_variant(req: &RequestCommand) -> bool {
    matches!(
        req,
        RequestCommand::Quotes(_)
            | RequestCommand::Trades(_)
            | RequestCommand::FundingRates(_)
            | RequestCommand::Bars(_)
            | RequestCommand::BookDeltas(_)
            | RequestCommand::BookDepth(_)
            | RequestCommand::Join(_)
    )
}

impl DataEngine {
    pub(super) fn execute_time_range_pipeline_request(
        &mut self,
        req: RequestCommand,
    ) -> anyhow::Result<()> {
        let request_id = *req.request_id();
        let (start_dt, end_dt, start_ns, end_ns) = self.bound_time_range_pipeline_dates(&req)?;
        let ts_init = self.clock.borrow().timestamp_ns();
        let parent =
            time_range_parent_request_with_dates(req, Some(start_dt), Some(end_dt), ts_init);
        let response_client_id = self.time_range_pipeline_response_client_id(&parent);
        let generator = DefaultTimeRangeGenerator::new(request_params(&parent), start_ns, end_ns)?;
        if matches!(parent, RequestCommand::Join(_)) && !generator.can_yield_initial_window() {
            anyhow::bail!("Cannot execute time-range RequestJoin without a child window");
        }

        self.time_range_pipeline_requests.insert(
            request_id,
            TimeRangePipelineState {
                parent,
                generator,
                start_ns,
                end_ns,
                response_client_id,
                data_count: 0,
                last_response: None,
            },
        );

        if let Err(e) = self.dispatch_next_time_range_pipeline_request(request_id, None) {
            self.abort_time_range_pipeline(request_id);
            return Err(e);
        }

        Ok(())
    }

    fn bound_time_range_pipeline_dates(
        &self,
        req: &RequestCommand,
    ) -> anyhow::Result<(
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
        UnixNanos,
        UnixNanos,
    )> {
        let now_ns = self.clock.borrow().timestamp_ns();
        let now = now_ns.to_datetime_utc();
        let zero = chrono::DateTime::<chrono::Utc>::from_timestamp_nanos(0);
        let (start, end) = time_range_request_dates(req);
        let mut start = start.unwrap_or(zero);
        let mut end = end.unwrap_or(now);
        let query_past_data = request_params(req)
            .and_then(|params| params.get("subscription_name"))
            .is_none();

        if query_past_data {
            start = start.min(now);
            end = end.min(now);
        }

        Ok((
            start,
            end,
            datetime_to_unix_nanos(start)?,
            datetime_to_unix_nanos(end)?,
        ))
    }

    fn time_range_pipeline_response_client_id(&mut self, req: &RequestCommand) -> ClientId {
        let request_client_id = req.client_id().copied();
        let venue = req.venue().copied();
        self.get_client(request_client_id.as_ref(), venue.as_ref())
            .map(|client| client.client_id())
            .or(request_client_id)
            .unwrap_or_else(|| ClientId::new("CATALOG"))
    }

    fn dispatch_next_time_range_pipeline_request(
        &mut self,
        parent_id: UUID4,
        data_received: Option<bool>,
    ) -> anyhow::Result<()> {
        let ts_init = self.clock.borrow().timestamp_ns();
        let child = {
            let Some(state) = self.time_range_pipeline_requests.get_mut(&parent_id) else {
                anyhow::bail!("No active time-range pipeline for {parent_id}");
            };

            state.generator.next_window(data_received).map(|window| {
                time_range_child_request(&state.parent, window.start_ns, window.end_ns, ts_init)
            })
        };
        let Some(child) = child else {
            self.emit_empty_time_range_pipeline_response(parent_id);
            return Ok(());
        };

        let child_id = *child.request_id();
        self.time_range_pipeline_parent_request_id
            .insert(child_id, parent_id);

        if let Err(e) = self.execute_request(child) {
            self.time_range_pipeline_parent_request_id.remove(&child_id);
            return Err(e);
        }

        Ok(())
    }

    fn abort_time_range_pipeline(&mut self, parent_id: UUID4) {
        self.time_range_pipeline_requests.remove(&parent_id);
        self.time_range_pipeline_parent_request_id
            .retain(|_, p_id| *p_id != parent_id);
    }

    pub(super) fn handle_time_range_pipeline_child_response(
        &mut self,
        parent_id: UUID4,
        resp: &DataResponse,
    ) {
        if !self.time_range_pipeline_requests.contains_key(&parent_id) {
            log::error!("No active time-range pipeline for child response {parent_id}");
            return;
        }

        let data_count = response_params(resp)
            .and_then(|params| params.get("data_count"))
            .and_then(serde_json::Value::as_u64)
            .or_else(|| resp.record_count().map(|count| count as u64))
            .unwrap_or(0);

        if let Some(state) = self.time_range_pipeline_requests.get_mut(&parent_id) {
            state.data_count += data_count;
            state.last_response = Some(resp.clone());
        }

        self.handle_time_range_pipeline_payload(parent_id, resp);

        if let Err(e) =
            self.dispatch_next_time_range_pipeline_request(parent_id, Some(data_count > 0))
        {
            log::error!("Error dispatching time-range pipeline child for {parent_id}: {e}");
            self.emit_empty_time_range_pipeline_response(parent_id);
        }
    }

    fn handle_time_range_pipeline_payload(&self, parent_id: UUID4, resp: &DataResponse) {
        match resp {
            DataResponse::Quotes(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, resp.correlation_id()) {
                    self.handle_quotes(&r.data);
                }
            }
            DataResponse::Trades(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, resp.correlation_id()) {
                    self.handle_trades(&r.data);
                }
            }
            DataResponse::FundingRates(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, resp.correlation_id()) {
                    self.handle_funding_rates(&r.data);
                }
            }
            DataResponse::Bars(r) => {
                if !log_if_empty_response(&r.data, &r.bar_type, resp.correlation_id()) {
                    self.handle_bars(&r.data);
                }
            }
            DataResponse::BookDeltas(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, resp.correlation_id()) {
                    self.handle_book_deltas_response(r);
                }
            }
            DataResponse::BookDepth(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, resp.correlation_id()) {
                    self.handle_book_depth_response(r);
                }
            }
            _ => {
                log::error!(
                    "Time-range pipeline child response {parent_id} must contain a supported date-range variant"
                );
                return;
            }
        }

        self.process_time_range_pipeline_aggregation_response(parent_id, resp);
    }

    fn process_time_range_pipeline_aggregation_response(
        &self,
        parent_id: UUID4,
        resp: &DataResponse,
    ) {
        let Some(state) = self.request_bar_aggregations.get(&parent_id).cloned() else {
            return;
        };

        match resp {
            DataResponse::Quotes(r) => {
                for quote in &r.data {
                    self.update_request_bar_aggregators_from_quote(&state, parent_id, *quote);
                }
            }
            DataResponse::Trades(r) => {
                for trade in &r.data {
                    self.update_request_bar_aggregators_from_trade(&state, parent_id, *trade);
                }
            }
            DataResponse::Bars(r) => {
                for bar in &r.data {
                    self.update_request_bar_aggregators_from_bar(&state, parent_id, *bar);
                }
            }
            _ => {}
        }
    }

    fn emit_empty_time_range_pipeline_response(&mut self, parent_id: UUID4) {
        let Some(state) = self.time_range_pipeline_requests.remove(&parent_id) else {
            return;
        };
        self.time_range_pipeline_parent_request_id
            .retain(|_, p_id| *p_id != parent_id);

        let mut params = request_params(&state.parent).cloned().unwrap_or_default();
        if state.data_count != 0 {
            params.insert(
                "data_count".to_string(),
                serde_json::json!(state.data_count),
            );
        }

        let Some(response) = empty_time_range_parent_response(
            parent_id,
            &state,
            self.clock.borrow().timestamp_ns(),
            Some(params),
        ) else {
            log::error!("Cannot emit empty time-range response for parent {parent_id}");
            return;
        };
        self.response(response);
    }
}

impl DefaultTimeRangeGenerator {
    fn new(
        params: Option<&Params>,
        start_ns: UnixNanos,
        end_ns: UnixNanos,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            prev_request_end_ns: start_ns.as_u64(),
            last_end_ns: end_ns.as_u64(),
            durations_ns: parse_time_range_durations(params)?,
            point_data: params
                .and_then(|params| params.get_bool(TIME_RANGE_POINT_DATA))
                .unwrap_or(false),
            iteration_index: 0,
            duration_index: 0,
            last_duration_ns: 0,
            stopped: false,
        })
    }

    fn next_window(&mut self, data_received: Option<bool>) -> Option<TimeRangeWindow> {
        if self.stopped {
            return None;
        }

        if let Some(data_received) = data_received {
            self.iteration_index = self.iteration_index.saturating_add(1);

            if self.last_duration_ns == 0 {
                self.stopped = true;
                return None;
            }

            if data_received {
                self.duration_index = 0;
            }
        }

        let Some(duration_ns) = self.durations_ns.get(self.duration_index).copied() else {
            self.stopped = true;
            return None;
        };
        self.duration_index += 1;

        let offset = u64::from(self.iteration_index > 0 && !self.point_data);
        let request_start_ns = self.prev_request_end_ns.saturating_add(offset);
        if request_start_ns > self.last_end_ns {
            self.stopped = true;
            return None;
        }

        let request_end_ns = if let Some(duration_ns) = duration_ns {
            self.last_duration_ns = duration_ns;
            request_start_ns
                .checked_add(duration_ns)
                .and_then(|end| end.checked_sub(offset))
                .unwrap_or(u64::MAX)
                .min(self.last_end_ns)
        } else {
            self.last_duration_ns = 0;
            self.last_end_ns
        };

        self.prev_request_end_ns = if self.point_data && request_start_ns == self.last_end_ns {
            self.last_end_ns.saturating_add(1)
        } else {
            request_end_ns
        };

        let end_ns = if self.point_data {
            request_start_ns
        } else {
            request_end_ns
        };

        Some(TimeRangeWindow {
            start_ns: request_start_ns,
            end_ns,
        })
    }

    fn can_yield_initial_window(&self) -> bool {
        self.prev_request_end_ns <= self.last_end_ns && !self.durations_ns.is_empty()
    }
}

fn time_range_request_dates(
    req: &RequestCommand,
) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
) {
    match req {
        RequestCommand::BookDeltas(cmd) => (cmd.start, cmd.end),
        RequestCommand::BookDepth(cmd) => (cmd.start, cmd.end),
        RequestCommand::Quotes(cmd) => (cmd.start, cmd.end),
        RequestCommand::Trades(cmd) => (cmd.start, cmd.end),
        RequestCommand::FundingRates(cmd) => (cmd.start, cmd.end),
        RequestCommand::Bars(cmd) => (cmd.start, cmd.end),
        RequestCommand::Join(cmd) => (cmd.start, cmd.end),
        _ => (None, None),
    }
}

fn time_range_parent_request_with_dates(
    req: RequestCommand,
    start: Option<chrono::DateTime<chrono::Utc>>,
    end: Option<chrono::DateTime<chrono::Utc>>,
    ts_init: UnixNanos,
) -> RequestCommand {
    match req {
        RequestCommand::Quotes(cmd) => RequestCommand::Quotes(RequestQuotes {
            start,
            end,
            ts_init,
            ..cmd
        }),
        RequestCommand::Trades(cmd) => RequestCommand::Trades(RequestTrades {
            start,
            end,
            ts_init,
            ..cmd
        }),
        RequestCommand::FundingRates(cmd) => RequestCommand::FundingRates(RequestFundingRates {
            start,
            end,
            ts_init,
            ..cmd
        }),
        RequestCommand::BookDeltas(cmd) => RequestCommand::BookDeltas(RequestBookDeltas {
            start,
            end,
            ts_init,
            ..cmd
        }),
        RequestCommand::BookDepth(cmd) => RequestCommand::BookDepth(RequestBookDepth {
            start,
            end,
            ts_init,
            ..cmd
        }),
        RequestCommand::Bars(cmd) => RequestCommand::Bars(RequestBars {
            start,
            end,
            ts_init,
            ..cmd
        }),
        RequestCommand::Join(cmd) => RequestCommand::Join(RequestJoin {
            start,
            end,
            ts_init,
            ..cmd
        }),
        _ => req,
    }
}

fn time_range_child_request(
    parent: &RequestCommand,
    start_ns: u64,
    end_ns: u64,
    ts_init: UnixNanos,
) -> RequestCommand {
    let start = Some(UnixNanos::from(start_ns).to_datetime_utc());
    let end = Some(UnixNanos::from(end_ns).to_datetime_utc());
    let request_id = UUID4::new();

    match parent {
        RequestCommand::Quotes(cmd) => RequestCommand::Quotes(RequestQuotes {
            instrument_id: cmd.instrument_id,
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id,
            ts_init,
            params: time_range_child_params(cmd.params.as_ref()),
        }),
        RequestCommand::Trades(cmd) => RequestCommand::Trades(RequestTrades {
            instrument_id: cmd.instrument_id,
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id,
            ts_init,
            params: time_range_child_params(cmd.params.as_ref()),
        }),
        RequestCommand::FundingRates(cmd) => RequestCommand::FundingRates(RequestFundingRates {
            instrument_id: cmd.instrument_id,
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id,
            ts_init,
            params: time_range_child_params(cmd.params.as_ref()),
        }),
        RequestCommand::BookDeltas(cmd) => RequestCommand::BookDeltas(RequestBookDeltas {
            instrument_id: cmd.instrument_id,
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id,
            ts_init,
            params: time_range_child_params(cmd.params.as_ref()),
        }),
        RequestCommand::BookDepth(cmd) => RequestCommand::BookDepth(RequestBookDepth {
            instrument_id: cmd.instrument_id,
            start,
            end,
            limit: cmd.limit,
            depth: cmd.depth,
            client_id: cmd.client_id,
            request_id,
            ts_init,
            params: time_range_child_params(cmd.params.as_ref()),
        }),
        RequestCommand::Bars(cmd) => RequestCommand::Bars(RequestBars {
            bar_type: cmd.bar_type,
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id,
            ts_init,
            params: time_range_child_params(cmd.params.as_ref()),
        }),
        RequestCommand::Join(cmd) => RequestCommand::Join(RequestJoin {
            request_ids: cmd.request_ids.clone(),
            start,
            end,
            request_id,
            ts_init,
            params: time_range_child_params(cmd.params.as_ref()),
            correlation_id: Some(cmd.request_id),
        }),
        _ => parent.clone(),
    }
}

fn time_range_child_params(params: Option<&Params>) -> Option<Params> {
    let mut params = params.cloned()?;
    params.shift_remove(TIME_RANGE_GENERATOR);
    remove_request_bar_aggregation_params(&mut params);
    Some(params)
}

fn empty_time_range_parent_response(
    parent_id: UUID4,
    state: &TimeRangePipelineState,
    ts_init: UnixNanos,
    params: Option<Params>,
) -> Option<DataResponse> {
    let start = Some(state.start_ns);
    let end = Some(state.end_ns);

    let response = match &state.parent {
        RequestCommand::Quotes(cmd) => DataResponse::Quotes(QuotesResponse::new(
            parent_id,
            state.response_client_id,
            cmd.instrument_id,
            Vec::new(),
            start,
            end,
            ts_init,
            params,
        )),
        RequestCommand::Trades(cmd) => DataResponse::Trades(TradesResponse::new(
            parent_id,
            state.response_client_id,
            cmd.instrument_id,
            Vec::new(),
            start,
            end,
            ts_init,
            params,
        )),
        RequestCommand::FundingRates(cmd) => DataResponse::FundingRates(FundingRatesResponse::new(
            parent_id,
            state.response_client_id,
            cmd.instrument_id,
            Vec::new(),
            start,
            end,
            ts_init,
            params,
        )),
        RequestCommand::Bars(cmd) => DataResponse::Bars(BarsResponse::new(
            parent_id,
            state.response_client_id,
            cmd.bar_type,
            Vec::new(),
            start,
            end,
            ts_init,
            params,
        )),
        RequestCommand::BookDeltas(cmd) => DataResponse::BookDeltas(BookDeltasResponse::new(
            parent_id,
            state.response_client_id,
            cmd.instrument_id,
            Vec::new(),
            start,
            end,
            ts_init,
            params,
        )),
        RequestCommand::BookDepth(cmd) => DataResponse::BookDepth(BookDepthResponse::new(
            parent_id,
            state.response_client_id,
            cmd.instrument_id,
            Vec::new(),
            start,
            end,
            ts_init,
            params,
        )),
        RequestCommand::Join(_) => empty_time_range_response_from_template(
            state.last_response.as_ref()?,
            parent_id,
            start,
            end,
            ts_init,
            params,
        )?,
        _ => return None,
    };

    Some(response)
}

fn empty_time_range_response_from_template(
    template: &DataResponse,
    parent_id: UUID4,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    ts_init: UnixNanos,
    params: Option<Params>,
) -> Option<DataResponse> {
    match template {
        DataResponse::Quotes(r) => Some(DataResponse::Quotes(QuotesResponse::new(
            parent_id,
            r.client_id,
            r.instrument_id,
            Vec::new(),
            start,
            end,
            ts_init,
            params,
        ))),
        DataResponse::Trades(r) => Some(DataResponse::Trades(TradesResponse::new(
            parent_id,
            r.client_id,
            r.instrument_id,
            Vec::new(),
            start,
            end,
            ts_init,
            params,
        ))),
        DataResponse::FundingRates(r) => {
            Some(DataResponse::FundingRates(FundingRatesResponse::new(
                parent_id,
                r.client_id,
                r.instrument_id,
                Vec::new(),
                start,
                end,
                ts_init,
                params,
            )))
        }
        DataResponse::Bars(r) => Some(DataResponse::Bars(BarsResponse::new(
            parent_id,
            r.client_id,
            r.bar_type,
            Vec::new(),
            start,
            end,
            ts_init,
            params,
        ))),
        DataResponse::BookDeltas(r) => Some(DataResponse::BookDeltas(BookDeltasResponse::new(
            parent_id,
            r.client_id,
            r.instrument_id,
            Vec::new(),
            start,
            end,
            ts_init,
            params,
        ))),
        DataResponse::BookDepth(r) => Some(DataResponse::BookDepth(BookDepthResponse::new(
            parent_id,
            r.client_id,
            r.instrument_id,
            Vec::new(),
            start,
            end,
            ts_init,
            params,
        ))),
        other => {
            log::error!(
                "Cannot fabricate empty time-range response for variant {}",
                other.kind(),
            );
            None
        }
    }
}

fn parse_time_range_durations(params: Option<&Params>) -> anyhow::Result<Vec<Option<u64>>> {
    let Some(value) = params.and_then(|params| params.get(TIME_RANGE_DURATIONS_SECONDS)) else {
        return Ok(vec![None]);
    };

    let values = value
        .as_array()
        .context("`durations_seconds` request parameter must be an array")?;
    values
        .iter()
        .map(parse_time_range_duration)
        .collect::<anyhow::Result<Vec<_>>>()
}

fn parse_time_range_duration(value: &Value) -> anyhow::Result<Option<u64>> {
    if value.is_null() {
        return Ok(None);
    }

    let seconds = value.as_f64().with_context(|| {
        format!("`durations_seconds` request parameter must contain numbers or null, was {value}")
    })?;

    if !seconds.is_finite() || seconds < 0.0 {
        anyhow::bail!(
            "`durations_seconds` request parameter must contain non-negative finite values, was {value}"
        );
    }

    let nanos = seconds * NANOSECONDS_IN_SECOND as f64;
    if nanos < 1.0 {
        anyhow::bail!(
            "`durations_seconds` request parameter must contain values of at least one nanosecond, was {value}"
        );
    }

    if nanos > u64::MAX as f64 {
        anyhow::bail!("`durations_seconds` value is too large, was {value}");
    }

    Ok(Some(nanos as u64))
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::data::{RequestCommand, RequestInstrument};
    use nautilus_core::{Params, UUID4, UnixNanos};
    use nautilus_model::identifiers::{ClientId, InstrumentId};
    use rstest::rstest;
    use serde_json::json;

    use super::{
        DefaultTimeRangeGenerator, TimeRangePipelineState, empty_time_range_parent_response,
    };

    #[rstest]
    #[case(json!("invalid"), "must be an array")]
    #[case(json!(["invalid"]), "must contain numbers or null")]
    #[case(json!([-1]), "non-negative finite values")]
    #[case(json!([0]), "at least one nanosecond")]
    #[case(json!([0.0000000001]), "at least one nanosecond")]
    #[case(json!([1.0e20]), "too large")]
    fn test_time_range_generator_rejects_invalid_durations(
        #[case] durations: serde_json::Value,
        #[case] expected: &str,
    ) {
        let params: Params = serde_json::from_value(json!({
            "durations_seconds": durations,
        }))
        .unwrap();

        let result =
            DefaultTimeRangeGenerator::new(Some(&params), UnixNanos::from(0), UnixNanos::from(10));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(expected));
    }

    #[rstest]
    fn test_empty_time_range_parent_response_returns_none_for_unsupported_parent() {
        let state = TimeRangePipelineState {
            parent: RequestCommand::Instrument(RequestInstrument::new(
                InstrumentId::from("BTCUSDT.BINANCE"),
                None,
                None,
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
            )),
            generator: DefaultTimeRangeGenerator::new(
                None,
                UnixNanos::from(0u64),
                UnixNanos::from(1u64),
            )
            .unwrap(),
            start_ns: UnixNanos::from(0u64),
            end_ns: UnixNanos::from(1u64),
            response_client_id: ClientId::new("TEST"),
            data_count: 0,
            last_response: None,
        };

        let result =
            empty_time_range_parent_response(UUID4::new(), &state, UnixNanos::from(2u64), None);

        assert!(result.is_none());
    }
}
