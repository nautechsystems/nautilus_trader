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
    BarsResponse, BookDeltasResponse, BookDepthResponse, CustomDataResponse, DataResponse,
    FundingRatesResponse, InstrumentResponse, InstrumentsResponse, QuotesResponse, RequestBars,
    RequestBookDeltas, RequestBookDepth, RequestCommand, RequestCustomData, RequestFundingRates,
    RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades, SubscribeBars,
    SubscribeCommand, SubscribeCustomData, SubscribeQuotes, SubscribeTrades, TradesResponse,
};
use nautilus_core::{
    Params, UUID4, UnixNanos,
    correctness::{FAILED, check_key_not_in_map},
};
use nautilus_model::{
    data::{
        Bar, CustomData, Data, FundingRateUpdate, OrderBookDelta, OrderBookDepth10, QuoteTick,
        TradeTick,
    },
    identifiers::{ClientId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use serde_json::Value;
use ustr::Ustr;

use super::{DataEngine, requests::request_params};

const PARAM_SKIP_CATALOG_DATA: &str = "skip_catalog_data";
const PARAM_UPDATE_CATALOG: &str = "update_catalog";
const PARAM_FORCE_INSTRUMENT_UPDATE: &str = "force_instrument_update";
const PARAM_SUBSCRIPTION_NAME: &str = "subscription_name";
const PARAM_FROM_DAY_START: &str = "from_day_start";
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
        if matches!(
            req,
            RequestCommand::Instrument(_) | RequestCommand::Instruments(_)
        ) {
            return self.dispatch_instrument_catalog_request(req);
        }

        let Some(key) = request_identifier(&req) else {
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

        // Floor the catalog window to the UTC day boundary so the day-start F_SNAPSHOT frame is
        // selected and read for the snapshot replay; client gaps keep the original window.
        // The parent request keeps its original start, so the merged response trims back to it.
        let (catalog_start_dt, catalog_start_ns) = if matches!(req, RequestCommand::BookDeltas(_))
            && request_params(&req)
                .and_then(|p| p.get_bool(PARAM_FROM_DAY_START))
                .unwrap_or(true)
        {
            let floored = floor_to_utc_day(start_dt);
            (floored, datetime_to_unix_nanos_or_zero(floored))
        } else {
            (start_dt, start_ns)
        };

        let query_interval = vec![(start_ns.as_u64(), end_ns.as_u64())];
        let catalog_query_interval = vec![(catalog_start_ns.as_u64(), end_ns.as_u64())];
        let mut missing_intervals = query_interval.clone();
        let mut has_catalog_data = false;
        let mut winning_catalog: Option<Ustr> = None;

        for (name, catalog) in &self.catalogs {
            let catalog_intervals = catalog_missing_intervals(
                catalog,
                catalog_start_ns.as_u64(),
                end_ns.as_u64(),
                &key,
            )?;

            if catalog_intervals != catalog_query_interval {
                has_catalog_data = true;
                winning_catalog = Some(*name);
                // Client legs fill only the requested window, not the pre-start range
                missing_intervals = if catalog_start_ns == start_ns {
                    catalog_intervals
                } else {
                    catalog_missing_intervals(catalog, start_ns.as_u64(), end_ns.as_u64(), &key)?
                };
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
            let empty = build_empty_response(&req, start_ns, end_ns, used_client_id, now_ns)?;
            self.response(empty);
            return Ok(());
        }

        let parent_id = *req.request_id();
        self.new_request_pipeline(req.clone(), n_requests);

        if n_catalog_requests == 1
            && let Some(catalog_name) = winning_catalog
        {
            let leg = with_dates_for_pipeline(&req, Some(catalog_start_dt), Some(end_dt), now_ns);
            let leg_id = *leg.request_id();
            self.register_request_pipeline_leg(leg_id, parent_id);

            match self.query_catalog_leg(
                &leg,
                catalog_name,
                catalog_start_ns,
                end_ns,
                used_client_id,
                now_ns,
            ) {
                Ok(resp) => self.response(resp),
                Err(e) => {
                    log::error!(
                        "Catalog leg query failed for parent {parent_id} (catalog {catalog_name}): {e}"
                    );
                    let empty = match build_empty_response(
                        &leg,
                        start_ns,
                        end_ns,
                        used_client_id,
                        now_ns,
                    ) {
                        Ok(empty) => empty,
                        Err(e) => {
                            self.abort_request_pipeline(parent_id);
                            return Err(e);
                        }
                    };
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
            RequestCommand::FundingRates(cmd) => {
                let data: Vec<FundingRateUpdate> = catalog.funding_rates(
                    Some(vec![cmd.instrument_id.to_string()]),
                    Some(start_ns),
                    Some(end_ns),
                )?;
                Ok(build_funding_rates_catalog_response(
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
            RequestCommand::Data(cmd) => {
                let identifiers = cmd
                    .data_type
                    .identifier()
                    .map(|identifier| vec![identifier.to_string()]);
                let where_clause = cmd
                    .params
                    .as_ref()
                    .and_then(|params| params.get_str("filter_expr"));
                let data = catalog.query_custom_data_dynamic(
                    cmd.data_type.type_name(),
                    identifiers.as_deref(),
                    Some(start_ns),
                    Some(end_ns),
                    where_clause,
                    None,
                    true,
                )?;
                Ok(build_custom_data_catalog_response(
                    cmd,
                    custom_data_from_dynamic(data),
                    start_ns,
                    end_ns,
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
            RequestCommand::BookDepth(cmd) => {
                let data: Vec<OrderBookDepth10> = catalog.order_book_depth10(
                    Some(vec![cmd.instrument_id.to_string()]),
                    Some(start_ns),
                    Some(end_ns),
                )?;
                Ok(build_book_depth_catalog_response(
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

    fn dispatch_instrument_catalog_request(&mut self, req: RequestCommand) -> anyhow::Result<()> {
        match req {
            RequestCommand::Instrument(cmd) => self.dispatch_instrument_request(cmd),
            RequestCommand::Instruments(cmd) => self.dispatch_instruments_request(cmd),
            _ => self.dispatch_request_to_client(req).map(|_| ()),
        }
    }

    fn dispatch_instrument_request(&mut self, cmd: RequestInstrument) -> anyhow::Result<()> {
        let force_instrument_update = cmd
            .params
            .as_ref()
            .and_then(|params| params.get_bool(PARAM_FORCE_INSTRUMENT_UPDATE))
            .unwrap_or(false);

        if force_instrument_update {
            return self
                .dispatch_request_to_client(RequestCommand::Instrument(cmd))
                .map(|_| ());
        }

        let identifier = cmd.instrument_id.to_string();
        let Some(catalog_name) = self.catalog_with_last_timestamp("instruments", &identifier)?
        else {
            return self
                .dispatch_request_to_client(RequestCommand::Instrument(cmd))
                .map(|_| ());
        };

        let now_ns = self.clock.borrow().timestamp_ns();
        let used_client_id = self
            .get_client(cmd.client_id.as_ref(), Some(&cmd.instrument_id.venue))
            .map(|client| client.client_id());
        let (start_dt, end_dt) =
            bound_request_dates(cmd.start, cmd.end, now_ns.to_datetime_utc(), true);
        let start_ns = datetime_to_unix_nanos_or_zero(start_dt);
        let end_ns = datetime_to_unix_nanos_or_zero(end_dt);
        let query_end = cmd.end.map(datetime_to_unix_nanos_or_zero);
        let catalog = self.catalogs.get(&catalog_name).ok_or_else(|| {
            anyhow::anyhow!("Catalog {catalog_name} disappeared between timestamp query and read")
        })?;
        let mut data = catalog.instruments(
            Some(std::slice::from_ref(&identifier)),
            Some(start_ns),
            query_end,
        )?;
        data = latest_instruments(data);

        if let Some(instrument) = data.into_iter().next() {
            let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                cmd.request_id,
                resolve_response_client_id(cmd.client_id, used_client_id),
                cmd.instrument_id,
                instrument,
                Some(start_ns),
                Some(end_ns),
                now_ns,
                Some(catalog_response_params(cmd.params.as_ref())),
            )));
            self.response(response);
            return Ok(());
        }

        self.dispatch_request_to_client(RequestCommand::Instrument(cmd))
            .map(|_| ())
    }

    fn dispatch_instruments_request(&mut self, cmd: RequestInstruments) -> anyhow::Result<()> {
        let update_catalog = cmd
            .params
            .as_ref()
            .and_then(|params| params.get_bool(PARAM_UPDATE_CATALOG))
            .unwrap_or(false);
        let force_instrument_update = cmd
            .params
            .as_ref()
            .and_then(|params| params.get_bool(PARAM_FORCE_INSTRUMENT_UPDATE))
            .unwrap_or(false);

        if update_catalog || force_instrument_update {
            return self
                .dispatch_request_to_client(RequestCommand::Instruments(cmd))
                .map(|_| ());
        }

        let now_ns = self.clock.borrow().timestamp_ns();
        let used_client_id = self
            .get_client(cmd.client_id.as_ref(), cmd.venue.as_ref())
            .map(|client| client.client_id());
        let (start_dt, end_dt) =
            bound_request_dates(cmd.start, cmd.end, now_ns.to_datetime_utc(), true);
        let start_ns = datetime_to_unix_nanos_or_zero(start_dt);
        let end_ns = datetime_to_unix_nanos_or_zero(end_dt);
        let query_end = cmd.end.map(datetime_to_unix_nanos_or_zero);
        let mut data = Vec::new();

        for catalog in self.catalogs.values() {
            data.extend(catalog.instruments(None, Some(start_ns), query_end)?);
        }

        if let Some(venue) = cmd.venue {
            data.retain(|instrument| instrument.venue() == venue);
        }

        if instrument_only_last(cmd.params.as_ref()) {
            data = latest_instruments(data);
        }

        let response = DataResponse::Instruments(InstrumentsResponse::new(
            cmd.request_id,
            resolve_response_client_id(cmd.client_id, used_client_id),
            instrument_response_venue(cmd.venue, &data),
            data,
            Some(start_ns),
            Some(end_ns),
            now_ns,
            Some(catalog_response_params(cmd.params.as_ref())),
        ));
        self.response(response);
        Ok(())
    }

    fn catalog_with_last_timestamp(
        &self,
        data_cls: &str,
        identifier: &str,
    ) -> anyhow::Result<Option<Ustr>> {
        for (name, catalog) in &self.catalogs {
            if catalog
                .query_last_timestamp(data_cls, Some(identifier))?
                .is_some()
            {
                return Ok(Some(*name));
            }
        }

        Ok(None)
    }
}

struct RequestCatalogKey {
    data_cls: String,
    type_name: Option<String>,
    identifier: Option<String>,
}

pub(super) fn is_date_range_variant(req: &RequestCommand) -> bool {
    matches!(
        req,
        RequestCommand::Data(_)
            | RequestCommand::Instrument(_)
            | RequestCommand::Instruments(_)
            | RequestCommand::Quotes(_)
            | RequestCommand::Trades(_)
            | RequestCommand::FundingRates(_)
            | RequestCommand::Bars(_)
            | RequestCommand::BookDeltas(_)
            | RequestCommand::BookDepth(_)
    )
}

fn request_identifier(req: &RequestCommand) -> Option<RequestCatalogKey> {
    match req {
        RequestCommand::Data(cmd) => Some(RequestCatalogKey {
            data_cls: format!("custom/{}", cmd.data_type.type_name()),
            type_name: Some(cmd.data_type.type_name().to_string()),
            identifier: cmd.data_type.identifier().map(String::from),
        }),
        RequestCommand::Quotes(cmd) => Some(RequestCatalogKey::new(
            "quotes",
            Some(cmd.instrument_id.to_string()),
        )),
        RequestCommand::Trades(cmd) => Some(RequestCatalogKey::new(
            "trades",
            Some(cmd.instrument_id.to_string()),
        )),
        RequestCommand::FundingRates(cmd) => Some(RequestCatalogKey::new(
            "funding_rate_update",
            Some(cmd.instrument_id.to_string()),
        )),
        RequestCommand::Bars(cmd) => Some(RequestCatalogKey::new(
            "bars",
            Some(cmd.bar_type.to_string()),
        )),
        RequestCommand::BookDeltas(cmd) => Some(RequestCatalogKey::new(
            "order_book_deltas",
            Some(cmd.instrument_id.to_string()),
        )),
        RequestCommand::BookDepth(cmd) => Some(RequestCatalogKey::new(
            "order_book_depths",
            Some(cmd.instrument_id.to_string()),
        )),
        _ => None,
    }
}

impl RequestCatalogKey {
    fn new(data_cls: &str, identifier: Option<String>) -> Self {
        Self {
            data_cls: data_cls.to_string(),
            type_name: None,
            identifier,
        }
    }
}

fn catalog_missing_intervals(
    catalog: &ParquetDataCatalog,
    start: u64,
    end: u64,
    key: &RequestCatalogKey,
) -> anyhow::Result<Vec<(u64, u64)>> {
    if let Some(type_name) = key.type_name.as_deref()
        && let Some(identifier) = key.identifier.as_deref()
    {
        let directory = catalog.make_path_custom_data(type_name, Some(identifier))?;
        let intervals = catalog.get_directory_intervals(&directory)?;
        return Ok(missing_interval_diff(start, end, &intervals));
    }

    catalog.get_missing_intervals_for_request(start, end, &key.data_cls, key.identifier.as_deref())
}

fn request_start(req: &RequestCommand) -> Option<DateTime<Utc>> {
    match req {
        RequestCommand::Data(cmd) => cmd.start,
        RequestCommand::Instrument(cmd) => cmd.start,
        RequestCommand::Instruments(cmd) => cmd.start,
        RequestCommand::Quotes(cmd) => cmd.start,
        RequestCommand::Trades(cmd) => cmd.start,
        RequestCommand::FundingRates(cmd) => cmd.start,
        RequestCommand::Bars(cmd) => cmd.start,
        RequestCommand::BookDeltas(cmd) => cmd.start,
        RequestCommand::BookDepth(cmd) => cmd.start,
        _ => None,
    }
}

fn request_end(req: &RequestCommand) -> Option<DateTime<Utc>> {
    match req {
        RequestCommand::Data(cmd) => cmd.end,
        RequestCommand::Instrument(cmd) => cmd.end,
        RequestCommand::Instruments(cmd) => cmd.end,
        RequestCommand::Quotes(cmd) => cmd.end,
        RequestCommand::Trades(cmd) => cmd.end,
        RequestCommand::FundingRates(cmd) => cmd.end,
        RequestCommand::Bars(cmd) => cmd.end,
        RequestCommand::BookDeltas(cmd) => cmd.end,
        RequestCommand::BookDepth(cmd) => cmd.end,
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

fn floor_to_utc_day(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt.date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight is always a valid time")
        .and_utc()
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
        RequestCommand::FundingRates(cmd) => RequestCommand::FundingRates(RequestFundingRates {
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
        RequestCommand::BookDepth(cmd) => RequestCommand::BookDepth(RequestBookDepth {
            instrument_id: cmd.instrument_id,
            start,
            end,
            limit: cmd.limit,
            depth: cmd.depth,
            client_id: cmd.client_id,
            request_id: new_id,
            ts_init,
            params: cmd.params.clone(),
        }),
        RequestCommand::Data(cmd) => RequestCommand::Data(RequestCustomData {
            client_id: cmd.client_id,
            data_type: cmd.data_type.clone(),
            start,
            end,
            limit: cmd.limit,
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
) -> anyhow::Result<DataResponse> {
    let response = match req {
        RequestCommand::Data(cmd) => DataResponse::Data(CustomDataResponse::new(
            cmd.request_id,
            cmd.client_id,
            None,
            cmd.data_type.clone(),
            Vec::<CustomData>::new(),
            Some(start),
            Some(end),
            ts_init,
            cmd.params.clone(),
        )),
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
        RequestCommand::FundingRates(cmd) => DataResponse::FundingRates(FundingRatesResponse::new(
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
        RequestCommand::BookDepth(cmd) => DataResponse::BookDepth(BookDepthResponse::new(
            cmd.request_id,
            resolve_response_client_id(cmd.client_id, used_client_id),
            cmd.instrument_id,
            Vec::new(),
            Some(start),
            Some(end),
            ts_init,
            cmd.params.clone(),
        )),
        _ => {
            anyhow::bail!("Cannot build empty catalog response for non-catalog-eligible request")
        }
    };

    Ok(response)
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

fn build_funding_rates_catalog_response(
    cmd: &RequestFundingRates,
    data: Vec<FundingRateUpdate>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> DataResponse {
    let params = catalog_response_params(cmd.params.as_ref());
    DataResponse::FundingRates(FundingRatesResponse::new(
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

fn build_custom_data_catalog_response(
    cmd: &RequestCustomData,
    data: Vec<CustomData>,
    start: UnixNanos,
    end: UnixNanos,
    ts_init: UnixNanos,
) -> DataResponse {
    let params = catalog_response_params(cmd.params.as_ref());
    DataResponse::Data(CustomDataResponse::new(
        cmd.request_id,
        cmd.client_id,
        None,
        cmd.data_type.clone(),
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

fn build_book_depth_catalog_response(
    cmd: &RequestBookDepth,
    data: Vec<OrderBookDepth10>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> DataResponse {
    let params = catalog_response_params(cmd.params.as_ref());
    DataResponse::BookDepth(BookDepthResponse::new(
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

fn custom_data_from_dynamic(data: Vec<Data>) -> Vec<CustomData> {
    data.into_iter()
        .filter_map(|item| match item {
            Data::Custom(custom) => Some(custom),
            other => {
                log::error!("Custom catalog query returned non-custom data {other:?}");
                None
            }
        })
        .collect()
}

fn instrument_only_last(params: Option<&Params>) -> bool {
    params
        .and_then(|params| params.get_bool("only_last"))
        .unwrap_or(true)
}

fn latest_instruments(data: Vec<InstrumentAny>) -> Vec<InstrumentAny> {
    let mut instruments: AHashMap<_, InstrumentAny> = AHashMap::new();

    for instrument in data {
        let id = instrument.id();
        match instruments.get(&id) {
            Some(existing) if existing.ts_init() >= instrument.ts_init() => {}
            _ => {
                instruments.insert(id, instrument);
            }
        }
    }

    let mut data: Vec<_> = instruments.into_values().collect();
    data.sort_by_key(|instrument| instrument.id().to_string());
    data
}

fn instrument_response_venue(request_venue: Option<Venue>, data: &[InstrumentAny]) -> Venue {
    request_venue.unwrap_or_else(|| {
        data.iter()
            .map(Instrument::venue)
            .min_by_key(std::string::ToString::to_string)
            .unwrap_or_else(|| Venue::from(CATALOG_CLIENT_ID))
    })
}

fn missing_interval_diff(start: u64, end: u64, closed_intervals: &[(u64, u64)]) -> Vec<(u64, u64)> {
    if closed_intervals.is_empty() {
        return vec![(start, end)];
    }

    let mut missing = Vec::new();
    let mut cursor = start;

    for &(closed_start, closed_end) in closed_intervals {
        if closed_end < cursor {
            continue;
        }

        if closed_start > end {
            break;
        }

        if closed_start > cursor {
            missing.push((cursor, closed_start.saturating_sub(1)));
        }

        cursor = cursor.max(closed_end.saturating_add(1));

        if cursor > end {
            break;
        }
    }

    if cursor <= end {
        missing.push((cursor, end));
    }

    missing
}

fn resolve_response_client_id(
    request_client_id: Option<ClientId>,
    used_client_id: Option<ClientId>,
) -> ClientId {
    request_client_id
        .or(used_client_id)
        .unwrap_or_else(|| ClientId::new(CATALOG_CLIENT_ID))
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::data::RequestJoin;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_build_empty_response_rejects_non_catalog_variant() {
        let request = RequestCommand::Join(RequestJoin::new(
            vec![UUID4::new()],
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ));

        let result = build_empty_response(
            &request,
            UnixNanos::from(1u64),
            UnixNanos::from(2u64),
            None,
            UnixNanos::from(3u64),
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            "Cannot build empty catalog response for non-catalog-eligible request"
        );
    }
}
