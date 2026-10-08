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

use ibapi::client::Client;

use super::{core::*, parse::parse_order_data_to_report};

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

    pub(super) fn parse_historical_fill_report(
        &self,
        cmd: &GenerateFillReports,
        exec_data: &ExecutionData,
        commission: f64,
        commission_currency: &str,
        ts_init: UnixNanos,
    ) -> Option<FillReport> {
        let instrument_id = match self.resolve_historical_execution_instrument_id(exec_data) {
            Ok(instrument_id) => instrument_id,
            Err(e) => {
                Self::warn_historical_fill_report_parse_error(exec_data, &e);
                return None;
            }
        };

        self.build_historical_fill_report(
            cmd,
            exec_data,
            instrument_id,
            commission,
            commission_currency,
            ts_init,
        )
    }

    fn build_historical_fill_report(
        &self,
        cmd: &GenerateFillReports,
        exec_data: &ExecutionData,
        instrument_id: InstrumentId,
        commission: f64,
        commission_currency: &str,
        ts_init: UnixNanos,
    ) -> Option<FillReport> {
        if let Some(filter_id) = cmd.instrument_id
            && instrument_id != filter_id
        {
            return None;
        }

        if let Some(filter_venue_order_id) = cmd.venue_order_id
            && ib_venue_order_id(exec_data.execution.order_id, exec_data.execution.perm_id)
                != filter_venue_order_id
        {
            return None;
        }

        if let Some(end) = cmd.end {
            match parse_execution_time(&exec_data.execution.time) {
                Ok(ts_event) if ts_event > end => return None,
                Ok(_) => {}
                Err(e) => {
                    Self::warn_historical_fill_report_parse_error(exec_data, &e);
                    return None;
                }
            }
        }

        match parse_execution_to_fill_report(
            &exec_data.execution,
            &exec_data.contract,
            commission,
            commission_currency,
            instrument_id,
            self.account_id,
            &self.instrument_provider,
            ts_init,
            None, // avg_px (not available in historical fills)
        ) {
            Ok(report) => Some(report),
            Err(e) => {
                Self::warn_historical_fill_report_parse_error(exec_data, &e);
                None
            }
        }
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

    pub(super) fn position_avg_px_open(
        &self,
        instrument_id: &InstrumentId,
        instrument: &InstrumentAny,
        average_cost: f64,
    ) -> Option<Decimal> {
        if average_cost <= 0.0 {
            return None;
        }

        let price_magnifier = self.instrument_provider.get_price_magnifier(instrument_id) as f64;
        let multiplier = instrument.multiplier().as_f64();
        let converted_avg_cost = average_cost / (multiplier * price_magnifier);
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

                    // Convert IB contract to instrument ID
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
                    ib_orders.push(data.order);
                }
                Ok(_) => {
                    // Ignore other order types
                }
                Err(e) => return Err(e.into()),
            }
        }

        if !cmd.open_only {
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

                let instrument_id = self
                    .context
                    .resolve_report_contract_instrument_id(&data.contract)
                    .with_context(|| {
                        format!(
                            "Failed to resolve completed IB order contract ID {} ({:?})",
                            data.contract.contract_id, data.contract.security_type
                        )
                    })?;

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
        }

        parse::link_order_contingencies(&mut reports, &ib_orders);

        Ok(reports)
    }

    pub(super) async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let client = self.client.as_ref();

        let filter =
            InteractiveBrokersExecutionClient::execution_filter(self.ib_account, cmd.start);

        let timeout_dur = self.request_timeout;
        let subscription = tokio::time::timeout(timeout_dur, client.executions(filter))
            .await
            .context("Timeout requesting executions")??;
        let mut subscription = subscription.filter_data();
        let mut reports = Vec::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let mut pending_exec_data: AHashMap<String, ExecutionData> = AHashMap::new();
        let mut pending_commissions: AHashMap<String, (f64, String)> = AHashMap::new();
        let mut combo_executions = Vec::new();

        while let Some(exec_result) = subscription.next().await {
            match exec_result {
                Ok(Executions::ExecutionData(exec_data)) => {
                    let execution_id = exec_data.execution.execution_id.clone();
                    if parse::is_combo_execution(&exec_data) {
                        combo_executions.push(exec_data);
                    } else if let Some((commission, commission_currency)) =
                        pending_commissions.remove(&execution_id)
                    {
                        if let Some(report) = self.context.parse_historical_fill_report(
                            &cmd,
                            &exec_data,
                            commission,
                            &commission_currency,
                            ts_init,
                        ) {
                            reports.push(report);
                        }
                    } else {
                        pending_exec_data.insert(execution_id, exec_data);
                    }
                }
                Ok(Executions::CommissionReport(commission)) => {
                    if let Some(exec_data) = pending_exec_data.remove(&commission.execution_id) {
                        if let Some(report) = self.context.parse_historical_fill_report(
                            &cmd,
                            &exec_data,
                            commission.commission,
                            &commission.currency,
                            ts_init,
                        ) {
                            reports.push(report);
                        }
                    } else {
                        pending_commissions.insert(
                            commission.execution_id,
                            (commission.commission, commission.currency),
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("Error receiving execution data: {e}");
                }
            }
        }

        anyhow::ensure!(
            pending_exec_data.is_empty(),
            "IB did not provide commission reports for execution IDs: {}",
            pending_exec_data
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        );

        if !combo_executions.is_empty() {
            self.resolve_combo_fill_reports(&cmd, client, combo_executions, &mut reports, ts_init)
                .await?;
        }

        Ok(reports)
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

                    // Convert IB avg_cost to Nautilus Price, accounting for price magnifier and multiplier
                    // Python: converted_avg_cost = avg_cost / (multiplier * price_magnifier)
                    let avg_px_open = self.context.position_avg_px_open(
                        &instrument_id,
                        &instrument,
                        position.average_cost,
                    );

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
                    );

                    reports.push(report);
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
    // order's fill with zero commission, as on the live path, and the combo's leg executions
    // are dropped: they share its order IDs, so they would fill the spread order at leg prices,
    // and startup reconciles leg positions from IB's position reports.
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
        let combo_venue_order_ids: AHashSet<VenueOrderId> = combo_perm_ids
            .iter()
            .map(|perm_id| ib_venue_order_id(0, *perm_id))
            .collect();
        reports.retain(|report| !combo_venue_order_ids.contains(&report.venue_order_id));

        for exec_data in combo_executions {
            let perm_id = exec_data.execution.perm_id;
            let instrument_id = match combo_contracts
                .get(&perm_id)
                .context("IB returned no combo order for the execution")
                .and_then(|contract| self.context.resolve_report_contract_instrument_id(contract))
            {
                Ok(instrument_id) => instrument_id,
                Err(e) => {
                    IbReportContext::warn_historical_fill_report_parse_error(&exec_data, &e);
                    continue;
                }
            };

            if let Some(report) = self.context.build_historical_fill_report(
                cmd,
                &exec_data,
                instrument_id,
                0.0,
                exec_data.contract.currency.as_str(),
                ts_init,
            ) {
                reports.push(report);
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
