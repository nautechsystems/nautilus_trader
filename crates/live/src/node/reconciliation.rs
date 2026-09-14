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

//! Position fill reconciliation for the live node.

use std::time::Duration;

use indexmap::{IndexMap, IndexSet};
use nautilus_common::{
    clients::ExecutionClient, enums::LogLevel, messages::execution::GenerateFillReports,
};
use nautilus_core::{DurationNanos, UUID4};
use nautilus_execution::reconciliation::create_inferred_reconciliation_trade_id;
use nautilus_model::{
    events::OrderEventAny,
    identifiers::{ClientId, ClientOrderId, PositionId},
    orders::{Order, OrderAny},
    position::PositionReplayEvent,
    reports::{FillReport, PositionStatusReport},
    types::{Money, Quantity},
};
use rust_decimal::Decimal;

use crate::execution::manager::{
    ExecutionManager, InstrumentAccountKey, PositionReportCheck, ReportClientCoverage,
    TargetedOrderQuery,
};

impl TargetedOrderQuery {
    pub(crate) const fn client_order_id(&self) -> ClientOrderId {
        self.client_order_id
    }
}

#[derive(Debug)]
pub(crate) struct PositionFillReportQuery {
    pub key: InstrumentAccountKey,
    pub client_id: ClientId,
    pub command: GenerateFillReports,
}

#[derive(Debug)]
pub(crate) struct PositionFillReportPlan {
    pub queries: Vec<PositionFillReportQuery>,
    pub discrepancy_keys: IndexSet<InstrumentAccountKey>,
}

#[derive(Debug)]
pub(crate) enum PositionFillReportPreparation {
    Ready,
    InferredOverlap,
    Unattributed,
}

impl ExecutionManager {
    pub(crate) fn prepare_position_fill_report_plan(
        &mut self,
        check: &mut PositionReportCheck,
        reports: &[PositionStatusReport],
        queried_clients: &IndexSet<ClientId>,
        failed_clients: &IndexSet<ClientId>,
        clients: &[&dyn ExecutionClient],
    ) -> PositionFillReportPlan {
        let mut venue_positions: IndexMap<InstrumentAccountKey, Vec<PositionStatusReport>> =
            IndexMap::new();

        for report in reports {
            if self.should_reconcile_instrument(&report.instrument_id) {
                venue_positions
                    .entry((report.instrument_id, report.account_id))
                    .or_default()
                    .push(report.clone());
            }
        }

        let keys = check
            .client_coverage
            .keys()
            .copied()
            .chain(venue_positions.iter().filter_map(|(key, reports)| {
                reports
                    .iter()
                    .any(|report| report.signed_decimal_qty != Decimal::ZERO)
                    .then_some(*key)
            }))
            .collect::<IndexSet<_>>();

        let active_keys = keys.clone();
        let query_end = self.clock.borrow().timestamp_ns();
        let lookback = DurationNanos::try_from_mins(self.config.position_check_lookback_mins)
            .expect("position lookback validated at construction");
        let query_start = query_end.saturating_sub(lookback);
        let mut discrepancy_keys = IndexSet::new();
        let mut queries = Vec::new();

        for key in keys {
            let coverage = check
                .client_coverage
                .entry(key)
                .or_insert_with(|| Self::resolve_position_report_client_coverage(key, clients));
            let prepared_revision = *check.activity_revisions.entry(key).or_default();
            let venue_reports = venue_positions
                .get(&key)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let comparison = self.position_quantity_comparison(key, venue_reports);
            let tolerance = self.position_reconciliation_tolerance(key.1);

            if comparison.quantities_match(tolerance) {
                self.position_reconciliation_states.shift_remove(&key);
                continue;
            }

            discrepancy_keys.insert(key);

            if self.position_activity_revision(&key) > prepared_revision
                || self.position_local_activity.within(
                    &key,
                    Duration::from(self.config.position_check_threshold_ns),
                )
            {
                continue;
            }

            let report_shape = comparison.report_shape();
            let retries = self
                .position_reconciliation_states
                .get(&key)
                .filter(|state| state.report_shape == report_shape)
                .map_or(0, |state| state.retries);
            if retries >= self.config.position_check_retries {
                continue;
            }

            let ReportClientCoverage::Resolved(responsible_clients) = coverage else {
                log::warn!(
                    "Skipping fill report query for {}/{}: responsible execution client coverage is unavailable",
                    key.0,
                    key.1,
                );
                continue;
            };

            if responsible_clients.is_empty()
                || !responsible_clients.is_subset(queried_clients)
                || !responsible_clients.is_disjoint(failed_clients)
            {
                log::warn!(
                    "Skipping fill report query for {}/{}: responsible position report coverage is incomplete",
                    key.0,
                    key.1,
                );
                continue;
            }

            for client_id in responsible_clients.iter() {
                let mut command = GenerateFillReports::new(
                    UUID4::new(),
                    query_end,
                    Some(key.0),
                    None,
                    Some(query_start),
                    Some(query_end),
                    None,
                    Some(check.command.command_id),
                );
                command.log_receipt_level = LogLevel::Debug;
                queries.push(PositionFillReportQuery {
                    key,
                    client_id: *client_id,
                    command,
                });
            }
        }

        self.position_reconciliation_states
            .retain(|key, _| active_keys.contains(key));

        PositionFillReportPlan {
            queries,
            discrepancy_keys,
        }
    }

    pub(crate) fn position_report_check_key_is_stable(
        &self,
        check: &PositionReportCheck,
        key: &InstrumentAccountKey,
    ) -> bool {
        check
            .activity_revisions
            .get(key)
            .is_some_and(|revision| self.position_activity_revision(key) == *revision)
    }

    pub(crate) fn prepare_position_fill_report(
        &self,
        report: &mut FillReport,
        venue_reports: &[PositionStatusReport],
    ) -> anyhow::Result<PositionFillReportPreparation> {
        let cache = self.cache.borrow();
        let venue_client_order_id = cache.client_order_id(&report.venue_order_id).copied();
        if let (Some(report_client_order_id), Some(venue_client_order_id)) =
            (report.client_order_id, venue_client_order_id)
        {
            anyhow::ensure!(
                report_client_order_id == venue_client_order_id,
                "fill {} client order ID {report_client_order_id} conflicts with venue order mapping {venue_client_order_id}",
                report.trade_id,
            );
        }

        let client_order_id = report.client_order_id.or(venue_client_order_id);
        let order = client_order_id.and_then(|id| cache.order(&id));
        if let Some(order) = &order {
            anyhow::ensure!(
                order.instrument_id() == report.instrument_id
                    && order.order_side() == report.order_side
                    && order
                        .account_id()
                        .is_none_or(|account_id| account_id == report.account_id)
                    && order
                        .venue_order_id()
                        .is_none_or(|venue_order_id| venue_order_id == report.venue_order_id),
                "fill {} conflicts with cached order {}",
                report.trade_id,
                order.client_order_id(),
            );
        }

        let hedge_context = report.venue_position_id.is_some()
            || venue_reports
                .iter()
                .any(|venue_report| venue_report.venue_position_id.is_some());
        let mapped_position_id = client_order_id
            .and_then(|client_order_id| cache.position_id(&client_order_id))
            .copied();

        if hedge_context
            && let (Some(venue_position_id), Some(mapped_position_id)) =
                (report.venue_position_id, mapped_position_id)
        {
            anyhow::ensure!(
                venue_position_id == mapped_position_id,
                "fill {} position ID {venue_position_id} conflicts with cached order position {mapped_position_id}",
                report.trade_id,
            );
        }

        if let Some(order) = order
            && Self::has_active_inferred_fill(&order)?
        {
            return Ok(PositionFillReportPreparation::InferredOverlap);
        }

        if !hedge_context {
            return Ok(PositionFillReportPreparation::Ready);
        }

        if report.venue_position_id.is_some() {
            return Ok(PositionFillReportPreparation::Ready);
        }

        let Some(position_id) = mapped_position_id else {
            return Ok(PositionFillReportPreparation::Unattributed);
        };

        let position = cache.position(&position_id).ok_or_else(|| {
            anyhow::anyhow!(
                "fill {} maps to position {position_id}, which is not cached",
                report.trade_id,
            )
        })?;

        anyhow::ensure!(
            position.account_id == report.account_id
                && position.instrument_id == report.instrument_id,
            "fill {} maps to position {position_id} with a different account or instrument",
            report.trade_id,
        );
        anyhow::ensure!(
            position.is_open(),
            "fill {} maps to non-open position {position_id}",
            report.trade_id,
        );
        anyhow::ensure!(
            !position.is_opposite_side(report.order_side) || report.last_qty <= position.quantity,
            "fill {} without a venue position ID would cross position {position_id}",
            report.trade_id,
        );

        report.venue_position_id = Some(position_id);
        Ok(PositionFillReportPreparation::Ready)
    }

    fn has_active_inferred_fill(order: &OrderAny) -> anyhow::Result<bool> {
        let events = order.events();
        let trade_ids = order.trade_ids();

        let Some((first, remaining)) = events.split_first() else {
            return Ok(false);
        };

        let mut projected = OrderAny::from_events(vec![(*first).clone()]).map_err(|e| {
            anyhow::anyhow!(
                "cannot replay order {} for inferred fill detection: {e}",
                order.client_order_id(),
            )
        })?;

        for event in remaining {
            projected.apply((*event).clone()).map_err(|e| {
                anyhow::anyhow!(
                    "cannot replay order {} for inferred fill detection: {e}",
                    order.client_order_id(),
                )
            })?;

            let OrderEventAny::Filled(fill) = event else {
                continue;
            };

            if !fill.reconciliation || !trade_ids.contains(&&fill.trade_id) {
                continue;
            }

            let external_position_id = PositionId::new(format!("{}-EXTERNAL", fill.instrument_id));
            let position_ids = [fill.position_id, Some(external_position_id)];

            let inferred = position_ids.into_iter().flatten().any(|position_id| {
                create_inferred_reconciliation_trade_id(
                    fill.account_id,
                    fill.instrument_id,
                    fill.client_order_id,
                    Some(fill.venue_order_id),
                    fill.order_side,
                    fill.order_type,
                    projected.filled_qty(),
                    fill.last_qty,
                    fill.last_px,
                    position_id,
                    fill.ts_event,
                ) == fill.trade_id
            });

            if inferred {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub(crate) fn position_contains_fill_report(&self, report: &FillReport) -> bool {
        let cache = self.cache.borrow();
        let client_order_id = report
            .client_order_id
            .or_else(|| cache.client_order_id(&report.venue_order_id).copied());
        let positions = cache.positions(
            None,
            Some(&report.instrument_id),
            None,
            Some(&report.account_id),
            None,
        );
        let mut matched = false;
        let mut quantity = Quantity::zero(report.last_qty.precision);
        let mut commission = Money::zero(report.commission.currency);

        for position in positions {
            if report
                .venue_position_id
                .is_some_and(|position_id| position.id != position_id)
            {
                continue;
            }

            for replay_event in &position.replay_events {
                let PositionReplayEvent::Filled(fill) = replay_event else {
                    continue;
                };

                if fill.account_id != report.account_id
                    || fill.instrument_id != report.instrument_id
                    || fill.venue_order_id != report.venue_order_id
                    || fill.trade_id != report.trade_id
                    || fill.order_side != report.order_side
                    || fill.last_px != report.last_px
                    || fill.liquidity_side != report.liquidity_side
                    || client_order_id.is_some_and(|id| fill.client_order_id != id)
                    || report
                        .venue_position_id
                        .is_some_and(|id| fill.position_id != Some(id))
                {
                    continue;
                }

                let Some(fill_commission) = fill.commission else {
                    return false;
                };

                if fill_commission.currency != report.commission.currency {
                    return false;
                }

                let Some(next_quantity) = quantity.checked_add(fill.last_qty) else {
                    return false;
                };

                let Some(next_commission) = commission.checked_add(fill_commission) else {
                    return false;
                };

                matched = true;
                quantity = next_quantity;
                commission = next_commission;
            }
        }

        matched && quantity == report.last_qty && commission == report.commission
    }

    pub(crate) fn remove_targeted_order_queries(&mut self, client_order_ids: &[ClientOrderId]) {
        for client_order_id in client_order_ids {
            self.targeted_order_queries.shift_remove(client_order_id);
        }
    }
}
