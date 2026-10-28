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
//!
//! Bulk order, position, and authoritative fill stages retain client admission until their receipts
//! are finalized or discarded. Each stage collects all queries, then accepts only if `now < deadline`.
//! Accepted continuations and reconciliation run synchronously, even if finalization crosses
//! the deadline. Shutdown discards unused position snapshots without running their continuations.
//! Discarding a batch skips retained continuations, not earlier inline effects.
//!
//! Targeted queries accept each status and companion fill before its continuation, sharing the
//! original task deadline. The node retains accepted results across later expiry or cancellation.
//! Normal shutdown reconciles them; fatal callback failure discards them without further dispatch.

use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc, time::Duration};

use futures_util::{StreamExt, stream::FuturesUnordered};
use indexmap::{IndexMap, IndexSet};
use nautilus_common::{
    clients::ExecutionClient,
    live::dst,
    messages::{ExecutionEvent, ExecutionReport, execution::GenerateFillReports},
};
use nautilus_core::UUID4;
use nautilus_model::{
    identifiers::{ClientId, ClientOrderId},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
};

use super::{LiveNode, NodeState};
use crate::{
    execution::{
        client::{CollectedReport, LiveExecutionClient, ReportCollector},
        manager::{
            InstrumentAccountKey, OpenOrderReportCheck, PositionFillReportPreparation,
            PositionFillReportQuery, PositionReportCheck, SourcedOrderStatusReport,
            TargetedOrderQuery, TargetedOrderReportResult, collect_targeted_order_reports,
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
        let clients = self.exec_clients.clone();
        let deadline = dst::time::Instant::now() + self.config.timeout_reconciliation;

        Some(OpenOrderReportTask {
            future: Box::pin(collect_report_before_deadline(
                deadline,
                request_open_order_reports(clients, check),
            )),
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

        let carried_results = Rc::new(RefCell::new(Vec::new()));
        let results = carried_results.clone();

        TargetedOrderReportTask {
            future: Box::pin(async move {
                let remaining = deadline.saturating_duration_since(dst::time::Instant::now());

                match dst::time::timeout(
                    remaining,
                    collect_targeted_order_reports(
                        queries,
                        clients,
                        query_delay,
                        deadline,
                        results,
                    ),
                )
                .await
                {
                    Ok(true) => ReportTaskOutcome::Completed(()),
                    Ok(false) | Err(_) => ReportTaskOutcome::TimedOut,
                }
            }),
            planned_client_order_ids,
            carried_results,
        }
    }

    /// Applies accepted results and clears every planned targeted marker.
    pub(super) fn complete_targeted_order_report_check(
        &mut self,
        task: TargetedOrderReportTask,
        outcome: &ReportTaskOutcome<()>,
    ) {
        let (planned_client_order_ids, results) =
            task.into_results(matches!(outcome, ReportTaskOutcome::Completed(())));
        self.cleanup_cancelled_report_tasks(&planned_client_order_ids);
        self.handle_targeted_order_report_result(results);

        if matches!(outcome, ReportTaskOutcome::TimedOut) {
            log::warn!(
                "Targeted order report collection expired after {:?}",
                self.config.timeout_reconciliation
            );
        }
    }

    fn handle_targeted_order_report_result(&mut self, results: Vec<TargetedOrderReportResult>) {
        let client_refs = self
            .exec_clients
            .iter()
            .map(|client| client as &dyn ExecutionClient)
            .collect::<Vec<_>>();
        let events = self
            .exec_manager
            .reconcile_targeted_order_reports(results, &client_refs);
        self.process_reconciliation_events(&events);
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
        let clients = self.exec_clients.clone();
        let deadline = dst::time::Instant::now() + self.config.timeout_reconciliation;

        Some(PositionReportTask {
            future: Box::pin(collect_report_before_deadline(deadline, async move {
                let result = request_position_reports(clients, check).await;
                PositionReportTaskResult::Positions(result)
            })),
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
            future: Box::pin(collect_report_before_deadline(deadline, async move {
                let collection = request_position_fill_reports(clients, queries).await;

                PositionReportTaskResult::Fills {
                    position_result,
                    collection,
                }
            })),
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
            reports,
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

        for (key, reports) in reports {
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

            let contained = self
                .exec_manager
                .position_fill_reports_contained_prefix(&reports);
            for mut report in reports.into_iter().skip(contained) {
                if self.exec_manager.position_activity_revision(&key) != expected_revision {
                    blocked = true;
                    break;
                }

                if self.exec_manager.position_contains_fill_report(&report) {
                    continue;
                }

                if self.exec_manager.is_unapplied_fill_report_expired(&report) {
                    log::warn!(
                        "Ignoring fill {} for {}/{}: not applied within the position check threshold",
                        report.trade_id,
                        key.0,
                        key.1,
                    );
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
                    Ok(PositionFillReportPreparation::SnapshotOverlap) => {
                        log::debug!(
                            "Ignoring fill {} for {}/{} because a reconciled position already includes it",
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
                        self.exec_manager.record_unapplied_fill_report(&report);
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
                    self.exec_manager.record_unapplied_fill_report(&report);
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

    /// Drops pending collections and reconciles accepted targeted results when dispatch is safe.
    pub(super) fn cancel_report_tasks(
        &mut self,
        open_order_report_task: &mut Option<OpenOrderReportTask>,
        targeted_order_report_task: &mut Option<TargetedOrderReportTask>,
        position_report_task: &mut Option<PositionReportTask>,
        reconcile_reports: bool,
    ) {
        drop(open_order_report_task.take());
        let (planned_client_order_ids, carried_results) = targeted_order_report_task
            .take()
            .map(|task| task.into_results(false))
            .unwrap_or_default();
        drop(position_report_task.take());

        for client in &self.exec_clients {
            client.cancel_report_task();
        }

        self.cleanup_cancelled_report_tasks(&planned_client_order_ids);

        if reconcile_reports {
            self.handle_targeted_order_report_result(carried_results);
        }
    }

    pub(super) async fn finish_report_tasks(
        clients: &[LiveExecutionClient],
        errors: &mut Vec<String>,
    ) {
        for client in clients {
            client.cancel_report_task();
        }

        let mut tasks = clients
            .iter()
            .map(|client| async move { (client.client_id(), client.join_report_task().await) })
            .collect::<FuturesUnordered<_>>();

        while let Some((client_id, result)) = tasks.next().await {
            if let Err(e) = result {
                errors.push(format!("{client_id} report collection: {e}"));
            }
        }
    }
}

async fn collect_report_before_deadline<T>(
    deadline: dst::time::Instant,
    collection: impl Future<Output = T>,
) -> ReportTaskOutcome<T> {
    let remaining = deadline.saturating_duration_since(dst::time::Instant::now());
    match dst::time::timeout(remaining, collection).await {
        Ok(result) if dst::time::Instant::now() < deadline => ReportTaskOutcome::Completed(result),
        Ok(_) | Err(_) => ReportTaskOutcome::TimedOut,
    }
}

async fn request_open_order_reports(
    clients: Vec<LiveExecutionClient>,
    check: OpenOrderReportCheck,
) -> OpenOrderReportCollection {
    let mut receipts = Vec::new();
    let mut queried_clients = IndexSet::new();
    let mut failed_clients = IndexSet::new();

    for client in clients {
        let client_id = client.client_id();
        queried_clients.insert(client_id);
        match client.collect_order_status_reports(&check.command).await {
            Ok(receipt) => receipts.push((client_id, receipt)),
            Err(e) => {
                failed_clients.insert(client_id);
                log::warn!(
                    "Failed to generate order status reports from {}: {e}",
                    client.client_id()
                );
            }
        }
    }

    OpenOrderReportCollection {
        check,
        receipts,
        queried_clients,
        failed_clients,
    }
}

async fn request_position_reports(
    clients: Vec<LiveExecutionClient>,
    check: PositionReportCheck,
) -> PositionReportCollection {
    let mut receipts = Vec::new();

    for client in clients {
        let client_id = client.client_id();
        let receipt = client.collect_position_status_reports(&check.command).await;
        receipts.push((client_id, receipt));
    }

    PositionReportCollection { check, receipts }
}

/// Collects fill receipts without running their report-dependent continuations.
pub(super) async fn request_position_fill_reports(
    clients: Vec<LiveExecutionClient>,
    queries: Vec<PositionFillReportQuery>,
) -> PositionFillReportCollection {
    let mut collectors = IndexMap::new();
    let mut receipts = Vec::new();

    for query in queries {
        let receipt = async {
            let collector =
                ReportCollector::for_client(&mut collectors, &clients, query.client_id).await?;
            collector.fill_reports(query.command.clone()).await
        }
        .await;

        receipts.push((query, receipt));
    }

    PositionFillReportCollection { receipts }
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
    Pin<Box<dyn Future<Output = ReportTaskOutcome<OpenOrderReportCollection>>>>;

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

type TargetedOrderReportFuture = Pin<Box<dyn Future<Output = ReportTaskOutcome<()>>>>;

/// Pending targeted collection, accepted results, and order IDs to clear when the task ends.
pub(super) struct TargetedOrderReportTask {
    pub(super) future: TargetedOrderReportFuture,
    pub(super) planned_client_order_ids: Vec<ClientOrderId>,
    carried_results: Rc<RefCell<Vec<TargetedOrderReportResult>>>,
}

impl TargetedOrderReportTask {
    fn into_results(
        self,
        collection_complete: bool,
    ) -> (Vec<ClientOrderId>, Vec<TargetedOrderReportResult>) {
        drop(self.future);
        let mut results = std::mem::take(&mut *self.carried_results.borrow_mut());
        if !collection_complete {
            results.retain(TargetedOrderReportResult::has_report);
        }

        (self.planned_client_order_ids, results)
    }
}

/// Bulk order receipts finalized only after the whole collection is accepted.
pub(super) struct OpenOrderReportCollection {
    check: OpenOrderReportCheck,
    receipts: Vec<(ClientId, CollectedReport<Vec<OrderStatusReport>>)>,
    queried_clients: IndexSet<ClientId>,
    failed_clients: IndexSet<ClientId>,
}

impl OpenOrderReportCollection {
    pub(super) fn finish(self) -> OpenOrderReportResult {
        let mut reports = Vec::new();
        let mut failed_clients = self.failed_clients;

        for (client_id, collected) in self.receipts {
            match collected.finish() {
                Ok(finalized) => reports.extend(
                    finalized
                        .into_iter()
                        .map(|report| SourcedOrderStatusReport { client_id, report }),
                ),
                Err(e) => {
                    failed_clients.insert(client_id);
                    log::warn!("Failed to generate order status reports from {client_id}: {e}");
                }
            }
        }

        OpenOrderReportResult {
            check: self.check,
            reports,
            queried_clients: self.queried_clients,
            failed_clients,
        }
    }
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
    Positions(PositionReportCollection),
    Fills {
        position_result: PositionReportResult,
        collection: PositionFillReportCollection,
    },
}

/// Authoritative fills and the position snapshot that prompted their queries.
///
/// Report keys have complete, consistent query results, including successful empty groups.
pub(super) struct PositionFillReportResult {
    pub(super) position_result: PositionReportResult,
    pub(super) reports: IndexMap<InstrumentAccountKey, Vec<FillReport>>,
}

/// Position receipts retained until the complete snapshot is accepted.
pub(super) struct PositionReportCollection {
    check: PositionReportCheck,
    receipts: Vec<(
        ClientId,
        anyhow::Result<CollectedReport<Vec<PositionStatusReport>>>,
    )>,
}

impl PositionReportCollection {
    pub(super) fn finish(self) -> PositionReportResult {
        let mut reports = Vec::new();
        let mut queried_clients = IndexSet::new();
        let mut failed_clients = IndexSet::new();

        for (client_id, receipt) in self.receipts {
            queried_clients.insert(client_id);

            match receipt.and_then(CollectedReport::finish) {
                Ok(finalized) => reports.extend(finalized),
                Err(e) => {
                    failed_clients.insert(client_id);
                    log::warn!("Failed to generate position status reports from {client_id}: {e}");
                }
            }
        }

        PositionReportResult {
            check: self.check,
            reports,
            queried_clients,
            failed_clients,
        }
    }
}

/// Fill-query receipts finalized only after the whole stage is accepted.
pub(super) struct PositionFillReportCollection {
    receipts: Vec<(
        PositionFillReportQuery,
        anyhow::Result<CollectedReport<Vec<FillReport>>>,
    )>,
}

impl PositionFillReportCollection {
    pub(super) fn finish(self) -> IndexMap<InstrumentAccountKey, Vec<FillReport>> {
        let mut reports_by_key: IndexMap<InstrumentAccountKey, Option<Vec<FillReport>>> =
            IndexMap::new();

        for (query, receipt) in self.receipts {
            let collected = reports_by_key
                .entry(query.key)
                .or_insert_with(|| Some(Vec::new()));

            let command = query.command;

            match receipt.and_then(CollectedReport::finish) {
                Ok(reports)
                    if reports.iter().all(|report| {
                        fill_report_matches_query_scope(report, query.key, &command)
                    }) =>
                {
                    if let Some(collected) = collected {
                        collected.extend(
                            reports
                                .into_iter()
                                .filter(|report| fill_report_in_query_window(report, &command)),
                        );
                    }
                }
                Ok(_) => {
                    *collected = None;
                    log::warn!(
                        "Discarding fill reports for {}/{}: response contained an invalid report",
                        query.key.0,
                        query.key.1,
                    );
                }
                Err(e) => {
                    *collected = None;
                    log::warn!(
                        "Failed to generate fill reports from {} for {}/{}: {e}",
                        query.client_id,
                        query.key.0,
                        query.key.1,
                    );
                }
            }
        }

        let mut finalized = IndexMap::new();

        for (key, collected) in reports_by_key {
            let Some(collected) = collected else {
                continue;
            };

            let Some(reports) = deduplicate_fill_reports(collected) else {
                log::warn!(
                    "Discarding fill reports for {}/{}: response contained contradictory fills",
                    key.0,
                    key.1,
                );
                continue;
            };

            finalized.insert(key, reports);
        }

        finalized
    }
}

fn deduplicate_fill_reports(reports: Vec<FillReport>) -> Option<Vec<FillReport>> {
    let mut deduplicated = IndexMap::new();

    for report in reports {
        let fill_key = (report.account_id, report.instrument_id, report.trade_id);
        if let Some(existing) = deduplicated.get(&fill_key) {
            if !fill_reports_equivalent(existing, &report) {
                return None;
            }
        } else {
            deduplicated.insert(fill_key, report);
        }
    }

    let mut reports = deduplicated.into_values().collect::<Vec<_>>();
    reports.sort_by_key(|report| (report.ts_event, report.trade_id));
    Some(reports)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use futures_util::FutureExt;
    use nautilus_common::{
        clients::ExecutionReportTask, messages::execution::GeneratePositionStatusReports,
    };
    use nautilus_core::{Params, UnixNanos};
    use nautilus_model::{
        accounts::AccountAny,
        enums::{LiquiditySide, OmsType, OrderSide, PositionSide},
        identifiers::{AccountId, InstrumentId, TradeId, Venue, VenueOrderId},
        types::{AccountBalance, MarginBalance, Money, Price, Quantity},
    };
    use proptest::prelude::*;
    use rstest::{fixture, rstest};

    use super::*;

    #[rstest]
    #[case::start(1_000, Some(1_000), Some(2_000), true)]
    #[case::end(2_000, Some(1_000), Some(2_000), true)]
    #[case::before(999, Some(1_000), Some(2_000), false)]
    #[case::after(2_001, Some(1_000), Some(2_000), false)]
    #[case::point(1_000, Some(1_000), Some(1_000), true)]
    #[case::inverted(1_500, Some(2_000), Some(1_000), false)]
    #[case::unbounded_zero(0, None, None, true)]
    #[case::unbounded_max(u64::MAX, None, None, true)]
    #[case::start_only(u64::MAX, Some(u64::MAX), None, true)]
    #[case::end_only(0, None, Some(0), true)]
    fn test_fill_report_window_boundaries(
        #[case] event: u64,
        #[case] start: Option<u64>,
        #[case] end: Option<u64>,
        #[case] expected: bool,
        mut fill_report: FillReport,
    ) {
        fill_report.ts_event = UnixNanos::from(event);

        let command = GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(fill_report.instrument_id),
            None,
            start.map(UnixNanos::from),
            end.map(UnixNanos::from),
            None,
            None,
        );

        assert_eq!(
            fill_report_in_query_window(&fill_report, &command),
            expected
        );
    }

    proptest! {
        #[rstest]
        fn prop_fill_report_window_contains_exactly_inclusive_interval(
            event in any::<u64>(),
            start in proptest::option::of(any::<u64>()),
            end in proptest::option::of(any::<u64>()),
        ) {
            let mut report = fill_report();
            report.ts_event = UnixNanos::from(event);
            let command = GenerateFillReports::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(report.instrument_id),
                None,
                start.map(UnixNanos::from),
                end.map(UnixNanos::from),
                None,
                None,
            );
            let interval = start.unwrap_or(0)..=end.unwrap_or(u64::MAX);

            prop_assert_eq!(fill_report_in_query_window(&report, &command), interval.contains(&event));
        }
    }

    #[fixture]
    fn fill_report() -> FillReport {
        FillReport::new(
            AccountId::from("TEST-001"),
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            VenueOrderId::from("V-1"),
            TradeId::from("T-1"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            Price::from("100.0"),
            Money::from("0.10 USDT"),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1_500),
            UnixNanos::from(2_000),
            None,
        )
    }

    #[rstest]
    #[case::before(-1)]
    #[case::exact(0)]
    #[case::after(1)]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_position_report_acceptance_strict_deadline(
        #[case] offset_ns: i64,
        #[values(false, true)] positions: bool,
        #[values(false, true)] worker: bool,
    ) {
        let mut node = LiveNode::build("PositionDeadline".to_string(), None).unwrap();
        node.config.timeout_reconciliation = Duration::from_secs(1);
        let state = PositionCollectionState::default();
        let release = Arc::new(tokio::sync::Notify::new());

        let client = PositionCollectionClient {
            client_id: ClientId::from("POSITION-DEADLINE"),
            state: state.clone(),
            worker,
            gate_at: 2,
            release: release.clone(),
        };

        node.exec_clients
            .push(LiveExecutionClient::new(Box::new(client)));

        if positions {
            node.exec_clients.push(LiveExecutionClient::new(Box::new(
                PositionCollectionClient {
                    state: state.clone(),
                    worker,
                    gate_at: 2,
                    release: release.clone(),
                    client_id: ClientId::from("POSITION-DEADLINE-2"),
                },
            )));
        }

        let deadline = dst::time::Instant::now() + node.config.timeout_reconciliation;
        let mut task = if positions {
            node.start_position_report_check().unwrap()
        } else {
            node.start_position_fill_report_check(
                empty_position_result(&node),
                vec![position_fill_query(), position_fill_query()],
            )
        };

        let expected_calls = 2;
        while state.started.load(Ordering::SeqCst) < expected_calls {
            assert!(task.future.as_mut().now_or_never().is_none());
            tokio::task::yield_now().await;
        }

        assert_eq!(state.snapshots.get(), 0);
        assert!(state.trades.borrow().is_empty());

        let boundary = if offset_ns < 0 {
            deadline.checked_sub(Duration::from_nanos(1)).unwrap()
        } else {
            deadline + Duration::from_nanos(offset_ns.unsigned_abs())
        };

        advance_position_clock(boundary.saturating_duration_since(dst::time::Instant::now())).await;
        assert_eq!(dst::time::Instant::now(), boundary);
        release.notify_one();
        let outcome = task.future.await;
        if offset_ns < 0 {
            let ReportTaskOutcome::Completed(collection) = outcome else {
                panic!("ready before deadline")
            };

            assert_eq!(state.snapshots.get(), 0);
            assert!(state.trades.borrow().is_empty());

            match collection {
                PositionReportTaskResult::Positions(collection) => {
                    let result = collection.finish();
                    assert_eq!(
                        result.reports,
                        vec![position_snapshot(), position_snapshot()]
                    );
                    assert_eq!(
                        result.queried_clients,
                        IndexSet::from([
                            ClientId::from("POSITION-DEADLINE"),
                            ClientId::from("POSITION-DEADLINE-2")
                        ])
                    );
                    assert!(result.failed_clients.is_empty());
                    assert_eq!(state.snapshots.get(), usize::from(worker));
                }
                PositionReportTaskResult::Fills { collection, .. } => {
                    let result = collection.finish();
                    let query = position_fill_query();
                    assert_eq!(
                        result,
                        IndexMap::from([(query.key, vec![position_trade()])])
                    );
                    assert_eq!(result.keys().copied().collect::<Vec<_>>(), vec![query.key]);
                    assert_eq!(
                        *state.commands.borrow(),
                        vec![query.command.clone(), query.command]
                    );
                    assert_eq!(
                        *state.trades.borrow(),
                        if worker {
                            vec![TradeId::from("T-DEADLINE")]
                        } else {
                            vec![]
                        }
                    );
                }
            }
        } else {
            assert!(matches!(outcome, ReportTaskOutcome::TimedOut));
            assert_eq!(state.snapshots.get(), 0);
            assert!(state.trades.borrow().is_empty());
        }

        let mut errors = Vec::new();
        LiveNode::finish_report_tasks(&node.exec_clients, &mut errors).await;
        assert!(errors.is_empty());
    }

    #[rstest]
    #[case::expired(false)]
    #[case::cancelled(true)]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_position_report_acceptance_rejected_fill_retries_and_filters_replay(
        #[case] cancel: bool,
    ) {
        let mut node = LiveNode::build("PositionRetry".to_string(), None).unwrap();
        node.config.timeout_reconciliation = Duration::from_secs(1);
        let state = PositionCollectionState::default();
        let release = Arc::new(tokio::sync::Notify::new());
        node.exec_clients.push(LiveExecutionClient::new(Box::new(
            PositionCollectionClient {
                client_id: ClientId::from("POSITION-DEADLINE"),
                state: state.clone(),
                worker: true,
                gate_at: 2,
                release,
            },
        )));

        let queries = vec![position_fill_query(), position_fill_query()];
        let mut task = node.start_position_fill_report_check(
            empty_position_result(&node),
            vec![position_fill_query(), position_fill_query()],
        );

        while state.started.load(Ordering::SeqCst) < 2 {
            assert!(task.future.as_mut().now_or_never().is_none());
            tokio::task::yield_now().await;
        }

        if cancel {
            drop(task);
        } else {
            advance_position_clock(Duration::from_secs(1)).await;
            assert!(matches!(task.future.await, ReportTaskOutcome::TimedOut));
        }

        let mut errors = Vec::new();
        LiveNode::finish_report_tasks(&node.exec_clients, &mut errors).await;
        assert!(errors.is_empty());
        assert!(state.trades.borrow().is_empty());
        let retry = request_position_fill_reports(node.exec_clients.clone(), queries)
            .await
            .finish();
        assert_eq!(
            retry,
            IndexMap::from([(position_fill_query().key, vec![position_trade()])])
        );
        assert_eq!(*state.trades.borrow(), vec![TradeId::from("T-DEADLINE")]);
        let replay =
            request_position_fill_reports(node.exec_clients.clone(), vec![position_fill_query()])
                .await
                .finish();
        assert_eq!(
            replay,
            IndexMap::from([(position_fill_query().key, vec![])])
        );
        assert_eq!(
            replay.keys().copied().collect::<Vec<_>>(),
            vec![position_fill_query().key]
        );
        assert_eq!(state.started.load(Ordering::SeqCst), 5);
        assert_eq!(
            *state.commands.borrow(),
            vec![position_fill_query().command; 5]
        );
    }

    #[rstest]
    #[case::first(0)]
    #[case::last(1)]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_position_report_acceptance_keeps_healthy_snapshot_after_finish_error(
        #[case] failed: usize,
    ) {
        let mut node = LiveNode::build("PositionFinishError".to_string(), None).unwrap();
        let states = [
            PositionCollectionState::default(),
            PositionCollectionState::default(),
        ];
        let ids = [
            ClientId::from("POSITION-DEADLINE"),
            ClientId::from("POSITION-DEADLINE-2"),
        ];
        states[failed].finish_error.set(true);
        for (state, client_id) in states.iter().zip(ids) {
            node.exec_clients.push(LiveExecutionClient::new(Box::new(
                PositionCollectionClient {
                    client_id,
                    state: state.clone(),
                    worker: true,
                    gate_at: 0,
                    release: Arc::new(tokio::sync::Notify::new()),
                },
            )));
        }

        let ReportTaskOutcome::Completed(PositionReportTaskResult::Positions(collection)) =
            node.start_position_report_check().unwrap().future.await
        else {
            panic!("collection completes")
        };

        let result = collection.finish();
        assert_eq!(result.reports, vec![position_snapshot()]);
        assert_eq!(result.queried_clients, IndexSet::from(ids));
        assert_eq!(result.failed_clients, IndexSet::from([ids[failed]]));
        assert_eq!(
            states
                .iter()
                .map(|state| state.snapshots.get())
                .collect::<Vec<_>>(),
            vec![1, 1]
        );
    }

    #[cfg_attr(
        all(feature = "simulation", madsim),
        expect(
            clippy::unused_async,
            reason = "the Tokio clock requires an awaitable advance operation"
        )
    )]
    async fn advance_position_clock(duration: Duration) {
        #[cfg(all(feature = "simulation", madsim))]
        madsim::time::advance(duration);
        #[cfg(not(all(feature = "simulation", madsim)))]
        tokio::time::advance(duration).await;
    }

    fn empty_position_result(node: &LiveNode) -> PositionReportResult {
        PositionReportResult {
            check: node
                .exec_manager
                .prepare_position_report_check(UUID4::new(), &[]),
            reports: vec![],
            queried_clients: IndexSet::new(),
            failed_clients: IndexSet::new(),
        }
    }

    fn position_fill_query() -> PositionFillReportQuery {
        let report = position_trade();

        PositionFillReportQuery {
            key: (report.instrument_id, report.account_id),
            client_id: ClientId::from("POSITION-DEADLINE"),
            command: GenerateFillReports::new(
                UUID4::from("00000000-0000-4000-8000-000000000001"),
                UnixNanos::from(100),
                Some(report.instrument_id),
                None,
                Some(UnixNanos::from(1)),
                Some(UnixNanos::from(100)),
                None,
                None,
            ),
        }
    }

    fn position_trade() -> FillReport {
        FillReport::new(
            AccountId::from("POSITION-DEADLINE-001"),
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            VenueOrderId::from("V-DEADLINE"),
            TradeId::from("T-DEADLINE"),
            OrderSide::Buy,
            Quantity::from("2.0"),
            Price::from("103.0"),
            Money::from("0.3 USDT"),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(40),
            UnixNanos::from(50),
            Some(UUID4::from("00000000-0000-4000-8000-000000000002")),
        )
    }

    fn position_snapshot() -> PositionStatusReport {
        PositionStatusReport::new(
            AccountId::from("POSITION-DEADLINE-001"),
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            PositionSide::Long,
            Quantity::from("2.0"),
            UnixNanos::from(50),
            UnixNanos::from(60),
            Some(UUID4::from("00000000-0000-4000-8000-000000000003")),
            None,
            None,
        )
    }

    #[derive(Clone, Default)]
    struct PositionCollectionState {
        finish_error: Rc<Cell<bool>>,
        started: Arc<AtomicUsize>,
        snapshots: Rc<Cell<usize>>,
        trades: Rc<RefCell<Vec<TradeId>>>,
        commands: Rc<RefCell<Vec<GenerateFillReports>>>,
    }

    struct PositionCollectionClient {
        client_id: ClientId,
        state: PositionCollectionState,
        worker: bool,
        gate_at: usize,
        release: Arc<tokio::sync::Notify>,
    }

    impl PositionCollectionClient {
        fn collection<T: Send + 'static>(
            &self,
            value: T,
        ) -> impl Future<Output = anyhow::Result<T>> + Send + 'static {
            let started = self.state.started.clone();
            let gate_at = self.gate_at;
            let release = self.release.clone();
            async move {
                let count = started.fetch_add(1, Ordering::SeqCst) + 1;
                if count == gate_at {
                    release.notified().await;
                }

                Ok(value)
            }
        }
    }

    #[async_trait::async_trait(?Send)]
    impl ExecutionClient for PositionCollectionClient {
        fn is_connected(&self) -> bool {
            true
        }

        fn client_id(&self) -> ClientId {
            self.client_id
        }

        fn account_id(&self) -> AccountId {
            AccountId::from("POSITION-DEADLINE-001")
        }

        fn venue(&self) -> Venue {
            position_trade().instrument_id.venue
        }

        fn oms_type(&self) -> OmsType {
            OmsType::Hedging
        }

        fn get_account(&self) -> Option<AccountAny> {
            None
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn generate_account_state(
            &self,
            _: Vec<AccountBalance>,
            _: Vec<MarginBalance>,
            _: bool,
            _: UnixNanos,
            _: Option<Params>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn generate_position_status_reports_task(
            &self,
            _: &GeneratePositionStatusReports,
        ) -> Option<ExecutionReportTask<Vec<PositionStatusReport>>> {
            (self.worker && self.state.started.load(Ordering::SeqCst) + 1 != self.gate_at).then(
                || {
                    let snapshots = self.state.snapshots.clone();
                    let finish_error = self.state.finish_error.clone();

                    ExecutionReportTask::new(
                        self.collection(vec![position_snapshot()]),
                        move |reports| {
                            snapshots.set(snapshots.get() + 1);
                            anyhow::ensure!(!finish_error.get(), "position finalization failed");
                            Ok(reports)
                        },
                    )
                },
            )
        }

        async fn generate_position_status_reports(
            &self,
            _: &GeneratePositionStatusReports,
        ) -> anyhow::Result<Vec<PositionStatusReport>> {
            self.collection(vec![position_snapshot()]).await
        }

        fn generate_fill_reports_task(
            &self,
            cmd: &GenerateFillReports,
        ) -> Option<ExecutionReportTask<Vec<FillReport>>> {
            (self.worker && self.state.started.load(Ordering::SeqCst) + 1 != self.gate_at).then(
                || {
                    self.state.commands.borrow_mut().push(cmd.clone());
                    let trades = self.state.trades.clone();

                    ExecutionReportTask::new(
                        self.collection(vec![position_trade()]),
                        move |mut reports| {
                            reports.retain(|report| {
                                let mut trades = trades.borrow_mut();
                                if trades.contains(&report.trade_id) {
                                    false
                                } else {
                                    trades.push(report.trade_id);
                                    true
                                }
                            });

                            Ok(reports)
                        },
                    )
                },
            )
        }

        async fn generate_fill_reports(
            &self,
            cmd: GenerateFillReports,
        ) -> anyhow::Result<Vec<FillReport>> {
            self.state.commands.borrow_mut().push(cmd);
            self.collection(vec![position_trade()]).await
        }
    }
}
