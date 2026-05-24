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

use ahash::AHashMap;
use chrono::{DateTime, Utc};
use nautilus_common::messages::data::{
    BarsResponse, BookDeltasResponse, DataResponse, QuotesResponse, RequestBars, RequestBookDeltas,
    RequestCommand, RequestQuotes, RequestTrades, SubscribeBars, SubscribeCommand,
    SubscribeCustomData, SubscribeQuotes, SubscribeTrades, TradesResponse,
};
use nautilus_core::{
    Params, UUID4, UnixNanos,
    correctness::{FAILED, check_key_not_in_map},
};
use nautilus_model::{
    data::{Bar, OrderBookDelta, QuoteTick, TradeTick},
    identifiers::ClientId,
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use serde_json::Value;
use ustr::Ustr;

use super::{DataEngine, requests::request_params};

const PARAM_SKIP_CATALOG_DATA: &str = "skip_catalog_data";
const PARAM_UPDATE_CATALOG: &str = "update_catalog";
const PARAM_SUBSCRIPTION_NAME: &str = "subscription_name";
const CATALOG_CLIENT_ID: &str = "CATALOG";

pub(crate) type CatalogMap = AHashMap<Ustr, ParquetDataCatalog>;

impl DataEngine {
    /// Registers the `catalog` with the engine with an optional specific `name`.
    ///
    /// # Panics
    ///
    /// Panics if a catalog with the same `name` has already been registered.
    pub fn register_catalog(&mut self, catalog: ParquetDataCatalog, name: Option<&str>) {
        let name = Ustr::from(name.unwrap_or("catalog_0"));

        check_key_not_in_map(&name, &self.catalogs, "name", "catalogs").expect(FAILED);

        self.catalogs.insert(name, catalog);
        log::info!("Registered catalog <{name}>");
    }

    pub(super) fn subscribe_command_with_prefilled_start_ns(
        &self,
        cmd: SubscribeCommand,
    ) -> anyhow::Result<SubscribeCommand> {
        match cmd {
            SubscribeCommand::Quotes(cmd) if Self::is_start_ns_missing(cmd.params.as_ref()) => {
                let identifier = cmd.instrument_id.to_string();
                let params = self.params_with_prefilled_start_ns(
                    cmd.params.as_ref(),
                    "quotes",
                    &identifier,
                )?;
                Ok(SubscribeCommand::Quotes(SubscribeQuotes { params, ..cmd }))
            }
            SubscribeCommand::Trades(cmd) if Self::is_start_ns_missing(cmd.params.as_ref()) => {
                let identifier = cmd.instrument_id.to_string();
                let params = self.params_with_prefilled_start_ns(
                    cmd.params.as_ref(),
                    "trades",
                    &identifier,
                )?;
                Ok(SubscribeCommand::Trades(SubscribeTrades { params, ..cmd }))
            }
            SubscribeCommand::Bars(cmd)
                if cmd.bar_type.is_externally_aggregated()
                    && Self::is_start_ns_missing(cmd.params.as_ref()) =>
            {
                let identifier = cmd.bar_type.to_string();
                let params =
                    self.params_with_prefilled_start_ns(cmd.params.as_ref(), "bars", &identifier)?;
                Ok(SubscribeCommand::Bars(SubscribeBars { params, ..cmd }))
            }
            SubscribeCommand::Data(cmd) if Self::is_start_ns_missing(cmd.params.as_ref()) => {
                let type_name = cmd.data_type.type_name().to_string();
                let identifier = cmd.data_type.identifier().map(String::from);
                let params = self.params_with_custom_data_prefilled_start_ns(
                    cmd.params.as_ref(),
                    &type_name,
                    identifier.as_deref(),
                )?;
                Ok(SubscribeCommand::Data(SubscribeCustomData {
                    params,
                    ..cmd
                }))
            }
            _ => Ok(cmd),
        }
    }

    fn is_start_ns_missing(params: Option<&Params>) -> bool {
        params.is_none_or(|params| !params.contains_key("start_ns"))
    }

    fn params_with_prefilled_start_ns(
        &self,
        params: Option<&Params>,
        data_cls: &str,
        identifier: &str,
    ) -> anyhow::Result<Option<Params>> {
        let last_timestamp = self.catalog_last_timestamp(data_cls, identifier)?;

        Ok(Some(Self::params_with_start_ns(params, last_timestamp)))
    }

    fn params_with_custom_data_prefilled_start_ns(
        &self,
        params: Option<&Params>,
        type_name: &str,
        identifier: Option<&str>,
    ) -> anyhow::Result<Option<Params>> {
        let last_timestamp = self.catalog_custom_data_last_timestamp(type_name, identifier)?;

        Ok(Some(Self::params_with_start_ns(params, last_timestamp)))
    }

    fn params_with_start_ns(params: Option<&Params>, last_timestamp: Option<u64>) -> Params {
        let start_ns = last_timestamp.map_or(Value::Null, |last_timestamp| {
            Value::from(last_timestamp.saturating_add(1))
        });
        let mut params = params.cloned().unwrap_or_else(Params::new);

        params.insert("start_ns".to_string(), start_ns);

        params
    }

    fn catalog_last_timestamp(
        &self,
        data_cls: &str,
        identifier: &str,
    ) -> anyhow::Result<Option<u64>> {
        for catalog in self.catalogs.values() {
            if let Some(last_timestamp) =
                catalog.query_last_timestamp(data_cls, Some(identifier))?
            {
                return Ok(Some(last_timestamp));
            }
        }

        Ok(None)
    }

    fn catalog_custom_data_last_timestamp(
        &self,
        type_name: &str,
        identifier: Option<&str>,
    ) -> anyhow::Result<Option<u64>> {
        for catalog in self.catalogs.values() {
            let last_timestamp = if let Some(identifier) = identifier {
                let directory = catalog.make_path_custom_data(type_name, Some(identifier))?;
                let intervals = catalog.get_directory_intervals(&directory)?;
                intervals.last().map(|(_, last_timestamp)| *last_timestamp)
            } else {
                let data_cls = format!("custom/{type_name}");
                catalog.query_last_timestamp(&data_cls, None)?
            };

            if let Some(last_timestamp) = last_timestamp {
                return Ok(Some(last_timestamp));
            }
        }

        Ok(None)
    }

    pub(super) fn catalogs_registered(&self) -> bool {
        !self.catalogs.is_empty()
    }

    // Mirrors Cython `_handle_date_range_request` (engine.pyx:2071-2144): bound the
    // request window, walk the catalogs to find one whose missing-intervals differ
    // from the full requested range, then fan the parent out via the pipeline with
    // one catalog leg plus one client leg per missing interval. With no catalog
    // match and no resolvable client the engine emits an empty response keyed by
    // the parent request id.
    pub(super) fn dispatch_date_range_request(
        &mut self,
        req: RequestCommand,
    ) -> anyhow::Result<()> {
        let Some((data_cls, identifier)) = request_identifier(&req) else {
            return self.dispatch_request_to_client(req).map(|_| ());
        };

        let now_ns = self.clock.borrow().timestamp_ns();
        let now_dt = now_ns.to_datetime_utc();
        let query_past_data = request_params(&req)
            .and_then(|p| p.get(PARAM_SUBSCRIPTION_NAME))
            .is_none();

        let (start_dt, end_dt) = bound_request_dates(
            request_start(&req),
            request_end(&req),
            now_dt,
            query_past_data,
        );
        let start_ns = datetime_to_unix_nanos_or_zero(start_dt);
        let end_ns = datetime_to_unix_nanos_or_zero(end_dt);

        if start_ns > end_ns {
            anyhow::bail!(
                "Cannot dispatch request, start {start_ns} was greater than end {end_ns}"
            );
        }

        let client_id = req.client_id().copied();
        let venue = req.venue().copied();
        let used_client_id = self
            .get_client(client_id.as_ref(), venue.as_ref())
            .map(|client| client.client_id());

        let query_interval = vec![(start_ns.as_u64(), end_ns.as_u64())];
        let mut missing_intervals = query_interval.clone();
        let mut has_catalog_data = false;
        let mut winning_catalog: Option<Ustr> = None;

        for (name, catalog) in &self.catalogs {
            let intervals = catalog.get_missing_intervals_for_request(
                start_ns.as_u64(),
                end_ns.as_u64(),
                data_cls,
                Some(&identifier),
            )?;

            if intervals != query_interval {
                missing_intervals = intervals;
                has_catalog_data = true;
                winning_catalog = Some(*name);
                break;
            }
        }

        let skip_catalog_data = request_params(&req)
            .and_then(|p| p.get_bool(PARAM_SKIP_CATALOG_DATA))
            .unwrap_or(false);

        // When `skip_catalog_data` is set the client must serve the full parent window;
        // dropping the catalog leg without resetting the missing intervals would leave
        // the catalog-covered range unanswered.
        if skip_catalog_data {
            missing_intervals = query_interval;
        }

        let n_client_requests = if used_client_id.is_some() {
            missing_intervals.len()
        } else {
            0
        };
        let n_catalog_requests = usize::from(has_catalog_data && !skip_catalog_data);
        let n_requests = n_client_requests + n_catalog_requests;

        if n_requests == 0 {
            let empty = build_empty_response(&req, start_ns, end_ns, used_client_id, now_ns);
            self.response(empty);
            return Ok(());
        }

        let parent_id = *req.request_id();
        self.new_request_pipeline(req.clone(), n_requests);

        if n_catalog_requests == 1
            && let Some(catalog_name) = winning_catalog
        {
            let leg = with_dates_for_pipeline(&req, Some(start_dt), Some(end_dt), now_ns);
            let leg_id = *leg.request_id();
            self.register_request_pipeline_leg(leg_id, parent_id);

            match self.query_catalog_leg(
                &leg,
                catalog_name,
                start_ns,
                end_ns,
                used_client_id,
                now_ns,
            ) {
                Ok(resp) => self.response(resp),
                Err(e) => {
                    log::error!(
                        "Catalog leg query failed for parent {parent_id} (catalog {catalog_name}): {e}"
                    );
                    let empty =
                        build_empty_response(&leg, start_ns, end_ns, used_client_id, now_ns);
                    self.response(empty);
                }
            }
        }

        if n_client_requests > 0 {
            for (leg_start_ns, leg_end_ns) in &missing_intervals {
                let leg_start_dt = UnixNanos::from(*leg_start_ns).to_datetime_utc();
                let leg_end_dt = UnixNanos::from(*leg_end_ns).to_datetime_utc();
                let leg =
                    with_dates_for_pipeline(&req, Some(leg_start_dt), Some(leg_end_dt), now_ns);
                let leg_id = *leg.request_id();
                self.register_request_pipeline_leg(leg_id, parent_id);

                if let Err(e) = self.dispatch_request_to_client(leg) {
                    // Abort the whole pipeline so the parent does not stay half-registered
                    // waiting on a leg the client never accepted. Any catalog leg already
                    // buffered for this parent is discarded with the pipeline state.
                    log::error!("Client leg dispatch failed for parent {parent_id}: {e}");
                    self.abort_request_pipeline(parent_id);
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    fn abort_request_pipeline(&mut self, parent_id: UUID4) {
        self.request_pipeline_n_components.remove(&parent_id);
        self.request_pipeline_parent_request.remove(&parent_id);
        self.request_pipeline_responses.remove(&parent_id);
        self.request_pipeline_parent_request_id
            .retain(|_, p_id| *p_id != parent_id);
    }

    fn query_catalog_leg(
        &mut self,
        leg: &RequestCommand,
        catalog_name: Ustr,
        start_ns: UnixNanos,
        end_ns: UnixNanos,
        used_client_id: Option<ClientId>,
        ts_init: UnixNanos,
    ) -> anyhow::Result<DataResponse> {
        let catalog = self.catalogs.get_mut(&catalog_name).ok_or_else(|| {
            anyhow::anyhow!("Catalog {catalog_name} disappeared between intervals query and read")
        })?;

        match leg {
            RequestCommand::Quotes(cmd) => {
                let data: Vec<QuoteTick> = catalog.quote_ticks(
                    Some(vec![cmd.instrument_id.to_string()]),
                    Some(start_ns),
                    Some(end_ns),
                )?;
                Ok(build_quotes_catalog_response(
                    cmd,
                    data,
                    start_ns,
                    end_ns,
                    used_client_id,
                    ts_init,
                ))
            }
            RequestCommand::Trades(cmd) => {
                let data: Vec<TradeTick> = catalog.trade_ticks(
                    Some(vec![cmd.instrument_id.to_string()]),
                    Some(start_ns),
                    Some(end_ns),
                )?;
                Ok(build_trades_catalog_response(
                    cmd,
                    data,
                    start_ns,
                    end_ns,
                    used_client_id,
                    ts_init,
                ))
            }
            RequestCommand::Bars(cmd) => {
                let data: Vec<Bar> = catalog.bars(
                    Some(vec![cmd.bar_type.to_string()]),
                    Some(start_ns),
                    Some(end_ns),
                )?;
                Ok(build_bars_catalog_response(
                    cmd,
                    data,
                    start_ns,
                    end_ns,
                    used_client_id,
                    ts_init,
                ))
            }
            RequestCommand::BookDeltas(cmd) => {
                let data: Vec<OrderBookDelta> = catalog.order_book_deltas(
                    Some(vec![cmd.instrument_id.to_string()]),
                    Some(start_ns),
                    Some(end_ns),
                )?;
                Ok(build_book_deltas_catalog_response(
                    cmd,
                    data,
                    start_ns,
                    end_ns,
                    used_client_id,
                    ts_init,
                ))
            }
            _ => {
                anyhow::bail!("query_catalog_leg called with non-catalog-eligible variant {leg:?}")
            }
        }
    }
}

pub(super) fn is_date_range_variant(req: &RequestCommand) -> bool {
    matches!(
        req,
        RequestCommand::Quotes(_)
            | RequestCommand::Trades(_)
            | RequestCommand::Bars(_)
            | RequestCommand::BookDeltas(_)
    )
}

fn request_identifier(req: &RequestCommand) -> Option<(&'static str, String)> {
    match req {
        RequestCommand::Quotes(cmd) => Some(("quotes", cmd.instrument_id.to_string())),
        RequestCommand::Trades(cmd) => Some(("trades", cmd.instrument_id.to_string())),
        RequestCommand::Bars(cmd) => Some(("bars", cmd.bar_type.to_string())),
        RequestCommand::BookDeltas(cmd) => {
            Some(("order_book_deltas", cmd.instrument_id.to_string()))
        }
        _ => None,
    }
}

fn request_start(req: &RequestCommand) -> Option<DateTime<Utc>> {
    match req {
        RequestCommand::Quotes(cmd) => cmd.start,
        RequestCommand::Trades(cmd) => cmd.start,
        RequestCommand::Bars(cmd) => cmd.start,
        RequestCommand::BookDeltas(cmd) => cmd.start,
        _ => None,
    }
}

fn request_end(req: &RequestCommand) -> Option<DateTime<Utc>> {
    match req {
        RequestCommand::Quotes(cmd) => cmd.end,
        RequestCommand::Trades(cmd) => cmd.end,
        RequestCommand::Bars(cmd) => cmd.end,
        RequestCommand::BookDeltas(cmd) => cmd.end,
        _ => None,
    }
}

fn bound_request_dates(
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    query_past_data: bool,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let zero = DateTime::<Utc>::from_timestamp_nanos(0);
    let mut start = start.unwrap_or(zero);
    let mut end = end.unwrap_or(now);

    if query_past_data {
        if start > now {
            start = now;
        }

        if end > now {
            end = now;
        }
    }

    (start, end)
}

fn datetime_to_unix_nanos_or_zero(dt: DateTime<Utc>) -> UnixNanos {
    UnixNanos::from(u64::try_from(dt.timestamp_nanos_opt().unwrap_or(0).max(0)).unwrap_or(0))
}

fn with_dates_for_pipeline(
    req: &RequestCommand,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    ts_init: UnixNanos,
) -> RequestCommand {
    let new_id = UUID4::new();

    match req {
        RequestCommand::Quotes(cmd) => RequestCommand::Quotes(RequestQuotes {
            instrument_id: cmd.instrument_id,
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id: new_id,
            ts_init,
            params: cmd.params.clone(),
        }),
        RequestCommand::Trades(cmd) => RequestCommand::Trades(RequestTrades {
            instrument_id: cmd.instrument_id,
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id: new_id,
            ts_init,
            params: cmd.params.clone(),
        }),
        RequestCommand::BookDeltas(cmd) => RequestCommand::BookDeltas(RequestBookDeltas {
            instrument_id: cmd.instrument_id,
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id: new_id,
            ts_init,
            params: cmd.params.clone(),
        }),
        RequestCommand::Bars(cmd) => RequestCommand::Bars(RequestBars {
            bar_type: cmd.bar_type,
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id: new_id,
            ts_init,
            params: cmd.params.clone(),
        }),
        // `Join` and the non-date-range variants should never reach this path; the dispatcher
        // gates on `is_date_range_variant` first. Cloning preserves behaviour if a caller
        // reaches this arm.
        _ => req.clone(),
    }
}

fn build_empty_response(
    req: &RequestCommand,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> DataResponse {
    match req {
        RequestCommand::Quotes(cmd) => DataResponse::Quotes(QuotesResponse::new(
            cmd.request_id,
            resolve_response_client_id(cmd.client_id, used_client_id),
            cmd.instrument_id,
            Vec::new(),
            Some(start),
            Some(end),
            ts_init,
            cmd.params.clone(),
        )),
        RequestCommand::Trades(cmd) => DataResponse::Trades(TradesResponse::new(
            cmd.request_id,
            resolve_response_client_id(cmd.client_id, used_client_id),
            cmd.instrument_id,
            Vec::new(),
            Some(start),
            Some(end),
            ts_init,
            cmd.params.clone(),
        )),
        RequestCommand::Bars(cmd) => DataResponse::Bars(BarsResponse::new(
            cmd.request_id,
            resolve_response_client_id(cmd.client_id, used_client_id),
            cmd.bar_type,
            Vec::new(),
            Some(start),
            Some(end),
            ts_init,
            cmd.params.clone(),
        )),
        RequestCommand::BookDeltas(cmd) => DataResponse::BookDeltas(BookDeltasResponse::new(
            cmd.request_id,
            resolve_response_client_id(cmd.client_id, used_client_id),
            cmd.instrument_id,
            Vec::new(),
            Some(start),
            Some(end),
            ts_init,
            cmd.params.clone(),
        )),
        _ => unreachable!("build_empty_response called with non-catalog-eligible variant"),
    }
}

fn build_quotes_catalog_response(
    cmd: &RequestQuotes,
    data: Vec<QuoteTick>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> DataResponse {
    let params = catalog_response_params(cmd.params.as_ref());
    DataResponse::Quotes(QuotesResponse::new(
        cmd.request_id,
        resolve_response_client_id(cmd.client_id, used_client_id),
        cmd.instrument_id,
        data,
        Some(start),
        Some(end),
        ts_init,
        Some(params),
    ))
}

fn build_trades_catalog_response(
    cmd: &RequestTrades,
    data: Vec<TradeTick>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> DataResponse {
    let params = catalog_response_params(cmd.params.as_ref());
    DataResponse::Trades(TradesResponse::new(
        cmd.request_id,
        resolve_response_client_id(cmd.client_id, used_client_id),
        cmd.instrument_id,
        data,
        Some(start),
        Some(end),
        ts_init,
        Some(params),
    ))
}

fn build_bars_catalog_response(
    cmd: &RequestBars,
    data: Vec<Bar>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> DataResponse {
    let params = catalog_response_params(cmd.params.as_ref());
    DataResponse::Bars(BarsResponse::new(
        cmd.request_id,
        resolve_response_client_id(cmd.client_id, used_client_id),
        cmd.bar_type,
        data,
        Some(start),
        Some(end),
        ts_init,
        Some(params),
    ))
}

fn build_book_deltas_catalog_response(
    cmd: &RequestBookDeltas,
    data: Vec<OrderBookDelta>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> DataResponse {
    let params = catalog_response_params(cmd.params.as_ref());
    DataResponse::BookDeltas(BookDeltasResponse::new(
        cmd.request_id,
        resolve_response_client_id(cmd.client_id, used_client_id),
        cmd.instrument_id,
        data,
        Some(start),
        Some(end),
        ts_init,
        Some(params),
    ))
}

fn catalog_response_params(existing: Option<&Params>) -> Params {
    let mut params = existing.cloned().unwrap_or_else(Params::new);
    params.insert(PARAM_UPDATE_CATALOG.to_string(), Value::Bool(false));
    params
}

fn resolve_response_client_id(
    request_client_id: Option<ClientId>,
    used_client_id: Option<ClientId>,
) -> ClientId {
    request_client_id
        .or(used_client_id)
        .unwrap_or_else(|| ClientId::new(CATALOG_CLIENT_ID))
}
