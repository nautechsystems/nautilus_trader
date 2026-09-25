// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Interactive Brokers execution command handling.

use nautilus_common::live::sender::EventSender;

use super::core::*;

impl InteractiveBrokersExecutionClient {
    #[allow(clippy::too_many_arguments)] // Command execution uses explicit client state.
    pub(super) async fn handle_submit_order_async(
        cmd: &SubmitOrder,
        client: &Arc<Client>,
        orders: &OrderTracker,
        next_order_id: &Arc<Mutex<i32>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        clock: &'static AtomicTime,
        account_id: AccountId,
        ib_account: Ustr,
        order_submit_lock: &Arc<tokio::sync::Mutex<()>>,
    ) -> anyhow::Result<()> {
        if cmd.order_init.post_only {
            let ts_event = clock.get_time_ns();
            let detail = "`post_only` not supported by Interactive Brokers";
            let reason = coded_denial_reason(DENIAL_POST_ONLY_UNSUPPORTED, detail);
            Self::send_order_denied_to(
                cmd.order_init.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.order_init.client_order_id,
                &reason,
                exec_sender,
                ts_event,
            )?;
            anyhow::bail!(reason);
        }

        let is_inverse = instrument_provider
            .find(&cmd.instrument_id)
            .is_some_and(|instrument| instrument.is_inverse());

        if cmd.order_init.quote_quantity && !is_inverse {
            let ts_event = clock.get_time_ns();
            let detail = "Quote quantity requires an inverse instrument";
            let reason = coded_denial_reason(DENIAL_QUOTE_QUANTITY_UNSUPPORTED, detail);
            Self::send_order_denied_to(
                cmd.order_init.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.order_init.client_order_id,
                &reason,
                exec_sender,
                ts_event,
            )?;
            anyhow::bail!(reason);
        }

        if matches!(
            cmd.order_init.order_type,
            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
        ) && let Some(trailing_offset_type) = cmd.order_init.trailing_offset_type
            && trailing_offset_type != TrailingOffsetType::Price
        {
            let ts_event = clock.get_time_ns();
            let detail = format!(
                "`TrailingOffsetType` {trailing_offset_type:?} is not supported (only PRICE is supported)"
            );
            let reason = coded_denial_reason(DENIAL_TRAILING_OFFSET_TYPE_UNSUPPORTED, &detail);
            Self::send_order_denied_to(
                cmd.order_init.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.order_init.client_order_id,
                &reason,
                exec_sender,
                ts_event,
            )?;
            anyhow::bail!("{reason}");
        }

        let contract =
            Self::resolve_contract_for_instrument(cmd.instrument_id, instrument_provider)?;
        let contract = Self::contract_with_order_exchange_param(contract, cmd.params.as_ref())?;

        let order_any = OrderAny::try_from(cmd.order_init.clone())
            .context("Failed to construct order from `OrderInitialized`")?;
        let order_ref = cmd.order_init.client_order_id.to_string();
        let _submit_guard = order_submit_lock.lock().await;
        let ib_order_id = Self::reserve_next_local_order_id(next_order_id)?;
        let mut ib_order =
            nautilus_order_to_ib_order(&order_any, instrument_provider, ib_order_id, &order_ref)
                .context("Failed to transform order")?;
        Self::assign_ib_account(&mut ib_order, ib_account);

        Self::cache_order_tracking(
            ib_order_id,
            cmd.order_init.client_order_id,
            cmd.instrument_id,
            cmd.order_init.trader_id,
            cmd.strategy_id,
            cmd.order_init.order_side,
            cmd.order_init.order_type,
            orders,
        )?;

        let ts_event = clock.get_time_ns();
        let event = OrderSubmitted::new(
            cmd.order_init.trader_id,
            cmd.strategy_id,
            cmd.instrument_id,
            cmd.order_init.client_order_id,
            account_id,
            UUID4::new(),
            ts_event,
            ts_event,
        );

        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Submitted(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order submitted event: {e}"))?;

        if let Err(e) = client.submit_order(ib_order_id, &contract, &ib_order).await {
            return Self::handle_order_submit_failure(
                &e,
                "Failed to submit order",
                ib_order_id,
                account_id,
                ts_event,
                orders,
                exec_sender,
                clock,
            );
        }

        tracing::debug!(
            "Submitted order {} as IB order ID {}",
            cmd.order_init.client_order_id,
            ib_order_id
        );

        Ok(())
    }

    // Modifies from IB's current copy of the open order so attributes Nautilus orders cannot
    // carry (goodAfterTime, OCA group, outsideRth, conditions) survive, including for orders
    // restored from a previous session.
    pub(super) async fn handle_modify_order_async(
        cmd: &ModifyOrder,
        client: &Arc<Client>,
        orders: &OrderTracker,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        ib_account: Ustr,
        request_timeout_secs: u64,
    ) -> anyhow::Result<()> {
        let target_ib_order_id = Self::target_ib_order_id_for_modify(
            cmd,
            client,
            orders,
            ib_account,
            request_timeout_secs,
        )
        .await?;

        Self::handle_modify_open_order_async(
            cmd,
            client,
            target_ib_order_id,
            orders,
            instrument_provider,
            request_timeout_secs,
        )
        .await
    }

    async fn target_ib_order_id_for_modify(
        cmd: &ModifyOrder,
        client: &Arc<Client>,
        orders: &OrderTracker,
        ib_account: Ustr,
        request_timeout_secs: u64,
    ) -> anyhow::Result<Option<i32>> {
        if let Some(venue_order_id) = &cmd.venue_order_id {
            let order_selector = IbOrderSelector::from_venue_order_id(venue_order_id)?;
            let order_id =
                Self::resolve_ib_order_id(client, order_selector, ib_account, request_timeout_secs)
                    .await?;
            return Ok(Some(order_id));
        }

        Ok(orders
            .lock()?
            .order_id_map
            .get(&cmd.client_order_id)
            .copied())
    }

    pub(super) fn apply_modify_fields_to_ib_order(
        cmd: &ModifyOrder,
        ib_order: &mut ibapi::orders::Order,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
    ) {
        if let Some(quantity) = cmd.quantity {
            ib_order.total_quantity = quantity.as_f64();
        }

        let price_magnifier = instrument_provider.get_price_magnifier(&cmd.instrument_id) as f64;

        if let Some(price) = cmd.price {
            ib_order.limit_price = Some(price.as_f64() / price_magnifier);
        }

        if let Some(trigger_price) = cmd.trigger_price {
            let converted_trigger_price = trigger_price.as_f64() / price_magnifier;
            if matches!(ib_order.order_type.as_str(), "TRAIL" | "TRAIL LIMIT") {
                ib_order.trail_stop_price = Some(converted_trigger_price);
            } else {
                ib_order.aux_price = Some(converted_trigger_price);
            }
        }
    }

    async fn handle_modify_open_order_async(
        cmd: &ModifyOrder,
        client: &Arc<Client>,
        target_ib_order_id: Option<i32>,
        orders: &OrderTracker,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        request_timeout_secs: u64,
    ) -> anyhow::Result<()> {
        let timeout_dur = Duration::from_secs(request_timeout_secs);
        let subscription = tokio::time::timeout(timeout_dur, client.all_open_orders())
            .await
            .context("Timeout requesting open orders for modify")??;
        let mut subscription = subscription.filter_data();

        let client_order_id = cmd.client_order_id.to_string();

        while let Some(order_result) = subscription.next().await {
            match order_result {
                Ok(Orders::OrderData(data)) => {
                    if !Self::is_active_open_order(&data.order) {
                        continue;
                    }

                    let matches_order_id =
                        target_ib_order_id.is_some_and(|order_id| data.order_id == order_id);
                    let matches_order_ref = parse::normalized_order_ref(&data.order.order_ref)
                        == Some(client_order_id.as_str());

                    if !matches_order_id && !matches_order_ref {
                        continue;
                    }

                    let ib_order_id = data.order_id;
                    let contract = data.contract;
                    let contract =
                        Self::contract_with_order_exchange_param(contract, cmd.params.as_ref())?;
                    let mut ib_order = data.order;

                    Self::apply_modify_fields_to_ib_order(cmd, &mut ib_order, instrument_provider);

                    {
                        let mut state = orders.lock()?;
                        state.order_id_map.insert(cmd.client_order_id, ib_order_id);
                        state
                            .venue_order_id_map
                            .insert(ib_order_id, cmd.client_order_id);
                        if let Some(order) = state.active_orders.get_mut(&ib_order_id) {
                            order.instrument_id = cmd.instrument_id;
                        }
                    }
                    Self::mark_pending_modify(cmd, ib_order_id, orders, &ib_order)?;

                    if let Err(e) = client.submit_order(ib_order_id, &contract, &ib_order).await {
                        if Self::is_definitive_order_submit_error(&e) {
                            Self::clear_pending_modify(ib_order_id, orders);
                            return Err(e)
                                .context("IB rejected the modified open order before sending it");
                        }
                        tracing::error!(
                            "Modify outcome is unknown after attempting to send open order {} to IB: {e}",
                            cmd.client_order_id
                        );
                        return Ok(());
                    }

                    tracing::debug!(
                        "Modified open order {} (IB order ID: {}) after cache miss",
                        cmd.client_order_id,
                        ib_order_id
                    );

                    return Ok(());
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Error receiving open order data for modify: {e}");
                }
            }
        }

        anyhow::bail!(
            "Order not found for modify in IB open orders: client_order_id={}, venue_order_id={:?}",
            cmd.client_order_id,
            cmd.venue_order_id,
        )
    }

    pub(super) fn mark_pending_modify(
        cmd: &ModifyOrder,
        ib_order_id: i32,
        orders: &OrderTracker,
        ib_order: &ibapi::orders::Order,
    ) -> anyhow::Result<()> {
        let mut state = orders.lock()?;
        let order = state
            .active_orders
            .get_mut(&ib_order_id)
            .ok_or_else(|| anyhow::anyhow!("IB order {ib_order_id} is not tracked for modify"))?;
        anyhow::ensure!(
            order.client_order_id == cmd.client_order_id,
            "IB order {ib_order_id} is tracked as {}, not {}",
            order.client_order_id,
            cmd.client_order_id,
        );
        anyhow::ensure!(
            order.instrument_id == cmd.instrument_id,
            "IB order {ib_order_id} is tracked for {}, not {}",
            order.instrument_id,
            cmd.instrument_id,
        );
        anyhow::ensure!(
            order.pending_modify.is_none(),
            "IB order {ib_order_id} already has a pending modify",
        );
        order.pending_modify = Some(PendingModifyValues {
            total_quantity: ib_order.total_quantity,
            limit_price: ib_order.limit_price,
            aux_price: ib_order.aux_price,
            // IB moves a trailing stop's trigger with the market, so only a requested one is checked
            trail_stop_price: cmd.trigger_price.and(ib_order.trail_stop_price),
        });
        Ok(())
    }

    fn clear_pending_modify(ib_order_id: i32, orders: &OrderTracker) {
        if let Ok(mut state) = orders.lock()
            && let Some(order) = state.active_orders.get_mut(&ib_order_id)
        {
            order.pending_modify = None;
        }
    }

    #[allow(clippy::too_many_arguments)] // Order-list submission shares explicit client state.
    pub(super) async fn handle_submit_order_list_async(
        cmd: &SubmitOrderList,
        orders: &[OrderAny],
        client: &Arc<Client>,
        order_tracker: &OrderTracker,
        next_order_id: &Arc<Mutex<i32>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        clock: &'static AtomicTime,
        account_id: AccountId,
        ib_account: Ustr,
        strategy_id: StrategyId,
        order_submit_lock: &Arc<tokio::sync::Mutex<()>>,
    ) -> anyhow::Result<()> {
        let num_orders = orders.len();
        anyhow::ensure!(!orders.is_empty(), "Cannot submit an empty order list");

        let _submit_guard = order_submit_lock.lock().await;
        let mut ib_order_ids = AHashMap::with_capacity(num_orders);

        for order in orders {
            let ib_order_id = Self::reserve_next_local_order_id(next_order_id)?;
            ib_order_ids.insert(order.client_order_id(), ib_order_id);
        }

        // Every order is prepared before the first reaches IB, so a list that cannot be
        // prepared leaves nothing parked at the venue and is denied as a whole
        let prepared = match Self::prepare_order_list(
            cmd,
            orders,
            &ib_order_ids,
            order_tracker,
            instrument_provider,
            ib_account,
        ) {
            Ok(prepared) => prepared,
            Err(e) => {
                let reason = coded_denial_reason(DENIAL_ORDER_LIST_INVALID, &format!("{e:#}"));
                Self::deny_unsubmitted_order_list(
                    orders,
                    &reason,
                    strategy_id,
                    exec_sender,
                    clock,
                )?;
                return Err(e);
            }
        };

        for (index, (order, (order_contract, ib_order))) in orders.iter().zip(prepared).enumerate()
        {
            let ib_order_id = ib_order_ids[&order.client_order_id()];

            Self::cache_order_tracking(
                ib_order_id,
                order.client_order_id(),
                order.instrument_id(),
                order.trader_id(),
                strategy_id,
                order.order_side(),
                order.order_type(),
                order_tracker,
            )?;

            let ts_event = clock.get_time_ns();
            let event = OrderSubmitted::new(
                order.trader_id(),
                strategy_id,
                order.instrument_id(),
                order.client_order_id(),
                account_id,
                UUID4::new(),
                ts_event,
                ts_event,
            );

            exec_sender
                .send(ExecutionEvent::Order(OrderEventAny::Submitted(event)))
                .map_err(|e| anyhow::anyhow!("Failed to send order submitted event: {e}"))?;

            if let Err(e) = client
                .submit_order(ib_order_id, &order_contract, &ib_order)
                .await
            {
                if let Err(denial_error) = Self::deny_unsubmitted_order_list(
                    &orders[index + 1..],
                    DENIAL_ORDER_LIST_SIBLING_SUBMIT_FAILED,
                    strategy_id,
                    exec_sender,
                    clock,
                ) {
                    tracing::error!(
                        "Failed to deny unsubmitted order-list siblings after order {} failed: {denial_error}",
                        order.client_order_id()
                    );
                }
                Self::cancel_untransmitted_order_list_predecessors(
                    &orders[..index],
                    &ib_order_ids,
                    client,
                    order_tracker,
                    strategy_id,
                    account_id,
                    exec_sender,
                    clock,
                )
                .await;
                return Self::handle_order_submit_failure(
                    &e,
                    "Failed to submit order from list",
                    ib_order_id,
                    account_id,
                    ts_event,
                    order_tracker,
                    exec_sender,
                    clock,
                );
            }

            tracing::debug!(
                "Submitted order {} from list as IB order ID {}",
                order.client_order_id(),
                ib_order_id,
            );
        }

        Ok(())
    }

    pub(super) fn prepare_order_list(
        cmd: &SubmitOrderList,
        orders: &[OrderAny],
        ib_order_ids: &AHashMap<ClientOrderId, i32>,
        order_tracker: &OrderTracker,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        ib_account: Ustr,
    ) -> anyhow::Result<Vec<(ibapi::contracts::Contract, ibapi::orders::Order)>> {
        let num_orders = orders.len();
        let mut prepared = Vec::with_capacity(num_orders);

        for (index, order) in orders.iter().enumerate() {
            let ib_order_id = ib_order_ids[&order.client_order_id()];

            let order_contract =
                Self::resolve_contract_for_instrument(order.instrument_id(), instrument_provider)?;
            let order_contract =
                Self::contract_with_order_exchange_param(order_contract, cmd.params.as_ref())?;

            let order_ref = order.client_order_id().to_string();
            let mut ib_order =
                nautilus_order_to_ib_order(order, instrument_provider, ib_order_id, &order_ref)
                    .context("Failed to transform order")?;
            Self::assign_ib_account(&mut ib_order, ib_account);
            ib_order.transmit = index == num_orders - 1;

            if let Some(parent_order_id) = order.parent_order_id() {
                let parent_ib_order_id = match ib_order_ids.get(&parent_order_id) {
                    Some(parent_ib_order_id) => *parent_ib_order_id,
                    None => *order_tracker
                        .lock()?
                        .order_id_map
                        .get(&parent_order_id)
                        .with_context(|| {
                            format!(
                                "Parent order ID {parent_order_id} not found for order {}",
                                order.client_order_id()
                            )
                        })?,
                };
                ib_order.parent_id = parent_ib_order_id;
            }

            prepared.push((order_contract, ib_order));
        }

        Ok(prepared)
    }

    #[allow(clippy::too_many_arguments)] // Failure resolution shares the submit context.
    async fn cancel_untransmitted_order_list_predecessors(
        predecessors: &[OrderAny],
        ib_order_ids: &AHashMap<ClientOrderId, i32>,
        client: &Arc<Client>,
        order_tracker: &OrderTracker,
        strategy_id: StrategyId,
        account_id: AccountId,
        exec_sender: &EventSender<ExecutionEvent>,
        clock: &'static AtomicTime,
    ) {
        // Predecessors were parked at IB with `transmit=false` and can no longer be
        // activated once a later sibling failed, so cancel them at the venue and give
        // each a definite terminal state.
        let mut failed_cancels: AHashSet<ClientOrderId> = AHashSet::new();

        for order in predecessors {
            let client_order_id = order.client_order_id();
            let Some(ib_order_id) = ib_order_ids.get(&client_order_id).copied() else {
                continue;
            };

            if let Err(e) = client.cancel_order(ib_order_id, "").await {
                tracing::error!(
                    "Failed to cancel untransmitted order-list predecessor {} (IB order ID {}): {e}",
                    client_order_id,
                    ib_order_id
                );
                failed_cancels.insert(client_order_id);
            }
        }

        if let Err(e) = Self::resolve_failed_order_list_predecessors(
            predecessors,
            ib_order_ids,
            &failed_cancels,
            order_tracker,
            strategy_id,
            account_id,
            exec_sender,
            clock,
        ) {
            tracing::error!(
                "Failed to resolve untransmitted order-list predecessors after sibling failure: {e}"
            );
        }
    }

    fn assign_ib_account(ib_order: &mut ibapi::orders::Order, ib_account: Ustr) {
        ib_order.account = ib_account.to_string();
        ib_order.clearing_account = ib_account.to_string();
    }

    #[allow(clippy::too_many_arguments)] // Failure resolution shares the submit context.
    pub(super) fn resolve_failed_order_list_predecessors(
        predecessors: &[OrderAny],
        ib_order_ids: &AHashMap<ClientOrderId, i32>,
        failed_cancels: &AHashSet<ClientOrderId>,
        order_tracker: &OrderTracker,
        strategy_id: StrategyId,
        account_id: AccountId,
        exec_sender: &EventSender<ExecutionEvent>,
        clock: &'static AtomicTime,
    ) -> anyhow::Result<()> {
        for order in predecessors {
            let client_order_id = order.client_order_id();
            let Some(ib_order_id) = ib_order_ids.get(&client_order_id).copied() else {
                continue;
            };

            if failed_cancels.contains(&client_order_id) {
                // The venue cancel failed, so the order may remain parked at TWS and
                // manually transmittable; keep tracking instead of claiming terminal.
                tracing::error!(
                    "Order-list predecessor {} (IB order ID {}) remains parked at IB after a failed cancel; keeping tracking until venue state resolves",
                    client_order_id,
                    ib_order_id
                );
                continue;
            }

            let perm_id = order_tracker.lock().ok().and_then(|state| {
                state
                    .active_orders
                    .get(&ib_order_id)
                    .map(|tracked| tracked.perm_id)
            });

            let ts_event = clock.get_time_ns();
            let event = OrderCanceled::new(
                order.trader_id(),
                strategy_id,
                order.instrument_id(),
                client_order_id,
                UUID4::new(),
                ts_event,
                ts_event,
                false,
                Some(ib_venue_order_id(ib_order_id, perm_id.unwrap_or(0))),
                Some(account_id),
                None,
            );
            exec_sender
                .send(ExecutionEvent::Order(OrderEventAny::Canceled(event)))
                .map_err(|e| {
                    anyhow::anyhow!("Failed to send order-list predecessor canceled event: {e}")
                })?;

            Self::remove_order_tracking(ib_order_id, client_order_id, order_tracker)?;
        }

        Ok(())
    }

    pub(super) fn deny_unsubmitted_order_list(
        orders: &[OrderAny],
        reason: &str,
        strategy_id: StrategyId,
        exec_sender: &EventSender<ExecutionEvent>,
        clock: &'static AtomicTime,
    ) -> anyhow::Result<()> {
        for order in orders {
            let ts_event = clock.get_time_ns();
            Self::send_order_denied_to(
                order.trader_id(),
                strategy_id,
                order.instrument_id(),
                order.client_order_id(),
                reason,
                exec_sender,
                ts_event,
            )?;
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // Failure emission preserves the submit context.
    pub(super) fn handle_order_submit_failure(
        error: &ibapi::Error,
        failure_prefix: &str,
        ib_order_id: i32,
        account_id: AccountId,
        ts_event: UnixNanos,
        orders: &OrderTracker,
        exec_sender: &EventSender<ExecutionEvent>,
        clock: &'static AtomicTime,
    ) -> anyhow::Result<()> {
        match Self::classify_order_submit_error(error) {
            CommandFailure::Ambiguous(reason) => {
                anyhow::bail!(
                    "{failure_prefix}; outcome is unknown after possible transmission: {reason}"
                );
            }
            CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                let context =
                    Self::get_tracked_order_context(ib_order_id, orders)?.with_context(|| {
                        format!("Tracked order context not found for {ib_order_id}")
                    })?;

                Self::remove_order_tracking(ib_order_id, context.client_order_id, orders)?;

                let reason = format!("{failure_prefix}: {reason}");
                let event = OrderRejected::new(
                    context.trader_id,
                    context.strategy_id,
                    context.instrument_id,
                    context.client_order_id,
                    account_id,
                    Ustr::from(&reason),
                    UUID4::new(),
                    ts_event,
                    clock.get_time_ns(),
                    false,
                    false,
                );
                exec_sender
                    .send(ExecutionEvent::Order(OrderEventAny::Rejected(event)))
                    .map_err(|e| anyhow::anyhow!("Failed to send order rejected event: {e}"))?;
                anyhow::bail!(reason);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::Symbol;

    use super::*;
    use crate::config::InteractiveBrokersInstrumentProviderConfig;

    fn modify_trigger_cmd() -> ModifyOrder {
        ModifyOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("CLIENT-001")),
            StrategyId::from("S-001"),
            InstrumentId::new(Symbol::from("AAPL"), Venue::from("NASDAQ")),
            ClientOrderId::from("O-001"),
            Some(VenueOrderId::from("1")),
            None,
            None,
            Some(Price::from("149.50")),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    }

    fn instrument_provider() -> Arc<InteractiveBrokersInstrumentProvider> {
        Arc::new(InteractiveBrokersInstrumentProvider::new(
            InteractiveBrokersInstrumentProviderConfig::default(),
        ))
    }

    #[rstest::rstest]
    fn modify_trailing_stop_routes_trigger_to_trail_stop_price() {
        let mut ib_order = ibapi::orders::Order {
            order_type: "TRAIL".to_string(),
            aux_price: Some(0.5),
            trailing_percent: Some(0.25),
            ..Default::default()
        };

        InteractiveBrokersExecutionClient::apply_modify_fields_to_ib_order(
            &modify_trigger_cmd(),
            &mut ib_order,
            &instrument_provider(),
        );

        assert_eq!(ib_order.aux_price, Some(0.5));
        assert_eq!(ib_order.trailing_percent, Some(0.25));
        assert_eq!(ib_order.trail_stop_price, Some(149.5));
    }

    #[rstest::rstest]
    fn modify_stop_order_routes_trigger_to_aux_price() {
        let mut ib_order = ibapi::orders::Order {
            order_type: "STP".to_string(),
            ..Default::default()
        };

        InteractiveBrokersExecutionClient::apply_modify_fields_to_ib_order(
            &modify_trigger_cmd(),
            &mut ib_order,
            &instrument_provider(),
        );

        assert_eq!(ib_order.aux_price, Some(149.5));
        assert_eq!(ib_order.trail_stop_price, None);
    }
}
