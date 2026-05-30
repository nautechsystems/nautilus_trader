// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

use super::*;

#[allow(dead_code)]
impl InteractiveBrokersExecutionClient {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_submit_order_async(
        cmd: &SubmitOrder,
        client: &Arc<Client>,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        next_order_id: &Arc<Mutex<i32>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        clock: &'static AtomicTime,
        account_id: AccountId,
        accepted_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
        order_submit_lock: &Arc<AsyncMutex<()>>,
    ) -> anyhow::Result<()> {
        if cmd.order_init.post_only {
            let ts_event = clock.get_time_ns();
            let event = OrderRejected::new(
                cmd.order_init.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.order_init.client_order_id,
                account_id,
                Ustr::from("`post_only` not supported by Interactive Brokers"),
                UUID4::new(),
                ts_event,
                ts_event,
                false,
                false,
            );
            exec_sender
                .send(ExecutionEvent::Order(OrderEventAny::Rejected(event)))
                .map_err(|e| anyhow::anyhow!("Failed to send order rejected event: {e}"))?;
            anyhow::bail!("`post_only` not supported by Interactive Brokers");
        }

        let is_inverse = instrument_provider
            .find(&cmd.instrument_id)
            .map(|instrument| instrument.is_inverse())
            .unwrap_or(false);

        if cmd.order_init.quote_quantity && !is_inverse {
            let ts_event = clock.get_time_ns();
            let event = OrderRejected::new(
                cmd.order_init.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.order_init.client_order_id,
                account_id,
                Ustr::from("UNSUPPORTED_QUOTE_QUANTITY"),
                UUID4::new(),
                ts_event,
                ts_event,
                false,
                false,
            );
            exec_sender
                .send(ExecutionEvent::Order(OrderEventAny::Rejected(event)))
                .map_err(|e| anyhow::anyhow!("Failed to send order rejected event: {e}"))?;
            anyhow::bail!("UNSUPPORTED_QUOTE_QUANTITY");
        }

        if matches!(
            cmd.order_init.order_type,
            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
        ) && let Some(trailing_offset_type) = cmd.order_init.trailing_offset_type
            && trailing_offset_type != TrailingOffsetType::Price
        {
            let ts_event = clock.get_time_ns();
            let reason = format!(
                "`TrailingOffsetType` {:?} is not supported (only PRICE is supported)",
                trailing_offset_type
            );
            let event = OrderRejected::new(
                cmd.order_init.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.order_init.client_order_id,
                account_id,
                Ustr::from(&reason),
                UUID4::new(),
                ts_event,
                ts_event,
                false,
                false,
            );
            exec_sender
                .send(ExecutionEvent::Order(OrderEventAny::Rejected(event)))
                .map_err(|e| anyhow::anyhow!("Failed to send order rejected event: {e}"))?;
            anyhow::bail!("{}", reason);
        }

        let contract =
            Self::resolve_contract_for_instrument(cmd.instrument_id, instrument_provider)?;
        let contract = Self::contract_with_order_exchange_param(contract, cmd.params.as_ref())?;

        let order_any = OrderAny::try_from(cmd.order_init.clone())
            .context("Failed to construct order from `OrderInitialized`")?;
        let order_ref = cmd.order_init.client_order_id.to_string();
        let _submit_guard = order_submit_lock.lock().await;
        let ib_order_id = Self::reserve_next_local_order_id(next_order_id)?;
        let mut ib_order = nautilus_order_to_ib_order(
            &order_any,
            &contract,
            instrument_provider,
            ib_order_id,
            &order_ref,
        )
        .context("Failed to transform order")?;
        let ib_account = account_id
            .to_string()
            .split_once('-')
            .map_or_else(|| account_id.to_string(), |(_, value)| value.to_string());
        ib_order.account = ib_account.clone();
        ib_order.clearing_account = ib_account;

        client
            .submit_order(ib_order_id, &contract, &ib_order)
            .await
            .context("Failed to submit order")?;

        Self::cache_order_tracking(
            ib_order_id,
            cmd.order_init.client_order_id,
            cmd.instrument_id,
            cmd.order_init.trader_id,
            cmd.strategy_id,
            order_id_map,
            venue_order_id_map,
            instrument_id_map,
            trader_id_map,
            strategy_id_map,
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

        accepted_orders
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock accepted orders map"))?
            .insert(cmd.order_init.client_order_id);

        let accepted_event = OrderAccepted::new(
            cmd.order_init.trader_id,
            cmd.strategy_id,
            cmd.instrument_id,
            cmd.order_init.client_order_id,
            VenueOrderId::from(ib_order_id.to_string()),
            account_id,
            UUID4::new(),
            ts_event,
            ts_event,
            false,
        );
        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Accepted(
                accepted_event,
            )))
            .map_err(|e| anyhow::anyhow!("Failed to send order accepted event: {e}"))?;

        tracing::info!(
            "Submitted order {} as IB order ID {}",
            cmd.order_init.client_order_id,
            ib_order_id
        );

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_modify_order_async(
        cmd: &ModifyOrder,
        client: &Arc<Client>,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        _exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        _clock: &'static AtomicTime,
        _account_id: AccountId,
        original_order: Option<&Arc<OrderAny>>,
        request_timeout_secs: u64,
    ) -> anyhow::Result<()> {
        let target_ib_order_id = Self::target_ib_order_id_for_modify(cmd, order_id_map)?;

        if let Some(original_order) = original_order {
            let ib_order_id = target_ib_order_id.context("Order ID not found in mapping")?;
            let contract =
                Self::resolve_contract_for_instrument(cmd.instrument_id, instrument_provider)?;
            let contract = Self::contract_with_order_exchange_param(contract, cmd.params.as_ref())?;

            let order_ref = original_order.client_order_id().to_string();
            let mut ib_order = nautilus_order_to_ib_order(
                original_order,
                &contract,
                instrument_provider,
                ib_order_id,
                &order_ref,
            )
            .context("Failed to transform order to IB order")?;

            Self::apply_modify_fields_to_ib_order(cmd, &mut ib_order, instrument_provider);

            client
                .submit_order(ib_order_id, &contract, &ib_order)
                .await
                .context("Failed to submit modified order")?;

            tracing::info!(
                "Modified order {} (IB order ID: {})",
                cmd.client_order_id,
                ib_order_id
            );

            return Ok(());
        }

        Self::handle_modify_open_order_async(
            cmd,
            client,
            target_ib_order_id,
            order_id_map,
            venue_order_id_map,
            instrument_id_map,
            instrument_provider,
            request_timeout_secs,
        )
        .await
    }

    fn target_ib_order_id_for_modify(
        cmd: &ModifyOrder,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
    ) -> anyhow::Result<Option<i32>> {
        if let Some(venue_order_id) = &cmd.venue_order_id {
            let order_id = venue_order_id
                .as_str()
                .parse()
                .context("Failed to parse venue_order_id as IB order id")?;
            return Ok(Some(order_id));
        }

        let map = order_id_map
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock order ID map"))?;
        Ok(map.get(&cmd.client_order_id).copied())
    }

    fn apply_modify_fields_to_ib_order(
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
            ib_order.aux_price = Some(trigger_price.as_f64() / price_magnifier);
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_modify_open_order_async(
        cmd: &ModifyOrder,
        client: &Arc<Client>,
        target_ib_order_id: Option<i32>,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        request_timeout_secs: u64,
    ) -> anyhow::Result<()> {
        let timeout_dur = Duration::from_secs(request_timeout_secs);
        let mut subscription = tokio::time::timeout(timeout_dur, client.all_open_orders())
            .await
            .context("Timeout requesting open orders for modify")??;

        let client_order_id = cmd.client_order_id.to_string();

        while let Some(order_result) = subscription.next().await {
            match order_result {
                Ok(Orders::OrderData(data)) => {
                    let matches_order_id =
                        target_ib_order_id.is_some_and(|order_id| data.order_id == order_id);
                    let matches_order_ref = data.order.order_ref == client_order_id;

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
                        let mut map = order_id_map
                            .lock()
                            .map_err(|_| anyhow::anyhow!("Failed to lock order ID map"))?;
                        map.insert(cmd.client_order_id, ib_order_id);
                    }
                    {
                        let mut map = venue_order_id_map
                            .lock()
                            .map_err(|_| anyhow::anyhow!("Failed to lock venue order ID map"))?;
                        map.insert(ib_order_id, cmd.client_order_id);
                    }
                    {
                        let mut map = instrument_id_map
                            .lock()
                            .map_err(|_| anyhow::anyhow!("Failed to lock instrument ID map"))?;
                        map.insert(ib_order_id, cmd.instrument_id);
                    }

                    client
                        .submit_order(ib_order_id, &contract, &ib_order)
                        .await
                        .context("Failed to submit modified open order")?;

                    tracing::info!(
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

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_submit_order_list_async(
        cmd: &SubmitOrderList,
        orders: &[OrderAny],
        client: &Arc<Client>,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        next_order_id: &Arc<Mutex<i32>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        clock: &'static AtomicTime,
        account_id: AccountId,
        strategy_id: StrategyId,
        accepted_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
        order_submit_lock: &Arc<AsyncMutex<()>>,
    ) -> anyhow::Result<()> {
        let num_orders = orders.len();
        anyhow::ensure!(!orders.is_empty(), "Cannot submit an empty order list");

        let _submit_guard = order_submit_lock.lock().await;
        let ib_account = account_id
            .to_string()
            .split_once('-')
            .map_or_else(|| account_id.to_string(), |(_, value)| value.to_string());
        let mut ib_order_ids = AHashMap::with_capacity(num_orders);

        for order in orders {
            let ib_order_id = Self::reserve_next_local_order_id(next_order_id)?;
            ib_order_ids.insert(order.client_order_id(), ib_order_id);
        }

        for order in orders {
            if let Some(parent_order_id) = order.parent_order_id()
                && !ib_order_ids.contains_key(&parent_order_id)
            {
                let map = order_id_map
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Failed to lock order ID map"))?;
                anyhow::ensure!(
                    map.contains_key(&parent_order_id),
                    "Parent order ID {parent_order_id} not found for order {}",
                    order.client_order_id(),
                );
            }
        }

        for (index, order) in orders.iter().enumerate() {
            let is_last = index == num_orders - 1;
            let ib_order_id = ib_order_ids[&order.client_order_id()];

            let order_contract =
                Self::resolve_contract_for_instrument(order.instrument_id(), instrument_provider)?;
            let order_contract =
                Self::contract_with_order_exchange_param(order_contract, cmd.params.as_ref())?;

            let order_ref = order.client_order_id().to_string();
            let mut ib_order = nautilus_order_to_ib_order(
                order,
                &order_contract,
                instrument_provider,
                ib_order_id,
                &order_ref,
            )
            .context("Failed to transform order")?;
            ib_order.account = ib_account.clone();
            ib_order.clearing_account = ib_account.clone();
            ib_order.transmit = is_last;

            if let Some(parent_order_id) = order.parent_order_id() {
                let parent_ib_order_id =
                    ib_order_ids.get(&parent_order_id).copied().or_else(|| {
                        order_id_map
                            .lock()
                            .ok()
                            .and_then(|map| map.get(&parent_order_id).copied())
                    });

                if let Some(parent_ib_order_id) = parent_ib_order_id {
                    ib_order.parent_id = parent_ib_order_id;
                }
            }

            client
                .submit_order(ib_order_id, &order_contract, &ib_order)
                .await
                .context("Failed to submit order from list")?;

            Self::cache_order_tracking(
                ib_order_id,
                order.client_order_id(),
                order.instrument_id(),
                order.trader_id(),
                strategy_id,
                order_id_map,
                venue_order_id_map,
                instrument_id_map,
                trader_id_map,
                strategy_id_map,
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

            accepted_orders
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock accepted orders map"))?
                .insert(order.client_order_id());

            let accepted_event = OrderAccepted::new(
                order.trader_id(),
                strategy_id,
                order.instrument_id(),
                order.client_order_id(),
                VenueOrderId::from(ib_order_id.to_string()),
                account_id,
                UUID4::new(),
                ts_event,
                ts_event,
                false,
            );
            exec_sender
                .send(ExecutionEvent::Order(OrderEventAny::Accepted(
                    accepted_event,
                )))
                .map_err(|e| anyhow::anyhow!("Failed to send order accepted event: {e}"))?;

            tracing::info!(
                "Submitted order {} from list as IB order ID {}",
                order.client_order_id(),
                ib_order_id,
            );
        }

        Ok(())
    }
}
