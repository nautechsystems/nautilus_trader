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

use ahash::{AHashMap, AHashSet};
use indexmap::IndexMap;
use jiff::Timestamp;
use nautilus_common::messages::data::{
    BarsResponse, BookDeltasResponse, BookDepthResponse, CustomDataResponse, DataRequestFailed,
    DataResponse, FundingRatesResponse, InstrumentResponse, InstrumentsResponse, QuotesResponse,
    RequestBars, RequestBookDeltas, RequestBookDepth, RequestCommand, RequestCustomData,
    RequestFundingRates, RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades,
    SubscribeBars, SubscribeCommand, SubscribeCustomData, SubscribeQuotes, SubscribeTrades,
    TradesResponse,
};
use nautilus_core::{
    Params, UUID4, UnixNanos,
    correctness::{FAILED, check_key_not_in_map},
};
use nautilus_model::{
    data::{
        Bar, BarType, BatchView, CustomData, DataBatch, FromDataBatch, FundingRateUpdate,
        NautilusDataType, OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick,
    },
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_persistence::catalog::traits::{CatalogInstrumentQuery, CatalogQuery, DataCatalog};
use serde_json::Value;
use ustr::Ustr;

use super::{DataEngine, requests::request_params};

const PARAM_SKIP_CATALOG_DATA: &str = "skip_catalog_data";
pub(super) const PARAM_UPDATE_CATALOG: &str = "update_catalog";
const PARAM_FORCE_INSTRUMENT_UPDATE: &str = "force_instrument_update";
const PARAM_SUBSCRIPTION_NAME: &str = "subscription_name";
const PARAM_FROM_DAY_START: &str = "from_day_start";
const CATALOG_CLIENT_ID: &str = "CATALOG";

pub(crate) type CatalogMap = AHashMap<Ustr, DataCatalog>;

impl DataEngine {
    /// Registers the `catalog` with the engine with an optional specific `name`.
    ///
    /// # Panics
    ///
    /// Panics if a catalog with the same `name` has already been registered.
    pub fn register_catalog(&mut self, catalog: DataCatalog, name: Option<&str>) {
        let name = Ustr::from(name.unwrap_or("catalog_0"));

        check_key_not_in_map(&name, &self.catalogs, "name", "catalogs").expect(FAILED);

        self.catalogs.insert(name, catalog);
        log::info!("Registered catalog <{name}>");
    }

    pub(super) fn subscribe_command_with_prefilled_start_ns(
        &mut self,
        cmd: SubscribeCommand,
    ) -> anyhow::Result<SubscribeCommand> {
        match cmd {
            SubscribeCommand::Quotes(cmd)
                if cmd.instrument_ids.len() == 1
                    && Self::is_start_ns_missing(cmd.params.as_ref()) =>
            {
                let identifier = cmd.instrument_ids[0].to_string();
                let params = self.params_with_prefilled_start_ns(
                    cmd.params.as_ref(),
                    &NautilusDataType::QuoteTick,
                    &identifier,
                )?;
                Ok(SubscribeCommand::Quotes(SubscribeQuotes { params, ..cmd }))
            }
            SubscribeCommand::Trades(cmd)
                if cmd.instrument_ids.len() == 1
                    && Self::is_start_ns_missing(cmd.params.as_ref()) =>
            {
                let identifier = cmd.instrument_ids[0].to_string();
                let params = self.params_with_prefilled_start_ns(
                    cmd.params.as_ref(),
                    &NautilusDataType::TradeTick,
                    &identifier,
                )?;
                Ok(SubscribeCommand::Trades(SubscribeTrades { params, ..cmd }))
            }
            SubscribeCommand::Bars(cmd)
                if cmd.bar_types.len() == 1
                    && cmd.single_bar_type()?.is_externally_aggregated()
                    && Self::is_start_ns_missing(cmd.params.as_ref()) =>
            {
                let identifier = cmd.single_bar_type()?.to_string();
                let params = self.params_with_prefilled_start_ns(
                    cmd.params.as_ref(),
                    &NautilusDataType::Bar,
                    &identifier,
                )?;
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
        &mut self,
        params: Option<&Params>,
        data_type: &NautilusDataType,
        identifier: &str,
    ) -> anyhow::Result<Option<Params>> {
        let last_timestamp = self.catalog_last_timestamp(data_type, identifier)?;

        Ok(Some(Self::params_with_start_ns(params, last_timestamp)))
    }

    fn params_with_custom_data_prefilled_start_ns(
        &mut self,
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

        let mut params = params
            .map(|params| IndexMap::clone(params))
            .unwrap_or_default();

        params.insert("start_ns".to_string(), start_ns);

        Params::new(params)
    }

    fn catalog_last_timestamp(
        &mut self,
        data_type: &NautilusDataType,
        identifier: &str,
    ) -> anyhow::Result<Option<u64>> {
        for catalog in self.catalogs.values_mut() {
            if let Some(last_timestamp) =
                catalog.query_last_timestamp(data_type.clone(), Some(identifier))?
            {
                return Ok(Some(last_timestamp));
            }
        }

        Ok(None)
    }

    fn catalog_custom_data_last_timestamp(
        &mut self,
        type_name: &str,
        identifier: Option<&str>,
    ) -> anyhow::Result<Option<u64>> {
        // `make_path_custom_data` / `get_directory_intervals` are inherent on
        // `ParquetDataCatalog` and not exposed through `Catalog`. Use the
        // trait-level `query_last_timestamp` with `NautilusDataType::Custom` so the
        // path works for any backend that implements the trait.
        let data_type = NautilusDataType::Custom {
            type_name: type_name.to_string(),
        };

        for catalog in self.catalogs.values_mut() {
            if let Some(last_timestamp) =
                catalog.query_last_timestamp(data_type.clone(), identifier)?
            {
                return Ok(Some(last_timestamp));
            }
        }

        Ok(None)
    }

    pub(super) fn update_catalog_from_response(
        &mut self,
        response: &DataResponse,
        request: &RequestCommand,
    ) {
        let Some(params) =
            request_params(request).filter(|p| p.get_bool(PARAM_UPDATE_CATALOG).unwrap_or(false))
        else {
            return;
        };

        if response
            .params()
            .is_some_and(|p| p.get_bool(PARAM_UPDATE_CATALOG) == Some(false))
        {
            return;
        }

        if self.catalogs.is_empty() {
            log::warn!("No catalog registered for response write-back");
            return;
        }

        if let DataResponse::Instrument(value) = response {
            self.write_response_instruments(std::slice::from_ref(&value.data), params);
            return;
        }

        if let DataResponse::Instruments(value) = response {
            self.write_response_instruments(&value.data, params);
            return;
        }

        if request_has_schema(request) {
            log::warn!(
                "Skipping catalog write-back because source schemas are not part of the catalog key, request={}",
                request.request_id()
            );
            return;
        }

        if request_limit(request).is_some() {
            log::warn!(
                "Skipping catalog write-back because a record limit can leave incomplete coverage, request={}",
                request.request_id()
            );
            return;
        }

        let batches = match response_batches(response) {
            Ok(batches) => batches,
            Err(e) => {
                log::error!("Cannot prepare response catalog write: {e}");
                return;
            }
        };

        let bounds = response_write_bounds(request, response);
        let data_type = response.data_kind();

        for (identifier, batch) in batches {
            if !request_contains_batch(request, identifier.as_deref(), &batch) {
                log::warn!(
                    "Skipping catalog write-back because identifier coverage is unverified, request={}, identifier={identifier:?}",
                    request.request_id()
                );
                continue;
            }

            if let Err(e) =
                self.write_response_batch(&batch, &data_type, identifier.as_deref(), params, bounds)
            {
                log::error!("Catalog write-back failed for {identifier:?}: {e}");
            }
        }
    }

    fn write_response_batch(
        &mut self,
        batch: &DataBatch,
        data_type: &NautilusDataType,
        identifier: Option<&str>,
        params: &Params,
        bounds: (Option<UnixNanos>, Option<UnixNanos>),
    ) -> anyhow::Result<()> {
        let name = self.catalog_for_update(data_type, identifier, params)?;
        let catalog = self
            .catalogs
            .get_mut(&name)
            .expect("selected catalog is registered");
        let (start, end) = bounds;
        if batch.is_empty() {
            if let (Some(start), Some(end)) = (start, end) {
                catalog.record_empty_coverage(data_type.clone(), identifier, start, end)?;
            }

            return Ok(());
        }

        catalog.write_data_batch(batch, start, end, Some(params.clone()))
    }

    fn write_response_instruments(&mut self, instruments: &[InstrumentAny], params: &Params) {
        let force = params
            .get_bool(PARAM_FORCE_INSTRUMENT_UPDATE)
            .unwrap_or(false);

        for instrument in instruments {
            let identifier = instrument.id().to_string();

            let result = (|| {
                let name = self.catalog_for_update(
                    &NautilusDataType::Instrument,
                    Some(&identifier),
                    params,
                )?;
                let catalog = self
                    .catalogs
                    .get_mut(&name)
                    .expect("selected catalog is registered");

                if !force {
                    let query = CatalogInstrumentQuery::new()
                        .with_instrument_ids(Some(vec![identifier.clone()]));

                    if !catalog.instruments(&query)?.is_empty() {
                        return Ok(());
                    }
                }

                catalog.write_instruments(std::slice::from_ref(instrument))
            })();

            if let Err(e) = result {
                log::error!("Instrument catalog write-back failed for {identifier}: {e}");
            }
        }
    }

    fn catalog_for_update(
        &mut self,
        data_type: &NautilusDataType,
        identifier: Option<&str>,
        params: &Params,
    ) -> anyhow::Result<Ustr> {
        if let Some(name) = params.get_str("catalog_name").map(Ustr::from) {
            anyhow::ensure!(
                self.catalogs.contains_key(&name),
                "Catalog '{name}' is not registered"
            );
            return Ok(name);
        }

        let mut names: Vec<_> = self.catalogs.keys().copied().collect();
        names.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for name in &names {
            let catalog = self.catalogs.get_mut(name).expect("catalog key exists");
            if catalog
                .query_last_timestamp(data_type.clone(), identifier)?
                .is_some()
            {
                return Ok(*name);
            }
        }

        names
            .first()
            .copied()
            .ok_or_else(|| anyhow::anyhow!("No catalog registered"))
    }

    pub(super) fn catalogs_registered(&self) -> bool {
        !self.catalogs.is_empty()
    }

    // Bounds the request window, walks the catalogs to find one whose missing
    // intervals differ from the full requested range, then fans the parent out via
    // the pipeline with one catalog leg plus one client leg per missing interval.
    // With no catalog match and no resolvable client, the engine emits an empty
    // response keyed by the parent request ID.
    pub(super) fn dispatch_date_range_request(
        &mut self,
        req: RequestCommand,
    ) -> anyhow::Result<()> {
        if request_has_schema(&req) {
            return self.dispatch_request_to_client(req).map(|_| ());
        }

        if matches!(
            req,
            RequestCommand::Instrument(_) | RequestCommand::Instruments(_)
        ) {
            return self.dispatch_instrument_catalog_request(req);
        }

        if matches!(&req, RequestCommand::Quotes(r) if r.instrument_ids.len() > 1)
            || matches!(&req, RequestCommand::Trades(r) if r.instrument_ids.len() > 1)
        {
            return self.dispatch_batch_catalog_request(req);
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
        let mut missing_intervals = query_interval.clone();
        let coverage = self.catalog_coverage(&key, catalog_start_ns.as_u64(), end_ns.as_u64())?;

        let winning_catalog = if let Some(CatalogCoverage {
            catalog: name,
            missing: intervals,
        }) = coverage
        {
            // Client legs fill only the requested window, not the pre-start range
            missing_intervals = if catalog_start_ns == start_ns {
                intervals
            } else {
                catalog_missing_intervals(
                    self.catalogs
                        .get_mut(&name)
                        .expect("selected catalog is registered"),
                    start_ns.as_u64(),
                    end_ns.as_u64(),
                    &key,
                )?
            };

            Some(name)
        } else {
            None
        };

        let skip_catalog_data = request_params(&req)
            .and_then(|p| p.get_bool(PARAM_SKIP_CATALOG_DATA))
            .unwrap_or(false);

        // When `skip_catalog_data` is set the client must serve the full parent window;
        // dropping the catalog leg without resetting the missing intervals would leave
        // the catalog-covered range unanswered.
        if skip_catalog_data {
            missing_intervals = query_interval;
        }

        let client_id = req.client_id().copied();
        let venue = req.venue().copied();
        let has_client = self
            .get_client(client_id.as_ref(), venue.as_ref())
            .is_some();

        if !has_client {
            missing_intervals.clear();
        }

        let mut plan = Vec::new();

        if !skip_catalog_data && let Some(catalog) = winning_catalog {
            let request =
                with_dates_for_pipeline(&req, Some(catalog_start_dt), Some(end_dt), now_ns);
            plan.push(RequestLeg::Catalog { request, catalog });
        }

        for (start, end) in missing_intervals {
            let start = UnixNanos::from(start).to_datetime_utc();
            let end = UnixNanos::from(end).to_datetime_utc();
            let request = with_dates_for_pipeline(&req, Some(start), Some(end), now_ns);
            plan.push(RequestLeg::Client(request));
        }

        self.dispatch_request_plan(req, plan)
    }

    fn dispatch_batch_catalog_request(&mut self, parent: RequestCommand) -> anyhow::Result<()> {
        let ids = match &parent {
            RequestCommand::Quotes(r) => r.instrument_ids.clone(),
            RequestCommand::Trades(r) => r.instrument_ids.clone(),
            _ => unreachable!("only quote and trade lists use catalog batch planning"),
        };

        let now_ns = self.clock.borrow().timestamp_ns();
        let query_past = request_params(&parent)
            .and_then(|p| p.get(PARAM_SUBSCRIPTION_NAME))
            .is_none();
        let (start, end) = bound_request_dates(
            request_start(&parent),
            request_end(&parent),
            now_ns.to_datetime_utc(),
            query_past,
        );
        let start_ns = datetime_to_unix_nanos_or_zero(start);
        let end_ns = datetime_to_unix_nanos_or_zero(end);
        anyhow::ensure!(start_ns <= end_ns, "Request start exceeds end");
        let skip_catalog = request_params(&parent)
            .and_then(|p| p.get_bool(PARAM_SKIP_CATALOG_DATA))
            .unwrap_or(false);
        let full = vec![(start_ns.as_u64(), end_ns.as_u64())];
        let mut clients: IndexMap<(u64, u64), Vec<InstrumentId>> = IndexMap::new();
        let mut plan = Vec::new();

        for id in ids {
            let mut member = with_request_instrument_ids(&parent, vec![id]);
            let key = request_identifier(&member).expect("single quote or trade identifier");
            let mut missing = full.clone();
            if !skip_catalog
                && let Some(CatalogCoverage {
                    catalog,
                    missing: intervals,
                }) = self.catalog_coverage(&key, start_ns.as_u64(), end_ns.as_u64())?
            {
                missing = intervals;
                member = with_dates_for_pipeline(&member, Some(start), Some(end), now_ns);
                plan.push(RequestLeg::Catalog {
                    request: member,
                    catalog,
                });
            }

            for interval in missing {
                clients.entry(interval).or_default().push(id);
            }
        }

        for ((start, end), ids) in clients {
            let member = with_request_instrument_ids(&parent, ids);
            let client_id = member.client_id().copied();
            let venue = member.venue().copied();

            if self
                .get_client(client_id.as_ref(), venue.as_ref())
                .is_none()
            {
                continue;
            }

            let start = UnixNanos::from(start).to_datetime_utc();
            let end = UnixNanos::from(end).to_datetime_utc();
            let request = with_dates_for_pipeline(&member, Some(start), Some(end), now_ns);
            plan.push(RequestLeg::Client(request));
        }

        self.dispatch_request_plan(parent, plan)
    }

    fn catalog_coverage(
        &mut self,
        key: &RequestCatalogKey,
        start: u64,
        end: u64,
    ) -> anyhow::Result<Option<CatalogCoverage>> {
        for (name, catalog) in &mut self.catalogs {
            let missing = catalog_missing_intervals(catalog, start, end, key)?;
            if missing != [(start, end)] {
                return Ok(Some(CatalogCoverage {
                    catalog: *name,
                    missing,
                }));
            }
        }

        Ok(None)
    }

    fn dispatch_request_plan(
        &mut self,
        parent: RequestCommand,
        plan: Vec<RequestLeg>,
    ) -> anyhow::Result<()> {
        let parent_id = *parent.request_id();

        if plan.is_empty() {
            let now = self.clock.borrow().timestamp_ns();
            let (start, end) = bound_request_dates(
                request_start(&parent),
                request_end(&parent),
                now.to_datetime_utc(),
                true,
            );
            let response = build_empty_response(
                &parent,
                datetime_to_unix_nanos_or_zero(start),
                datetime_to_unix_nanos_or_zero(end),
                None,
                now,
            )?;
            self.response(response);
            return Ok(());
        }

        self.new_request_pipeline(parent, plan.len());
        for leg in &plan {
            self.register_request_pipeline_leg(leg.request_id(), parent_id);
        }

        for leg in plan {
            let request_id = leg.request_id();

            let result = match leg {
                RequestLeg::Catalog { request, catalog } => {
                    let start = request_start(&request)
                        .map(datetime_to_unix_nanos_or_zero)
                        .expect("catalog leg has bounded start");
                    let end = request_end(&request)
                        .map(datetime_to_unix_nanos_or_zero)
                        .expect("catalog leg has bounded end");
                    let now = self.clock.borrow().timestamp_ns();
                    let client_id = request.client_id().copied();
                    let venue = request.venue().copied();
                    let client = self
                        .get_client(client_id.as_ref(), venue.as_ref())
                        .map(|value| value.client_id());
                    self.query_catalog_leg(&request, catalog, start, end, client, now)
                        .map(|response| self.response(response))
                }
                RequestLeg::Client(request) => self.dispatch_request_to_client(request).map(|_| ()),
                RequestLeg::Response(response) => {
                    self.response(response);
                    Ok(())
                }
            };

            if let Err(e) = result {
                self.request_failed(DataRequestFailed {
                    request_id,
                    reason: e.to_string(),
                    response: None,
                });
            }
        }

        Ok(())
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
                let data = quote_ticks_from_query_result(
                    catalog.query_batch(
                        &CatalogQuery::new(NautilusDataType::QuoteTick)
                            .with_identifiers(Some(vec![cmd.single_instrument_id()?.to_string()]))
                            .with_range(Some(start_ns), Some(end_ns)),
                    )?,
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
                let data = trade_ticks_from_query_result(
                    catalog.query_batch(
                        &CatalogQuery::new(NautilusDataType::TradeTick)
                            .with_identifiers(Some(vec![cmd.single_instrument_id()?.to_string()]))
                            .with_range(Some(start_ns), Some(end_ns)),
                    )?,
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
                let data = funding_rates_from_query_result(
                    catalog.query_batch(
                        &CatalogQuery::new(NautilusDataType::FundingRateUpdate)
                            .with_identifiers(Some(vec![cmd.single_instrument_id()?.to_string()]))
                            .with_range(Some(start_ns), Some(end_ns)),
                    )?,
                )?;
                build_funding_rates_catalog_response(
                    cmd,
                    data,
                    start_ns,
                    end_ns,
                    used_client_id,
                    ts_init,
                )
            }
            RequestCommand::Bars(cmd) => {
                let data = bars_from_query_result(
                    catalog.query_batch(
                        &CatalogQuery::new(NautilusDataType::Bar)
                            .with_identifiers(Some(vec![cmd.single_bar_type()?.to_string()]))
                            .with_range(Some(start_ns), Some(end_ns)),
                    )?,
                )?;
                build_bars_catalog_response(cmd, data, start_ns, end_ns, used_client_id, ts_init)
            }
            RequestCommand::Data(cmd) => {
                let identifiers = cmd
                    .data_type
                    .identifier()
                    .map(|identifier| vec![identifier.to_string()]);
                let where_clause = request_filter_expr(cmd.params.as_ref());
                let data = catalog.query_batch(
                    &CatalogQuery::new(NautilusDataType::Custom {
                        type_name: cmd.data_type.type_name().to_string(),
                    })
                    .with_identifiers(identifiers)
                    .with_range(Some(start_ns), Some(end_ns))
                    .with_where_clause(where_clause),
                )?;
                Ok(build_custom_data_catalog_response(
                    cmd,
                    custom_data_from_query_result(data)?,
                    start_ns,
                    end_ns,
                    ts_init,
                ))
            }
            RequestCommand::BookDeltas(cmd) => {
                let data = order_book_deltas_from_query_result(
                    catalog.query_batch(
                        &CatalogQuery::new(NautilusDataType::OrderBookDelta)
                            .with_identifiers(Some(vec![cmd.single_instrument_id()?.to_string()]))
                            .with_range(Some(start_ns), Some(end_ns)),
                    )?,
                )?;
                build_book_deltas_catalog_response(
                    cmd,
                    data,
                    start_ns,
                    end_ns,
                    used_client_id,
                    ts_init,
                )
            }
            RequestCommand::BookDepth(cmd) => {
                let data = order_book_depths_from_query_result(
                    catalog.query_batch(
                        &CatalogQuery::new(NautilusDataType::OrderBookDepth)
                            .with_identifiers(Some(vec![cmd.single_instrument_id()?.to_string()]))
                            .with_range(Some(start_ns), Some(end_ns)),
                    )?,
                )?;
                build_book_depth_catalog_response(
                    cmd,
                    data,
                    start_ns,
                    end_ns,
                    used_client_id,
                    ts_init,
                )
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
        if cmd.instrument_ids.len() > 1 {
            return self.dispatch_instrument_list(cmd);
        }

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

        let identifier = cmd.single_instrument_id()?.to_string();
        let now_ns = self.clock.borrow().timestamp_ns();
        let used_client_id = self
            .get_client(
                cmd.client_id.as_ref(),
                Some(&cmd.single_instrument_id()?.venue),
            )
            .map(|client| client.client_id());
        let (start_dt, end_dt) =
            bound_request_dates(cmd.start, cmd.end, now_ns.to_datetime_utc(), true);
        let start_ns = datetime_to_unix_nanos_or_zero(start_dt);
        let end_ns = datetime_to_unix_nanos_or_zero(end_dt);
        let start = Some(start_ns);
        let end = cmd.end.map(datetime_to_unix_nanos_or_zero);

        for catalog in self.catalogs.values_mut() {
            let data = latest_instruments(
                catalog.instruments(
                    &CatalogInstrumentQuery::new()
                        .with_instrument_ids(Some(vec![identifier.clone()]))
                        .with_range(start, end),
                )?,
            );

            if let Some(instrument) = data.into_iter().next() {
                let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                    cmd.request_id,
                    resolve_response_client_id(cmd.client_id, used_client_id),
                    cmd.single_instrument_id()?,
                    instrument,
                    Some(start_ns),
                    Some(end_ns),
                    now_ns,
                    Some(catalog_response_params(cmd.params.as_ref())),
                )));
                self.response(response);
                return Ok(());
            }
        }

        self.dispatch_request_to_client(RequestCommand::Instrument(cmd))
            .map(|_| ())
    }

    fn dispatch_instrument_list(&mut self, command: RequestInstrument) -> anyhow::Result<()> {
        let parent = RequestCommand::Instrument(command.clone());
        let force = command
            .params
            .as_ref()
            .and_then(|p| p.get_bool(PARAM_FORCE_INSTRUMENT_UPDATE))
            .unwrap_or(false);

        if force {
            return self.dispatch_request_to_client(parent).map(|_| ());
        }

        let now = self.clock.borrow().timestamp_ns();
        let (start, end) =
            bound_request_dates(command.start, command.end, now.to_datetime_utc(), true);
        let start_ns = datetime_to_unix_nanos_or_zero(start);
        let end_ns = datetime_to_unix_nanos_or_zero(end);
        let mut missing = Vec::new();
        let mut plan = Vec::new();

        for id in &command.instrument_ids {
            let mut instrument = None;

            for catalog in self.catalogs.values_mut() {
                let query = CatalogInstrumentQuery::new()
                    .with_instrument_ids(Some(vec![id.to_string()]))
                    .with_range(
                        Some(start_ns),
                        command.end.map(datetime_to_unix_nanos_or_zero),
                    );
                instrument = latest_instruments(catalog.instruments(&query)?)
                    .into_iter()
                    .find(|value| value.id() == *id);

                if instrument.is_some() {
                    break;
                }
            }

            if let Some(instrument) = instrument {
                let response = InstrumentResponse::new(
                    UUID4::new(),
                    command
                        .client_id
                        .unwrap_or_else(|| ClientId::new(CATALOG_CLIENT_ID)),
                    *id,
                    instrument,
                    Some(start_ns),
                    Some(end_ns),
                    now,
                    Some(catalog_response_params(command.params.as_ref())),
                );
                plan.push(RequestLeg::Response(DataResponse::Instrument(Box::new(
                    response,
                ))));
            } else {
                missing.push(*id);
            }
        }

        if !missing.is_empty() {
            let mut child = command;
            child.request_id = UUID4::new();
            child.instrument_ids = missing;
            plan.push(RequestLeg::Client(RequestCommand::Instrument(child)));
        }

        self.dispatch_request_plan(parent, plan)
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
        let (start_dt, _) = bound_request_dates(cmd.start, cmd.end, now_ns.to_datetime_utc(), true);
        let start = Some(datetime_to_unix_nanos_or_zero(start_dt));
        let end = cmd.end.map(datetime_to_unix_nanos_or_zero);
        let mut data = Vec::new();

        for catalog in self.catalogs.values_mut() {
            data.extend(
                catalog.instruments(
                    &CatalogInstrumentQuery::new()
                        .with_range(start, end)
                        .with_where_clause(request_filter_expr(cmd.params.as_ref())),
                )?,
            );
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
            // Report the queried window: results sorted by ID cannot be trimmed by `ts_init`
            start,
            end,
            now_ns,
            Some(catalog_response_params(cmd.params.as_ref())),
        ));
        self.response(response);
        Ok(())
    }
}

struct RequestCatalogKey {
    data_type: NautilusDataType,
    identifier: Option<String>,
}

fn response_write_bounds(
    request: &RequestCommand,
    response: &DataResponse,
) -> (Option<UnixNanos>, Option<UnixNanos>) {
    (
        request_start(request)
            .map(datetime_to_unix_nanos_or_zero)
            .or(response.start()),
        request_end(request)
            .map(datetime_to_unix_nanos_or_zero)
            .or(response.end()),
    )
}

fn response_batches(response: &DataResponse) -> anyhow::Result<Vec<(Option<String>, DataBatch)>> {
    let batches = match response {
        DataResponse::Quotes(r) => {
            let ids = response_instrument_ids(
                &r.instrument_ids,
                r.data.iter().map(|row| row.instrument_id),
            );
            let single = ids.len() == 1;
            ids.into_iter()
                .map(|id| {
                    let data = if single {
                        r.data.clone()
                    } else {
                        r.data
                            .iter()
                            .filter(|row| row.instrument_id == id)
                            .copied()
                            .collect::<Vec<_>>()
                            .into()
                    };

                    (Some(id.to_string()), DataBatch::Quote(data))
                })
                .collect()
        }
        DataResponse::Trades(r) => {
            let ids = response_instrument_ids(
                &r.instrument_ids,
                r.data.iter().map(|row| row.instrument_id),
            );
            let single = ids.len() == 1;
            ids.into_iter()
                .map(|id| {
                    let data = if single {
                        r.data.clone()
                    } else {
                        r.data
                            .iter()
                            .filter(|row| row.instrument_id == id)
                            .copied()
                            .collect::<Vec<_>>()
                            .into()
                    };

                    (Some(id.to_string()), DataBatch::Trade(data))
                })
                .collect()
        }
        DataResponse::BookDeltas(r) => vec![(
            Some(r.instrument_id.to_string()),
            DataBatch::BookDelta(r.data.clone()),
        )],
        DataResponse::BookDepth(r) => vec![(
            Some(r.instrument_id.to_string()),
            DataBatch::BookDepth(r.data.clone()),
        )],
        DataResponse::FundingRates(r) => vec![(
            Some(r.instrument_id.to_string()),
            DataBatch::FundingRate(r.data.clone()),
        )],
        DataResponse::Bars(r) => {
            vec![(Some(r.bar_type.to_string()), DataBatch::Bar(r.data.clone()))]
        }
        DataResponse::Data(r) => {
            let values = if let Some(data) = r.data.downcast_ref::<Vec<CustomData>>() {
                data.clone()
            } else if let Some(data) = r.data.downcast_ref::<CustomData>() {
                vec![data.clone()]
            } else {
                anyhow::bail!("Custom response payload is not catalog data");
            };

            vec![(
                r.data_type.identifier().map(String::from),
                DataBatch::Custom(values.into()),
            )]
        }
        _ => Vec::new(),
    };

    Ok(batches)
}

struct CatalogCoverage {
    catalog: Ustr,
    missing: Vec<(u64, u64)>,
}

enum RequestLeg {
    Catalog {
        request: RequestCommand,
        catalog: Ustr,
    },
    Client(RequestCommand),
    Response(DataResponse),
}

impl RequestLeg {
    fn request_id(&self) -> UUID4 {
        match self {
            Self::Catalog { request, .. } | Self::Client(request) => *request.request_id(),
            Self::Response(response) => *response.correlation_id(),
        }
    }
}

fn with_request_instrument_ids(parent: &RequestCommand, ids: Vec<InstrumentId>) -> RequestCommand {
    let mut request = parent.clone();
    match &mut request {
        RequestCommand::Quotes(r) => r.instrument_ids = ids,
        RequestCommand::Trades(r) => r.instrument_ids = ids,
        _ => unreachable!("catalog batch planning only supports quotes and trades"),
    }

    request
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
            data_type: NautilusDataType::Custom {
                type_name: cmd.data_type.type_name().to_string(),
            },
            identifier: cmd.data_type.identifier().map(String::from),
        }),
        RequestCommand::Quotes(cmd) => Some(RequestCatalogKey::new(
            NautilusDataType::QuoteTick,
            Some(cmd.single_instrument_id().ok()?.to_string()),
        )),
        RequestCommand::Trades(cmd) => Some(RequestCatalogKey::new(
            NautilusDataType::TradeTick,
            Some(cmd.single_instrument_id().ok()?.to_string()),
        )),
        RequestCommand::FundingRates(cmd) => Some(RequestCatalogKey::new(
            NautilusDataType::FundingRateUpdate,
            Some(cmd.single_instrument_id().ok()?.to_string()),
        )),
        RequestCommand::Bars(cmd) => Some(RequestCatalogKey::new(
            NautilusDataType::Bar,
            Some(cmd.single_bar_type().ok()?.to_string()),
        )),
        RequestCommand::BookDeltas(cmd) => Some(RequestCatalogKey::new(
            NautilusDataType::OrderBookDelta,
            Some(cmd.single_instrument_id().ok()?.to_string()),
        )),
        RequestCommand::BookDepth(cmd) => Some(RequestCatalogKey::new(
            NautilusDataType::OrderBookDepth,
            Some(cmd.single_instrument_id().ok()?.to_string()),
        )),
        _ => None,
    }
}

impl RequestCatalogKey {
    fn new(data_type: NautilusDataType, identifier: Option<String>) -> Self {
        Self {
            data_type,
            identifier,
        }
    }

    fn catalog_data_type(&self) -> NautilusDataType {
        self.data_type.clone()
    }
}

fn catalog_missing_intervals(
    catalog: &mut DataCatalog,
    start: u64,
    end: u64,
    key: &RequestCatalogKey,
) -> anyhow::Result<Vec<(u64, u64)>> {
    catalog.get_missing_intervals_for_request(
        UnixNanos::from(start),
        UnixNanos::from(end),
        key.catalog_data_type(),
        key.identifier.as_deref(),
    )
}

fn quote_ticks_from_query_result(data: DataBatch) -> anyhow::Result<BatchView<QuoteTick>> {
    match data {
        DataBatch::Quote(data) => Ok(data),
        _ => anyhow::bail!("expected a Quote batch, found another data batch variant"),
    }
}

fn trade_ticks_from_query_result(data: DataBatch) -> anyhow::Result<BatchView<TradeTick>> {
    match data {
        DataBatch::Trade(data) => Ok(data),
        _ => anyhow::bail!("expected a Trade batch, found another data batch variant"),
    }
}

fn funding_rates_from_query_result(
    data: DataBatch,
) -> anyhow::Result<BatchView<FundingRateUpdate>> {
    match data {
        DataBatch::FundingRate(data) => Ok(data),
        _ => anyhow::bail!("expected a FundingRate batch, found another data batch variant"),
    }
}

fn bars_from_query_result(data: DataBatch) -> anyhow::Result<BatchView<Bar>> {
    match data {
        DataBatch::Bar(data) => Ok(data),
        _ => anyhow::bail!("expected a Bar batch, found another data batch variant"),
    }
}

fn order_book_deltas_from_query_result(
    data: DataBatch,
) -> anyhow::Result<BatchView<OrderBookDelta>> {
    match data {
        DataBatch::BookDelta(data) => Ok(data),
        _ => anyhow::bail!("expected a BookDelta batch, found another data batch variant"),
    }
}

fn order_book_depths_from_query_result(
    data: DataBatch,
) -> anyhow::Result<BatchView<OrderBookDepth>> {
    match data {
        DataBatch::BookDepth(data) => Ok(data),
        _ => anyhow::bail!("expected a BookDepth batch, found another data batch variant"),
    }
}

fn request_limit(request: &RequestCommand) -> Option<std::num::NonZeroUsize> {
    match request {
        RequestCommand::Quotes(r) => r.limit,
        RequestCommand::Trades(r) => r.limit,
        RequestCommand::Bars(r) => r.limit,
        RequestCommand::FundingRates(r) => r.limit,
        RequestCommand::BookDeltas(r) => r.limit,
        RequestCommand::BookDepth(r) => r.limit,
        RequestCommand::Data(r) => r.limit,
        _ => None,
    }
}

fn request_has_schema(request: &RequestCommand) -> bool {
    matches!(
        request,
        RequestCommand::Quotes(_) | RequestCommand::Trades(_)
    ) && request_params(request).is_some_and(|params| params.contains_key("schema"))
}

fn request_contains_batch(
    request: &RequestCommand,
    identifier: Option<&str>,
    batch: &DataBatch,
) -> bool {
    let contains = |ids: &[InstrumentId]| {
        identifier
            .and_then(|value| value.parse::<InstrumentId>().ok())
            .is_some_and(|id| ids.contains(&id))
    };

    match (request, batch) {
        (RequestCommand::Quotes(r), DataBatch::Quote(_)) => contains(&r.instrument_ids),
        (RequestCommand::Trades(r), DataBatch::Trade(_)) => contains(&r.instrument_ids),
        (RequestCommand::BookDeltas(r), DataBatch::BookDelta(rows)) => {
            contains(&r.instrument_ids)
                && rows
                    .iter()
                    .all(|row| r.instrument_ids.contains(&row.instrument_id))
        }
        (RequestCommand::BookDepth(r), DataBatch::BookDepth(rows)) => {
            contains(&r.instrument_ids)
                && rows
                    .iter()
                    .all(|row| r.instrument_ids.contains(&row.instrument_id))
        }
        (RequestCommand::FundingRates(r), DataBatch::FundingRate(rows)) => {
            contains(&r.instrument_ids)
                && rows
                    .iter()
                    .all(|row| r.instrument_ids.contains(&row.instrument_id))
        }
        (RequestCommand::Bars(r), DataBatch::Bar(rows)) => {
            identifier
                .and_then(|value| value.parse::<BarType>().ok())
                .is_some_and(|bar_type| r.bar_types.contains(&bar_type))
                && rows.iter().all(|row| r.bar_types.contains(&row.bar_type))
        }
        (RequestCommand::Data(_), DataBatch::Custom(_)) => {
            request_identifier(request).is_some_and(|key| key.identifier.as_deref() == identifier)
        }
        _ => false,
    }
}

fn response_instrument_ids(
    requested: &[InstrumentId],
    actual: impl Iterator<Item = InstrumentId>,
) -> Vec<InstrumentId> {
    let actual = actual.collect::<AHashSet<_>>();
    if actual.iter().all(|id| requested.contains(id)) {
        return requested.to_vec();
    }

    let mut ids = actual.into_iter().collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

fn request_start(req: &RequestCommand) -> Option<Timestamp> {
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

fn request_end(req: &RequestCommand) -> Option<Timestamp> {
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

fn request_filter_expr(params: Option<&Params>) -> Option<String> {
    params
        .and_then(|params| params.get_str("filter_expr"))
        .filter(|filter_expr| !filter_expr.is_empty())
        .map(ToString::to_string)
}

fn bound_request_dates(
    start: Option<Timestamp>,
    end: Option<Timestamp>,
    now: Timestamp,
    query_past_data: bool,
) -> (Timestamp, Timestamp) {
    let zero = Timestamp::UNIX_EPOCH;
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

fn datetime_to_unix_nanos_or_zero(dt: Timestamp) -> UnixNanos {
    UnixNanos::from(u64::try_from(dt.as_nanosecond().max(0)).unwrap_or(0))
}

fn floor_to_utc_day(dt: Timestamp) -> Timestamp {
    let midnight = jiff::tz::Offset::UTC.to_datetime(dt).date().at(0, 0, 0, 0);
    jiff::tz::Offset::UTC
        .to_timestamp(midnight)
        .expect("midnight UTC is always valid")
}

fn with_dates_for_pipeline(
    req: &RequestCommand,
    start: Option<Timestamp>,
    end: Option<Timestamp>,
    ts_init: UnixNanos,
) -> RequestCommand {
    let new_id = UUID4::new();

    match req {
        RequestCommand::Quotes(cmd) => RequestCommand::Quotes(RequestQuotes {
            instrument_ids: cmd.instrument_ids.clone(),
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id: new_id,
            ts_init,
            params: cmd.params.clone(),
        }),
        RequestCommand::Trades(cmd) => RequestCommand::Trades(RequestTrades {
            instrument_ids: cmd.instrument_ids.clone(),
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id: new_id,
            ts_init,
            params: cmd.params.clone(),
        }),
        RequestCommand::FundingRates(cmd) => RequestCommand::FundingRates(RequestFundingRates {
            instrument_ids: cmd.instrument_ids.clone(),
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id: new_id,
            ts_init,
            params: cmd.params.clone(),
        }),
        RequestCommand::BookDeltas(cmd) => RequestCommand::BookDeltas(RequestBookDeltas {
            instrument_ids: cmd.instrument_ids.clone(),
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id: new_id,
            ts_init,
            params: cmd.params.clone(),
        }),
        RequestCommand::BookDepth(cmd) => RequestCommand::BookDepth(RequestBookDepth {
            instrument_ids: cmd.instrument_ids.clone(),
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
            bar_types: cmd.bar_types.clone(),
            start,
            end,
            limit: cmd.limit,
            client_id: cmd.client_id,
            request_id: new_id,
            ts_init,
            params: cmd.params.clone(),
        }),
        // `Join` and the non-date-range variants should never reach this path; the dispatcher
        // gates on `is_date_range_variant` first. Cloning preserves behavior if a caller
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
            cmd.instrument_ids.clone(),
            Vec::new(),
            Some(start),
            Some(end),
            ts_init,
            cmd.params.clone(),
        )),
        RequestCommand::Trades(cmd) => DataResponse::Trades(TradesResponse::new(
            cmd.request_id,
            resolve_response_client_id(cmd.client_id, used_client_id),
            cmd.instrument_ids.clone(),
            Vec::new(),
            Some(start),
            Some(end),
            ts_init,
            cmd.params.clone(),
        )),
        RequestCommand::FundingRates(cmd) => DataResponse::FundingRates(FundingRatesResponse::new(
            cmd.request_id,
            resolve_response_client_id(cmd.client_id, used_client_id),
            cmd.single_instrument_id()?,
            Vec::new(),
            Some(start),
            Some(end),
            ts_init,
            cmd.params.clone(),
        )),
        RequestCommand::Bars(cmd) => DataResponse::Bars(BarsResponse::new(
            cmd.request_id,
            resolve_response_client_id(cmd.client_id, used_client_id),
            cmd.single_bar_type()?,
            Vec::new(),
            Some(start),
            Some(end),
            ts_init,
            cmd.params.clone(),
        )),
        RequestCommand::BookDeltas(cmd) => DataResponse::BookDeltas(BookDeltasResponse::new(
            cmd.request_id,
            resolve_response_client_id(cmd.client_id, used_client_id),
            cmd.single_instrument_id()?,
            Vec::new(),
            Some(start),
            Some(end),
            ts_init,
            cmd.params.clone(),
        )),
        RequestCommand::BookDepth(cmd) => DataResponse::BookDepth(BookDepthResponse::new(
            cmd.request_id,
            resolve_response_client_id(cmd.client_id, used_client_id),
            cmd.single_instrument_id()?,
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
    data: BatchView<QuoteTick>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> DataResponse {
    let params = catalog_response_params(cmd.params.as_ref());
    DataResponse::Quotes(QuotesResponse::new(
        cmd.request_id,
        resolve_response_client_id(cmd.client_id, used_client_id),
        cmd.instrument_ids.clone(),
        data,
        Some(start),
        Some(end),
        ts_init,
        Some(params),
    ))
}

fn build_trades_catalog_response(
    cmd: &RequestTrades,
    data: BatchView<TradeTick>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> DataResponse {
    let params = catalog_response_params(cmd.params.as_ref());
    DataResponse::Trades(TradesResponse::new(
        cmd.request_id,
        resolve_response_client_id(cmd.client_id, used_client_id),
        cmd.instrument_ids.clone(),
        data,
        Some(start),
        Some(end),
        ts_init,
        Some(params),
    ))
}

fn build_funding_rates_catalog_response(
    cmd: &RequestFundingRates,
    data: BatchView<FundingRateUpdate>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> anyhow::Result<DataResponse> {
    let params = catalog_response_params(cmd.params.as_ref());
    Ok(DataResponse::FundingRates(FundingRatesResponse::new(
        cmd.request_id,
        resolve_response_client_id(cmd.client_id, used_client_id),
        cmd.single_instrument_id()?,
        data,
        Some(start),
        Some(end),
        ts_init,
        Some(params),
    )))
}

fn build_bars_catalog_response(
    cmd: &RequestBars,
    data: BatchView<Bar>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> anyhow::Result<DataResponse> {
    let params = catalog_response_params(cmd.params.as_ref());
    Ok(DataResponse::Bars(BarsResponse::new(
        cmd.request_id,
        resolve_response_client_id(cmd.client_id, used_client_id),
        cmd.single_bar_type()?,
        data,
        Some(start),
        Some(end),
        ts_init,
        Some(params),
    )))
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
    data: BatchView<OrderBookDelta>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> anyhow::Result<DataResponse> {
    let params = catalog_response_params(cmd.params.as_ref());
    Ok(DataResponse::BookDeltas(BookDeltasResponse::new(
        cmd.request_id,
        resolve_response_client_id(cmd.client_id, used_client_id),
        cmd.single_instrument_id()?,
        data,
        Some(start),
        Some(end),
        ts_init,
        Some(params),
    )))
}

fn build_book_depth_catalog_response(
    cmd: &RequestBookDepth,
    data: BatchView<OrderBookDepth>,
    start: UnixNanos,
    end: UnixNanos,
    used_client_id: Option<ClientId>,
    ts_init: UnixNanos,
) -> anyhow::Result<DataResponse> {
    let params = catalog_response_params(cmd.params.as_ref());
    Ok(DataResponse::BookDepth(BookDepthResponse::new(
        cmd.request_id,
        resolve_response_client_id(cmd.client_id, used_client_id),
        cmd.single_instrument_id()?,
        data,
        Some(start),
        Some(end),
        ts_init,
        Some(params),
    )))
}

fn catalog_response_params(existing: Option<&Params>) -> Params {
    let mut params = existing
        .map(|params| IndexMap::clone(params))
        .unwrap_or_default();
    params.insert(PARAM_UPDATE_CATALOG.to_string(), Value::Bool(false));
    Params::new(params)
}

fn custom_data_from_query_result(data: DataBatch) -> anyhow::Result<Vec<CustomData>> {
    CustomData::from_batch(data)
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
    use std::sync::Arc;

    use nautilus_common::messages::data::RequestJoin;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::full(0, 3)]
    #[case::subrange(1, 2)]
    fn test_catalog_quote_response_uses_selected_batch_view(
        #[case] start: usize,
        #[case] end: usize,
    ) {
        let first = QuoteTick {
            ts_init: UnixNanos::from(11),
            ..Default::default()
        };

        let mut second = first;
        second.ts_init = UnixNanos::from(23);
        let mut third = first;
        third.ts_init = UnixNanos::from(37);
        let original = Arc::new(vec![first, second, third]);
        let result = quote_ticks_from_query_result(DataBatch::Quote(BatchView::new(
            Arc::clone(&original),
            start..end,
        )))
        .unwrap();
        assert_eq!(result.as_ref(), &original[start..end]);
        assert_eq!(result.range(), start..end);
        assert!(Arc::ptr_eq(result.arc(), &original));
    }

    #[rstest]
    fn test_response_batches_preserves_narrowed_payloads(#[values(false, true)] empty: bool) {
        use nautilus_model::data::stubs::{stub_delta, stub_depth10};

        macro_rules! check {
            ($response:ident, $variant:ident, $batch:ident, $key:expr, $seed:expr) => {{
                let key = $key;
                let mut row = $seed;
                row.ts_init = UnixNanos::from(23);
                let mut before = row.clone();
                before.ts_init = UnixNanos::from(11);
                let mut after = row.clone();
                after.ts_init = UnixNanos::from(37);
                let backing = Arc::new(vec![before, row.clone(), after]);
                let end = if empty { 1 } else { 2 };
                let response = DataResponse::$variant($response::new(
                    UUID4::new(),
                    ClientId::from("SOURCE"),
                    key,
                    BatchView::new(Arc::clone(&backing), 1..end),
                    None,
                    None,
                    UnixNanos::from(41),
                    None,
                ));

                let batches = response_batches(&response).unwrap();

                assert_eq!(batches.len(), 1);
                assert_eq!(batches[0].0.as_deref(), Some(key.to_string().as_str()));
                let DataBatch::$batch(view) = &batches[0].1 else {
                    panic!("Unexpected batch {:?}", batches[0].1);
                };
                assert_eq!(view.range(), 1..end);
                assert!(Arc::ptr_eq(view.arc(), &backing));
                assert_eq!(
                    view.as_ref(),
                    if empty {
                        &[]
                    } else {
                        std::slice::from_ref(&row)
                    }
                );
            }};
        }

        let delta = stub_delta();
        let depth = stub_depth10();
        let quote = QuoteTick::default();
        let trade = TradeTick::default();
        let bar = Bar::default();

        let funding = FundingRateUpdate::new(
            quote.instrument_id,
            "0.001".parse().unwrap(),
            Some(60),
            Some(UnixNanos::from(47)),
            UnixNanos::from(17),
            UnixNanos::from(23),
        );

        check!(
            BookDeltasResponse,
            BookDeltas,
            BookDelta,
            delta.instrument_id,
            delta
        );
        check!(
            BookDepthResponse,
            BookDepth,
            BookDepth,
            depth.instrument_id,
            depth
        );
        check!(QuotesResponse, Quotes, Quote, quote.instrument_id, quote);
        check!(TradesResponse, Trades, Trade, trade.instrument_id, trade);
        check!(
            FundingRatesResponse,
            FundingRates,
            FundingRate,
            funding.instrument_id,
            funding
        );
        check!(BarsResponse, Bars, Bar, bar.bar_type, bar);
    }

    #[rstest]
    #[case::deltas(NautilusDataType::OrderBookDelta)]
    #[case::depth(NautilusDataType::OrderBookDepth)]
    #[case::quotes(NautilusDataType::QuoteTick)]
    #[case::trades(NautilusDataType::TradeTick)]
    #[case::bars(NautilusDataType::Bar)]
    #[case::funding(NautilusDataType::FundingRateUpdate)]
    fn test_request_contains_batch_uses_visible_requested_members(
        #[case] kind: NautilusDataType,
        #[values(false, true)] remapped: bool,
        #[values(false, true)] empty: bool,
    ) {
        use nautilus_model::data::stubs::{stub_delta, stub_depth10};

        let requested = InstrumentId::from("AUD/USD.SIM");

        let actual = if remapped {
            InstrumentId::from("GBP/USD.SIM")
        } else {
            requested
        };

        let requested_bar = format!("{requested}-1-MINUTE-LAST-EXTERNAL")
            .parse::<BarType>()
            .unwrap();
        let actual_bar = format!("{actual}-1-MINUTE-LAST-EXTERNAL")
            .parse::<BarType>()
            .unwrap();

        macro_rules! request {
            ($variant:ident, $request:ident, $id:expr $(, $depth:expr)?) => {
                RequestCommand::$variant($request::new(
                    $id, None, None, None, $($depth,)? None, UUID4::new(), UnixNanos::from(37), None,
                ))
            };
        }

        macro_rules! response {
            ($variant:ident, $response:ident, $key:expr, $row:expr) => {{
                let data = BatchView::new(Arc::new(vec![$row]), 0..usize::from(!empty));
                DataResponse::$variant($response::new(
                    UUID4::new(),
                    ClientId::from("SOURCE"),
                    $key,
                    data,
                    None,
                    None,
                    UnixNanos::from(41),
                    None,
                ))
            }};
        }

        let (request, response) = match kind {
            NautilusDataType::OrderBookDelta => {
                let row = OrderBookDelta {
                    instrument_id: actual,
                    ..stub_delta()
                };

                (
                    request!(BookDeltas, RequestBookDeltas, requested),
                    response!(BookDeltas, BookDeltasResponse, requested, row),
                )
            }
            NautilusDataType::OrderBookDepth => {
                let row = OrderBookDepth {
                    instrument_id: actual,
                    ..stub_depth10()
                };

                (
                    request!(BookDepth, RequestBookDepth, requested, None),
                    response!(BookDepth, BookDepthResponse, requested, row),
                )
            }
            NautilusDataType::QuoteTick => {
                let row = QuoteTick {
                    instrument_id: actual,
                    ..Default::default()
                };

                (
                    request!(Quotes, RequestQuotes, requested),
                    response!(Quotes, QuotesResponse, requested, row),
                )
            }
            NautilusDataType::TradeTick => {
                let row = TradeTick {
                    instrument_id: actual,
                    ..Default::default()
                };

                (
                    request!(Trades, RequestTrades, requested),
                    response!(Trades, TradesResponse, requested, row),
                )
            }
            NautilusDataType::Bar => {
                let row = Bar {
                    bar_type: actual_bar,
                    ..Default::default()
                };

                (
                    request!(Bars, RequestBars, requested_bar),
                    response!(Bars, BarsResponse, requested_bar, row),
                )
            }
            NautilusDataType::FundingRateUpdate => {
                let row = FundingRateUpdate::new(
                    actual,
                    "0.001".parse().unwrap(),
                    Some(60),
                    Some(UnixNanos::from(47)),
                    UnixNanos::from(17),
                    UnixNanos::from(23),
                );
                (
                    request!(FundingRates, RequestFundingRates, requested),
                    response!(FundingRates, FundingRatesResponse, requested, row),
                )
            }
            _ => panic!("unexpected data type"),
        };

        let batches = response_batches(&response).unwrap();
        let (identifier, batch) = &batches[0];

        assert_eq!(batches.len(), 1);
        assert_eq!(
            request_contains_batch(&request, identifier.as_deref(), batch),
            empty || !remapped,
        );
    }

    #[rstest]
    #[case::quotes(true)]
    #[case::trades(false)]
    fn test_response_batches_keep_expanded_parent_instruments(#[case] quotes: bool) {
        let requested = [
            InstrumentId::from("ES.FUT.GLBX"),
            InstrumentId::from("NQ.FUT.GLBX"),
        ];
        let first = InstrumentId::from("ESH24.GLBX");
        let second = InstrumentId::from("NQH24.GLBX");

        let mut response = if quotes {
            let rows = vec![
                QuoteTick {
                    instrument_id: first,
                    ..Default::default()
                },
                QuoteTick {
                    instrument_id: second,
                    ..Default::default()
                },
            ];
            DataResponse::Quotes(QuotesResponse::new(
                UUID4::new(),
                ClientId::from("SOURCE"),
                requested,
                rows,
                None,
                None,
                UnixNanos::from(3),
                None,
            ))
        } else {
            let rows = vec![
                TradeTick {
                    instrument_id: first,
                    ..Default::default()
                },
                TradeTick {
                    instrument_id: second,
                    ..Default::default()
                },
            ];
            DataResponse::Trades(TradesResponse::new(
                UUID4::new(),
                ClientId::from("SOURCE"),
                requested,
                rows,
                None,
                None,
                UnixNanos::from(3),
                None,
            ))
        };

        macro_rules! narrow {
            ($response:expr, $row:ident) => {{
                let mut rows = $response.data.to_vec();
                let hidden = $row {
                    instrument_id: InstrumentId::from("HIDDEN.GLBX"),
                    ..Default::default()
                };
                rows.insert(0, hidden);
                rows.push(hidden);
                $response.data = BatchView::new(Arc::new(rows), 1..3);
            }};
        }

        match &mut response {
            DataResponse::Quotes(r) => narrow!(r, QuoteTick),
            DataResponse::Trades(r) => narrow!(r, TradeTick),
            _ => unreachable!(),
        }

        let batches = response_batches(&response).unwrap();

        assert_eq!(batches.len(), 2);

        for ((identifier, batch), id) in batches.iter().zip([first, second]) {
            assert_eq!(identifier.as_deref(), Some(id.to_string().as_str()));

            match batch {
                DataBatch::Quote(rows) => assert_eq!(
                    &rows[..],
                    &[QuoteTick {
                        instrument_id: id,
                        ..Default::default()
                    }]
                ),
                DataBatch::Trade(rows) => assert_eq!(
                    &rows[..],
                    &[TradeTick {
                        instrument_id: id,
                        ..Default::default()
                    }]
                ),
                other => panic!("Unexpected batch {other:?}"),
            }
        }
    }

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
