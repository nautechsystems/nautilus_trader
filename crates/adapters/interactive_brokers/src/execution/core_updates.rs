// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

use ibapi::subscriptions::SubscriptionItem;

use super::*;
use crate::{
    common::enums::{IbAction, IbOrderStatus, IbOrderType},
    execution::parse,
};

impl InteractiveBrokersExecutionClient {
    /// Starts the order update subscription stream.
    ///
    /// # Errors
    ///
    /// Returns an error if starting the subscription fails.
    pub(super) async fn start_order_updates(&mut self) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;

        let timeout_dur = Duration::from_secs(self.config.request_timeout);
        log::debug!(
            "Starting IB order update stream subscription (timeout={:?}, client_id={}, account_id={})",
            timeout_dur,
            self.client_id(),
            self.account_id()
        );
        let mut subscription = tokio::time::timeout(timeout_dur, client.order_update_stream())
            .await
            .context("Timeout starting order update stream")??;

        let order_id_map = Arc::clone(&self.order_id_map);
        let venue_order_id_map = Arc::clone(&self.venue_order_id_map);
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;
        let commission_cache = Arc::clone(&self.commission_cache);
        let pending_execution_cache = Arc::clone(&self.pending_execution_cache);
        let instrument_id_map = Arc::clone(&self.instrument_id_map);
        let trader_id_map = Arc::clone(&self.trader_id_map);
        let strategy_id_map = Arc::clone(&self.strategy_id_map);
        let active_order_contexts = Arc::clone(&self.active_order_contexts);
        let terminal_order_contexts = Arc::clone(&self.terminal_order_contexts);
        let spread_fill_tracking = Arc::clone(&self.spread_fill_tracking);
        let order_avg_prices = Arc::clone(&self.order_avg_prices);
        let pending_combo_fills = Arc::clone(&self.pending_combo_fills);
        let pending_combo_fill_avgs = Arc::clone(&self.pending_combo_fill_avgs);
        let order_fill_progress = Arc::clone(&self.order_fill_progress);
        let pending_cancel_orders = Arc::clone(&self.pending_cancel_orders);

        let handle = get_runtime().spawn(async move {
            Self::process_order_update_stream(
                &mut subscription,
                &order_id_map,
                &venue_order_id_map,
                &instrument_provider,
                &exec_sender,
                clock,
                account_id,
                &commission_cache,
                &pending_execution_cache,
                &instrument_id_map,
                &trader_id_map,
                &strategy_id_map,
                &active_order_contexts,
                &terminal_order_contexts,
                &spread_fill_tracking,
                &order_avg_prices,
                &pending_combo_fills,
                &pending_combo_fill_avgs,
                &order_fill_progress,
                &pending_cancel_orders,
            )
            .await;
        });

        self.order_update_handle.replace(handle);

        log::debug!("IB order update stream subscription started");

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn process_order_update_stream(
        subscription: &mut ibapi::subscriptions::Subscription<OrderUpdate>,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        clock: &'static AtomicTime,
        account_id: AccountId,
        commission_cache: &Arc<Mutex<CommissionCache>>,
        pending_execution_cache: &Arc<Mutex<PendingExecutionCache>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        active_order_contexts: &Arc<Mutex<AHashMap<i32, TrackedOrderContext>>>,
        terminal_order_contexts: &Arc<Mutex<FifoCacheMap<i32, TrackedOrderContext, 10_000>>>,
        spread_fill_tracking: &Arc<Mutex<AHashMap<ClientOrderId, ahash::AHashSet<String>>>>,
        order_avg_prices: &Arc<Mutex<AHashMap<ClientOrderId, Price>>>,
        pending_combo_fills: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
        pending_cancel_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
    ) {
        while let Some(update_result) = subscription.next().await {
            match update_result {
                Ok(SubscriptionItem::Data(update)) => {
                    if let Err(e) = Self::handle_order_update(
                        &update,
                        order_id_map,
                        venue_order_id_map,
                        instrument_provider,
                        exec_sender,
                        clock,
                        account_id,
                        commission_cache,
                        instrument_id_map,
                        trader_id_map,
                        strategy_id_map,
                        active_order_contexts,
                        terminal_order_contexts,
                        spread_fill_tracking,
                        order_avg_prices,
                        pending_combo_fills,
                        pending_combo_fill_avgs,
                        order_fill_progress,
                        pending_cancel_orders,
                        pending_execution_cache,
                    )
                    .await
                    {
                        tracing::error!("Error handling order update: {e}");
                    }
                }
                Ok(SubscriptionItem::Notice(notice)) => {
                    tracing::debug!("Received IB order update notice: {notice:?}");
                }
                Err(e) => {
                    tracing::error!("Error receiving order update: {e}");
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_order_update(
        update: &OrderUpdate,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        clock: &'static AtomicTime,
        account_id: AccountId,
        commission_cache: &Arc<Mutex<CommissionCache>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        active_order_contexts: &Arc<Mutex<AHashMap<i32, TrackedOrderContext>>>,
        terminal_order_contexts: &Arc<Mutex<FifoCacheMap<i32, TrackedOrderContext, 10_000>>>,
        spread_fill_tracking: &Arc<Mutex<AHashMap<ClientOrderId, ahash::AHashSet<String>>>>,
        order_avg_prices: &Arc<Mutex<AHashMap<ClientOrderId, Price>>>,
        pending_combo_fills: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
        pending_cancel_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
        pending_live_exec_data: &Arc<Mutex<PendingExecutionCache>>,
    ) -> anyhow::Result<()> {
        let ts_init = clock.get_time_ns();

        match update {
            OrderUpdate::OrderStatus(status) => {
                Self::handle_order_status(
                    status,
                    order_id_map,
                    venue_order_id_map,
                    instrument_provider,
                    exec_sender,
                    ts_init,
                    account_id,
                    instrument_id_map,
                    trader_id_map,
                    strategy_id_map,
                    active_order_contexts,
                    terminal_order_contexts,
                    order_avg_prices,
                    pending_combo_fills,
                    pending_combo_fill_avgs,
                    order_fill_progress,
                    pending_cancel_orders,
                    spread_fill_tracking,
                )
                .await?;
            }
            OrderUpdate::ExecutionData(exec_data) => {
                let execution_id = exec_data.execution.execution_id.clone();
                let has_commission = commission_cache
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Failed to lock commission cache"))?
                    .contains_key(&execution_id);

                if !has_commission {
                    tracing::debug!(
                        "Buffering execution data {} until commission report arrives",
                        execution_id
                    );
                    pending_live_exec_data
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Failed to lock pending live execution data"))?
                        .insert(execution_id, exec_data.clone());
                    return Ok(());
                }

                Self::handle_execution_data(
                    exec_data,
                    order_id_map,
                    venue_order_id_map,
                    instrument_provider,
                    exec_sender,
                    ts_init,
                    account_id,
                    commission_cache,
                    spread_fill_tracking,
                    instrument_id_map,
                    active_order_contexts,
                    terminal_order_contexts,
                    order_avg_prices,
                    pending_combo_fills,
                    pending_combo_fill_avgs,
                    order_fill_progress,
                )
                .await?;
            }
            OrderUpdate::CommissionReport(commission) => {
                let pending_exec_data = pending_live_exec_data
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Failed to lock pending live execution data"))?
                    .remove(&commission.execution_id);

                {
                    let mut cache = commission_cache
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Failed to lock commission cache"))?;
                    // IB uses -1.0 as a pending-sentinel before the real commission arrives;
                    // clamp only that sentinel to zero (legitimate rebates can be negative).
                    let commission_value = if commission.commission == -1.0_f64 {
                        0.0_f64
                    } else {
                        commission.commission
                    };
                    cache.insert(
                        commission.execution_id.clone(),
                        (commission_value, commission.currency.clone()),
                    );
                }

                if let Some(exec_data) = pending_exec_data {
                    Self::handle_execution_data(
                        &exec_data,
                        order_id_map,
                        venue_order_id_map,
                        instrument_provider,
                        exec_sender,
                        ts_init,
                        account_id,
                        commission_cache,
                        spread_fill_tracking,
                        instrument_id_map,
                        active_order_contexts,
                        terminal_order_contexts,
                        order_avg_prices,
                        pending_combo_fills,
                        pending_combo_fill_avgs,
                        order_fill_progress,
                    )
                    .await?;
                }
            }
            OrderUpdate::OpenOrder(order_data) => {
                if order_data.order.what_if
                    && IbOrderStatus::from_str(order_data.order_state.status.as_str())
                        .is_ok_and(|status| status == IbOrderStatus::PreSubmitted)
                {
                    Self::handle_whatif_order(
                        order_data,
                        venue_order_id_map,
                        instrument_id_map,
                        trader_id_map,
                        strategy_id_map,
                        instrument_provider,
                        exec_sender,
                        clock.get_time_ns(),
                        account_id,
                    )
                    .await?;
                } else {
                    let status_str = order_data.order_state.status.as_str();
                    tracing::debug!(
                        "Received open order: order_id={}, status={}, order_ref={}",
                        order_data.order_id,
                        status_str,
                        order_data.order.order_ref
                    );

                    let client_order_id = if let Some(order_ref) =
                        parse::normalized_order_ref(&order_data.order.order_ref)
                    {
                        Some(ClientOrderId::from(order_ref))
                    } else {
                        let map = venue_order_id_map
                            .lock()
                            .map_err(|_| anyhow::anyhow!("Failed to lock venue order ID map"))?;
                        map.get(&order_data.order_id).copied()
                    };

                    if let Some(client_order_id) = client_order_id
                        && IbOrderStatus::from_str(status_str).is_ok_and(IbOrderStatus::is_accepted)
                    {
                        let instrument_id = {
                            Self::get_mapped_instrument_id(order_data.order_id, instrument_id_map)?
                                .map(Ok)
                                .unwrap_or_else(|| {
                                    Self::resolve_contract_instrument_id(
                                        instrument_provider,
                                        &order_data.contract,
                                    )
                                })?
                        };
                        let venue_order_id =
                            parse::ib_venue_order_id(order_data.order_id, order_data.order.perm_id);
                        if Self::emit_order_accepted_if_needed(
                            order_data.order_id,
                            venue_order_id,
                            account_id,
                            ts_init,
                            active_order_contexts,
                            exec_sender,
                        )? {
                            tracing::debug!(
                                "Order {} accepted (IB openOrder status: {})",
                                client_order_id,
                                status_str
                            );
                        }

                        Self::emit_order_updated_from_open_order(
                            order_data,
                            client_order_id,
                            instrument_id,
                            trader_id_map,
                            strategy_id_map,
                            instrument_provider,
                            exec_sender,
                            ts_init,
                            account_id,
                        )?;
                    }
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_order_updated_from_open_order(
        order_data: &ibapi::orders::OrderData,
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
    ) -> anyhow::Result<()> {
        let Some(instrument) = instrument_provider.find(&instrument_id) else {
            return Ok(());
        };

        if order_data.order.total_quantity <= 0.0 {
            return Ok(());
        }

        let (trader_id, strategy_id) = Self::get_required_order_actor_ids(
            order_data.order_id,
            trader_id_map,
            strategy_id_map,
        )?;
        let price_magnifier = instrument_provider.get_price_magnifier(&instrument_id) as f64;
        let (price, trigger_price) = Self::open_order_price_fields(
            order_data,
            price_magnifier,
            instrument.price_precision(),
        );
        let quantity = Quantity::new(order_data.order.total_quantity, instrument.size_precision());
        let venue_order_id =
            parse::ib_venue_order_id(order_data.order_id, order_data.order.perm_id);
        let event = OrderUpdated::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            quantity,
            UUID4::new(),
            ts_init,
            ts_init,
            false,
            Some(venue_order_id),
            Some(account_id),
            price,
            trigger_price,
            None,
            false,
        );

        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Updated(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order updated event: {e}"))
    }

    fn open_order_price_fields(
        order_data: &ibapi::orders::OrderData,
        price_magnifier: f64,
        price_precision: u8,
    ) -> (Option<Price>, Option<Price>) {
        let order_type = IbOrderType::from_str(order_data.order.order_type.as_str())
            .map_or(OrderType::Market, IbOrderType::nautilus_order_type);
        let price = order_data
            .order
            .limit_price
            .map(|price| Price::new(price * price_magnifier, price_precision));
        let trigger_price = order_data
            .order
            .aux_price
            .map(|price| Price::new(price * price_magnifier, price_precision));

        match order_type {
            OrderType::Market | OrderType::MarketToLimit | OrderType::TrailingStopMarket => {
                (None, None)
            }
            OrderType::Limit | OrderType::TrailingStopLimit => (price, None),
            OrderType::StopMarket | OrderType::MarketIfTouched => (None, trigger_price),
            OrderType::StopLimit | OrderType::LimitIfTouched => (price, trigger_price),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_order_status(
        status: &IBOrderStatus,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        active_order_contexts: &Arc<Mutex<AHashMap<i32, TrackedOrderContext>>>,
        terminal_order_contexts: &Arc<Mutex<FifoCacheMap<i32, TrackedOrderContext, 10_000>>>,
        order_avg_prices: &Arc<Mutex<AHashMap<ClientOrderId, Price>>>,
        pending_combo_fills: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
        pending_cancel_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
        spread_fill_tracking: &Arc<Mutex<AHashMap<ClientOrderId, ahash::AHashSet<String>>>>,
    ) -> anyhow::Result<()> {
        let client_order_id = {
            let map = venue_order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock venue order ID map"))?;
            map.get(&status.order_id).copied()
        };

        let Some(client_order_id) = client_order_id else {
            tracing::debug!("Order status for unknown order ID: {}", status.order_id);
            return Ok(());
        };

        let instrument_id = Self::get_mapped_instrument_id(status.order_id, instrument_id_map)?;

        let Some(instrument_id) = instrument_id else {
            tracing::debug!("Instrument ID not found for order ID: {}", status.order_id);
            return Ok(());
        };

        Self::update_order_avg_price(
            client_order_id,
            &instrument_id,
            status.average_fill_price.unwrap_or(0.0),
            status.filled,
            instrument_provider,
            order_avg_prices,
            pending_combo_fill_avgs,
            order_fill_progress,
        )?;

        let ib_order_status = IbOrderStatus::from_str(status.status.as_str()).ok();

        if ib_order_status == Some(IbOrderStatus::Inactive) && status.why_held == "locate" {
            tracing::warn!(
                "Order {} held for short-sell locate, order remains active",
                client_order_id
            );
            return Ok(());
        }

        let venue_order_id = parse::ib_venue_order_id(status.order_id, status.perm_id);
        let is_terminal = ib_order_status.is_some_and(IbOrderStatus::is_terminal);

        if matches!(
            ib_order_status,
            Some(IbOrderStatus::Filled | IbOrderStatus::Cancelled | IbOrderStatus::ApiCancelled)
        ) {
            Self::emit_order_accepted_if_needed(
                status.order_id,
                venue_order_id,
                account_id,
                ts_init,
                active_order_contexts,
                exec_sender,
            )?;
        }

        if is_terminal {
            Self::flush_pending_combo_fills(
                client_order_id,
                pending_combo_fills,
                pending_combo_fill_avgs,
                order_fill_progress,
                exec_sender,
            )?;
            pending_combo_fills
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock pending combo fills"))?
                .remove(&client_order_id);
            pending_combo_fill_avgs
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock pending combo avg chunks"))?
                .remove(&client_order_id);
            order_fill_progress
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock order fill progress"))?
                .remove(&client_order_id);
        }

        let status_str = status.status.as_str();

        match ib_order_status {
            Some(IbOrderStatus::Submitted | IbOrderStatus::PreSubmitted) => {
                if Self::emit_order_accepted_if_needed(
                    status.order_id,
                    venue_order_id,
                    account_id,
                    ts_init,
                    active_order_contexts,
                    exec_sender,
                )? {
                    tracing::debug!(
                        "Order {} accepted (IB status: {})",
                        client_order_id,
                        status_str
                    );
                } else {
                    tracing::debug!(
                        "Order {} already accepted (IB status: {})",
                        client_order_id,
                        status_str
                    );
                }
            }
            Some(IbOrderStatus::Filled) => {
                tracing::debug!(
                    "Order {} filled (IB status: {})",
                    client_order_id,
                    status_str
                );
            }
            Some(IbOrderStatus::Cancelled | IbOrderStatus::ApiCancelled) => {
                pending_cancel_orders
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Failed to lock pending cancel orders map"))?
                    .remove(&client_order_id);

                let (trader_id, strategy_id) = Self::get_required_order_actor_ids(
                    status.order_id,
                    trader_id_map,
                    strategy_id_map,
                )?;

                let event = OrderCanceled::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );
                exec_sender
                    .send(ExecutionEvent::Order(OrderEventAny::Canceled(event)))
                    .map_err(|e| anyhow::anyhow!("Failed to send order canceled event: {e}"))?;
                tracing::debug!("Order {} canceled", client_order_id);
            }
            Some(IbOrderStatus::PendingCancel) => {
                Self::emit_order_pending_cancel(
                    status.order_id,
                    client_order_id,
                    instrument_id_map,
                    trader_id_map,
                    strategy_id_map,
                    pending_cancel_orders,
                    exec_sender,
                    ts_init,
                    account_id,
                )?;
                tracing::debug!("Order {} pending cancel", client_order_id);
            }
            _ => {
                tracing::debug!(
                    "Order status update for order {}: {} (status: {})",
                    client_order_id,
                    status_str,
                    status_str
                );
            }
        }

        if is_terminal {
            Self::evict_terminal_order_state(
                client_order_id,
                status.order_id,
                order_id_map,
                venue_order_id_map,
                instrument_id_map,
                trader_id_map,
                strategy_id_map,
                active_order_contexts,
                terminal_order_contexts,
                order_avg_prices,
                pending_combo_fills,
                pending_combo_fill_avgs,
                order_fill_progress,
                pending_cancel_orders,
                spread_fill_tracking,
            )?;
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn evict_terminal_order_state(
        client_order_id: ClientOrderId,
        order_id: i32,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        active_order_contexts: &Arc<Mutex<AHashMap<i32, TrackedOrderContext>>>,
        terminal_order_contexts: &Arc<Mutex<FifoCacheMap<i32, TrackedOrderContext, 10_000>>>,
        order_avg_prices: &Arc<Mutex<AHashMap<ClientOrderId, Price>>>,
        pending_combo_fills: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
        pending_cancel_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
        spread_fill_tracking: &Arc<Mutex<AHashMap<ClientOrderId, ahash::AHashSet<String>>>>,
    ) -> anyhow::Result<()> {
        let avg_px = order_avg_prices
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock order avg prices"))?
            .get(&client_order_id)
            .copied();

        if let Some(mut context) = active_order_contexts
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock active order contexts"))?
            .remove(&order_id)
        {
            context.avg_px = avg_px;
            terminal_order_contexts
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock terminal order contexts"))?
                .insert(order_id, context);
        }

        order_id_map
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock order ID map"))?
            .remove(&client_order_id);
        venue_order_id_map
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock venue order ID map"))?
            .remove(&order_id);
        instrument_id_map
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock instrument ID map"))?
            .remove(&order_id);
        trader_id_map
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock trader ID map"))?
            .remove(&order_id);
        strategy_id_map
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock strategy ID map"))?
            .remove(&order_id);
        order_avg_prices
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock order avg prices"))?
            .remove(&client_order_id);
        pending_combo_fills
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending combo fills"))?
            .remove(&client_order_id);
        pending_combo_fill_avgs
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending combo avg chunks"))?
            .remove(&client_order_id);
        order_fill_progress
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock order fill progress"))?
            .remove(&client_order_id);
        pending_cancel_orders
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending cancel orders map"))?
            .remove(&client_order_id);
        spread_fill_tracking
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock spread fill tracking"))?
            .remove(&client_order_id);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_execution_data(
        exec_data: &ExecutionData,
        order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        commission_cache: &Arc<Mutex<CommissionCache>>,
        spread_fill_tracking: &Arc<Mutex<AHashMap<ClientOrderId, ahash::AHashSet<String>>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        active_order_contexts: &Arc<Mutex<AHashMap<i32, TrackedOrderContext>>>,
        terminal_order_contexts: &Arc<Mutex<FifoCacheMap<i32, TrackedOrderContext, 10_000>>>,
        order_avg_prices: &Arc<Mutex<AHashMap<ClientOrderId, Price>>>,
        pending_combo_fills: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
    ) -> anyhow::Result<()> {
        let tracked_context = Self::get_tracked_order_context(
            exec_data.execution.order_id,
            active_order_contexts,
            terminal_order_contexts,
        )?;
        let mapped_client_order_id = {
            let map = venue_order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock venue order ID map"))?;
            map.get(&exec_data.execution.order_id).copied()
        };

        let client_order_id = if let Some(client_order_id) = mapped_client_order_id {
            client_order_id
        } else if let Some(context) = tracked_context.as_ref() {
            context.client_order_id
        } else if let Some(order_ref) =
            parse::normalized_order_ref(&exec_data.execution.order_reference)
        {
            let client_order_id = ClientOrderId::from(order_ref);
            order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock order ID map"))?
                .insert(client_order_id, exec_data.execution.order_id);
            venue_order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock venue order ID map"))?
                .insert(exec_data.execution.order_id, client_order_id);
            client_order_id
        } else {
            tracing::debug!(
                "Execution data for unknown order ID: {}",
                exec_data.execution.order_id
            );
            return Ok(());
        };

        let instrument_id = if let Some(mapped_id) =
            Self::get_mapped_instrument_id(exec_data.execution.order_id, instrument_id_map)?
        {
            mapped_id
        } else if let Some(cached_id) =
            instrument_provider.get_instrument_id_by_contract_id(exec_data.contract.contract_id)
        {
            cached_id
        } else {
            Self::resolve_contract_instrument_id(instrument_provider, &exec_data.contract)?
        };

        let (commission, commission_currency) = {
            let mut cache = commission_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock commission cache"))?;
            let Some((commission, commission_currency)) =
                cache.remove(&exec_data.execution.execution_id)
            else {
                tracing::debug!(
                    "Execution data {} is waiting for commission report",
                    exec_data.execution.execution_id
                );
                return Ok(());
            };
            (commission, commission_currency)
        };

        let is_bag = matches!(
            exec_data.contract.security_type,
            ibapi::contracts::SecurityType::Spread
        ) || !exec_data.contract.combo_legs.is_empty();

        let spread_instrument_id = {
            let map = instrument_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock instrument ID map"))?;
            map.get(&exec_data.execution.order_id).copied()
        }
        .or_else(|| {
            tracked_context
                .as_ref()
                .map(|context| context.instrument_id)
        });

        let is_spread = if let Some(spread_id) = spread_instrument_id {
            if let Some(instrument) = instrument_provider.find(&spread_id) {
                instrument.is_spread()
            } else {
                false
            }
        } else {
            false
        };

        let is_spread_id = instrument_id.symbol.as_str().contains("_(")
            || instrument_id.symbol.as_str().contains(")_");

        let avg_px = {
            let avg_prices = order_avg_prices
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock order avg prices"))?;
            avg_prices.get(&client_order_id).copied()
        }
        .or_else(|| tracked_context.as_ref().and_then(|context| context.avg_px));

        let venue_order_id =
            parse::ib_venue_order_id(exec_data.execution.order_id, exec_data.execution.perm_id);

        if tracked_context.is_some() {
            Self::emit_order_accepted_for_fill_if_needed(
                exec_data.execution.order_id,
                venue_order_id,
                account_id,
                parse_execution_time(&exec_data.execution.time)?,
                active_order_contexts,
                terminal_order_contexts,
                exec_sender,
            )?;
        }

        if (is_bag || is_spread_id)
            && is_spread
            && let Some(spread_id) = spread_instrument_id
            && let Some(context) = tracked_context.as_ref()
        {
            if let Err(e) = Self::handle_spread_execution(
                exec_data,
                client_order_id,
                spread_id,
                &instrument_id,
                commission,
                &commission_currency,
                instrument_provider,
                exec_sender,
                ts_init,
                account_id,
                spread_fill_tracking,
                context,
                pending_combo_fills,
                pending_combo_fill_avgs,
                order_fill_progress,
                avg_px,
            )
            .await
            {
                tracing::warn!(
                    "Error handling spread execution, falling back to regular fill: {e}"
                );
            } else {
                return Ok(());
            }
        }

        let fill_report = parse_execution_to_fill_report(
            &exec_data.execution,
            &exec_data.contract,
            commission,
            &commission_currency,
            instrument_id,
            account_id,
            instrument_provider,
            ts_init,
            avg_px,
        )?;

        if let Some(context) = tracked_context {
            let quote_currency = instrument_provider
                .find(&context.instrument_id)
                .with_context(|| {
                    format!(
                        "Instrument {} not found for tracked fill",
                        context.instrument_id
                    )
                })?
                .quote_currency();
            let event = OrderFilled::new(
                context.trader_id,
                context.strategy_id,
                context.instrument_id,
                context.client_order_id,
                fill_report.venue_order_id,
                fill_report.account_id,
                fill_report.trade_id,
                context.order_side,
                context.order_type,
                fill_report.last_qty,
                fill_report.last_px,
                quote_currency,
                fill_report.liquidity_side,
                UUID4::new(),
                fill_report.ts_event,
                fill_report.ts_init,
                false,
                fill_report.venue_position_id,
                Some(fill_report.commission),
                None,
            );
            exec_sender.send(ExecutionEvent::Order(OrderEventAny::Filled(event)))?;
        } else {
            exec_sender.send(ExecutionEvent::Report(ExecutionReport::Fill(Box::new(
                fill_report,
            ))))?;
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update_order_avg_price(
        client_order_id: ClientOrderId,
        instrument_id: &InstrumentId,
        avg_fill_price: f64,
        filled: f64,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        order_avg_prices: &Arc<Mutex<AHashMap<ClientOrderId, Price>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
    ) -> anyhow::Result<()> {
        let is_spread_order = is_spread_instrument_id(instrument_id);
        if filled <= 0.0 || !parse::should_use_avg_fill_price(avg_fill_price, instrument_id) {
            return Ok(());
        }

        let Some(instrument) = instrument_provider.find(instrument_id) else {
            return Ok(());
        };

        let price_magnifier = instrument_provider.get_price_magnifier(instrument_id) as f64;
        let converted_avg_price = avg_fill_price * price_magnifier;
        let avg_px = Price::new(converted_avg_price, instrument.price_precision());

        order_avg_prices
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock order avg prices"))?
            .insert(client_order_id, avg_px);

        let filled_decimal = Decimal::from_f64_retain(filled)
            .ok_or_else(|| anyhow::anyhow!("Failed to convert filled qty to Decimal: {filled}"))?;
        let avg_decimal = Decimal::from_f64_retain(converted_avg_price).ok_or_else(|| {
            anyhow::anyhow!("Failed to convert avg fill price to Decimal: {converted_avg_price}")
        })?;

        let mut progress = order_fill_progress
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock order fill progress"))?;
        let (previous_filled, previous_notional) = progress
            .get(&client_order_id)
            .copied()
            .unwrap_or((Decimal::ZERO, Decimal::ZERO));
        let total_notional = filled_decimal * avg_decimal;
        progress.insert(client_order_id, (filled_decimal, total_notional));
        drop(progress);

        let fill_delta = filled_decimal - previous_filled;
        if fill_delta <= Decimal::ZERO || !is_spread_order {
            return Ok(());
        }

        let notional_delta = total_notional - previous_notional;
        let partial_avg_decimal = notional_delta / fill_delta;
        let partial_avg_px =
            Price::from_decimal_dp(partial_avg_decimal, instrument.price_precision())
                .map_err(|e| anyhow::anyhow!("Failed to create avg_px price: {e}"))?;

        pending_combo_fill_avgs
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending combo avg chunks"))?
            .entry(client_order_id)
            .or_insert_with(VecDeque::new)
            .push_back((fill_delta, partial_avg_px));

        Ok(())
    }

    pub(super) fn flush_pending_combo_fills(
        client_order_id: ClientOrderId,
        pending_combo_fills: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
    ) -> anyhow::Result<()> {
        let mut combo_fills = pending_combo_fills
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending combo fills"))?;
        let mut avg_chunks = pending_combo_fill_avgs
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock pending combo avg chunks"))?;

        loop {
            let maybe_fill = combo_fills
                .get(&client_order_id)
                .and_then(|fills| fills.front().cloned());
            let maybe_avg = avg_chunks
                .get(&client_order_id)
                .and_then(|chunks| chunks.front().cloned());

            let (fill, (avg_qty, avg_px)) = match (maybe_fill, maybe_avg) {
                (Some(fill), Some(avg)) => (fill, avg),
                _ => break,
            };

            let fill_qty_decimal = fill.last_qty.as_decimal();
            if fill_qty_decimal > avg_qty {
                break;
            }

            let event = OrderFilled::new(
                fill.trader_id,
                fill.strategy_id,
                fill.instrument_id,
                fill.client_order_id,
                fill.venue_order_id,
                fill.account_id,
                fill.trade_id,
                fill.order_side,
                fill.order_type,
                fill.last_qty,
                avg_px,
                fill.quote_currency,
                fill.liquidity_side,
                UUID4::new(),
                fill.ts_event,
                fill.ts_init,
                false,
                None,
                Some(fill.commission),
                None,
            );
            exec_sender.send(ExecutionEvent::Order(OrderEventAny::Filled(event)))?;

            if let Some(fills) = combo_fills.get_mut(&client_order_id) {
                fills.pop_front();
                if fills.is_empty() {
                    combo_fills.remove(&client_order_id);
                }
            }

            if let Some(chunks) = avg_chunks.get_mut(&client_order_id) {
                if fill_qty_decimal == avg_qty {
                    chunks.pop_front();
                } else {
                    chunks[0] = (avg_qty - fill_qty_decimal, avg_px);
                }

                if chunks.is_empty() {
                    avg_chunks.remove(&client_order_id);
                }
            }
        }

        if !combo_fills.contains_key(&client_order_id) {
            order_fill_progress
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock order fill progress"))?
                .remove(&client_order_id);
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_whatif_order(
        order_data: &ibapi::orders::OrderData,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
    ) -> anyhow::Result<()> {
        let client_order_id = {
            let map = venue_order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock venue order ID map"))?;
            map.get(&order_data.order_id).copied()
        };

        if client_order_id.is_none() {
            tracing::debug!(
                "What-if order for unknown order ID: {}",
                order_data.order_id
            );
            return Ok(());
        }
        let client_order_id = client_order_id.expect("checked above");

        let instrument_id = Self::get_mapped_instrument_id(order_data.order_id, instrument_id_map)?
            .map(Ok)
            .unwrap_or_else(|| {
                Self::resolve_contract_instrument_id(instrument_provider, &order_data.contract)
            })?;

        let (trader_id, strategy_id) = Self::get_required_order_actor_ids(
            order_data.order_id,
            trader_id_map,
            strategy_id_map,
        )?;

        let reason_json = serde_json::to_string(&order_data.order_state)
            .unwrap_or_else(|_| format!("whatIf analysis for order {}", order_data.order_id));

        let event = OrderRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            account_id,
            Ustr::from(&reason_json),
            UUID4::new(),
            ts_init,
            ts_init,
            false,
            false,
        );

        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Rejected(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order rejected event: {e}"))?;

        tracing::debug!(
            "What-if analysis completed for order {}: margin change={:?}, commission={:?}",
            client_order_id,
            order_data
                .order_state
                .initial_margin_after
                .and_then(|after| order_data
                    .order_state
                    .initial_margin_before
                    .map(|before| after - before)),
            order_data.order_state.commission
        );

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_spread_execution(
        exec_data: &ExecutionData,
        client_order_id: ClientOrderId,
        spread_instrument_id: InstrumentId,
        leg_instrument_id: &InstrumentId,
        commission: f64,
        commission_currency: &str,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        spread_fill_tracking: &Arc<Mutex<AHashMap<ClientOrderId, ahash::AHashSet<String>>>>,
        context: &TrackedOrderContext,
        pending_combo_fills: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
        avg_px: Option<Price>,
    ) -> anyhow::Result<()> {
        let trade_id = TradeId::new(&exec_data.execution.execution_id);
        let fill_id = trade_id.to_string();

        let fill_count = {
            let mut tracking = spread_fill_tracking
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock spread fill tracking"))?;

            let fill_set = tracking
                .entry(client_order_id)
                .or_insert_with(ahash::AHashSet::new);

            if fill_set.contains(&fill_id) {
                tracing::debug!(
                    "Fill {} already processed for spread order {}, skipping",
                    fill_id,
                    client_order_id
                );
                return Ok(());
            }

            fill_set.insert(fill_id);
            fill_set.len()
        };

        let (leg_id, ratio) = Self::get_leg_instrument_id_and_ratio(
            &exec_data.contract,
            leg_instrument_id,
            instrument_provider,
        );

        let spread_n_legs =
            crate::common::parse::parse_spread_instrument_id_to_legs(&spread_instrument_id)?.len();

        if (fill_count - 1) % spread_n_legs == 0 {
            let pending_combo_fill = Self::build_pending_combo_fill(
                exec_data,
                client_order_id,
                spread_instrument_id,
                leg_id,
                ratio,
                commission,
                commission_currency,
                instrument_provider,
                ts_init,
                account_id,
                context,
            )?;
            let combo_qty = pending_combo_fill.last_qty.as_decimal();
            pending_combo_fills
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock pending combo fills"))?
                .entry(client_order_id)
                .or_insert_with(VecDeque::new)
                .push_back(pending_combo_fill);
            let mut avg_chunks = pending_combo_fill_avgs
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock pending combo avg chunks"))?;

            if !avg_chunks.contains_key(&client_order_id)
                && let Some(avg_px) = avg_px
            {
                avg_chunks.insert(client_order_id, VecDeque::from([(combo_qty, avg_px)]));
            }
            drop(avg_chunks);
            Self::flush_pending_combo_fills(
                client_order_id,
                pending_combo_fills,
                pending_combo_fill_avgs,
                order_fill_progress,
                exec_sender,
            )?;
        }

        Self::generate_leg_fill(
            exec_data,
            client_order_id,
            spread_instrument_id,
            leg_id,
            ratio,
            commission,
            commission_currency,
            instrument_provider,
            exec_sender,
            ts_init,
            account_id,
            avg_px,
        )?;

        Ok(())
    }

    pub(super) fn get_leg_instrument_id_and_ratio(
        contract: &ibapi::contracts::Contract,
        leg_instrument_id: &InstrumentId,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
    ) -> (InstrumentId, i32) {
        if let Some(leg_id) =
            instrument_provider.get_instrument_id_by_contract_id(contract.contract_id)
        {
            if let Some(combo_leg) = contract.combo_legs.iter().find(|leg| {
                if let Some(matched_id) =
                    instrument_provider.get_instrument_id_by_contract_id(leg.contract_id)
                {
                    matched_id == leg_id
                } else {
                    false
                }
            }) {
                let ratio = IbAction::from_str(combo_leg.action.as_str())
                    .map_or(-combo_leg.ratio, |action| {
                        action.signed_multiplier() * combo_leg.ratio
                    });
                return (leg_id, ratio);
            }
        }

        (*leg_instrument_id, 1)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_pending_combo_fill(
        exec_data: &ExecutionData,
        client_order_id: ClientOrderId,
        spread_instrument_id: InstrumentId,
        _leg_instrument_id: InstrumentId,
        ratio: i32,
        commission: f64,
        commission_currency: &str,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        ts_init: UnixNanos,
        account_id: AccountId,
        context: &TrackedOrderContext,
    ) -> anyhow::Result<PendingComboFill> {
        let spread_instrument = instrument_provider
            .find(&spread_instrument_id)
            .context("Spread instrument not found")?;

        let combo_quantity_value = exec_data.execution.shares / (ratio.abs() as f64);
        let combo_quantity =
            Quantity::new(combo_quantity_value, spread_instrument.size_precision());

        let n_legs = spread_instrument_id.symbol.as_str().matches('_').count() + 1;
        let combo_commission_value = commission * (n_legs as f64) / (ratio.abs() as f64);
        let commission_money =
            Money::new(combo_commission_value, Currency::from(commission_currency));

        let ts_event = parse_execution_time(&exec_data.execution.time)?;

        Ok(PendingComboFill {
            trader_id: context.trader_id,
            strategy_id: context.strategy_id,
            account_id,
            instrument_id: spread_instrument_id,
            venue_order_id: parse::ib_venue_order_id(
                exec_data.execution.order_id,
                exec_data.execution.perm_id,
            ),
            trade_id: TradeId::new(&exec_data.execution.execution_id),
            order_side: context.order_side,
            order_type: context.order_type,
            last_qty: combo_quantity,
            commission: commission_money,
            liquidity_side: LiquiditySide::NoLiquiditySide,
            quote_currency: spread_instrument.quote_currency(),
            client_order_id,
            ts_event,
            ts_init,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn generate_leg_fill(
        exec_data: &ExecutionData,
        client_order_id: ClientOrderId,
        spread_instrument_id: InstrumentId,
        leg_instrument_id: InstrumentId,
        _ratio: i32,
        commission: f64,
        commission_currency: &str,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        avg_px: Option<Price>,
    ) -> anyhow::Result<()> {
        let leg_instrument = instrument_provider
            .find(&leg_instrument_id)
            .context("Leg instrument not found")?;

        let price_magnifier = instrument_provider.get_price_magnifier(&leg_instrument_id) as f64;
        let execution_price = exec_data.execution.price * price_magnifier;
        let leg_price = Price::new(execution_price, leg_instrument.price_precision());

        let leg_quantity =
            Quantity::new(exec_data.execution.shares, leg_instrument.size_precision());

        let order_side = IbAction::from_str(exec_data.execution.side.as_str())?.order_side();

        let commission_money = Money::new(commission, Currency::from(commission_currency));

        let leg_position = Self::get_leg_position(&spread_instrument_id, &leg_instrument_id);
        let leg_client_order_id = ClientOrderId::new(format!(
            "{}-LEG-{}",
            client_order_id, leg_instrument_id.symbol
        ));
        let leg_trade_id = TradeId::new(format!(
            "{}-{}",
            exec_data.execution.execution_id, leg_position
        ));
        let venue_order_id =
            parse::ib_venue_order_id(exec_data.execution.order_id, exec_data.execution.perm_id);
        let leg_venue_order_id =
            VenueOrderId::new(format!("{}-LEG-{}", venue_order_id.as_str(), leg_position));

        let ts_event = parse_execution_time(&exec_data.execution.time)?;

        let mut fill_report = FillReport::new(
            account_id,
            leg_instrument_id,
            leg_venue_order_id,
            leg_trade_id,
            order_side,
            leg_quantity,
            leg_price,
            commission_money,
            LiquiditySide::NoLiquiditySide,
            Some(leg_client_order_id),
            None,
            ts_event,
            ts_init,
            None,
        );

        if let Some(price) = avg_px {
            fill_report.avg_px = Some(price.as_decimal());
        }

        exec_sender.send(ExecutionEvent::Report(ExecutionReport::Fill(Box::new(
            fill_report,
        ))))?;

        tracing::debug!(
            "Generated leg fill: instrument_id={}, client_order_id={}, quantity={}, price={}",
            leg_instrument_id,
            leg_client_order_id,
            leg_quantity,
            leg_price
        );

        Ok(())
    }

    pub(super) fn get_leg_position(
        spread_instrument_id: &InstrumentId,
        leg_instrument_id: &InstrumentId,
    ) -> usize {
        let legs =
            match crate::common::parse::parse_spread_instrument_id_to_legs(spread_instrument_id) {
                Ok(legs) => legs,
                Err(e) => {
                    log::warn!(
                        "Failed to parse spread instrument ID {} for leg position: {e}",
                        spread_instrument_id
                    );
                    return 0;
                }
            };

        for (idx, (parsed_leg_id, _)) in legs.iter().enumerate() {
            if *parsed_leg_id == *leg_instrument_id {
                return idx;
            }
        }

        log::warn!(
            "Leg instrument ID {} not found in spread instrument ID {}",
            leg_instrument_id,
            spread_instrument_id
        );
        0
    }

    fn resolve_contract_instrument_id(
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        contract: &Contract,
    ) -> anyhow::Result<InstrumentId> {
        match instrument_provider.resolve_instrument_id_for_contract(contract) {
            Ok(instrument_id) => Ok(instrument_id),
            Err(provider_error) if contract.security_type != SecurityType::Spread => {
                ib_contract_to_instrument_id_simple(contract).with_context(|| {
                    format!(
                        "Failed to resolve IBKR contract to instrument ID using provider ({provider_error}) or simple conversion",
                    )
                })
            }
            Err(provider_error) => Err(provider_error)
                .context("Failed to resolve BAG contract to spread instrument ID"),
        }
    }
}
