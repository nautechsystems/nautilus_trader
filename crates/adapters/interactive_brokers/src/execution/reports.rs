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

use ibapi::{client::Client, orders::Order as IBOrder};
use nautilus_live::execution::reports::retain_order_status_reports;

use super::{
    core::*,
    parse::{FillEvidence, link_order_contingencies, parse_order_data_to_report},
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

    // Returns `None` for an order whose contract does not resolve, such as a BAG spread another
    // node loaded at runtime, so the order is left out of the reports instead of failing them all.
    // Connecting preloads the instruments of cached orders, so such an order is not one the node
    // holds.
    pub(super) fn resolve_order_report_instrument_id(
        &self,
        data: &ibapi::orders::OrderData,
    ) -> Option<InstrumentId> {
        match self.resolve_report_contract_instrument_id(&data.contract) {
            Ok(instrument_id) => Some(instrument_id),
            Err(e) => {
                tracing::warn!(
                    order_id = data.order_id,
                    perm_id = data.order.perm_id,
                    con_id = data.contract.contract_id,
                    sec_type = ?data.contract.security_type,
                    error = format!("{e:#}"),
                    "Skipping IB order report for unresolved contract",
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

    pub(super) async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let plural_cmd = GenerateOrderStatusReports {
            command_id: cmd.command_id,
            ts_init: cmd.ts_init,
            open_only: false,
            instrument_id: cmd.instrument_id,
            start: None,
            end: None,
            params: cmd.params.clone(),
            log_receipt_level: LogLevel::Info,
            correlation_id: cmd.correlation_id,
            causation_id: cmd.causation_id,
        };

        let reports = self.generate_order_status_reports(&plural_cmd).await?;

        // Filter by client_order_id and venue_order_id
        let report = reports.into_iter().find(|r| {
            let matches_client = if let Some(filter_client_id) = cmd.client_order_id {
                r.client_order_id == Some(filter_client_id)
            } else {
                true
            };

            let matches_venue = if let Some(filter_venue_id) = cmd.venue_order_id {
                r.venue_order_id == filter_venue_id
            } else {
                true
            };

            matches_client && matches_venue
        });

        Ok(report)
    }

    pub(super) async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
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

                    let Some(instrument_id) =
                        self.context.resolve_order_report_instrument_id(&data)
                    else {
                        continue;
                    };

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
                    ib_orders.push(data.order);
                }
                Ok(_) => {
                    // Ignore other order types
                }
                Err(e) => return Err(e.into()),
            }
        }

        if !cmd.open_only {
            self.collect_completed_orders(cmd, &mut reports, &mut ib_orders, ts_init)
                .await?;
        }

        link_order_contingencies(&mut reports, &ib_orders);
        if !cmd.open_only {
            retain_order_status_reports(&mut reports, cmd);
        }

        Ok(reports)
    }

    async fn collect_completed_orders(
        &self,
        cmd: &GenerateOrderStatusReports,
        reports: &mut Vec<OrderStatusReport>,
        ib_orders: &mut Vec<IBOrder>,
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

            let Some(instrument_id) = self.context.resolve_order_report_instrument_id(&data) else {
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
                ib_orders.push(data.order);
            }
        }

        Ok(())
    }

    pub(super) async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        Ok(self.generate_fill_evidence(cmd).await?.reports)
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

        let (mut evidence, combo_executions) = collector.finish()?;

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

        let timeout_dur = self.request_timeout;
        let subscription = tokio::time::timeout(timeout_dur, client.positions())
            .await
            .context("Timeout requesting positions")??;
        let mut subscription = subscription.filter_data();
        let mut reports = Vec::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let ib_account = self.ib_account;

        // Process positions until PositionEnd; return empty list when none (reconciliation parity:
        // never return None/missing for "no positions").
        while let Some(position_result) = subscription.next().await {
            match position_result {
                Ok(PositionUpdate::Position(position)) => {
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
                Ok(PositionUpdate::PositionEnd) => {
                    // End of position list
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }

        if reports.is_empty()
            && let Some(instrument_id) = cmd.instrument_id
        {
            let precision = self
                .context
                .instrument_provider
                .find(&instrument_id)
                .map_or(0, |instrument| instrument.size_precision());
            reports.push(PositionStatusReport::new(
                self.context.account_id,
                instrument_id,
                PositionSide::Flat,
                Quantity::zero(precision),
                ts_init,
                ts_init,
                None,
                None,
                None,
            ));
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
    ///
    /// # Errors
    ///
    /// Returns an error if an execution has no commission report.
    pub(super) fn finish(mut self) -> anyhow::Result<(FillEvidence, Vec<ExecutionData>)> {
        anyhow::ensure!(
            self.pending_exec_data.is_empty(),
            "IB did not provide commission reports for execution IDs: {}",
            self.pending_exec_data
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        );

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

        Ok((self.evidence, self.combo_executions))
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
    use nautilus_model::{enums::ContingencyType, instruments::stubs::equity_aapl};
    use rstest::rstest;
    use serde::Deserialize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::config::InteractiveBrokersInstrumentProviderConfig;

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
        provider.insert_test_instrument(equity_aapl().into(), 265598, 1);
        let command = GenerateOrderStatusReportsBuilder::default()
            .ts_init(UnixNanos::new(1))
            .open_only(open_only)
            .start(start.map(|value| parse_execution_time(value).unwrap()))
            .end(end.map(|value| parse_execution_time(value).unwrap()))
            .build()
            .unwrap();

        let reports = tokio::time::timeout(Duration::from_secs(10), async {
            let client = Arc::new(Client::connect(&address.to_string(), 7).await.unwrap());
            let context = IbReportContext::new(provider, AccountId::from("IB-DU123"));

            let report_client = IbReportClient::new(
                Arc::clone(&client),
                context,
                Ustr::from("DU123"),
                Duration::from_secs(2),
            );
            let reports = report_client
                .generate_order_status_reports(&command)
                .await
                .unwrap();
            client.disconnect().await;
            done_tx.send(()).unwrap();
            server.await.unwrap();
            reports
        })
        .await
        .unwrap();

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
        provider.insert_test_instrument(instrument.into(), 265598, 1);
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
                report_client
                    .collect_completed_orders(&command, &mut reports, &mut orders, command.ts_init)
                    .await
                    .unwrap();
                let reports_first = reports.clone();
                let orders_first = orders.clone();
                report_client
                    .collect_completed_orders(&command, &mut reports, &mut orders, command.ts_init)
                    .await
                    .unwrap();
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
