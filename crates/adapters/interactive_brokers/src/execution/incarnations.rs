// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Tracks distinct broker orders sharing a Nautilus order reference.

use super::{core::*, parse::parse_order_data_to_report};
use crate::common::enums::IbAction;

#[derive(Clone, Debug)]
pub(super) struct BrokerIncarnation {
    pub(super) data: Option<ibapi::orders::OrderData>,
    pub(super) route: Option<(i32, i32)>,
    pub(super) terminal: bool,
    pub(super) seen: bool,
    pub(super) route_conflict: bool,
}

#[derive(Clone, Debug)]
pub(super) struct OrderIncarnationGroup {
    pub(super) parent: TrackedOrder,
    pub(super) account_id: AccountId,
    pub(super) primary_order_id: i32,
    pub(super) members: AHashMap<i64, BrokerIncarnation>,
}

impl OrderTrackerState {
    pub(super) fn observe_order_binding(&mut self, binding: &ibapi::orders::OrderBound) {
        if let Some(parent) = self.incarnation_parents.get(&binding.perm_id).copied() {
            if let Some(member) = self
                .groups
                .get_mut(&parent)
                .and_then(|group| group.members.get_mut(&binding.perm_id))
            {
                member.route = Some((binding.client_id, binding.order_id));
                member.route_conflict = false;
                member.seen = !member.terminal;
            }
        }
    }

    pub(super) fn group_for_client(&self, client_order_id: ClientOrderId) -> Option<ClientOrderId> {
        if self.groups.contains_key(&client_order_id) {
            return Some(client_order_id);
        }
        self.groups.iter().find_map(|(parent, group)| {
            group
                .members
                .keys()
                .any(|perm_id| {
                    ClientOrderId::for_duplicate_order(
                        group.account_id,
                        parse::ib_venue_order_id(0, *perm_id),
                    )
                    .ok()
                        == Some(client_order_id)
                })
                .then_some(*parent)
        })
    }

    pub(super) fn incarnation_data(&self, perm_id: i64) -> Option<&ibapi::orders::OrderData> {
        let parent = self.incarnation_parents.get(&perm_id)?;
        self.groups
            .get(parent)
            .or_else(|| self.group_history.get(parent))?
            .members
            .get(&perm_id)?
            .data
            .as_ref()
    }

    pub(super) fn group_cancel_candidates(
        &self,
        instrument_id: InstrumentId,
        account_id: AccountId,
        order_side: Option<OrderSide>,
    ) -> impl Iterator<Item = ClientOrderId> + '_ {
        self.groups
            .iter()
            .filter(move |(_, group)| {
                group.parent.instrument_id == instrument_id
                    && group.account_id == account_id
                    && order_side.is_none_or(|side| side == group.parent.order_side)
                    && group.members.values().any(|member| !member.terminal)
            })
            .map(|(parent, _)| *parent)
    }

    pub(super) fn group_context(&self, perm_id: i64) -> Option<(i32, TrackedOrder, AccountId)> {
        let parent = self.incarnation_parents.get(&perm_id)?;
        let group = self
            .groups
            .get(parent)
            .or_else(|| self.group_history.get(parent))?;
        Some((
            group.primary_order_id,
            group.parent.clone(),
            group.account_id,
        ))
    }

    pub(super) fn record_incarnation(
        &mut self,
        primary_order_id: i32,
        parent: &TrackedOrder,
        account_id: AccountId,
        perm_id: i64,
        data: Option<&ibapi::orders::OrderData>,
        completed: bool,
    ) {
        let parent_id = parent.client_order_id;
        if !self.groups.contains_key(&parent_id) {
            let group = self.group_history.remove(&parent_id).unwrap_or_else(|| {
                let mut members = AHashMap::new();
                let snapshot = self.order_snapshots.get(&parent.perm_id).cloned();
                members.insert(
                    parent.perm_id,
                    BrokerIncarnation {
                        route: Some((self.client_id, primary_order_id)),
                        terminal: self.terminal_orders.contains_key(&primary_order_id),
                        seen: false,
                        route_conflict: false,
                        data: snapshot,
                    },
                );
                OrderIncarnationGroup {
                    parent: parent.clone(),
                    account_id,
                    primary_order_id,
                    members,
                }
            });
            self.groups.insert(parent_id, group);
        }
        let group = self.groups.get_mut(&parent_id).expect("group was inserted");
        self.incarnation_parents.insert(parent.perm_id, parent_id);
        self.incarnation_parents.insert(perm_id, parent_id);
        let new_member = !group.members.contains_key(&perm_id);
        let member = group.members.entry(perm_id).or_insert(BrokerIncarnation {
            data: None,
            route: None,
            terminal: false,
            seen: false,
            route_conflict: false,
        });

        if let Some(data) = data {
            let terminal = completed
                || matches!(
                    data.order_state.status,
                    OrderStatusKind::Filled
                        | OrderStatusKind::Cancelled
                        | OrderStatusKind::ApiCancelled
                );
            let route = (data.order.client_id, data.order_id);
            member.route_conflict |= member.seen && member.route != Some(route);
            member.route = Some(route);
            member.terminal |= terminal;
            member.seen = !member.terminal;
            let mut snapshot = data.clone();
            if let Some(previous) = &member.data {
                if previous.order_state.status == OrderStatusKind::Filled
                    || (member.terminal && !terminal)
                {
                    snapshot.order_state.status = previous.order_state.status.clone();
                }
                snapshot.order.filled_quantity = snapshot
                    .order
                    .filled_quantity
                    .max(previous.order.filled_quantity);
            }
            member.data = Some(snapshot);
        }

        if new_member {
            tracing::warn!(
                "IB order {parent_id} has multiple permanent IDs: {} and {perm_id}; retaining separate broker orders",
                parent.perm_id
            );
        }
    }

    pub(super) fn observe_order_data(
        &mut self,
        data: &ibapi::orders::OrderData,
        account_id: AccountId,
        completed: bool,
    ) -> anyhow::Result<Option<TrackedOrder>> {
        let correlation = self.correlate(
            data.order.client_id,
            data.order_id,
            data.order.perm_id,
            &data.order.order_ref,
        )?;

        match correlation {
            OrderCorrelation::Tracked { order_id, context } => {
                if data.order.perm_id > 0 {
                    self.order_snapshots
                        .insert(data.order.perm_id, data.clone());
                    if let Some(order) = self.order_mut(order_id) {
                        if order.perm_id == 0 {
                            order.perm_id = data.order.perm_id;
                        }
                    }
                }

                if self.groups.contains_key(&context.client_order_id) {
                    self.record_incarnation(
                        order_id,
                        &context,
                        account_id,
                        data.order.perm_id,
                        Some(data),
                        completed,
                    );
                }
                Ok(None)
            }
            OrderCorrelation::Duplicate { order_id, context } => {
                self.record_incarnation(
                    order_id,
                    &context,
                    account_id,
                    data.order.perm_id,
                    Some(data),
                    completed,
                );
                Ok(Some(context))
            }
            OrderCorrelation::Untracked { .. } => Ok(None),
        }
    }

    pub(super) fn observe_incarnation_status(
        &mut self,
        status: &IBOrderStatus,
    ) -> Option<(TrackedOrder, ibapi::orders::OrderData)> {
        let parent_id = *self.incarnation_parents.get(&status.perm_id)?;
        let group = self.groups.get_mut(&parent_id)?;
        let member = group.members.get_mut(&status.perm_id)?;
        let was_terminal = member.terminal;
        let terminal = matches!(
            status.status,
            OrderStatusKind::Filled | OrderStatusKind::Cancelled | OrderStatusKind::ApiCancelled
        );
        member.terminal |= terminal;
        if let Some(data) = member.data.as_mut() {
            if !was_terminal
                || status.status == OrderStatusKind::Filled
                || (terminal && data.order_state.status != OrderStatusKind::Filled)
            {
                data.order_state.status = status.status.clone();
            }

            if status.filled.is_finite() && status.filled >= 0.0 {
                data.order.filled_quantity = data.order.filled_quantity.max(status.filled);
            }
        }

        if status.perm_id == group.parent.perm_id {
            return None;
        }
        member.data.clone().map(|data| (group.parent.clone(), data))
    }

    pub(super) fn group_cancel_routes(
        &self,
        parent_id: ClientOrderId,
        order_side: Option<OrderSide>,
    ) -> (Vec<(i64, i32)>, Vec<i64>) {
        let Some(group) = self.groups.get(&parent_id) else {
            return (Vec::new(), Vec::new());
        };
        let mut route_counts = AHashMap::new();

        for member in group
            .members
            .values()
            .filter(|member| !member.terminal && member.seen)
        {
            if let Some(route) = member.route {
                *route_counts.entry(route).or_insert(0_usize) += 1;
            }
        }
        let mut routes = Vec::new();
        let mut unresolved = Vec::new();

        for (perm_id, member) in &group.members {
            if member.terminal {
                continue;
            }

            // A sided request skips members whose broker side is unknown
            let member_side = member
                .data
                .as_ref()
                .map(|data| IbAction::from(data.order.action).order_side());
            if order_side.is_some_and(|side| member_side != Some(side)) {
                continue;
            }

            match member.route {
                Some((client_id, order_id))
                    if member.seen
                        && !member.route_conflict
                        && client_id == self.client_id
                        && order_id != 0
                        && route_counts.get(&(client_id, order_id)) == Some(&1) =>
                {
                    routes.push((*perm_id, order_id));
                }
                _ => unresolved.push(*perm_id),
            }
        }
        routes.sort_unstable();
        unresolved.sort_unstable();
        (routes, unresolved)
    }

    pub(super) fn archive_finished_groups(&mut self) {
        let closed: Vec<_> = self
            .groups
            .iter()
            .filter_map(|(id, group)| {
                group
                    .members
                    .values()
                    .all(|member| member.terminal)
                    .then_some(*id)
            })
            .collect();

        for id in closed {
            if let Some(group) = self.groups.remove(&id) {
                for perm_id in group.members.keys() {
                    if let Ok(child) = ClientOrderId::for_duplicate_order(
                        group.account_id,
                        parse::ib_venue_order_id(0, *perm_id),
                    ) {
                        self.auxiliary_orders.remove(&child);
                        if let Some(order_id) = self.order_id_map.remove(&child) {
                            if self
                                .active_orders
                                .get(&order_id)
                                .is_some_and(|order| order.client_order_id == child)
                            {
                                let order = self
                                    .active_orders
                                    .remove(&order_id)
                                    .expect("owned child was checked");
                                self.terminal_orders.insert(order_id, order);
                            }

                            if self.venue_order_id_map.get(&order_id) == Some(&child) {
                                self.venue_order_id_map.remove(&order_id);
                            }
                        }
                    }
                }
                self.group_history.insert(id, group);
            }
        }
        self.incarnation_parents
            .retain(|_, id| self.groups.contains_key(id) || self.group_history.contains_key(id));
    }
}

impl InteractiveBrokersExecutionClient {
    pub(super) async fn cancel_incarnation_group(
        parent_id: ClientOrderId,
        client: &Arc<Client>,
        orders: &OrderTracker,
        provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        ib_account: Ustr,
        request_timeout_secs: u64,
        order_side: Option<OrderSide>,
    ) -> anyhow::Result<()> {
        {
            let mut state = orders.lock()?;
            if let Some(group) = state.groups.get_mut(&parent_id) {
                for member in group.members.values_mut() {
                    member.seen = false;
                    member.route_conflict = false;
                }
            } else {
                return Ok(());
            }
        }
        let refresh = async {
            for completed in [false, true] {
                let subscription = if completed {
                    client.completed_orders(false).await?
                } else {
                    client.all_open_orders().await?
                };
                let mut subscription = subscription.filter_data();
                while let Some(item) = subscription.next().await {
                    let Orders::OrderData(data) = item? else {
                        continue;
                    };

                    if data.order.account != ib_account {
                        continue;
                    }
                    let parent = {
                        let state = orders.lock()?;
                        let Some(group) = state.groups.get(&parent_id) else {
                            continue;
                        };

                        if !group.members.contains_key(&data.order.perm_id)
                            && parse::normalized_order_ref(&data.order.order_ref)
                                != Some(parent_id.as_str())
                        {
                            continue;
                        }
                        group.parent.clone()
                    };
                    let resolved = provider.resolve_instrument_id_for_contract(&data.contract)?;
                    anyhow::ensure!(
                        resolved == parent.instrument_id,
                        "IB shared order reference {parent_id} has conflicting instruments"
                    );
                    let primary_id = orders
                        .lock()?
                        .groups
                        .get(&parent_id)
                        .expect("group exists")
                        .primary_order_id;
                    orders.lock()?.record_incarnation(
                        primary_id,
                        &parent,
                        account_id,
                        data.order.perm_id,
                        Some(&data),
                        completed,
                    );

                    if data.order.perm_id != parent.perm_id {
                        let mut report = parse_order_data_to_report(
                            &data,
                            parent.instrument_id,
                            account_id,
                            provider,
                            ts_init,
                        )?;
                        report.client_order_id = Some(parent_id);
                        exec_sender.send(ExecutionEvent::Report(ExecutionReport::Order(
                            Box::new(report),
                        )))?;
                    }
                }
            }
            Ok::<_, anyhow::Error>(())
        };
        tokio::time::timeout(Duration::from_secs(request_timeout_secs), refresh)
            .await
            .context("timed out refreshing IB order incarnations")??;
        let (routes, unresolved, parent) = {
            let state = orders.lock()?;
            let (routes, unresolved) = state.group_cancel_routes(parent_id, order_side);
            let parent = state
                .groups
                .get(&parent_id)
                .expect("group exists")
                .parent
                .clone();
            (routes, unresolved, parent)
        };

        if !unresolved.is_empty() {
            tracing::warn!(
                "IB cancellation for {parent_id} has unresolved or unbound permanent IDs {unresolved:?}; no group completion is inferred"
            );
        }

        for (perm_id, order_id) in routes {
            let client_order_id = if perm_id == parent.perm_id {
                parent_id
            } else {
                ClientOrderId::for_duplicate_order(
                    account_id,
                    parse::ib_venue_order_id(order_id, perm_id),
                )?
            };
            let send = tokio::time::timeout(
                Duration::from_secs(request_timeout_secs),
                client.cancel_order(order_id, ""),
            )
            .await;

            match send {
                Ok(Ok(_)) => {
                    let event = OrderPendingCancel::new(
                        parent.trader_id,
                        parent.strategy_id,
                        parent.instrument_id,
                        client_order_id,
                        Some(account_id),
                        UUID4::new(),
                        ts_init,
                        ts_init,
                        false,
                        Some(parse::ib_venue_order_id(order_id, perm_id)),
                    );

                    if let Err(e) =
                        exec_sender.send(ExecutionEvent::Order(OrderEventAny::PendingCancel(event)))
                    {
                        tracing::error!(
                            "IB cancel was sent but pending state could not be delivered: {e}"
                        );
                    }
                }
                Ok(Err(e)) => tracing::warn!(
                    "IB cancel outcome for permanent ID {perm_id} remains unresolved: {e}"
                ),
                Err(e) => {
                    tracing::warn!("IB cancel outcome for permanent ID {perm_id} timed out: {e}");
                }
            }
        }
        orders.lock()?.archive_finished_groups();
        Ok(())
    }
}
