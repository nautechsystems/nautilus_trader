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

//! Recurring reconciliation tasks and event dispatch for the live node.
//!
//! The node schedules order and position checks, owns collection deadlines and cancellation,
//! and sequences authoritative fills before synthetic fallback. It rechecks manager activity
//! state around requests and dispatch because local callbacks can invalidate prepared work.
//! [`ExecutionManager`](crate::execution::manager::ExecutionManager) owns cache-dependent
//! validation and individual reconciliation operations. This module owns recurring task lifecycles
//! and leaves activity and retry state with the manager.

use std::{future::Future, pin::Pin, time::Duration};

use indexmap::{IndexMap, IndexSet};
use nautilus_common::{
    clients::ExecutionClient,
    live::dst,
    messages::{
        ExecutionEvent, ExecutionReport,
        execution::{
            GenerateFillReports, GenerateOrderStatusReports, GeneratePositionStatusReports,
        },
    },
};
use nautilus_core::UUID4;
use nautilus_model::{
    identifiers::{ClientId, ClientOrderId},
    reports::{FillReport, PositionStatusReport},
};

use super::{LiveNode, NodeState};
use crate::{
    execution::{
        client::LiveExecutionClient,
        manager::{
            InstrumentAccountKey, OpenOrderReportCheck, PositionFillReportPreparation,
            PositionFillReportQuery, PositionReportCheck, SourcedOrderStatusReport,
            TargetedOrderQuery, TargetedOrderReportResult, request_targeted_order_reports,
        },
    },
    runner::AsyncRunner,
};

const POSITION_FILLS_PER_CYCLE: usize = 64;

impl LiveNode {
    /// Runs due checks while serializing order and position reconciliation.
    pub(super) fn run_reconciliation_checks(
        &mut self,
        now: dst::time::Instant,
        intervals: ReconciliationCheckIntervals,
        state: &mut ReconciliationCheckState<'_>,
    ) {
        if reconciliation_check_due(now, *state.last_inflight_check, intervals.inflight) {
            if self.state() == NodeState::ShuttingDown {
                return;
            }

            let result = self.exec_manager.check_inflight_orders();
            self.process_reconciliation_events(&result.events);
            for cmd in result.queries {
                AsyncRunner::handle_exec_command(cmd);
            }

            *state.last_inflight_check = now;
        }

        let open_due = reconciliation_check_due(now, *state.last_open_check, intervals.open);
        let position_due =
            reconciliation_check_due(now, *state.last_position_check, intervals.position);

        if (open_due || position_due) && self.state() == NodeState::ShuttingDown {
            return;
        }

        if state.open_order_report_task.is_some() || state.targeted_order_report_task.is_some() {
            if open_due {
                log::debug!("Open-order reconciliation already in progress");
                *state.last_open_check = now;
            }

            if position_due {
                log::debug!(
                    "Position reconciliation delayed: open-order reconciliation in progress"
                );
            }

            return;
        }

        if state.position_report_task.is_some() {
            if position_due {
                log::debug!("Position reconciliation already in progress");
                *state.last_position_check = now;
            }

            if open_due {
                log::debug!(
                    "Open-order reconciliation delayed: position reconciliation in progress"
                );
            }

            return;
        }

        if position_due && (!open_due || *state.last_position_check < *state.last_open_check) {
            *state.position_report_task = self.start_position_report_check();
            *state.last_position_check = now;
        } else if open_due {
            *state.open_order_report_task = self.start_open_order_report_check();
            *state.last_open_check = now;
        }
    }

    fn start_open_order_report_check(&mut self) -> Option<OpenOrderReportTask> {
        if self.exec_clients.is_empty() {
            log::debug!("No execution clients to check orders consistency");
            return None;
        }

        let client_refs = self
            .exec_clients
            .iter()
            .map(|client| client as &dyn ExecutionClient)
            .collect::<Vec<_>>();
        let check = self
            .exec_manager
            .prepare_open_order_report_check(UUID4::new(), &client_refs);
        let command = check.command.clone();
        let clients = self.exec_clients.clone();
        let deadline = dst::time::Instant::now() + self.config.timeout_reconciliation;

        Some(OpenOrderReportTask {
            future: Box::pin(async move {
                let remaining = deadline.saturating_duration_since(dst::time::Instant::now());
                match dst::time::timeout(remaining, request_open_order_reports(clients, command))
                    .await
                {
                    Ok(result) => ReportTaskOutcome::Completed(OpenOrderReportResult {
                        check,
                        reports: result.reports,
                        queried_clients: result.queried_clients,
                        failed_clients: result.failed_clients,
                    }),
                    Err(_) => ReportTaskOutcome::TimedOut,
                }
            }),
        })
    }

    /// Starts targeted queries and retains their order IDs for cancellation.
    pub(super) fn start_targeted_order_report_check(
        &self,
        queries: Vec<TargetedOrderQuery>,
    ) -> TargetedOrderReportTask {
        let clients = self.exec_clients.clone();
        let query_delay = Duration::from_millis(u64::from(
            self.config.exec_engine.single_order_query_delay_ms,
        ));
        let planned_client_order_ids = queries
            .iter()
            .map(TargetedOrderQuery::client_order_id)
            .collect();
        let deadline = dst::time::Instant::now() + self.config.timeout_reconciliation;

        TargetedOrderReportTask {
            future: Box::pin(async move {
                let client_refs = clients
                    .iter()
                    .map(|client| client as &dyn ExecutionClient)
                    .collect::<Vec<_>>();
                let remaining = deadline.saturating_duration_since(dst::time::Instant::now());
                match dst::time::timeout(
                    remaining,
                    request_targeted_order_reports(queries, &client_refs, query_delay),
                )
                .await
                {
                    Ok(result) => ReportTaskOutcome::Completed(result),
                    Err(_) => ReportTaskOutcome::TimedOut,
                }
            }),
            planned_client_order_ids,
        }
    }

    fn start_position_report_check(&self) -> Option<PositionReportTask> {
        if self.exec_clients.is_empty() {
            log::debug!("No execution clients to check positions consistency");
            return None;
        }

        let client_refs = self
            .exec_clients
            .iter()
            .map(|client| client as &dyn ExecutionClient)
            .collect::<Vec<_>>();
        let check = self
            .exec_manager
            .prepare_position_report_check(UUID4::new(), &client_refs);
        let command = check.command.clone();
        let clients = self.exec_clients.clone();
        let deadline = dst::time::Instant::now() + self.config.timeout_reconciliation;

        Some(PositionReportTask {
            future: Box::pin(async move {
                let remaining = deadline.saturating_duration_since(dst::time::Instant::now());
                match dst::time::timeout(remaining, request_position_reports(clients, command))
                    .await
                {
                    Ok(result) => ReportTaskOutcome::Completed(
                        PositionReportTaskResult::Positions(PositionReportResult {
                            check,
                            reports: result.reports,
                            queried_clients: result.queried_clients,
                            failed_clients: result.failed_clients,
                        }),
                    ),
                    Err(_) => ReportTaskOutcome::TimedOut,
                }
            }),
        })
    }

    fn start_position_fill_report_check(
        &self,
        position_result: PositionReportResult,
        queries: Vec<PositionFillReportQuery>,
    ) -> PositionReportTask {
        let clients = self.exec_clients.clone();
        let deadline = dst::time::Instant::now() + self.config.timeout_reconciliation;

        PositionReportTask {
            future: Box::pin(async move {
                let remaining = deadline.saturating_duration_since(dst::time::Instant::now());
                match dst::time::timeout(remaining, request_position_fill_reports(clients, queries))
                    .await
                {
                    Ok(result) => ReportTaskOutcome::Completed(PositionReportTaskResult::Fills(
                        PositionFillReportResult {
                            position_result,
                            reports: result.reports,
                            successful_keys: result.successful_keys,
                        },
                    )),
                    Err(_) => ReportTaskOutcome::TimedOut,
                }
            }),
        }
    }

    /// Plans authoritative fill queries for reported position discrepancies.
    pub(super) fn handle_position_report_result(
        &mut self,
        mut result: PositionReportResult,
    ) -> Option<PositionReportTask> {
        let client_refs = self
            .exec_clients
            .iter()
            .map(|client| client as &dyn ExecutionClient)
            .collect::<Vec<_>>();
        let plan = self.exec_manager.plan_position_fill_reports(
            &mut result.check,
            &result.reports,
            &result.queried_clients,
            &result.failed_clients,
            &client_refs,
        );

        if plan.queries.is_empty() {
            if !plan.discrepancy_keys.is_empty() {
                log::debug!(
                    "Position discrepancies remain deferred because no authoritative fill query is currently safe"
                );
            }

            return None;
        }

        Some(self.start_position_fill_report_check(result, plan.queries))
    }

    /// Applies authoritative fills before considering synthetic reconciliation.
    pub(super) fn handle_position_fill_report_result(&mut self, result: PositionFillReportResult) {
        let PositionFillReportResult {
            mut position_result,
            mut reports,
            successful_keys,
        } = result;
        let mut venue_reports = IndexMap::new();
        for report in &position_result.reports {
            venue_reports
                .entry((report.instrument_id, report.account_id))
                .or_insert_with(Vec::new)
                .push(report.clone());
        }

        let mut fallback_keys = IndexSet::new();
        let mut dispatches = 0;

        for key in successful_keys {
            if !self
                .exec_manager
                .position_report_check_is_current(&position_result.check, &key)
            {
                log::debug!(
                    "Deferring position reconciliation for {}/{}: local activity occurred before fill reports were applied",
                    key.0,
                    key.1,
                );
                continue;
            }

            let mut expected_revision = self.exec_manager.position_activity_revision(&key);
            let key_venue_reports = venue_reports
                .get(&key)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let mut applied_fill = false;
            let mut blocked = false;

            for mut report in reports.shift_remove(&key).unwrap_or_default() {
                if self.exec_manager.position_activity_revision(&key) != expected_revision {
                    blocked = true;
                    break;
                }

                if self.exec_manager.position_contains_fill_report(&report) {
                    continue;
                }

                if dispatches >= POSITION_FILLS_PER_CYCLE {
                    log::warn!(
                        "Deferring remaining authoritative fills after reaching the per-cycle dispatch limit"
                    );
                    blocked = true;
                    break;
                }

                match self
                    .exec_manager
                    .prepare_position_fill_report(&mut report, key_venue_reports)
                {
                    Ok(PositionFillReportPreparation::Ready) => {}
                    Ok(PositionFillReportPreparation::InferredOverlap) => {
                        log::debug!(
                            "Ignoring fill {} for {}/{} because its order contains an active inferred fill",
                            report.trade_id,
                            key.0,
                            key.1,
                        );
                        continue;
                    }
                    Ok(PositionFillReportPreparation::Unattributed) => {
                        log::debug!(
                            "Ignoring unattributable hedge fill {} for {}/{} before synthetic fallback",
                            report.trade_id,
                            key.0,
                            key.1,
                        );
                        continue;
                    }
                    Err(e) => {
                        log::warn!(
                            "Deferring fill {} for {}/{}: {e}",
                            report.trade_id,
                            key.0,
                            key.1,
                        );
                        blocked = true;
                        break;
                    }
                }

                if self.exec_manager.position_activity_revision(&key) != expected_revision {
                    blocked = true;
                    break;
                }

                self.process_exec_event(ExecutionEvent::Report(ExecutionReport::Fill(Box::new(
                    report.clone(),
                ))));
                dispatches += 1;
                let next_revision = expected_revision.saturating_add(1);
                if self.exec_manager.position_activity_revision(&key) != next_revision
                    || !self.exec_manager.position_contains_fill_report(&report)
                {
                    log::warn!(
                        "Deferring position reconciliation for {}/{}: authoritative fill {} was not applied exactly",
                        key.0,
                        key.1,
                        report.trade_id,
                    );
                    blocked = true;
                    break;
                }

                expected_revision = next_revision;
                applied_fill = true;
            }

            if self.exec_manager.position_activity_revision(&key) != expected_revision {
                blocked = true;
            }

            if !blocked && !applied_fill {
                fallback_keys.insert(key);
            } else if !blocked {
                log::debug!(
                    "Deferring synthetic position reconciliation for {}/{} until the next fresh position report after applying authoritative fills",
                    key.0,
                    key.1,
                );
            }
        }

        if fallback_keys.is_empty() {
            return;
        }

        position_result.retain_keys(&fallback_keys);
        let events = self.exec_manager.reconcile_position_reports(
            &position_result.check,
            position_result.reports,
            &position_result.queried_clients,
            &position_result.failed_clients,
        );
        self.process_reconciliation_events(&events);
    }

    fn flush_pending_exec_client_instruments(&self) {
        for client in &self.exec_clients {
            client.flush_pending_instruments();
        }
    }

    /// Flushes deferred instruments and clears cancelled targeted queries.
    pub(super) fn cleanup_cancelled_report_tasks(
        &mut self,
        planned_client_order_ids: &[ClientOrderId],
    ) {
        self.flush_pending_exec_client_instruments();
        self.exec_manager
            .remove_targeted_order_queries(planned_client_order_ids);
    }

    /// Drops report futures before releasing their deferred client work.
    pub(super) fn cancel_report_tasks(
        &mut self,
        open_order_report_task: &mut Option<OpenOrderReportTask>,
        targeted_order_report_task: &mut Option<TargetedOrderReportTask>,
        position_report_task: &mut Option<PositionReportTask>,
    ) {
        let planned_client_order_ids = targeted_order_report_task
            .as_ref()
            .map(|task| task.planned_client_order_ids.clone())
            .unwrap_or_default();

        drop(open_order_report_task.take());
        drop(targeted_order_report_task.take());
        drop(position_report_task.take());
        self.cleanup_cancelled_report_tasks(&planned_client_order_ids);
    }
}

async fn request_open_order_reports(
    clients: Vec<LiveExecutionClient>,
    command: GenerateOrderStatusReports,
) -> OpenOrderReportQueryResult {
    let mut all_reports = Vec::new();
    let mut queried_clients = IndexSet::new();
    let mut failed_clients = IndexSet::new();

    for client in clients {
        let client_id = client.client_id();
        queried_clients.insert(client_id);

        match client.generate_order_status_reports(&command).await {
            Ok(reports) => {
                all_reports.extend(
                    reports
                        .into_iter()
                        .map(|report| SourcedOrderStatusReport { client_id, report }),
                );
            }
            Err(e) => {
                failed_clients.insert(client_id);
                log::warn!(
                    "Failed to generate order status reports from {}: {e}",
                    client.client_id()
                );
            }
        }
    }

    OpenOrderReportQueryResult {
        reports: all_reports,
        queried_clients,
        failed_clients,
    }
}

async fn request_position_reports(
    clients: Vec<LiveExecutionClient>,
    command: GeneratePositionStatusReports,
) -> PositionReportQueryResult {
    let mut all_reports = Vec::new();
    let mut queried_clients = IndexSet::new();
    let mut failed_clients = IndexSet::new();

    for client in clients {
        let client_id = client.client_id();
        queried_clients.insert(client_id);

        match client.generate_position_status_reports(&command).await {
            Ok(reports) => {
                all_reports.extend(reports);
            }
            Err(e) => {
                failed_clients.insert(client_id);
                log::warn!(
                    "Failed to generate position status reports from {}: {e}",
                    client.client_id()
                );
            }
        }
    }

    PositionReportQueryResult {
        reports: all_reports,
        queried_clients,
        failed_clients,
    }
}

/// Collects scoped fill reports, rejecting failed or contradictory groups.
pub(super) async fn request_position_fill_reports(
    clients: Vec<LiveExecutionClient>,
    queries: Vec<PositionFillReportQuery>,
) -> PositionFillReportQueryResult {
    let mut reports_by_key: IndexMap<InstrumentAccountKey, Vec<FillReport>> = IndexMap::new();
    let mut queried_keys = IndexSet::new();
    let mut failed_keys = IndexSet::new();

    for query in queries {
        queried_keys.insert(query.key);

        let Some(client) = clients
            .iter()
            .find(|client| client.client_id() == query.client_id)
        else {
            failed_keys.insert(query.key);
            log::warn!(
                "Failed to generate fill reports for {}/{}: execution client {} is unavailable",
                query.key.0,
                query.key.1,
                query.client_id,
            );
            continue;
        };

        let command = query.command;
        match client.generate_fill_reports(command.clone()).await {
            Ok(reports)
                if reports
                    .iter()
                    .all(|report| fill_report_matches_query_scope(report, query.key, &command)) =>
            {
                reports_by_key.entry(query.key).or_default().extend(
                    reports
                        .into_iter()
                        .filter(|report| fill_report_in_query_window(report, &command)),
                );
            }
            Ok(_) => {
                failed_keys.insert(query.key);
                log::warn!(
                    "Discarding fill reports for {}/{}: response contained an invalid report",
                    query.key.0,
                    query.key.1,
                );
            }
            Err(e) => {
                failed_keys.insert(query.key);
                log::warn!(
                    "Failed to generate fill reports from {} for {}/{}: {e}",
                    query.client_id,
                    query.key.0,
                    query.key.1,
                );
            }
        }
    }

    let mut successful_keys = IndexSet::new();

    for key in queried_keys {
        if failed_keys.contains(&key) {
            reports_by_key.shift_remove(&key);
            continue;
        }

        let mut deduplicated = IndexMap::new();
        let mut contradictory = false;

        for report in reports_by_key.shift_remove(&key).unwrap_or_default() {
            let fill_key = (report.account_id, report.instrument_id, report.trade_id);
            if let Some(existing) = deduplicated.get(&fill_key) {
                if !fill_reports_equivalent(existing, &report) {
                    contradictory = true;
                    break;
                }
            } else {
                deduplicated.insert(fill_key, report);
            }
        }

        let mut reports = deduplicated.into_values().collect::<Vec<_>>();
        reports.sort_by_key(|report| (report.ts_event, report.trade_id));

        if contradictory {
            log::warn!(
                "Discarding fill reports for {}/{}: response contained contradictory fills",
                key.0,
                key.1,
            );
            continue;
        }

        successful_keys.insert(key);
        reports_by_key.insert(key, reports);
    }

    PositionFillReportQueryResult {
        reports: reports_by_key,
        successful_keys,
    }
}

fn fill_report_matches_query_scope(
    report: &FillReport,
    key: InstrumentAccountKey,
    command: &GenerateFillReports,
) -> bool {
    report.instrument_id == key.0
        && report.account_id == key.1
        && command.instrument_id == Some(key.0)
        && command
            .venue_order_id
            .is_none_or(|venue_order_id| report.venue_order_id == venue_order_id)
        && !report.last_qty.is_zero()
}

fn fill_report_in_query_window(report: &FillReport, command: &GenerateFillReports) -> bool {
    command.start.is_none_or(|start| report.ts_event >= start)
        && command.end.is_none_or(|end| report.ts_event <= end)
}

fn fill_reports_equivalent(left: &FillReport, right: &FillReport) -> bool {
    left.account_id == right.account_id
        && left.instrument_id == right.instrument_id
        && left.venue_order_id == right.venue_order_id
        && left.trade_id == right.trade_id
        && left.order_side == right.order_side
        && left.last_qty == right.last_qty
        && left.last_px == right.last_px
        && left.commission == right.commission
        && left.liquidity_side == right.liquidity_side
        && left.avg_px == right.avg_px
        && left.ts_event == right.ts_event
        && left.client_order_id == right.client_order_id
        && left.venue_position_id == right.venue_position_id
}

/// Checks whether an enabled interval has elapsed on the monotonic clock.
pub(super) fn reconciliation_check_due(
    now: dst::time::Instant,
    last: dst::time::Instant,
    interval: Duration,
) -> bool {
    interval > Duration::ZERO
        && now
            .checked_duration_since(last)
            .is_some_and(|elapsed| elapsed >= interval)
}

/// Polling intervals for inflight orders, open orders, and positions.
#[derive(Clone, Copy)]
pub(super) struct ReconciliationCheckIntervals {
    pub(super) inflight: Duration,
    pub(super) open: Duration,
    pub(super) position: Duration,
}

/// Last check instants and report tasks owned by the node loop.
pub(super) struct ReconciliationCheckState<'a> {
    pub(super) last_inflight_check: &'a mut dst::time::Instant,
    pub(super) last_open_check: &'a mut dst::time::Instant,
    pub(super) last_position_check: &'a mut dst::time::Instant,
    pub(super) open_order_report_task: &'a mut Option<OpenOrderReportTask>,
    pub(super) targeted_order_report_task: &'a mut Option<TargetedOrderReportTask>,
    pub(super) position_report_task: &'a mut Option<PositionReportTask>,
}

/// Report completion or expiry of its collection deadline.
pub(super) enum ReportTaskOutcome<T> {
    Completed(T),
    TimedOut,
}

type OpenOrderReportFuture =
    Pin<Box<dyn Future<Output = ReportTaskOutcome<OpenOrderReportResult>>>>;

/// Pending bulk order reports and their preparation snapshot.
pub(super) struct OpenOrderReportTask {
    pub(super) future: OpenOrderReportFuture,
}

/// Bulk order reports, client outcomes, and their preparation snapshot.
pub(super) struct OpenOrderReportResult {
    pub(super) check: OpenOrderReportCheck,
    pub(super) reports: Vec<SourcedOrderStatusReport>,
    pub(super) queried_clients: IndexSet<ClientId>,
    pub(super) failed_clients: IndexSet<ClientId>,
}

type TargetedOrderReportFuture =
    Pin<Box<dyn Future<Output = ReportTaskOutcome<Vec<TargetedOrderReportResult>>>>>;

/// Pending targeted reports and order IDs to clear on cancellation.
pub(super) struct TargetedOrderReportTask {
    pub(super) future: TargetedOrderReportFuture,
    pub(super) planned_client_order_ids: Vec<ClientOrderId>,
}

struct OpenOrderReportQueryResult {
    reports: Vec<SourcedOrderStatusReport>,
    queried_clients: IndexSet<ClientId>,
    failed_clients: IndexSet<ClientId>,
}

type PositionReportFuture =
    Pin<Box<dyn Future<Output = ReportTaskOutcome<PositionReportTaskResult>>>>;

/// Pending position reports or their subsequent authoritative fill queries.
pub(super) struct PositionReportTask {
    pub(super) future: PositionReportFuture,
}

/// Position reports, client outcomes, and their preparation snapshot.
pub(super) struct PositionReportResult {
    pub(super) check: PositionReportCheck,
    pub(super) reports: Vec<PositionStatusReport>,
    pub(super) queried_clients: IndexSet<ClientId>,
    pub(super) failed_clients: IndexSet<ClientId>,
}

impl PositionReportResult {
    fn retain_keys(&mut self, keys: &IndexSet<InstrumentAccountKey>) {
        self.check
            .client_coverage
            .retain(|key, _| keys.contains(key));
        self.check
            .activity_revisions
            .retain(|key, _| keys.contains(key));
        self.reports
            .retain(|report| keys.contains(&(report.instrument_id, report.account_id)));
    }
}

/// Completed position reports or subsequent authoritative fill reports.
pub(super) enum PositionReportTaskResult {
    Positions(PositionReportResult),
    Fills(PositionFillReportResult),
}

/// Authoritative fills and the position snapshot that prompted their queries.
pub(super) struct PositionFillReportResult {
    pub(super) position_result: PositionReportResult,
    pub(super) reports: IndexMap<InstrumentAccountKey, Vec<FillReport>>,
    pub(super) successful_keys: IndexSet<InstrumentAccountKey>,
}

struct PositionReportQueryResult {
    reports: Vec<PositionStatusReport>,
    queried_clients: IndexSet<ClientId>,
    failed_clients: IndexSet<ClientId>,
}

/// Fill reports grouped by keys with complete, consistent query results.
pub(super) struct PositionFillReportQueryResult {
    pub(super) reports: IndexMap<InstrumentAccountKey, Vec<FillReport>>,
    pub(super) successful_keys: IndexSet<InstrumentAccountKey>,
}
