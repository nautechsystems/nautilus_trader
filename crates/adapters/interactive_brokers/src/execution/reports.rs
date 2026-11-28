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

//! Report collection for the Interactive Brokers execution client.
//!
//! Collection owns everything it uses and never touches the live cache, so it can run on a
//! runtime worker as an `ExecutionReportTask` and inline through the same code.

use ibapi::{
    accounts::{PositionUpdateMulti, types::AccountId as IbAccountId},
    client::Client,
    orders::Order as IBOrder,
};
use nautilus_live::execution::reports::retain_order_status_reports;

use super::{
    core::*,
    parse::{
        FillEvidence, MissingCommission, api_order_id, ib_venue_order_id, link_order_contingencies,
        normalized_order_ref, order_ref_client_order_id, parse_order_data_to_report,
    },
};

/// Inputs for turning IB report data into reports, with no venue access.
#[derive(Clone)]
pub(super) struct IbReportContext {
    instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
    account_id: AccountId,
}

impl IbReportContext {
    pub(super) fn new(
        instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
        account_id: AccountId,
    ) -> Self {
        Self {
            instrument_provider,
            account_id,
        }
    }

    // Returns `Ok(None)` for an execution outside the command's filters and an error for one
    // that cannot be parsed
    fn build_historical_fill_report(
        &self,
        cmd: &GenerateFillReports,
        exec_data: &ExecutionData,
        instrument_id: InstrumentId,
        commission: f64,
        commission_currency: &str,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Option<FillReport>> {
        if let Some(filter_id) = cmd.instrument_id
            && instrument_id != filter_id
        {
            return Ok(None);
        }

        if let Some(filter_venue_order_id) = cmd.venue_order_id
            && ib_venue_order_id(exec_data.execution.order_id, exec_data.execution.perm_id)
                != filter_venue_order_id
        {
            return Ok(None);
        }

        if let Some(end) = cmd.end
            && parse_execution_time(&exec_data.execution.time)? > end
        {
            return Ok(None);
        }

        parse_execution_to_fill_report(
            &exec_data.execution,
            &exec_data.contract,
            commission,
            commission_currency,
            instrument_id,
            self.account_id,
            &self.instrument_provider,
            ts_init,
            None, // avg_px (not available in historical fills)
        )
        .map(Some)
    }

    fn resolve_historical_execution_instrument_id(
        &self,
        exec_data: &ExecutionData,
    ) -> anyhow::Result<InstrumentId> {
        self.resolve_report_contract_instrument_id(&exec_data.contract)
    }

    pub(super) fn resolve_report_contract_instrument_id(
        &self,
        contract: &Contract,
    ) -> anyhow::Result<InstrumentId> {
        self.instrument_provider
            .resolve_instrument_id_for_contract(contract)
            .context("Failed to resolve IBKR report contract to instrument ID")
    }

    /// Returns `None` for a completed order whose contract does not resolve, such as a BAG whose
    /// spread this node never loaded, so the order is left out of the reports instead of failing
    /// them all. An unresolved open order still fails them.
    pub(super) fn resolve_completed_order_instrument_id(
        &self,
        data: &ibapi::orders::OrderData,
    ) -> Option<InstrumentId> {
        match self.resolve_report_contract_instrument_id(&data.contract) {
            Ok(instrument_id) => Some(instrument_id),
            Err(e) => {
                tracing::warn!(
                    order_id = data.order.order_id,
                    perm_id = data.order.perm_id,
                    order_ref = data.order.order_ref.as_str(),
                    con_id = data.contract.contract_id,
                    sec_type = ?data.contract.security_type,
                    error = %format_args!("{e:#}"),
                    "Skipping completed IB order report for unresolved contract",
                );
                None
            }
        }
    }

    /// IB's average cost is a price in the contract's currency times the multiplier.
    pub(super) fn position_avg_px_open(
        instrument: &InstrumentAny,
        average_cost: f64,
    ) -> Option<Decimal> {
        if average_cost <= 0.0 {
            return None;
        }

        let multiplier = instrument.multiplier().as_f64();
        let converted_avg_cost = average_cost / multiplier;
        Decimal::from_f64_retain(converted_avg_cost)
            .map(|price| price.round_dp(instrument.price_precision() as u32))
    }

    pub(super) fn flat_position_report(
        &self,
        instrument_id: InstrumentId,
        ts_init: UnixNanos,
    ) -> PositionStatusReport {
        let precision = self
            .instrument_provider
            .find(&instrument_id)
            .map_or(0, |instrument| instrument.size_precision());

        PositionStatusReport::new(
            self.account_id,
            instrument_id,
            PositionSide::Flat,
            Quantity::zero(precision),
            ts_init,
            ts_init,
            None,
            None,
            None,
        )
    }

    pub(super) fn warn_historical_fill_report_parse_error(
        exec_data: &ExecutionData,
        error: &anyhow::Error,
    ) {
        tracing::warn!(
            symbol = exec_data.contract.symbol.as_str(),
            sec_type = ?exec_data.contract.security_type,
            exchange = exec_data.contract.exchange.as_str(),
            primary_exchange = exec_data.contract.primary_exchange.as_str(),
            local_symbol = exec_data.contract.local_symbol.as_str(),
            con_id = exec_data.contract.contract_id,
            order_id = exec_data.execution.order_id,
            order_ref = exec_data.execution.order_reference.as_str(),
            execution_id = exec_data.execution.execution_id.as_str(),
            error = %error,
            "Failed to parse IBKR historical fill report",
        );
    }
}

pub(super) struct OrderEvidence {
    pub(super) reports: Vec<OrderStatusReport>,
    pub(super) open_order_ids: AHashSet<VenueOrderId>,
    /// Completed orders in the window left out of `reports` because their contract did not resolve.
    pub(super) unresolved_orders: usize,
}

/// The orders IB reports for the account, before the command's status and time filters.
struct ReportedOrders {
    /// Reports for the orders whose contract resolves.
    reports: Vec<OrderStatusReport>,
    /// The IB order each report was parsed from, by index.
    orders: Vec<IBOrder>,
    open_order_ids: AHashSet<VenueOrderId>,
    /// Completed orders in the window whose contract does not resolve.
    unresolved: Vec<IBOrder>,
}

fn ib_order_with_api_order_id(data: ibapi::orders::OrderData) -> IBOrder {
    let order_id = api_order_id(&data);
    IBOrder {
        order_id,
        ..data.order
    }
}

/// The IDs a targeted order status query identifies its order by.
///
/// A permanent venue order ID decides against an order with a permanent ID, and a numeric one
/// against an order this API client placed, by its API order ID; another client's API order IDs
/// repeat, so they decide nothing. A venue order ID that identifies an order rules out every other
/// order. Otherwise the client order ID decides: it identifies an order whose ref equals it as
/// supplied or normalizes to it, and an order whose ref gives no client order ID by the venue
/// order ID it is adopted under.
struct OrderQuery {
    client_order_id: Option<ClientOrderId>,
    venue_order_id: Option<VenueOrderId>,
    ib_client_id: i32,
}

impl OrderQuery {
    /// Returns whether the order is the queried one, or `None` when no ID decides.
    fn identifies(&self, order: &IBOrder) -> Option<bool> {
        self.venue_order_id
            .and_then(|venue_order_id| self.venue_order_id_identifies(venue_order_id, order))
            .or_else(|| {
                self.client_order_id
                    .map(|client_order_id| Self::client_order_id_identifies(client_order_id, order))
            })
    }

    fn venue_order_id_identifies(
        &self,
        venue_order_id: VenueOrderId,
        order: &IBOrder,
    ) -> Option<bool> {
        let id = venue_order_id.as_str();

        if let Some(perm_id) = id
            .strip_prefix("PERM-")
            .and_then(|perm_id| perm_id.parse::<i64>().ok())
        {
            (order.perm_id != 0).then_some(order.perm_id == perm_id)
        } else {
            id.parse::<i32>().ok().and_then(|order_id| {
                (order.client_id == self.ib_client_id).then_some(order.order_id == order_id)
            })
        }
    }

    fn client_order_id_identifies(client_order_id: ClientOrderId, order: &IBOrder) -> bool {
        let id = client_order_id.as_str();
        let adopted = order_ref_client_order_id(&order.order_ref).is_none()
            && ib_venue_order_id(order.order_id, order.perm_id).as_str() == id;

        order.order_ref == id || normalized_order_ref(&order.order_ref) == Some(id) || adopted
    }

    /// Returns the report of the one order this query identifies, or `None` when the query rules
    /// out every order IB reports.
    ///
    /// # Errors
    ///
    /// Returns an error if an unresolved order is not ruled out, more than one order is not ruled
    /// out, or the one remaining order is not identified, so the caller does not read the queried
    /// order as absent at the venue.
    fn select_report(
        &self,
        reported: impl IntoIterator<Item = (OrderStatusReport, IBOrder)>,
        unresolved: &[IBOrder],
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let named = |order: &IBOrder| {
            self.venue_order_id
                .and_then(|venue_order_id| self.venue_order_id_identifies(venue_order_id, order))
                == Some(true)
        };
        let reported = reported.into_iter().collect::<Vec<_>>();
        let any_named =
            reported.iter().any(|(_, order)| named(order)) || unresolved.iter().any(named);
        let verdict = |order: &IBOrder| {
            if any_named {
                Some(named(order))
            } else {
                self.identifies(order)
            }
        };

        let mut reported = reported
            .into_iter()
            .filter_map(|(report, order)| {
                let identified = verdict(&order);
                (identified != Some(false)).then_some((report, order, identified))
            })
            .collect::<Vec<_>>();
        let unresolved = unresolved
            .iter()
            .filter(|order| verdict(order) != Some(false))
            .collect::<Vec<_>>();

        match (reported.as_mut_slice(), unresolved.as_slice()) {
            ([], []) => return Ok(None),
            ([(report, order, Some(true))], []) => {
                // The report normalizes the order ref, which loses a client order ID placed with
                // a colon, so it carries the queried ID when the ref is that ID.
                if let Some(client_order_id) = self
                    .client_order_id
                    .filter(|client_order_id| order.order_ref == client_order_id.as_str())
                {
                    report.client_order_id = Some(client_order_id);
                }
                return Ok(reported.pop().map(|(report, _, _)| report));
            }
            _ => {}
        }

        let perm_ids = reported
            .iter()
            .map(|(_, order, _)| order.perm_id)
            .chain(unresolved.iter().map(|order| order.perm_id))
            .collect::<Vec<_>>();
        let query = [
            self.client_order_id.map(|id| id.to_string()),
            self.venue_order_id.map(|id| id.to_string()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
        anyhow::bail!(
            "IB orders with permanent IDs {perm_ids:?} leave order {query} unidentified, {} of \
             them with a contract that does not resolve",
            unresolved.len()
        );
    }
}

/// Owned Interactive Brokers report collection.
pub(super) struct IbReportClient {
    client: Arc<Client>,
    context: IbReportContext,
    ib_account: Ustr,
    request_timeout: Duration,
}

impl IbReportClient {
    pub(super) fn new(
        client: Arc<Client>,
        context: IbReportContext,
        ib_account: Ustr,
        request_timeout: Duration,
    ) -> Self {
        Self {
            client,
            context,
            ib_account,
            request_timeout,
        }
    }

    /// Returns the report of the queried order among every order IB reports for the account,
    /// matched by its IDs whatever its instrument, or `None` when IB's orders rule it out.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or IB's orders leave the queried order unidentified,
    /// since the engine reads `None` as the order being absent at the venue.
    pub(super) async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let plural_cmd = GenerateOrderStatusReports {
            command_id: cmd.command_id,
            ts_init: cmd.ts_init,
            open_only: false,
            instrument_id: None,
            start: None,
            end: None,
            params: cmd.params.clone(),
            log_receipt_level: LogLevel::Info,
            correlation_id: cmd.correlation_id,
            causation_id: cmd.causation_id,
        };

        let ReportedOrders {
            reports,
            orders,
            unresolved,
            ..
        } = self.collect_reported_orders(&plural_cmd).await?;

        OrderQuery {
            client_order_id: cmd.client_order_id,
            venue_order_id: cmd.venue_order_id,
            ib_client_id: self.client.client_id(),
        }
        .select_report(reports.into_iter().zip(orders), &unresolved)
    }

    pub(super) async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        Ok(self.generate_order_evidence(cmd).await?.reports)
    }

    pub(super) async fn generate_order_evidence(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<OrderEvidence> {
        let ReportedOrders {
            mut reports,
            open_order_ids,
            unresolved,
            ..
        } = self.collect_reported_orders(cmd).await?;

        if !cmd.open_only {
            retain_order_status_reports(&mut reports, cmd);
        }

        Ok(OrderEvidence {
            reports,
            open_order_ids,
            unresolved_orders: unresolved.len(),
        })
    }

    async fn collect_reported_orders(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<ReportedOrders> {
        let client = self.client.as_ref();

        let timeout_dur = self.request_timeout;
        let subscription = tokio::time::timeout(timeout_dur, client.all_open_orders())
            .await
            .context("Timeout requesting open orders")??;
        let mut subscription = subscription.filter_data();
        let mut reports = Vec::new();
        let mut ib_orders = Vec::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let ib_account = self.ib_account;

        while let Some(order_result) = subscription.next().await {
            match order_result {
                Ok(Orders::OrderData(data)) => {
                    if !data.order.account.is_empty() && data.order.account != ib_account {
                        continue;
                    }

                    // An open order may be one this node holds, so an unresolved one fails the
                    // reports rather than reading as absent at the venue.
                    let instrument_id = self
                        .context
                        .resolve_report_contract_instrument_id(&data.contract)
                        .with_context(|| {
                            format!(
                                "Failed to resolve IB order {} contract ID {} ({:?})",
                                data.order_id,
                                data.contract.contract_id,
                                data.contract.security_type
                            )
                        })?;

                    // Filter by instrument_id if specified
                    if let Some(filter_id) = cmd.instrument_id
                        && instrument_id != filter_id
                    {
                        continue;
                    }

                    // Parse to order status report using minimal OrderStatus
                    // Note: OrderState doesn't have filled/average_fill_price, so we use defaults
                    let report = parse_order_data_to_report(
                        &data,
                        instrument_id,
                        self.context.account_id,
                        &self.context.instrument_provider,
                        ts_init,
                    )
                    .with_context(|| format!("Failed to parse IB order {}", data.order_id))?;
                    reports.push(report);
                    ib_orders.push(ib_order_with_api_order_id(data));
                }
                Ok(_) => {
                    // Ignore other order types
                }
                Err(e) => return Err(e.into()),
            }
        }

        let open_order_ids = reports.iter().map(|report| report.venue_order_id).collect();
        let mut unresolved = Vec::new();

        if !cmd.open_only {
            self.collect_completed_orders(
                cmd,
                &mut reports,
                &mut ib_orders,
                &mut unresolved,
                ts_init,
            )
            .await?;
        }

        link_order_contingencies(&mut reports, &ib_orders);

        Ok(ReportedOrders {
            reports,
            orders: ib_orders,
            open_order_ids,
            unresolved,
        })
    }

    async fn collect_completed_orders(
        &self,
        cmd: &GenerateOrderStatusReports,
        reports: &mut Vec<OrderStatusReport>,
        ib_orders: &mut Vec<IBOrder>,
        unresolved: &mut Vec<IBOrder>,
        ts_init: UnixNanos,
    ) -> anyhow::Result<()> {
        let client = self.client.as_ref();
        let timeout_dur = self.request_timeout;
        let ib_account = self.ib_account;
        let completed = tokio::time::timeout(timeout_dur, client.completed_orders(false))
            .await
            .context("Timeout requesting completed orders")??;
        let mut completed = completed.filter_data();

        while let Some(order_result) = completed.next().await {
            let Orders::OrderData(data) = order_result? else {
                continue;
            };

            if !data.order.account.is_empty() && data.order.account != ib_account {
                continue;
            }

            // An OCA group that reduces its members on a fill cancels an unfilled member
            // by reducing its quantity to zero, leaving nothing to reconcile
            if data.order.total_quantity == 0.0 && data.order.filled_quantity == 0.0 {
                tracing::debug!(
                    "Skipping completed IB order {} with zero quantity",
                    data.order.perm_id
                );
                continue;
            }

            let Some(instrument_id) = self.context.resolve_completed_order_instrument_id(&data)
            else {
                // An order completed outside the window would be filtered out anyway
                let completed_at = parse_execution_time(&data.order_state.completed_time).ok();
                let in_window = completed_at.is_none_or(|completed_at| {
                    cmd.start.is_none_or(|start| completed_at >= start)
                        && cmd.end.is_none_or(|end| completed_at <= end)
                });

                if in_window {
                    unresolved.push(ib_order_with_api_order_id(data));
                }
                continue;
            };

            if cmd
                .instrument_id
                .is_some_and(|filter_id| instrument_id != filter_id)
            {
                continue;
            }

            let report = parse_order_data_to_report(
                &data,
                instrument_id,
                self.context.account_id,
                &self.context.instrument_provider,
                ts_init,
            )
            .context("Failed to parse completed IB order")?;

            if !reports
                .iter()
                .any(|existing| existing.venue_order_id == report.venue_order_id)
            {
                reports.push(report);
                ib_orders.push(ib_order_with_api_order_id(data));
            }
        }

        Ok(())
    }

    pub(super) async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.generate_fill_evidence(cmd).await?.into_fill_reports()
    }

    /// Returns the fill reports with the evidence an entry price rebuild needs: the combo leg
    /// executions they drop and the executions the adapter could not parse.
    pub(super) async fn generate_fill_evidence(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<FillEvidence> {
        let client = self.client.as_ref();

        let filter =
            InteractiveBrokersExecutionClient::execution_filter(self.ib_account, cmd.start);

        let timeout_dur = self.request_timeout;
        let subscription = tokio::time::timeout(timeout_dur, client.executions(filter))
            .await
            .context("Timeout requesting executions")??;
        let mut subscription = subscription.filter_data();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let mut collector = FillReportCollector::new(&self.context, &cmd, ts_init);

        while let Some(exec_result) = subscription.next().await {
            collector.push(exec_result);
        }

        let (mut evidence, combo_executions) = collector.finish();

        if !combo_executions.is_empty() {
            self.resolve_combo_fill_reports(
                &cmd,
                client,
                combo_executions,
                &mut evidence.reports,
                ts_init,
            )
            .await?;
        }

        Ok(evidence)
    }

    /// Inserts `report`, replacing an earlier report for the same account, instrument, and
    /// venue position, and returns whether a report was replaced.
    ///
    /// IB holds one position per account and contract but resends the whole set while the
    /// positions subscription is open (for example after a [2100] account-data notice), so a
    /// later report supersedes the earlier one instead of counting the quantity twice.
    pub(super) fn upsert_position_report(
        reports: &mut Vec<PositionStatusReport>,
        report: PositionStatusReport,
    ) -> bool {
        if let Some(existing) = reports.iter_mut().find(|existing| {
            existing.account_id == report.account_id
                && existing.instrument_id == report.instrument_id
                && existing.venue_position_id == report.venue_position_id
        }) {
            *existing = report;
            return true;
        }

        reports.push(report);
        false
    }

    pub(super) async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let client = self.client.as_ref();

        // The request ID keeps this snapshot separate from the live position subscription
        let account = IbAccountId(self.ib_account.to_string());
        let timeout_dur = self.request_timeout;
        let subscription =
            tokio::time::timeout(timeout_dur, client.positions_multi(Some(&account), None))
                .await
                .context("Timeout requesting positions")??;
        let mut subscription = subscription.filter_data();
        let mut reports = Vec::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let ib_account = self.ib_account;

        loop {
            let update = subscription
                .next()
                .await
                .context("IB position snapshot ended before PositionMultiEnd")??;

            match update {
                PositionUpdateMulti::Position(position) => {
                    // Filter for the specific account
                    if position.account != ib_account {
                        continue;
                    }

                    let instrument = match self
                        .context
                        .instrument_provider
                        .get_instrument(client, &position.contract)
                        .await
                    {
                        Ok(Some(instrument)) => instrument,
                        Ok(None) => anyhow::bail!(
                            "Cannot resolve position instrument for IB contract ID {} ({:?})",
                            position.contract.contract_id,
                            position.contract.security_type
                        ),
                        Err(e) => return Err(e).context(format!(
                            "Failed to resolve position instrument for IB contract ID {} ({:?})",
                            position.contract.contract_id, position.contract.security_type
                        )),
                    };

                    let instrument_id = instrument.id();

                    // Filter by instrument_id if specified
                    if let Some(filter_id) = cmd.instrument_id
                        && instrument_id != filter_id
                    {
                        continue;
                    }

                    // Determine position side
                    let position_side = if position.position == 0.0 {
                        PositionSide::Flat
                    } else if position.position > 0.0 {
                        PositionSide::Long
                    } else {
                        PositionSide::Short
                    };

                    let quantity =
                        Quantity::new(position.position.abs(), instrument.size_precision());

                    // Convert IB avg_cost to a Nautilus Price by removing the multiplier
                    let avg_px_open =
                        IbReportContext::position_avg_px_open(&instrument, position.average_cost);

                    let report = PositionStatusReport::new(
                        self.context.account_id,
                        instrument_id,
                        position_side,
                        quantity,
                        ts_init, // ts_last
                        ts_init, // ts_init
                        None,    // report_id: auto-generated
                        None,    // venue_position_id
                        avg_px_open,
                    )
                    // `position_avg_px_open` rounds IB's average cost to the price precision,
                    // and that cost includes commissions: reconciliation allows one tick for both
                    .with_avg_px_open_precision(instrument.price_precision());

                    if Self::upsert_position_report(&mut reports, report) {
                        tracing::debug!(
                            "Superseded duplicate IB position report for {instrument_id}"
                        );
                    }
                }
                PositionUpdateMulti::PositionEnd => break,
            }
        }

        if reports.is_empty()
            && let Some(instrument_id) = cmd.instrument_id
        {
            let report = self.context.flat_position_report(instrument_id, ts_init);
            reports.push(report);
        }

        Ok(reports)
    }

    // A combo-level execution carries a generic BAG contract without its legs, so its spread
    // resolves through the combo order's contract by permanent ID. It becomes the spread
    // order's fill with zero commission, as on the live path.
    async fn resolve_combo_fill_reports(
        &self,
        cmd: &GenerateFillReports,
        client: &Client,
        combo_executions: Vec<ExecutionData>,
        reports: &mut Vec<FillReport>,
        ts_init: UnixNanos,
    ) -> anyhow::Result<()> {
        let combo_perm_ids: AHashSet<i64> = combo_executions
            .iter()
            .map(|exec_data| exec_data.execution.perm_id)
            .collect();
        let combo_contracts = self.combo_order_contracts(client, &combo_perm_ids).await?;

        for exec_data in combo_executions {
            let perm_id = exec_data.execution.perm_id;

            let report = combo_contracts
                .get(&perm_id)
                .context("IB returned no combo order for the execution")
                .and_then(|contract| self.context.resolve_report_contract_instrument_id(contract))
                .and_then(|instrument_id| {
                    self.context.build_historical_fill_report(
                        cmd,
                        &exec_data,
                        instrument_id,
                        0.0,
                        exec_data.contract.currency.as_str(),
                        ts_init,
                    )
                });

            match report {
                Ok(Some(report)) => reports.push(report),
                Ok(None) => {}
                Err(e) => IbReportContext::warn_historical_fill_report_parse_error(&exec_data, &e),
            }
        }

        Ok(())
    }

    async fn combo_order_contracts(
        &self,
        client: &Client,
        perm_ids: &AHashSet<i64>,
    ) -> anyhow::Result<AHashMap<i64, Contract>> {
        let timeout_dur = self.request_timeout;
        tokio::time::timeout(timeout_dur, async {
            let mut contracts = AHashMap::new();
            let mut open = client.all_open_orders().await?;
            while let Some(item) = open.next().await {
                if let SubscriptionItem::Data(Orders::OrderData(data)) = item?
                    && perm_ids.contains(&data.order.perm_id)
                {
                    contracts.insert(data.order.perm_id, data.contract);
                }
            }

            let mut completed = client.completed_orders(false).await?;
            while let Some(item) = completed.next().await {
                if let SubscriptionItem::Data(Orders::OrderData(data)) = item?
                    && perm_ids.contains(&data.order.perm_id)
                {
                    contracts.insert(data.order.perm_id, data.contract);
                }
            }

            Ok::<_, anyhow::Error>(contracts)
        })
        .await
        .context("timed out reading IB combo orders")?
    }
}

/// Pairs IB executions with their commission reports into fill reports.
///
/// It records each execution it cannot turn into a fill report, since the fills it returns are
/// then incomplete for that instrument, or for every instrument when the execution's instrument
/// is unknown.
pub(super) struct FillReportCollector<'a> {
    context: &'a IbReportContext,
    cmd: &'a GenerateFillReports,
    ts_init: UnixNanos,
    evidence: FillEvidence,
    pending_exec_data: AHashMap<String, ExecutionData>,
    pending_commissions: AHashMap<String, (f64, String)>,
    combo_executions: Vec<ExecutionData>,
}

impl<'a> FillReportCollector<'a> {
    pub(super) fn new(
        context: &'a IbReportContext,
        cmd: &'a GenerateFillReports,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            context,
            cmd,
            ts_init,
            evidence: FillEvidence::default(),
            pending_exec_data: AHashMap::new(),
            pending_commissions: AHashMap::new(),
            combo_executions: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, item: Result<Executions, ibapi::Error>) {
        match item {
            Ok(Executions::ExecutionData(exec_data)) => {
                let execution_id = exec_data.execution.execution_id.clone();
                if parse::is_combo_execution(&exec_data) {
                    self.combo_executions.push(exec_data);
                } else if let Some((commission, commission_currency)) =
                    self.pending_commissions.remove(&execution_id)
                {
                    self.add_fill(&exec_data, commission, &commission_currency);
                } else {
                    self.pending_exec_data.insert(execution_id, exec_data);
                }
            }
            Ok(Executions::CommissionReport(commission))
                if parse::is_pending_commission(commission.commission) => {}
            Ok(Executions::CommissionReport(commission)) => {
                if let Some(exec_data) = self.pending_exec_data.remove(&commission.execution_id) {
                    self.add_fill(&exec_data, commission.commission, &commission.currency);
                } else {
                    self.pending_commissions.insert(
                        commission.execution_id,
                        (commission.commission, commission.currency),
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Error receiving execution data: {e}");
                self.evidence.missing_all = true;
            }
        }
    }

    /// Returns the fill evidence and the combo-level executions, whose spread fills need the
    /// combo order contracts.
    ///
    /// The combo leg executions move from the fill reports to the evidence's combo leg reports:
    /// they share the combo's order IDs, so they would fill the spread order at leg prices.
    pub(super) fn finish(mut self) -> (FillEvidence, Vec<ExecutionData>) {
        // An execution without its commission report leaves its instrument's fills incomplete,
        // like an execution that cannot be parsed. It is recorded so a standalone query fails
        // and a mass status leaves the order out. One outside the request's filters is ignored,
        // as its fill would be.
        for (execution_id, exec_data) in std::mem::take(&mut self.pending_exec_data) {
            let instrument_id = self
                .context
                .resolve_historical_execution_instrument_id(&exec_data)
                .ok();
            let venue_order_id =
                ib_venue_order_id(exec_data.execution.order_id, exec_data.execution.perm_id);

            if !self.is_in_scope(&exec_data, instrument_id, venue_order_id) {
                continue;
            }

            tracing::warn!("IB did not provide a commission report for execution {execution_id}");

            match instrument_id {
                Some(instrument_id) => {
                    self.evidence.missing_instruments.insert(instrument_id);
                }
                None => self.evidence.missing_all = true,
            }

            self.evidence.missing_commissions.push(MissingCommission {
                execution_id,
                venue_order_id,
            });
        }

        // A commission report without its execution means an execution never arrived
        if !self.pending_commissions.is_empty() {
            self.evidence.missing_all = true;
        }

        let combo_venue_order_ids: AHashSet<VenueOrderId> = self
            .combo_executions
            .iter()
            .map(|exec_data| ib_venue_order_id(0, exec_data.execution.perm_id))
            .collect();
        self.evidence.combo_leg_reports = self
            .evidence
            .reports
            .extract_if(.., |report| {
                combo_venue_order_ids.contains(&report.venue_order_id)
            })
            .collect();

        let deferred_orders = self.evidence.orders_missing_commission();
        self.combo_executions.retain(|exec_data| {
            let venue_order_id =
                ib_venue_order_id(exec_data.execution.order_id, exec_data.execution.perm_id);
            !deferred_orders.contains(&venue_order_id)
        });

        (self.evidence, self.combo_executions)
    }

    // Applies the request's instrument, venue order, and end filters. An execution whose
    // instrument or time cannot be read stays in scope, so it is not silently ignored.
    fn is_in_scope(
        &self,
        exec_data: &ExecutionData,
        instrument_id: Option<InstrumentId>,
        venue_order_id: VenueOrderId,
    ) -> bool {
        if let (Some(filter_id), Some(instrument_id)) = (self.cmd.instrument_id, instrument_id)
            && instrument_id != filter_id
        {
            return false;
        }

        if self
            .cmd
            .venue_order_id
            .is_some_and(|filter_id| filter_id != venue_order_id)
        {
            return false;
        }

        !self.cmd.end.is_some_and(|end| {
            parse_execution_time(&exec_data.execution.time).is_ok_and(|time| time > end)
        })
    }

    fn add_fill(&mut self, exec_data: &ExecutionData, commission: f64, commission_currency: &str) {
        let instrument_id = match self
            .context
            .resolve_historical_execution_instrument_id(exec_data)
        {
            Ok(instrument_id) => instrument_id,
            Err(e) => {
                IbReportContext::warn_historical_fill_report_parse_error(exec_data, &e);
                self.evidence.missing_all = true;
                return;
            }
        };

        match self.context.build_historical_fill_report(
            self.cmd,
            exec_data,
            instrument_id,
            commission,
            commission_currency,
            self.ts_init,
        ) {
            Ok(Some(report)) => self.evidence.reports.push(report),
            Ok(None) => {}
            Err(e) => {
                IbReportContext::warn_historical_fill_report_parse_error(exec_data, &e);
                self.evidence.missing_instruments.insert(instrument_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{ContingencyType, TimeInForce},
        instruments::stubs::equity_aapl,
    };
    use rstest::rstest;
    use serde::Deserialize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::config::InteractiveBrokersInstrumentProviderConfig;

    #[rstest]
    #[tokio::test]
    async fn test_position_snapshot_ignores_other_position_end_markers() {
        let exchanges = serde_json::from_str(include_str!(
            "../../test_data/position_snapshot_responses.json"
        ))
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve_report_exchanges(listener, exchanges, done_rx));

        let config = InteractiveBrokersInstrumentProviderConfig::default();
        let provider = Arc::new(InteractiveBrokersInstrumentProvider::new(config).unwrap());
        provider.insert_test_instrument(equity_aapl().into(), 265_598, 1);
        let command = GeneratePositionStatusReportsBuilder::default()
            .ts_init(UnixNanos::new(1))
            .build()
            .unwrap();

        let reports = tokio::time::timeout(Duration::from_secs(10), async {
            let client = Arc::new(Client::connect(&address.to_string(), 7).await.unwrap());
            let context = IbReportContext::new(provider, AccountId::from("IB-DU123"));

            let report_client = IbReportClient::new(
                client.clone(),
                context,
                Ustr::from("DU123"),
                Duration::from_secs(2),
            );

            let reports = report_client
                .generate_position_status_reports(&command)
                .await
                .unwrap();
            client.disconnect().await;
            done_tx.send(()).unwrap();
            server.await.unwrap();
            reports
        })
        .await
        .unwrap();

        let [report] = reports.as_slice() else {
            panic!("expected one IB position report");
        };

        assert_eq!(report.account_id, AccountId::from("IB-DU123"));
        assert_eq!(report.instrument_id, equity_aapl().id);
        assert_eq!(report.position_side, PositionSide::Long);
        assert_eq!(report.signed_decimal_qty, Decimal::from(3));
        assert_eq!(report.avg_px_open, Some(Decimal::new(1235, 1)));
    }

    #[rstest]
    #[case::unbounded(None, None, false, true, false)]
    #[case::before_start(Some("20260924 12:02:05 UTC"), None, false, false, false)]
    #[case::after_end(None, Some("20260924 12:02:03 UTC"), false, false, false)]
    #[case::at_start(Some("20260924 12:02:04 UTC"), None, false, true, false)]
    #[case::at_end(None, Some("20260924 12:02:04 UTC"), false, true, false)]
    #[case::open_only(Some("20260924 12:02:05 UTC"), None, true, false, false)]
    #[case::open_only_terminal(Some("20260924 12:02:05 UTC"), None, true, false, true)]
    #[tokio::test]
    async fn test_order_status_reports_filter_completed_orders(
        #[case] start: Option<&str>,
        #[case] end: Option<&str>,
        #[case] open_only: bool,
        #[case] includes_entry: bool,
        #[case] terminal_report: bool,
    ) {
        let mut exchanges = report_exchanges();

        // Simulate a stale terminal response in the open-order stream
        if terminal_report {
            replace_frame_bytes(&mut exchanges[1].responses[0], b"Submitted", b"Cancelled");
        }

        exchanges.truncate(if open_only { 2 } else { 3 });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve_report_exchanges(listener, exchanges, done_rx));

        let config = InteractiveBrokersInstrumentProviderConfig::default();
        let provider = InteractiveBrokersInstrumentProvider::new(config).unwrap();
        let provider = Arc::new(provider);
        provider.insert_test_instrument(equity_aapl().into(), 265_598, 1);
        let command = GenerateOrderStatusReportsBuilder::default()
            .ts_init(UnixNanos::new(1))
            .open_only(open_only)
            .start(start.map(|value| parse_execution_time(value).unwrap()))
            .end(end.map(|value| parse_execution_time(value).unwrap()))
            .build()
            .unwrap();

        let evidence = tokio::time::timeout(Duration::from_secs(10), async {
            let client = Arc::new(Client::connect(&address.to_string(), 7).await.unwrap());
            let context = IbReportContext::new(provider, AccountId::from("IB-DU123"));

            let report_client = IbReportClient::new(
                Arc::clone(&client),
                context,
                Ustr::from("DU123"),
                Duration::from_secs(2),
            );

            let evidence = report_client
                .generate_order_evidence(&command)
                .await
                .unwrap();
            client.disconnect().await;
            done_tx.send(()).unwrap();
            server.await.unwrap();
            evidence
        })
        .await
        .unwrap();

        assert_eq!(
            evidence.open_order_ids,
            AHashSet::from([VenueOrderId::from("PERM-11"), VenueOrderId::from("PERM-12")]),
        );
        let reports = evidence.reports;
        let venue_order_ids = reports
            .iter()
            .map(|report| report.venue_order_id)
            .collect::<Vec<_>>();
        let mut expected_ids = vec![VenueOrderId::from("PERM-11"), VenueOrderId::from("PERM-12")];
        if includes_entry {
            expected_ids.push(VenueOrderId::from("PERM-10"));
        }

        assert_eq!(venue_order_ids, expected_ids);

        for (report, client_id, sibling) in [
            (&reports[0], "O-STOP", "O-TAKE"),
            (&reports[1], "O-TAKE", "O-STOP"),
        ] {
            assert_eq!(report.client_order_id, Some(ClientOrderId::from(client_id)));
            assert_eq!(
                report.parent_order_id,
                (!open_only).then(|| ClientOrderId::from("O-ENTRY"))
            );
            assert_eq!(report.contingency_type, Some(ContingencyType::Ouo));
            assert_eq!(
                report.linked_order_ids,
                Some(vec![ClientOrderId::from(sibling)])
            );

            let expected_status = if terminal_report && client_id == "O-STOP" {
                OrderStatus::Canceled
            } else {
                OrderStatus::Accepted
            };

            assert_eq!(report.order_status, expected_status);
            assert_eq!(report.quantity, Quantity::from(1));
            assert_eq!(report.filled_qty, Quantity::from(0));
            assert_eq!(report.ts_last, report.ts_init);
        }

        if includes_entry {
            assert_eq!(reports[2].order_status, OrderStatus::Filled);
            assert_eq!(
                reports[2].ts_last,
                parse_execution_time("20260924 12:02:04 UTC").unwrap()
            );
        }
    }

    #[rstest]
    #[case::foreign_account(CompletedOrderCase::ForeignAccount, &[])]
    #[case::instrument_mismatch(CompletedOrderCase::InstrumentMismatch, &[])]
    #[case::duplicate_venue_id(CompletedOrderCase::DuplicateVenueId, &["PERM-10"])]
    #[case::zero_quantity(CompletedOrderCase::ZeroQuantity, &[])]
    #[tokio::test]
    async fn test_completed_order_reports_filtering(
        #[case] case: CompletedOrderCase,
        #[case] expected_ids: &[&str],
    ) {
        let mut exchanges = report_exchanges();
        exchanges.remove(1);
        let completed = &mut exchanges[1].responses[0];
        let mut command = GenerateOrderStatusReportsBuilder::default()
            .ts_init(UnixNanos::new(7))
            .open_only(false)
            .build()
            .unwrap();

        match case {
            CompletedOrderCase::ForeignAccount => {
                replace_frame_bytes(completed, b"DU123", b"DU999");
            }
            CompletedOrderCase::InstrumentMismatch => {
                command.instrument_id = Some(InstrumentId::from("MSFT.XNAS"));
            }
            CompletedOrderCase::DuplicateVenueId => {}
            CompletedOrderCase::ZeroQuantity => {
                replace_frame_bytes(completed, b"\x32\x01\x31", b"\x32\x01\x30");
                replace_frame_bytes(completed, b"\x9a\x07\x01\x31", b"\x9a\x07\x01\x30");
            }
        }

        exchanges.push(ReportExchange {
            request: exchanges[1].request,
            responses: exchanges[1].responses.clone(),
        });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve_report_exchanges(listener, exchanges, done_rx));
        let config = InteractiveBrokersInstrumentProviderConfig::default();
        let provider = InteractiveBrokersInstrumentProvider::new(config).unwrap();
        let instrument = equity_aapl();
        let instrument_id = instrument.id;
        provider.insert_test_instrument(instrument.into(), 265_598, 1);
        let context = IbReportContext::new(Arc::new(provider), AccountId::from("IB-DU123"));

        let (reports_first, orders_first, reports_second, orders_second) =
            tokio::time::timeout(Duration::from_secs(10), async {
                let client = Client::connect(&address.to_string(), 7).await.unwrap();
                let client = Arc::new(client);

                let report_client = IbReportClient::new(
                    Arc::clone(&client),
                    context,
                    Ustr::from("DU123"),
                    Duration::from_secs(2),
                );
                let mut reports = Vec::new();
                let mut orders = Vec::new();
                let mut unresolved = Vec::new();
                report_client
                    .collect_completed_orders(
                        &command,
                        &mut reports,
                        &mut orders,
                        &mut unresolved,
                        command.ts_init,
                    )
                    .await
                    .unwrap();
                let reports_first = reports.clone();
                let orders_first = orders.clone();
                report_client
                    .collect_completed_orders(
                        &command,
                        &mut reports,
                        &mut orders,
                        &mut unresolved,
                        command.ts_init,
                    )
                    .await
                    .unwrap();
                assert!(unresolved.is_empty());
                client.disconnect().await;
                done_tx.send(()).unwrap();
                server.await.unwrap();
                (reports_first, orders_first, reports, orders)
            })
            .await
            .unwrap();

        let venue_ids = reports_first
            .iter()
            .map(|report| report.venue_order_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(venue_ids, expected_ids);
        assert_eq!(orders_first.len(), expected_ids.len());
        assert_eq!(reports_second, reports_first);
        assert_eq!(orders_second, orders_first);

        if let Some(report) = reports_first.first() {
            assert_eq!(report.account_id, AccountId::from("IB-DU123"));
            assert_eq!(report.instrument_id, instrument_id);
            assert_eq!(report.client_order_id, Some(ClientOrderId::from("O-ENTRY")));
            assert_eq!(report.order_status, OrderStatus::Filled);
            assert_eq!(report.quantity, Quantity::from(1));
            assert_eq!(report.filled_qty, Quantity::from(1));
            assert_eq!(report.ts_init, command.ts_init);
            assert_eq!(
                report.ts_last,
                parse_execution_time("20260924 12:02:04 UTC").unwrap()
            );
        }
    }

    #[rstest]
    #[case::unbounded(None, 1)]
    #[case::after_start(Some("20260924 12:02:03 UTC"), 1)]
    #[case::before_start(Some("20260924 12:02:05 UTC"), 0)]
    #[tokio::test]
    async fn test_order_evidence_skips_unresolved_completed_order(
        #[case] start: Option<&str>,
        #[case] unresolved_orders: usize,
    ) {
        let mut exchanges = report_exchanges();
        make_unresolved_bag(&mut exchanges[2].responses[0]);
        let command = GenerateOrderStatusReportsBuilder::default()
            .ts_init(UnixNanos::new(1))
            .open_only(false)
            .start(start.map(|value| parse_execution_time(value).unwrap()))
            .build()
            .unwrap();

        let evidence = with_report_client(exchanges, async |report_client| {
            report_client.generate_order_evidence(&command).await
        })
        .await
        .unwrap();

        let venue_order_ids = evidence
            .reports
            .iter()
            .map(|report| report.venue_order_id)
            .collect::<Vec<_>>();
        assert_eq!(
            venue_order_ids,
            vec![VenueOrderId::from("PERM-11"), VenueOrderId::from("PERM-12")]
        );
        assert_eq!(evidence.unresolved_orders, unresolved_orders);
    }

    #[rstest]
    #[tokio::test]
    async fn test_order_status_reports_fail_on_unresolved_open_order(
        #[values(true, false)] open_only: bool,
    ) {
        let mut exchanges = report_exchanges();
        make_unresolved_bag(&mut exchanges[1].responses[0]);
        exchanges.truncate(2);
        let command = GenerateOrderStatusReportsBuilder::default()
            .ts_init(UnixNanos::new(1))
            .open_only(open_only)
            .build()
            .unwrap();

        let result = with_report_client(exchanges, async |report_client| {
            report_client.generate_order_status_reports(&command).await
        })
        .await;

        let error = result.unwrap_err();
        assert!(
            format!("{error:#}").contains("Failed to resolve IB order"),
            "{error:#}"
        );
    }

    #[rstest]
    #[case::unresolved("O-ENTRY", None, "AAPL.XNAS", Err(()))]
    #[case::resolved("O-STOP", Some("PERM-11"), "AAPL.XNAS", Ok(Some("PERM-11")))]
    #[case::resolved_on_another_instrument(
        "O-STOP",
        Some("PERM-11"),
        "MSFT.XNAS",
        Ok(Some("PERM-11"))
    )]
    #[case::genuine_miss("O-OTHER", Some("PERM-99"), "AAPL.XNAS", Ok(None))]
    #[tokio::test]
    async fn test_order_status_report_identifies_order_across_reported_orders(
        #[case] client_order_id: &str,
        #[case] venue_order_id: Option<&str>,
        #[case] instrument_id: &str,
        #[case] expected: Result<Option<&str>, ()>,
    ) {
        let mut exchanges = report_exchanges();
        make_unresolved_bag(&mut exchanges[2].responses[0]);
        let command = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::new(1),
            Some(InstrumentId::from(instrument_id)),
            Some(ClientOrderId::from(client_order_id)),
            venue_order_id.map(VenueOrderId::from),
            None,
            None,
        );

        let result = with_report_client(exchanges, async |report_client| {
            report_client.generate_order_status_report(&command).await
        })
        .await;

        match expected {
            Ok(expected) => assert_eq!(
                result.unwrap().map(|report| report.venue_order_id),
                expected.map(VenueOrderId::from)
            ),
            Err(()) => {
                let error = result.unwrap_err();
                assert!(error.to_string().contains("unidentified"), "{error:#}");
            }
        }
    }

    #[rstest]
    #[case::genuine_miss("O-OTHER", Some("PERM-99"), Ok(None))]
    #[case::genuine_miss_before_acknowledgement("O-OTHER", None, Ok(None))]
    #[case::order_ref_shared(
        "O-STOP",
        Some("PERM-11"),
        Ok(Some(("PERM-11", Some("O-STOP"))))
    )]
    #[case::order_ref_shared_without_venue_order_id("O-STOP", None, Err(()))]
    #[case::numeric_venue_order_id("O-STOP", Some("1"), Ok(Some(("PERM-11", Some("O-STOP")))))]
    #[case::numeric_venue_order_id_of_another_client("O-OTHER", Some("5"), Ok(None))]
    #[case::numeric_venue_order_id_of_another_client_by_order_ref(
        "O-ELSE",
        Some("7"),
        Ok(Some(("7", Some("O-ELSE"))))
    )]
    #[case::venue_order_id_identifies_contradicting_order_ref(
        "O-OTHER",
        Some("PERM-11"),
        Ok(Some(("PERM-11", Some("O-STOP"))))
    )]
    #[case::client_order_id_as_supplied("O-STOP:9", None, Ok(None))]
    #[case::order_ref_without_client_order_id(":12345", None, Ok(Some(("PERM-13", Some(":12345")))))]
    #[case::suffixed_order_ref("O-TAKE", None, Ok(Some(("PERM-12", Some("O-TAKE")))))]
    #[case::venue_order_id_rules_out_matching_order_ref("O-TAKE", Some("PERM-99"), Ok(None))]
    #[case::adopted("PERM-13", Some("PERM-13"), Ok(Some(("PERM-13", None))))]
    #[case::unresolved_adopted("PERM-20", Some("PERM-20"), Err(()))]
    #[case::unresolved_adopted_by_client_order_id("PERM-20", None, Err(()))]
    #[case::unresolved_by_client_order_id("O-ENTRY", None, Err(()))]
    #[case::unresolved_by_numeric_venue_order_id("O-ENTRY", Some("4"), Err(()))]
    fn test_order_query_selects_identified_report(
        #[case] client_order_id: &str,
        #[case] venue_order_id: Option<&str>,
        #[case] expected: Result<Option<(&str, Option<&str>)>, ()>,
    ) {
        let reported = [
            ib_order(1, 7, 11, "O-STOP"),
            ib_order(2, 7, 12, "O-TAKE:2"),
            ib_order(3, 7, 13, ":12345"),
            ib_order(6, 9, 14, "O-STOP"),
            ib_order(7, 9, 0, "O-ELSE"),
        ]
        .map(|order| (order_report(&order), order));
        let unresolved = [
            ib_order(4, 7, 10, "O-ENTRY"),
            ib_order(1, 8, 20, ""),
            ib_order(5, 8, 22, "O-MANUAL"),
        ];
        let query = OrderQuery {
            client_order_id: Some(ClientOrderId::from(client_order_id)),
            venue_order_id: venue_order_id.map(VenueOrderId::from),
            ib_client_id: 7,
        };

        let selected = query.select_report(reported, &unresolved);

        assert_eq!(
            selected
                .map(|report| report.map(|report| (report.venue_order_id, report.client_order_id)))
                .map_err(|_| ()),
            expected.map(|report| report.map(|(venue_order_id, client_order_id)| {
                (
                    VenueOrderId::from(venue_order_id),
                    client_order_id.map(ClientOrderId::from),
                )
            }))
        );
    }

    fn ib_order(order_id: i32, client_id: i32, perm_id: i64, order_ref: &str) -> IBOrder {
        IBOrder {
            order_id,
            client_id,
            perm_id,
            order_ref: order_ref.to_string(),
            ..Default::default()
        }
    }

    fn order_report(order: &IBOrder) -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("IB-DU123"),
            equity_aapl().id,
            order_ref_client_order_id(&order.order_ref),
            ib_venue_order_id(order.order_id, order.perm_id),
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from(1),
            Quantity::from(0),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        )
    }

    // Turns an order's AAPL stock contract into a BAG with no legs under an unknown contract ID,
    // which resolves to no instrument.
    fn make_unresolved_bag(frame: &mut [u8]) {
        replace_frame_bytes(frame, b"\x08\xfe\x9a\x10", b"\x08\xff\x9a\x10");
        replace_frame_bytes(frame, b"\x1a\x03STK", b"\x1a\x03BAG");
    }

    async fn with_report_client<T>(
        exchanges: Vec<ReportExchange>,
        run: impl AsyncFnOnce(&IbReportClient) -> T,
    ) -> T {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve_report_exchanges(listener, exchanges, done_rx));
        let config = InteractiveBrokersInstrumentProviderConfig::default();
        let provider = InteractiveBrokersInstrumentProvider::new(config).unwrap();
        provider.insert_test_instrument(equity_aapl().into(), 265_598, 1);
        let context = IbReportContext::new(Arc::new(provider), AccountId::from("IB-DU123"));

        tokio::time::timeout(Duration::from_secs(10), async {
            let client = Arc::new(Client::connect(&address.to_string(), 7).await.unwrap());
            let report_client = IbReportClient::new(
                Arc::clone(&client),
                context,
                Ustr::from("DU123"),
                Duration::from_secs(2),
            );
            let result = run(&report_client).await;
            client.disconnect().await;
            done_tx.send(()).unwrap();
            server.await.unwrap();
            result
        })
        .await
        .unwrap()
    }

    #[derive(Debug, Clone, Copy)]
    enum CompletedOrderCase {
        ForeignAccount,
        InstrumentMismatch,
        DuplicateVenueId,
        ZeroQuantity,
    }

    #[derive(Deserialize)]
    struct ReportExchange {
        request: i32,
        responses: Vec<Vec<u8>>,
    }

    fn report_exchanges() -> Vec<ReportExchange> {
        serde_json::from_str(include_str!("../../test_data/order_report_responses.json")).unwrap()
    }

    fn replace_frame_bytes(frame: &mut [u8], original: &[u8], replacement: &[u8]) {
        assert_eq!(original.len(), replacement.len());
        let offset = frame
            .windows(original.len())
            .position(|bytes| bytes == original)
            .unwrap();
        frame[offset..offset + original.len()].copy_from_slice(replacement);
    }

    async fn serve_report_exchanges(
        listener: tokio::net::TcpListener,
        exchanges: Vec<ReportExchange>,
        done: tokio::sync::oneshot::Receiver<()>,
    ) {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut magic = [0_u8; 4];
        socket.read_exact(&mut magic).await.unwrap();
        assert_eq!(magic, *b"API\0");
        let _version = read_ib_frame(&mut socket).await;
        let handshake = b"213\x0020260924 12:03:00 UTC\0";
        socket.write_u32(handshake.len() as u32).await.unwrap();
        socket.write_all(handshake).await.unwrap();
        for exchange in exchanges {
            let request = read_ib_frame(&mut socket).await;
            let message_id = i32::from_be_bytes(request[..4].try_into().unwrap());
            assert_eq!(message_id, exchange.request);

            for response in exchange.responses {
                socket.write_all(&response).await.unwrap();
            }
        }

        done.await.unwrap();
    }

    async fn read_ib_frame(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
        let length = socket.read_u32().await.unwrap();
        let mut frame = vec![0; length as usize];
        socket.read_exact(&mut frame).await.unwrap();
        frame
    }
}
