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

        let ib_order_id = Self::reserve_next_local_order_id(next_order_id)?;
        let contract =
            Self::resolve_contract_for_instrument(cmd.instrument_id, instrument_provider)?;

        let order_any = OrderAny::from(cmd.order_init.clone());
        let order_ref = cmd.order_init.client_order_id.to_string();
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
        _venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        _exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        _clock: &'static AtomicTime,
        _account_id: AccountId,
        original_order: &Arc<OrderAny>,
    ) -> anyhow::Result<()> {
        let ib_order_id = {
            let map = order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock order ID map"))?;
            map.get(&cmd.client_order_id)
                .copied()
                .context("Order ID not found in mapping")?
        };

        let contract =
            Self::resolve_contract_for_instrument(cmd.instrument_id, instrument_provider)?;

        let order_ref = original_order.client_order_id().to_string();
        let mut ib_order = nautilus_order_to_ib_order(
            original_order,
            &contract,
            instrument_provider,
            ib_order_id,
            &order_ref,
        )
        .context("Failed to transform order to IB order")?;

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

        client
            .submit_order(ib_order_id, &contract, &ib_order)
            .await
            .context("Failed to submit modified order")?;

        tracing::info!(
            "Modified order {} (IB order ID: {})",
            cmd.client_order_id,
            ib_order_id
        );

        Ok(())
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
    ) -> anyhow::Result<()> {
        let num_orders = orders.len();
        let is_bracket_order = num_orders == 3;

        let first_order = &orders[0];
        let contract = Self::resolve_contract_for_instrument(
            first_order.instrument_id(),
            instrument_provider,
        )?;

        if is_bracket_order {
            let parent_order = &orders[0];
            let tp_order = &orders[1];
            let sl_order = &orders[2];

            let parent_id = Self::reserve_next_local_order_id(next_order_id)?;
            let tp_id = Self::reserve_next_local_order_id(next_order_id)?;
            let sl_id = Self::reserve_next_local_order_id(next_order_id)?;

            let parent_ref = parent_order.client_order_id().to_string();
            let mut parent_ib_order = nautilus_order_to_ib_order(
                parent_order,
                &contract,
                instrument_provider,
                parent_id,
                &parent_ref,
            )
            .context("Failed to transform parent order")?;
            let ib_account = account_id
                .to_string()
                .split_once('-')
                .map_or_else(|| account_id.to_string(), |(_, value)| value.to_string());
            parent_ib_order.account = ib_account.clone();
            parent_ib_order.clearing_account = ib_account.clone();
            parent_ib_order.transmit = false;

            let tp_ref = tp_order.client_order_id().to_string();
            let mut tp_ib_order = nautilus_order_to_ib_order(
                tp_order,
                &contract,
                instrument_provider,
                tp_id,
                &tp_ref,
            )
            .context("Failed to transform TP order")?;
            tp_ib_order.account = ib_account.clone();
            tp_ib_order.clearing_account = ib_account.clone();
            tp_ib_order.parent_id = parent_id;
            tp_ib_order.transmit = false;

            let sl_ref = sl_order.client_order_id().to_string();
            let mut sl_ib_order = nautilus_order_to_ib_order(
                sl_order,
                &contract,
                instrument_provider,
                sl_id,
                &sl_ref,
            )
            .context("Failed to transform SL order")?;
            sl_ib_order.account = ib_account.clone();
            sl_ib_order.clearing_account = ib_account;
            sl_ib_order.parent_id = parent_id;
            sl_ib_order.transmit = true;

            client
                .submit_order(parent_id, &contract, &parent_ib_order)
                .await
                .context("Failed to submit parent order")?;
            client
                .submit_order(tp_id, &contract, &tp_ib_order)
                .await
                .context("Failed to submit TP order")?;
            client
                .submit_order(sl_id, &contract, &sl_ib_order)
                .await
                .context("Failed to submit SL order")?;

            Self::cache_order_tracking(
                parent_id,
                parent_order.client_order_id(),
                parent_order.instrument_id(),
                parent_order.trader_id(),
                strategy_id,
                order_id_map,
                venue_order_id_map,
                instrument_id_map,
                trader_id_map,
                strategy_id_map,
            )?;
            Self::cache_order_tracking(
                tp_id,
                tp_order.client_order_id(),
                tp_order.instrument_id(),
                tp_order.trader_id(),
                strategy_id,
                order_id_map,
                venue_order_id_map,
                instrument_id_map,
                trader_id_map,
                strategy_id_map,
            )?;
            Self::cache_order_tracking(
                sl_id,
                sl_order.client_order_id(),
                sl_order.instrument_id(),
                sl_order.trader_id(),
                strategy_id,
                order_id_map,
                venue_order_id_map,
                instrument_id_map,
                trader_id_map,
                strategy_id_map,
            )?;

            let ts_event = clock.get_time_ns();

            for order in orders {
                accepted_orders
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Failed to lock accepted orders map"))?
                    .insert(order.client_order_id());

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

                let accepted_event = OrderAccepted::new(
                    order.trader_id(),
                    strategy_id,
                    order.instrument_id(),
                    order.client_order_id(),
                    VenueOrderId::from(
                        if order.client_order_id() == parent_order.client_order_id() {
                            parent_id
                        } else if order.client_order_id() == tp_order.client_order_id() {
                            tp_id
                        } else {
                            sl_id
                        }
                        .to_string(),
                    ),
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
            }

            tracing::info!(
                "Submitted bracket order: parent={} (IB: {}), TP={} (IB: {}), SL={} (IB: {})",
                parent_order.client_order_id(),
                parent_id,
                tp_order.client_order_id(),
                tp_id,
                sl_order.client_order_id(),
                sl_id
            );
        } else {
            let oca_group_name = format!("OCA_{}", cmd.order_list.id);

            for (index, order) in orders.iter().enumerate() {
                let is_last = index == num_orders - 1;
                let ib_order_id = Self::reserve_next_local_order_id(next_order_id)?;

                let order_contract = Self::resolve_contract_for_instrument(
                    order.instrument_id(),
                    instrument_provider,
                )?;

                let order_ref = order.client_order_id().to_string();
                let mut ib_order = nautilus_order_to_ib_order(
                    order,
                    &order_contract,
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
                ib_order.oca_group = oca_group_name.clone();
                ib_order.oca_type = OcaType::from(1);
                ib_order.transmit = is_last;

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
                    "Submitted order {} from list as IB order ID {} (OCA group: {})",
                    order.client_order_id(),
                    ib_order_id,
                    oca_group_name
                );
            }
        }

        Ok(())
    }
}
