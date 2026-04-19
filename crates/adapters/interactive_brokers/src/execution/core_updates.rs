// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

use super::*;
use crate::execution::parse;

impl InteractiveBrokersExecutionClient {
    /// Starts the order update subscription stream.
    ///
    /// # Errors
    ///
    /// Returns an error if starting the subscription fails.
    pub(super) async fn start_order_updates(&self) -> anyhow::Result<()> {
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
        let instrument_id_map = Arc::clone(&self.instrument_id_map);
        let trader_id_map = Arc::clone(&self.trader_id_map);
        let strategy_id_map = Arc::clone(&self.strategy_id_map);
        let spread_fill_tracking = Arc::clone(&self.spread_fill_tracking);
        let order_avg_prices = Arc::clone(&self.order_avg_prices);
        let pending_combo_fills = Arc::clone(&self.pending_combo_fills);
        let pending_combo_fill_avgs = Arc::clone(&self.pending_combo_fill_avgs);
        let order_fill_progress = Arc::clone(&self.order_fill_progress);
        let accepted_orders = Arc::clone(&self.accepted_orders);
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
                &instrument_id_map,
                &trader_id_map,
                &strategy_id_map,
                &spread_fill_tracking,
                &order_avg_prices,
                &pending_combo_fills,
                &pending_combo_fill_avgs,
                &order_fill_progress,
                &accepted_orders,
                &pending_cancel_orders,
            )
            .await;
        });

        self.order_update_handle
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock order update handle"))?
            .replace(handle);

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
        commission_cache: &Arc<Mutex<AHashMap<String, (f64, String)>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        spread_fill_tracking: &Arc<Mutex<AHashMap<ClientOrderId, ahash::AHashSet<String>>>>,
        order_avg_prices: &Arc<Mutex<AHashMap<ClientOrderId, Price>>>,
        pending_combo_fills: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
        accepted_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
        pending_cancel_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
    ) {
        while let Some(update_result) = subscription.next().await {
            match update_result {
                Ok(update) => {
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
                        spread_fill_tracking,
                        order_avg_prices,
                        pending_combo_fills,
                        pending_combo_fill_avgs,
                        order_fill_progress,
                        accepted_orders,
                        pending_cancel_orders,
                    )
                    .await
                    {
                        tracing::error!("Error handling order update: {e}");
                    }
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
        commission_cache: &Arc<Mutex<AHashMap<String, (f64, String)>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        spread_fill_tracking: &Arc<Mutex<AHashMap<ClientOrderId, ahash::AHashSet<String>>>>,
        order_avg_prices: &Arc<Mutex<AHashMap<ClientOrderId, Price>>>,
        pending_combo_fills: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
        accepted_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
        pending_cancel_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
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
                    order_avg_prices,
                    pending_combo_fills,
                    pending_combo_fill_avgs,
                    order_fill_progress,
                    accepted_orders,
                    pending_cancel_orders,
                )
                .await?;
            }
            OrderUpdate::ExecutionData(exec_data) => {
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
                    trader_id_map,
                    strategy_id_map,
                    order_avg_prices,
                    pending_combo_fills,
                    pending_combo_fill_avgs,
                    order_fill_progress,
                )
                .await?;
            }
            OrderUpdate::CommissionReport(commission) => {
                let mut cache = commission_cache
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Failed to lock commission cache"))?;
                cache.insert(
                    commission.execution_id.clone(),
                    (commission.commission, commission.currency.clone()),
                );
            }
            OrderUpdate::OpenOrder(order_data) => {
                if order_data.order.what_if && order_data.order_state.status == "PreSubmitted" {
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

                    let client_order_id = if !order_data.order.order_ref.is_empty() {
                        let order_ref = if let Some(pos) = order_data.order.order_ref.rfind(':') {
                            &order_data.order.order_ref[..pos]
                        } else {
                            &order_data.order.order_ref
                        };
                        Some(ClientOrderId::from(order_ref))
                    } else {
                        let map = venue_order_id_map
                            .lock()
                            .map_err(|_| anyhow::anyhow!("Failed to lock venue order ID map"))?;
                        map.get(&order_data.order_id).copied()
                    };

                    if let Some(client_order_id) = client_order_id
                        && matches!(status_str, "Submitted" | "PreSubmitted")
                    {
                        let mut accepted = accepted_orders
                            .lock()
                            .map_err(|_| anyhow::anyhow!("Failed to lock accepted orders map"))?;

                        if !accepted.contains(&client_order_id) {
                            accepted.insert(client_order_id);

                            let instrument_id = {
                                Self::get_mapped_instrument_id(
                                    order_data.order_id,
                                    instrument_id_map,
                                )?
                                .map(Ok)
                                .unwrap_or_else(|| {
                                    crate::common::parse::ib_contract_to_instrument_id_simple(
                                        &order_data.contract,
                                    )
                                })?
                            };

                            let (trader_id, strategy_id) = Self::get_required_order_actor_ids(
                                order_data.order_id,
                                trader_id_map,
                                strategy_id_map,
                            )?;

                            let event = OrderAccepted::new(
                                trader_id,
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                VenueOrderId::from(order_data.order_id.to_string()),
                                account_id,
                                UUID4::new(),
                                ts_init,
                                ts_init,
                                false,
                            );
                            exec_sender
                                .send(ExecutionEvent::Order(OrderEventAny::Accepted(event)))
                                .map_err(|e| {
                                    anyhow::anyhow!("Failed to send order accepted event: {e}")
                                })?;

                            tracing::info!(
                                "Order {} accepted (IB openOrder status: {})",
                                client_order_id,
                                status_str
                            );
                        }
                    }
                }
            }
            OrderUpdate::Message(notice) => {
                tracing::debug!("Received notice: {notice:?}");
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_order_status(
        status: &IBOrderStatus,
        _order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        order_avg_prices: &Arc<Mutex<AHashMap<ClientOrderId, Price>>>,
        pending_combo_fills: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
        accepted_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
        pending_cancel_orders: &Arc<Mutex<ahash::AHashSet<ClientOrderId>>>,
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
            status.average_fill_price,
            status.filled,
            instrument_provider,
            order_avg_prices,
            pending_combo_fill_avgs,
            order_fill_progress,
        )?;

        if matches!(
            status.status.as_str(),
            "Filled" | "ApiCancelled" | "Cancelled" | "Inactive"
        ) {
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

        let venue_order_id = VenueOrderId::from(format!("{}", status.order_id));
        let status_str = status.status.as_str();

        match status_str {
            "Submitted" | "PreSubmitted" => {
                let mut accepted = accepted_orders
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Failed to lock accepted orders map"))?;

                if !accepted.contains(&client_order_id) {
                    accepted.insert(client_order_id);

                    let (trader_id, strategy_id) = Self::get_required_order_actor_ids(
                        status.order_id,
                        trader_id_map,
                        strategy_id_map,
                    )?;

                    let event = OrderAccepted::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        account_id,
                        UUID4::new(),
                        ts_init,
                        ts_init,
                        false,
                    );
                    exec_sender
                        .send(ExecutionEvent::Order(OrderEventAny::Accepted(event)))
                        .map_err(|e| anyhow::anyhow!("Failed to send order accepted event: {e}"))?;

                    tracing::info!(
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
            "Filled" => {
                tracing::debug!(
                    "Order {} filled (IB status: {})",
                    client_order_id,
                    status_str
                );
            }
            "Cancelled" | "ApiCancelled" => {
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
                tracing::info!("Order {} canceled", client_order_id);
            }
            "PendingCancel" => {
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
                tracing::info!("Order {} pending cancel", client_order_id);
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

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_execution_data(
        exec_data: &ExecutionData,
        _order_id_map: &Arc<Mutex<AHashMap<ClientOrderId, i32>>>,
        venue_order_id_map: &Arc<Mutex<AHashMap<i32, ClientOrderId>>>,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        commission_cache: &Arc<Mutex<AHashMap<String, (f64, String)>>>,
        spread_fill_tracking: &Arc<Mutex<AHashMap<ClientOrderId, ahash::AHashSet<String>>>>,
        instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
        order_avg_prices: &Arc<Mutex<AHashMap<ClientOrderId, Price>>>,
        pending_combo_fills: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<PendingComboFill>>>>,
        pending_combo_fill_avgs: &Arc<Mutex<AHashMap<ClientOrderId, VecDeque<(Decimal, Price)>>>>,
        order_fill_progress: &Arc<Mutex<AHashMap<ClientOrderId, (Decimal, Decimal)>>>,
    ) -> anyhow::Result<()> {
        let client_order_id = {
            let map = venue_order_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock venue order ID map"))?;
            map.get(&exec_data.execution.order_id).copied()
        };

        let Some(client_order_id) = client_order_id else {
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
            crate::common::parse::ib_contract_to_instrument_id_simple(&exec_data.contract)
                .context("Failed to convert IB contract to instrument ID")?
        };

        let (commission, commission_currency) = {
            let cache = commission_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock commission cache"))?;
            cache.get(&exec_data.execution.execution_id).map_or_else(
                || (0.0, "USD".to_string()),
                |(comm, curr)| (*comm, curr.clone()),
            )
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
        };

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
        };

        if (is_bag || is_spread_id)
            && is_spread
            && let Some(spread_id) = spread_instrument_id
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
                instrument_id_map,
                trader_id_map,
                strategy_id_map,
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

        exec_sender.send(ExecutionEvent::Report(ExecutionReport::Fill(Box::new(
            fill_report,
        ))))?;

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

        let filled_decimal = Decimal::from_str(&filled.to_string())
            .map_err(|e| anyhow::anyhow!("Failed to convert filled qty to Decimal: {e}"))?;
        let avg_decimal = Decimal::from_str(&converted_avg_price.to_string())
            .map_err(|e| anyhow::anyhow!("Failed to convert avg fill price to Decimal: {e}"))?;

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

            let mut report = FillReport::new(
                fill.account_id,
                fill.instrument_id,
                fill.venue_order_id,
                fill.trade_id,
                fill.order_side,
                fill.last_qty,
                fill.last_px,
                fill.commission,
                fill.liquidity_side,
                Some(fill.client_order_id),
                None,
                fill.ts_event,
                fill.ts_init,
                None,
            );
            report.avg_px = Some(avg_px.as_decimal());

            exec_sender.send(ExecutionEvent::Report(ExecutionReport::Fill(Box::new(
                report,
            ))))?;

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
        _instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
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
                crate::common::parse::ib_contract_to_instrument_id_simple(&order_data.contract)
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

        tracing::info!(
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
        _instrument_id_map: &Arc<Mutex<AHashMap<i32, InstrumentId>>>,
        trader_id_map: &Arc<Mutex<AHashMap<i32, TraderId>>>,
        strategy_id_map: &Arc<Mutex<AHashMap<i32, StrategyId>>>,
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

        let (trader_id, strategy_id) = {
            let trader_map = trader_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock trader ID map"))?;
            let strategy_map = strategy_id_map
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock strategy ID map"))?;

            let venue_order_id = exec_data.execution.order_id;
            (
                trader_map.get(&venue_order_id).copied(),
                strategy_map.get(&venue_order_id).copied(),
            )
        };

        let trader_id =
            trader_id.ok_or_else(|| anyhow::anyhow!("Trader ID not found for order"))?;
        let strategy_id =
            strategy_id.ok_or_else(|| anyhow::anyhow!("Strategy ID not found for order"))?;

        let (leg_id, ratio) = Self::get_leg_instrument_id_and_ratio(
            &exec_data.contract,
            leg_instrument_id,
            instrument_provider,
        );

        let spread_n_legs = spread_instrument_id.symbol.as_str().matches('_').count() + 1;
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
            )?;
            pending_combo_fills
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock pending combo fills"))?
                .entry(client_order_id)
                .or_insert_with(VecDeque::new)
                .push_back(pending_combo_fill);
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
            trader_id,
            strategy_id,
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
                let ratio = if combo_leg.action == "BUY" {
                    combo_leg.ratio
                } else {
                    -combo_leg.ratio
                };
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
    ) -> anyhow::Result<PendingComboFill> {
        let spread_instrument = instrument_provider
            .find(&spread_instrument_id)
            .context("Spread instrument not found")?;

        let price_magnifier = instrument_provider.get_price_magnifier(&spread_instrument_id) as f64;
        let execution_price = exec_data.execution.price * price_magnifier;
        let combo_price = Price::new(execution_price, spread_instrument.price_precision());

        let combo_quantity_value = exec_data.execution.shares / (ratio.abs() as f64);
        let combo_quantity =
            Quantity::new(combo_quantity_value, spread_instrument.size_precision());

        let execution_side_numeric = match exec_data.execution.side.as_str() {
            "BUY" | "BOT" => 1,
            "SELL" | "SLD" => -1,
            _ => anyhow::bail!("Unknown execution side: {}", exec_data.execution.side),
        };
        let leg_side_numeric = if ratio >= 0 { 1 } else { -1 };
        let combo_order_side = if execution_side_numeric == leg_side_numeric {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let n_legs = spread_instrument_id.symbol.as_str().matches('_').count() + 1;
        let combo_commission_value = commission * (n_legs as f64) / (ratio.abs() as f64);
        let commission_money =
            Money::new(combo_commission_value, Currency::from(commission_currency));

        let ts_event = parse_execution_time(&exec_data.execution.time)?;

        Ok(PendingComboFill {
            account_id,
            instrument_id: spread_instrument_id,
            venue_order_id: VenueOrderId::new(exec_data.execution.order_id.to_string()),
            trade_id: TradeId::new(&exec_data.execution.execution_id),
            order_side: combo_order_side,
            last_qty: combo_quantity,
            last_px: combo_price,
            commission: commission_money,
            liquidity_side: LiquiditySide::NoLiquiditySide,
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
        _trader_id: TraderId,
        _strategy_id: StrategyId,
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

        let order_side = match exec_data.execution.side.as_str() {
            "BUY" | "BOT" => OrderSide::Buy,
            "SELL" | "SLD" => OrderSide::Sell,
            _ => anyhow::bail!("Unknown execution side: {}", exec_data.execution.side),
        };

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
        let leg_venue_order_id = VenueOrderId::new(format!(
            "{}-LEG-{}",
            exec_data.execution.order_id, leg_position
        ));

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

        tracing::info!(
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
        let symbol_str = spread_instrument_id.symbol.as_str();
        let components: Vec<&str> = symbol_str.split('_').collect();

        for (idx, component) in components.iter().enumerate() {
            let symbol_part = if component.contains("((") {
                if let Some(end) = component.find("))") {
                    &component[end + 2..]
                } else {
                    continue;
                }
            } else if component.starts_with('(') {
                if let Some(end) = component.find(')') {
                    &component[end + 1..]
                } else {
                    continue;
                }
            } else {
                continue;
            };

            if leg_instrument_id.symbol.as_str() == symbol_part {
                return idx;
            }
        }

        0
    }
}
