// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Interactive Brokers order and execution update handling.

use nautilus_common::live::sender::EventSender;

use super::core::*;
use crate::{
    common::enums::{IbOrderStatus, IbOrderType},
    execution::parse,
};

const COMMISSION_REPORT_TIMEOUT: Duration = Duration::from_secs(5);

/// Venue access for notice handling spawned from the update and notice streams.
pub(super) struct NoticeVenueContext {
    pub(super) client: Arc<Client>,
    pub(super) request_timeout_secs: u64,
    pub(super) cancellation: CancellationToken,
    pub(super) tasks: TaskSpawner,
}

impl InteractiveBrokersExecutionClient {
    fn execution_in_account(exec_data: &ExecutionData, ib_account: Ustr) -> anyhow::Result<bool> {
        anyhow::ensure!(
            !exec_data.execution.account_number.is_empty(),
            "IB execution {} has no account identity",
            exec_data.execution.execution_id
        );
        Ok(exec_data.execution.account_number == ib_account)
    }

    fn execution_context(
        exec_data: &ExecutionData,
        orders: &OrderTracker,
        provider: &Arc<InteractiveBrokersInstrumentProvider>,
        account_id: AccountId,
    ) -> anyhow::Result<(i32, Option<TrackedOrder>, Option<ClientOrderId>)> {
        anyhow::ensure!(
            exec_data.execution.shares.is_finite()
                && exec_data.execution.shares > 0.0
                && exec_data.execution.price.is_finite(),
            "IB execution {} has invalid price or quantity",
            exec_data.execution.execution_id
        );
        let correlation = orders.lock()?.correlate(
            exec_data.execution.client_id,
            exec_data.execution.order_id,
            exec_data.execution.perm_id,
            &exec_data.execution.order_reference,
        )?;
        let context = match &correlation {
            OrderCorrelation::Tracked { context, .. }
            | OrderCorrelation::Duplicate { context, .. } => Some(context),
            OrderCorrelation::Untracked { .. } => None,
        };

        if let Some(context) = context {
            let instrument = provider.find(&context.instrument_id).with_context(|| {
                format!(
                    "IB execution instrument {} is unavailable",
                    context.instrument_id
                )
            })?;

            if !instrument.is_spread() {
                let contract = provider.resolve_contract_for_instrument(context.instrument_id)?;
                anyhow::ensure!(
                    contract.contract_id == 0
                        || exec_data.contract.contract_id == 0
                        || contract.contract_id == exec_data.contract.contract_id,
                    "IB execution {} conflicts with its tracked contract",
                    exec_data.execution.execution_id
                );
                let expected_side = if context.order_side == OrderSide::Buy {
                    "BOT"
                } else {
                    "SLD"
                };
                anyhow::ensure!(
                    matches!(&correlation, OrderCorrelation::Duplicate { .. })
                        || exec_data.execution.side.as_str() == expected_side,
                    "IB execution {} conflicts with its tracked side",
                    exec_data.execution.execution_id
                );
            }
        }

        match correlation {
            OrderCorrelation::Tracked { order_id, context } => {
                if exec_data.execution.perm_id > 0 {
                    if let Some(order) = orders.lock()?.order_mut(order_id) {
                        if order.perm_id == 0 {
                            order.perm_id = exec_data.execution.perm_id;
                        }
                    }
                }
                let client_order_id = context.client_order_id;
                Ok((order_id, Some(context), Some(client_order_id)))
            }
            OrderCorrelation::Untracked { client_order_id } => {
                Ok((exec_data.execution.order_id, None, client_order_id))
            }
            OrderCorrelation::Duplicate { order_id, context } => {
                let parent_id = context.client_order_id;
                orders.lock()?.record_incarnation(
                    order_id,
                    &context,
                    account_id,
                    exec_data.execution.perm_id,
                    None,
                    false,
                );
                Ok((order_id, None, Some(parent_id)))
            }
        }
    }

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

        let orders = self.orders.clone();
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;
        let ib_account = self.ib_account;
        let commission_cache = Arc::clone(&self.commission_cache);
        let pending_execution_cache = Arc::clone(&self.pending_execution_cache);
        let position_tracker = Arc::clone(&self.position_tracker);
        let is_connected = Arc::clone(&self.is_connected);
        let tasks = self
            .session_tasks
            .spawner()
            .context("failed to acquire IB execution session task spawner")?;
        let cancellation_token = tasks.cancellation_token();
        let venue_ctx = NoticeVenueContext {
            client: client.as_arc().clone(),
            request_timeout_secs: self.config.request_timeout,
            cancellation: tasks.cancellation_token(),
            tasks: tasks.clone(),
        };

        let future = async move {
            tokio::select! {
                () = Self::process_order_update_stream(
                    &mut subscription,
                    &orders,
                    &instrument_provider,
                    &exec_sender,
                    clock,
                    account_id,
                    ib_account,
                    &commission_cache,
                    &pending_execution_cache,
                    &position_tracker,
                    Some(&venue_ctx),
                ) => {
                    tracing::error!(
                        "IB order update stream ended; execution client is disconnected"
                    );
                    is_connected.store(false, Ordering::Relaxed);
                }
                () = cancellation_token.cancelled() => {}
            }
        };

        tasks
            .spawn(future)
            .context("failed to register IB order update task")?;

        self.start_global_notices()?;

        log::debug!("IB order update stream subscription started");

        Ok(())
    }

    fn start_global_notices(&self) -> anyhow::Result<()> {
        let client = self.ib_client.as_ref().context("IB client not connected")?;
        let mut notices = client
            .notice_stream()
            .context("Failed to subscribe to IB global notice stream")?;
        let exec_sender = get_exec_event_sender();
        let clock = get_atomic_clock_realtime();
        let account_id = self.core.account_id;
        let orders = self.orders.clone();
        let is_connected = Arc::clone(&self.is_connected);
        let tasks = self
            .session_tasks
            .spawner()
            .context("failed to acquire IB execution session task spawner")?;
        let cancellation_token = tasks.cancellation_token();
        let venue_ctx = NoticeVenueContext {
            client: client.as_arc().clone(),
            request_timeout_secs: self.config.request_timeout,
            cancellation: tasks.cancellation_token(),
            tasks: tasks.clone(),
        };

        let future = async move {
            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => return,
                    notice = notices.next() => {
                        let Some(notice) = notice else {
                            tracing::warn!(
                                "IB global notice stream ended; execution client is disconnected"
                            );
                            is_connected.store(false, Ordering::Relaxed);
                            return;
                        };

                        if let Err(e) = Self::handle_order_notice(
                            &notice,
                            &orders,
                            &exec_sender,
                            clock.get_time_ns(),
                            account_id,
                            Some(&venue_ctx),
                        ) {
                            tracing::error!("Error handling global IB notice: {e}");
                        }
                    }
                }
            }
        };

        tasks
            .spawn(future)
            .context("failed to register IB global notice task")?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // The stream task owns explicit shared client state.
    pub(super) async fn process_order_update_stream<S>(
        subscription: &mut S,
        orders: &OrderTracker,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        clock: &'static AtomicTime,
        account_id: AccountId,
        ib_account: Ustr,
        commission_cache: &Arc<Mutex<CommissionCache>>,
        pending_execution_cache: &Arc<Mutex<PendingExecutionCache>>,
        position_tracker: &PositionTracker,
        venue_ctx: Option<&NoticeVenueContext>,
    ) where
        S: futures_util::Stream<Item = Result<SubscriptionItem<OrderUpdate>, ibapi::Error>> + Unpin,
    {
        let mut commission_timeout = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_secs(1),
            Duration::from_secs(1),
        );

        loop {
            tokio::select! {
                update_result = subscription.next() => {
                    let Some(update_result) = update_result else {
                        break;
                    };

                    match update_result {
                        Ok(SubscriptionItem::Data(update)) => {
                            if let Err(e) = Self::handle_order_update(
                                &update,
                                orders,
                                instrument_provider,
                                exec_sender,
                                clock,
                                account_id,
                                ib_account,
                                commission_cache,
                                pending_execution_cache,
                                position_tracker,
                            )
                            .await
                            {
                                tracing::error!("Error handling order update: {e}");
                            }
                        }
                        Ok(SubscriptionItem::Notice(notice)) => {
                            if let Err(e) = Self::handle_order_notice(
                                &notice,
                                orders,
                                exec_sender,
                                clock.get_time_ns(),
                                account_id,
                                venue_ctx,
                            ) {
                                tracing::error!("Error handling IB order update notice: {e}");
                            }
                        }
                        Err(e) => {
                            tracing::error!("Error receiving order update: {e}");
                        }
                    }
                }
                _ = commission_timeout.tick() => {
                    if let Err(e) = Self::flush_executions_without_commission(
                        orders,
                        instrument_provider,
                        exec_sender,
                        clock,
                        account_id,
                        ib_account,
                        commission_cache,
                        pending_execution_cache,
                        position_tracker,
                    )
                    .await
                    {
                        tracing::error!("Error flushing IB executions without commission: {e}");
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // Timeout processing shares explicit client state.
    pub(super) async fn flush_executions_without_commission(
        orders: &OrderTracker,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        clock: &'static AtomicTime,
        account_id: AccountId,
        ib_account: Ustr,
        commission_cache: &Arc<Mutex<CommissionCache>>,
        pending_execution_cache: &Arc<Mutex<PendingExecutionCache>>,
        position_tracker: &PositionTracker,
    ) -> anyhow::Result<()> {
        let pending = pending_execution_cache.lock().drain();

        for (execution_id, (received_at, exec_data)) in pending {
            if received_at.elapsed() < COMMISSION_REPORT_TIMEOUT {
                pending_execution_cache
                    .lock()
                    .insert(execution_id, (received_at, exec_data));
                continue;
            }

            let commission_currency = instrument_provider
                .get_instrument_id_by_contract_id(exec_data.contract.contract_id)
                .and_then(|instrument_id| instrument_provider.find(&instrument_id))
                .map_or_else(
                    || exec_data.contract.currency.to_string(),
                    |instrument| instrument.quote_currency().code.to_string(),
                );

            if commission_currency.is_empty() {
                // Keep the execution pending so a late commissionReport or instrument load can
                // still resolve it; aborting here would drop the other drained entries.
                tracing::error!(
                    execution_id,
                    "Commission currency unavailable for IB execution; keeping the fill pending",
                );
                pending_execution_cache
                    .lock()
                    .insert(execution_id, (received_at, exec_data));
                continue;
            }

            tracing::warn!(
                execution_id,
                "IB commissionReport did not arrive within 5 seconds; emitting with zero commission",
            );
            commission_cache
                .lock()
                .insert(execution_id, (0.0, commission_currency));

            if let Err(e) = Self::handle_execution_data(
                &exec_data,
                orders,
                instrument_provider,
                exec_sender,
                clock.get_time_ns(),
                account_id,
                ib_account,
                commission_cache,
                position_tracker,
            )
            .await
            {
                tracing::error!(
                    execution_id = exec_data.execution.execution_id,
                    "Failed to emit IB execution after commission timeout: {e}",
                );
            }
        }

        Ok(())
    }

    pub(super) fn handle_order_notice(
        notice: &Notice,
        orders: &OrderTracker,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        venue_ctx: Option<&NoticeVenueContext>,
    ) -> anyhow::Result<()> {
        match notice.category() {
            NoticeCategory::Cancellation => {
                tracing::debug!("Received IB order cancellation notice: {notice}");
                return Ok(());
            }
            NoticeCategory::Warning
            | NoticeCategory::SystemMessage
            | NoticeCategory::DataAdvisory => {
                tracing::warn!("Received IB order notice: {notice}");
                return Ok(());
            }
            NoticeCategory::OrderRejection | NoticeCategory::Error => {}
            _ => {
                tracing::warn!("Received unclassified IB order notice: {notice}");
                return Ok(());
            }
        }

        let Some(order_id) = notice.request_id else {
            if notice.category() == NoticeCategory::OrderRejection {
                tracing::error!("Received request-less IB order rejection: {notice}");
            } else {
                tracing::error!("Received IB error notice: {notice}");
            }
            return Ok(());
        };
        let order = {
            let state = orders.lock()?;
            state.active_orders.get(&order_id).cloned()
        };
        let Some(order) = order else {
            // The transport forwards every request-scoped error to the order update
            // stream; ids that do not resolve to a tracked order belong to data or
            // historical requests.
            if notice.category() == NoticeCategory::OrderRejection {
                tracing::warn!(
                    "Received IB rejection for unknown order ID {}: {}",
                    order_id,
                    notice
                );
            } else {
                tracing::debug!(
                    "Ignoring IB error notice for untracked request ID {}: {}",
                    order_id,
                    notice
                );
            }
            return Ok(());
        };
        let client_order_id = order.client_order_id;

        if order.accepted {
            return Self::handle_accepted_order_notice(
                notice,
                &order,
                order_id,
                orders,
                exec_sender,
                ts_init,
                account_id,
                venue_ctx,
            );
        }

        // ibapi classifies every code outside its informational ranges as `Error`, including
        // notices for orders IB keeps working (10349 sets the TIF from an order preset), so
        // only an order IB lists nowhere is rejected
        if notice.category() == NoticeCategory::Error
            && let Some(venue_ctx) = venue_ctx
        {
            tracing::warn!(
                "IB notice for submitted order {client_order_id}; checking whether IB lists it: {notice}"
            );
            Self::spawn_order_presence_check(
                venue_ctx,
                order_id,
                client_order_id,
                Some(Ustr::from(notice.message.as_str())),
                orders.clone(),
                exec_sender.clone(),
                account_id,
            );
            return Ok(());
        }

        Self::emit_order_rejected(
            &order,
            account_id,
            notice.message.as_str(),
            ts_init,
            exec_sender,
        )?;
        Self::evict_terminal_order_state(client_order_id, order_id, orders)?;

        tracing::warn!("Order {} rejected: {}", client_order_id, notice.message);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // Notice resolution preserves the stream context.
    fn handle_accepted_order_notice(
        notice: &Notice,
        order: &TrackedOrder,
        order_id: i32,
        orders: &OrderTracker,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        venue_ctx: Option<&NoticeVenueContext>,
    ) -> anyhow::Result<()> {
        let client_order_id = order.client_order_id;

        // A venue-working order stays working: IB delivers 201-class codes for
        // rejected modifications and error codes that do not prove the order died.
        if Self::emit_modify_rejected_if_pending(
            order,
            order_id,
            notice.message.as_str(),
            orders,
            exec_sender,
            ts_init,
            account_id,
        )? {
            return Ok(());
        }

        // IB refuses a cancel of an order it no longer considers cancellable (161, 10148)
        // with an order notice; the presence check below still resolves an order that is gone
        let cancel_perm_id = {
            let mut state = orders.lock()?;
            state.active_orders.get_mut(&order_id).and_then(|order| {
                std::mem::replace(&mut order.pending_cancel, false).then_some(order.perm_id)
            })
        };

        if let Some(perm_id) = cancel_perm_id {
            let event = OrderCancelRejected::new(
                order.trader_id,
                order.strategy_id,
                order.instrument_id,
                client_order_id,
                Ustr::from(notice.message.as_str()),
                UUID4::new(),
                ts_init,
                ts_init,
                false,
                Some(ib_venue_order_id(order_id, perm_id)),
                Some(account_id),
            );
            exec_sender
                .send(ExecutionEvent::Order(OrderEventAny::CancelRejected(event)))
                .map_err(|e| anyhow::anyhow!("Failed to send order cancel rejected event: {e}"))?;
            tracing::warn!(
                "Cancel of order {client_order_id} rejected: {}",
                notice.message
            );
        } else {
            tracing::warn!(
                "IB notice for accepted order {} left non-terminal (venue state uncertain): {}",
                client_order_id,
                notice
            );
        }

        if let Some(venue_ctx) = venue_ctx {
            Self::spawn_order_presence_check(
                venue_ctx,
                order_id,
                client_order_id,
                None,
                orders.clone(),
                exec_sender.clone(),
                account_id,
            );
        }
        Ok(())
    }

    // Rejects the pending modify of an accepted order, returning whether one was pending.
    fn emit_modify_rejected_if_pending(
        order: &TrackedOrder,
        order_id: i32,
        reason: &str,
        orders: &OrderTracker,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
    ) -> anyhow::Result<bool> {
        let (was_pending_modify, perm_id) = {
            let mut state = orders.lock()?;
            state
                .active_orders
                .get_mut(&order_id)
                .map_or((false, 0), |order| {
                    (order.pending_modify.take().is_some(), order.perm_id)
                })
        };

        if !was_pending_modify {
            return Ok(false);
        }

        let event = OrderModifyRejected::new(
            order.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            Ustr::from(reason),
            UUID4::new(),
            ts_init,
            ts_init,
            false,
            Some(ib_venue_order_id(order_id, perm_id)),
            Some(account_id),
        );
        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order modify rejected event: {e}"))?;
        tracing::warn!(
            "Modify of order {} rejected: {}",
            order.client_order_id,
            reason
        );
        Ok(true)
    }

    fn spawn_order_presence_check(
        venue_ctx: &NoticeVenueContext,
        ib_order_id: i32,
        client_order_id: ClientOrderId,
        rejection_reason: Option<Ustr>,
        orders: OrderTracker,
        exec_sender: EventSender<ExecutionEvent>,
        account_id: AccountId,
    ) {
        let client = Arc::clone(&venue_ctx.client);
        let timeout_dur = Duration::from_secs(venue_ctx.request_timeout_secs);
        let cancellation = venue_ctx.cancellation.child_token();

        let future = async move {
            tokio::select! {
                () = Self::check_order_presence(
                    &client,
                    ib_order_id,
                    client_order_id,
                    rejection_reason,
                    timeout_dur,
                    &orders,
                    &exec_sender,
                    account_id,
                ) => {}
                () = cancellation.cancelled() => {}
            }
        };

        if let Err(e) = venue_ctx.tasks.spawn(future) {
            tracing::warn!("Skipping IB order presence check after shutdown began: {e}");
        }
    }

    #[allow(clippy::too_many_arguments)] // Venue resolution shares the notice context.
    async fn check_order_presence(
        client: &Arc<Client>,
        ib_order_id: i32,
        client_order_id: ClientOrderId,
        rejection_reason: Option<Ustr>,
        timeout_dur: Duration,
        orders: &OrderTracker,
        exec_sender: &EventSender<ExecutionEvent>,
        account_id: AccountId,
    ) {
        // Only a complete answer proves absence: ibapi ends a stream after an error, and a
        // stream cut short must leave the order working rather than reject it
        match Self::find_open_order(client, ib_order_id, timeout_dur).await {
            Ok(true) => {
                tracing::debug!("Order {client_order_id} remains open at the venue after notice");
                return;
            }
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(
                    "Presence check for {client_order_id} could not read open orders, leaving it working: {e:#}"
                );
                return;
            }
        }

        Self::resolve_absent_order(
            client,
            ib_order_id,
            client_order_id,
            rejection_reason,
            timeout_dur,
            orders,
            exec_sender,
            account_id,
        )
        .await;
    }

    // Queries completed orders for the terminal status of an order that is absent
    // from open orders after a venue notice, and emits the inferred terminal event
    // so the engine does not hold a dead order as working.
    #[allow(clippy::too_many_arguments)] // Venue resolution shares the notice context.
    async fn resolve_absent_order(
        client: &Arc<Client>,
        ib_order_id: i32,
        client_order_id: ClientOrderId,
        rejection_reason: Option<Ustr>,
        timeout_dur: Duration,
        orders: &OrderTracker,
        exec_sender: &EventSender<ExecutionEvent>,
        account_id: AccountId,
    ) {
        let tracked_perm_id = orders.lock().ok().and_then(|state| {
            state
                .active_orders
                .get(&ib_order_id)
                .map(|order| order.perm_id)
        });

        let completed = match Self::find_completed_order(
            client,
            ib_order_id,
            tracked_perm_id,
            timeout_dur,
        )
        .await
        {
            Ok(completed) => completed,
            Err(e) => {
                tracing::error!(
                    "Order {client_order_id} is absent from open orders and completed orders could not be read: {e:#}; venue state needs reconciliation"
                );
                return;
            }
        };

        let event = {
            let Ok(state) = orders.lock() else {
                tracing::error!(
                    "Order {client_order_id} absent-order resolution could not lock tracking"
                );
                return;
            };
            // A terminal order was already closed by the update stream during the query
            let Some(tracked) = state.active_orders.get(&ib_order_id) else {
                tracing::debug!(
                    "Order {client_order_id} is no longer active; absent-order resolution done"
                );
                return;
            };
            Self::absent_order_terminal_event(
                completed,
                rejection_reason,
                tracked,
                ib_order_id,
                account_id,
                get_atomic_clock_realtime().get_time_ns(),
            )
        };

        match event {
            Some(event) => {
                if exec_sender.send(ExecutionEvent::Order(event)).is_err() {
                    tracing::error!(
                        "Failed to send inferred terminal event for absent order {client_order_id}"
                    );
                    return;
                }

                if let Err(e) =
                    Self::evict_terminal_order_state(client_order_id, ib_order_id, orders)
                {
                    tracing::error!(
                        "Failed to evict tracking for resolved absent order {client_order_id}: {e}"
                    );
                }
            }
            None => {
                tracing::error!(
                    "Order {client_order_id} (IB order ID {ib_order_id}) is absent from open orders after a venue notice and completed orders gave no terminal status; venue state needs reconciliation"
                );
            }
        }
    }

    // Returns whether IB lists the order as open. ibapi ends the stream cleanly only after
    // the end message and yields an error first on a reset, so `?` fails a partial list.
    async fn find_open_order(
        client: &Arc<Client>,
        ib_order_id: i32,
        timeout_dur: Duration,
    ) -> anyhow::Result<bool> {
        tokio::time::timeout(timeout_dur, async {
            let mut subscription = client.all_open_orders().await?;
            while let Some(item) = subscription.next().await {
                match item? {
                    SubscriptionItem::Data(Orders::OrderData(data))
                        if data.order_id == ib_order_id =>
                    {
                        return Ok(true);
                    }
                    _ => {}
                }
            }
            Ok(false)
        })
        .await
        .context("timed out reading open orders")?
    }

    // Returns the completed status and filled quantity of the order, failing on a partial list.
    async fn find_completed_order(
        client: &Arc<Client>,
        ib_order_id: i32,
        tracked_perm_id: Option<i64>,
        timeout_dur: Duration,
    ) -> anyhow::Result<Option<(OrderStatusKind, i64, f64)>> {
        tokio::time::timeout(timeout_dur, async {
            let mut subscription = client.completed_orders(false).await?;
            while let Some(item) = subscription.next().await {
                match item? {
                    SubscriptionItem::Data(Orders::OrderData(data))
                        if data.order_id == ib_order_id
                            || (data.order.perm_id != 0
                                && Some(data.order.perm_id) == tracked_perm_id) =>
                    {
                        return Ok(Some((
                            data.order_state.status,
                            data.order.perm_id,
                            data.order.filled_quantity,
                        )));
                    }
                    _ => {}
                }
            }
            Ok(None)
        })
        .await
        .context("timed out reading completed orders")?
    }

    // Maps a completed-orders status for an absent order to the inferred terminal
    // event, or None when no event can be honestly inferred. An order absent from
    // completed orders too is rejected only when a notice before acceptance gives
    // the rejection reason.
    pub(super) fn absent_order_terminal_event(
        completed: Option<(OrderStatusKind, i64, f64)>,
        rejection_reason: Option<Ustr>,
        tracked: &TrackedOrder,
        ib_order_id: i32,
        account_id: AccountId,
        ts_init: UnixNanos,
    ) -> Option<OrderEventAny> {
        let Some((status, perm_id, filled)) = completed else {
            return rejection_reason.map(|reason| {
                OrderEventAny::Rejected(OrderRejected::new(
                    tracked.trader_id,
                    tracked.strategy_id,
                    tracked.instrument_id,
                    tracked.client_order_id,
                    account_id,
                    reason,
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    false,
                ))
            });
        };
        let venue_order_id = ib_venue_order_id(ib_order_id, perm_id);

        // A partially filled order cannot be rejected, so an Inactive remainder is canceled
        let partially_filled = filled > 0.0;

        match status {
            OrderStatusKind::Cancelled | OrderStatusKind::ApiCancelled => {
                Some(OrderEventAny::Canceled(OrderCanceled::new(
                    tracked.trader_id,
                    tracked.strategy_id,
                    tracked.instrument_id,
                    tracked.client_order_id,
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                    None,
                )))
            }
            OrderStatusKind::Inactive if partially_filled => {
                Some(OrderEventAny::Canceled(OrderCanceled::new(
                    tracked.trader_id,
                    tracked.strategy_id,
                    tracked.instrument_id,
                    tracked.client_order_id,
                    UUID4::new(),
                    ts_init,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                    None,
                )))
            }
            OrderStatusKind::Inactive => Some(OrderEventAny::Rejected(OrderRejected::new(
                tracked.trader_id,
                tracked.strategy_id,
                tracked.instrument_id,
                tracked.client_order_id,
                account_id,
                Ustr::from("IB reports Inactive after a venue notice"),
                UUID4::new(),
                ts_init,
                ts_init,
                false,
                false,
            ))),
            OrderStatusKind::Filled => {
                tracing::info!(
                    "Order {} completed as Filled after a venue notice; fills arrive through the update stream",
                    tracked.client_order_id
                );
                None
            }
            _ => {
                tracing::warn!(
                    "Order {} completed with status {} after a venue notice; no terminal event inferred",
                    tracked.client_order_id,
                    status.as_str()
                );
                None
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // Update dispatch preserves the stream context.
    pub(super) async fn handle_order_update(
        update: &OrderUpdate,
        orders: &OrderTracker,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        clock: &'static AtomicTime,
        account_id: AccountId,
        ib_account: Ustr,
        commission_cache: &Arc<Mutex<CommissionCache>>,
        pending_live_exec_data: &Arc<Mutex<PendingExecutionCache>>,
        position_tracker: &PositionTracker,
    ) -> anyhow::Result<()> {
        let ts_init = clock.get_time_ns();

        match update {
            OrderUpdate::OrderBound(binding) => {
                orders.lock()?.observe_order_binding(binding);
            }

            OrderUpdate::OrderStatus(status) => {
                Self::handle_order_status(
                    status,
                    orders,
                    instrument_provider,
                    exec_sender,
                    ts_init,
                    account_id,
                )
                .await?;
            }
            OrderUpdate::ExecutionData(exec_data) => {
                // ibapi also broadcasts `reqExecutions` replies here. Only unsolicited
                // execDetails are live fills; they decode to request ID -1 or 0, while
                // ibapi assigns positive request IDs
                if exec_data.request_id > 0 {
                    tracing::debug!(
                        "Ignoring IB execution {} replayed for request {}",
                        exec_data.execution.execution_id,
                        exec_data.request_id
                    );
                    return Ok(());
                }

                if !Self::execution_in_account(exec_data, ib_account)? {
                    return Ok(());
                }
                Self::execution_context(exec_data, orders, instrument_provider, account_id)?;
                // Record the own-fill delta immediately so the position stream cannot
                // observe the post-fill quantity before the fill is known; the id
                // cache makes the later commission-gated call a no-op.
                record_own_fill(
                    position_tracker,
                    &exec_data.execution.execution_id,
                    exec_data.contract.contract_id,
                    exec_data.execution.side,
                    exec_data.execution.shares,
                )
                .await?;

                let execution_id = exec_data.execution.execution_id.clone();
                tracing::debug!(
                    execution_id,
                    order_id = exec_data.execution.order_id,
                    contract_id = exec_data.contract.contract_id,
                    local_symbol = exec_data.contract.local_symbol,
                    security_type = ?exec_data.contract.security_type,
                    side = ?exec_data.execution.side,
                    shares = exec_data.execution.shares,
                    price = exec_data.execution.price,
                    cumulative_quantity = exec_data.execution.cumulative_quantity,
                    average_price = exec_data.execution.average_price,
                    "Received IB execDetails",
                );
                let has_commission = commission_cache.lock().contains_key(&execution_id);

                if !has_commission {
                    tracing::debug!(
                        "Buffering execution data {} until commission report arrives",
                        execution_id
                    );
                    pending_live_exec_data.lock().insert(
                        execution_id,
                        (tokio::time::Instant::now(), exec_data.clone()),
                    );
                    return Ok(());
                }

                Self::handle_execution_data(
                    exec_data,
                    orders,
                    instrument_provider,
                    exec_sender,
                    ts_init,
                    account_id,
                    ib_account,
                    commission_cache,
                    position_tracker,
                )
                .await?;
            }
            OrderUpdate::CommissionReport(commission) => {
                tracing::debug!(
                    execution_id = commission.execution_id,
                    commission = commission.commission,
                    currency = commission.currency,
                    "Received IB commissionReport",
                );
                let pending_exec_data = pending_live_exec_data
                    .lock()
                    .remove(&commission.execution_id);

                {
                    let mut cache = commission_cache.lock();
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

                if let Some((_, exec_data)) = pending_exec_data {
                    Self::handle_execution_data(
                        &exec_data,
                        orders,
                        instrument_provider,
                        exec_sender,
                        ts_init,
                        account_id,
                        ib_account,
                        commission_cache,
                        position_tracker,
                    )
                    .await?;
                }
            }
            OrderUpdate::OpenOrder(order_data) => {
                if !Self::is_active_open_order(&order_data.order) {
                    tracing::debug!(
                        "Ignoring deactivated open order: order_id={}, order_ref={}",
                        order_data.order_id,
                        order_data.order.order_ref
                    );
                    return Ok(());
                }

                if order_data.order.what_if
                    && IbOrderStatus::from_str(order_data.order_state.status.as_str())
                        .is_ok_and(|status| status == IbOrderStatus::PreSubmitted)
                {
                    Self::handle_whatif_order(
                        order_data,
                        orders,
                        instrument_provider,
                        exec_sender,
                        clock.get_time_ns(),
                        account_id,
                    )
                    .await?;
                } else {
                    anyhow::ensure!(
                        !order_data.order.account.is_empty(),
                        "IB open order {} has no account identity",
                        order_data.order_id
                    );

                    if order_data.order.account != ib_account {
                        return Ok(());
                    }
                    let mut normalized_order = order_data.clone();
                    let correlation = orders.lock()?.correlate(
                        order_data.order.client_id,
                        order_data.order_id,
                        order_data.order.perm_id,
                        &order_data.order.order_ref,
                    )?;

                    if let OrderCorrelation::Tracked { context, .. }
                    | OrderCorrelation::Duplicate { context, .. } = &correlation
                    {
                        let resolved = instrument_provider
                            .resolve_instrument_id_for_contract(&order_data.contract)?;
                        anyhow::ensure!(
                            resolved == context.instrument_id,
                            "IB order reference has conflicting instrument identity"
                        );
                    }
                    let sibling = orders
                        .lock()?
                        .observe_order_data(order_data, account_id, false)?;

                    if let Some(parent) = sibling {
                        let snapshot = orders
                            .lock()?
                            .incarnation_data(order_data.order.perm_id)
                            .cloned()
                            .unwrap_or_else(|| order_data.clone());
                        let order_data = &snapshot;
                        let mut report = parse::parse_order_data_to_report(
                            order_data,
                            parent.instrument_id,
                            account_id,
                            instrument_provider,
                            ts_init,
                        )?;
                        report.client_order_id = Some(parent.client_order_id);
                        exec_sender.send(ExecutionEvent::Report(ExecutionReport::Order(
                            Box::new(report),
                        )))?;
                        orders.lock()?.archive_finished_groups();
                        return Ok(());
                    }

                    if let OrderCorrelation::Tracked { order_id, .. } = correlation {
                        normalized_order.order_id = order_id;
                    }
                    let order_data = &normalized_order;
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
                        orders
                            .lock()?
                            .venue_order_id_map
                            .get(&order_data.order_id)
                            .copied()
                    };

                    if let Some(client_order_id) = client_order_id
                        && IbOrderStatus::from_str(status_str).is_ok_and(IbOrderStatus::is_accepted)
                    {
                        let instrument_id = {
                            Self::get_mapped_instrument_id(order_data.order_id, orders)?
                                .map_or_else(
                                    || {
                                        Self::resolve_contract_instrument_id(
                                            instrument_provider,
                                            &order_data.contract,
                                        )
                                    },
                                    Ok,
                                )?
                        };
                        let venue_order_id =
                            parse::ib_venue_order_id(order_data.order_id, order_data.order.perm_id);
                        if Self::emit_order_accepted_if_needed(
                            order_data.order_id,
                            venue_order_id,
                            account_id,
                            ts_init,
                            orders,
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
                            orders,
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

    #[allow(clippy::too_many_arguments)] // Event construction requires the full venue update.
    fn emit_order_updated_from_open_order(
        order_data: &ibapi::orders::OrderData,
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        orders: &OrderTracker,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
    ) -> anyhow::Result<()> {
        let Some(instrument) = instrument_provider.find(&instrument_id) else {
            anyhow::bail!(
                "Cannot update IB order {client_order_id}: instrument {instrument_id} is unavailable"
            );
        };

        if order_data.order.total_quantity <= 0.0 {
            return Ok(());
        }

        let actor_ids = {
            let state = orders.lock()?;
            state
                .active_orders
                .get(&order_data.order_id)
                .map(|order| (order.trader_id, order.strategy_id))
        };
        let Some((trader_id, strategy_id)) = actor_ids else {
            // External orders whose `order_ref` parses as a client order id refresh
            // through here without tracked state; nothing to update.
            tracing::debug!(
                "Skipping order update for untracked IB order {}",
                order_data.order_id
            );
            return Ok(());
        };

        {
            let mut state = orders.lock()?;
            if let Some(order) = state.active_orders.get_mut(&order_data.order_id) {
                if order.perm_id == 0 && order_data.order.perm_id != 0 {
                    order.perm_id = order_data.order.perm_id;
                }
                // Clear the pending modify only when this openOrder reflects the
                // requested values; an unrelated refresh (e.g. induced by an
                // all-open-orders query) must not resolve it.
                if order
                    .pending_modify
                    .as_ref()
                    .is_some_and(|pending| pending.matches(&order_data.order))
                {
                    order.pending_modify = None;
                }
            }
        }

        let price_magnifier = instrument_provider.get_price_magnifier(&instrument_id) as f64;
        let (price, trigger_price) = Self::open_order_price_fields(
            order_data,
            price_magnifier,
            instrument.price_precision(),
        );
        let quantity = Quantity::new(order_data.order.total_quantity, instrument.size_precision());

        // IB repeats openOrder for every open-orders query, including periodic reconciliation
        let update = Some((quantity, price, trigger_price));
        if let Some(order) = orders.lock()?.active_orders.get_mut(&order_data.order_id) {
            if order.last_update == update {
                return Ok(());
            }
            order.last_update = update;
        }

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
        // A trailing order's aux price is its offset; IB reports its trigger separately
        let trail_stop_price = order_data
            .order
            .trail_stop_price
            .map(|price| Price::new(price * price_magnifier, price_precision));

        match order_type {
            OrderType::Market | OrderType::MarketToLimit => (None, None),
            OrderType::Limit => (price, None),
            OrderType::TrailingStopMarket => (None, trail_stop_price),
            OrderType::TrailingStopLimit => (price, trail_stop_price),
            OrderType::StopMarket | OrderType::MarketIfTouched => (None, trigger_price),
            OrderType::StopLimit | OrderType::LimitIfTouched => (price, trigger_price),
        }
    }

    pub(super) async fn handle_order_status(
        status: &IBOrderStatus,
        orders: &OrderTracker,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
    ) -> anyhow::Result<()> {
        let sibling = {
            let mut state = orders.lock()?;
            let sibling = state.observe_incarnation_status(status);
            state.archive_finished_groups();
            sibling
        };

        if let Some((parent, data)) = sibling {
            let mut normalized = status.clone();
            normalized.status = data.order_state.status.clone();
            normalized.filled = data.order.filled_quantity;
            let status = &normalized;
            let mut report = parse_order_status_to_report(
                status,
                Some(&data.order),
                parent.instrument_id,
                account_id,
                instrument_provider,
                ts_init,
            )?;
            report.client_order_id = Some(parent.client_order_id);
            exec_sender.send(ExecutionEvent::Report(ExecutionReport::Order(Box::new(
                report,
            ))))?;
            orders.lock()?.archive_finished_groups();
            return Ok(());
        }
        let correlation =
            orders
                .lock()?
                .correlate(status.client_id, status.order_id, status.perm_id, "")?;
        let mut normalized = status.clone();
        let order = match correlation {
            OrderCorrelation::Tracked { order_id, context } => {
                let state = orders.lock()?;
                let Some(active) = state.active_orders.get(&order_id) else {
                    return Ok(());
                };
                anyhow::ensure!(
                    active.client_order_id == context.client_order_id,
                    "IB status refers to an order ID reused by another order"
                );
                normalized.order_id = order_id;
                context
            }
            OrderCorrelation::Duplicate { context, .. } => {
                tracing::warn!(
                    "IB sibling permanent ID {} for {} awaits authoritative order details",
                    status.perm_id,
                    context.client_order_id
                );
                return Ok(());
            }
            OrderCorrelation::Untracked { .. } => return Ok(()),
        };
        let status = &normalized;
        let client_order_id = order.client_order_id;
        let instrument_id = order.instrument_id;

        Self::update_order_avg_price(
            status.order_id,
            &instrument_id,
            status.average_fill_price.unwrap_or(0.0),
            status.filled,
            instrument_provider,
            orders,
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
                orders,
                exec_sender,
            )?;
        }

        if status.perm_id != 0 {
            let mut state = orders.lock()?;
            if let Some(order) = state.active_orders.get_mut(&status.order_id)
                && order.perm_id == 0
            {
                order.perm_id = status.perm_id;
            }
        }

        let status_str = status.status.as_str();

        match ib_order_status {
            Some(IbOrderStatus::Submitted | IbOrderStatus::PreSubmitted) => {
                if Self::emit_order_accepted_if_needed(
                    status.order_id,
                    venue_order_id,
                    account_id,
                    ts_init,
                    orders,
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
            Some(IbOrderStatus::Inactive) => {
                let reason = if status.why_held.is_empty() {
                    "IB reported order as inactive"
                } else {
                    status.why_held.as_str()
                };

                // IB can report a working order as Inactive after it refuses a modify, even
                // after a partial fill, so a pending modify is rejected and the order kept
                if order.accepted
                    && Self::emit_modify_rejected_if_pending(
                        &order,
                        status.order_id,
                        reason,
                        orders,
                        exec_sender,
                        ts_init,
                        account_id,
                    )?
                {
                    return Ok(());
                }

                if status.filled > 0.0 {
                    Self::emit_order_canceled(
                        &order,
                        venue_order_id,
                        account_id,
                        ts_init,
                        exec_sender,
                    )?;
                    tracing::warn!(
                        "Order {} canceled after partial fill when IB reported Inactive: {}",
                        client_order_id,
                        reason,
                    );
                } else if order.accepted {
                    tracing::warn!(
                        "IB reported accepted order {} as Inactive, left working (venue state uncertain): {}",
                        client_order_id,
                        reason,
                    );
                    return Ok(());
                } else {
                    Self::emit_order_rejected(&order, account_id, reason, ts_init, exec_sender)?;
                    tracing::warn!("Order {} rejected: {}", client_order_id, reason);
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
                if let Some(order) = orders.lock()?.active_orders.get_mut(&status.order_id) {
                    order.pending_cancel = false;
                }

                Self::emit_order_canceled(
                    &order,
                    venue_order_id,
                    account_id,
                    ts_init,
                    exec_sender,
                )?;
                tracing::debug!("Order {} canceled", client_order_id);
            }
            Some(IbOrderStatus::PendingCancel) => {
                Self::emit_order_pending_cancel(
                    status.order_id,
                    client_order_id,
                    venue_order_id,
                    orders,
                    exec_sender,
                    ts_init,
                    account_id,
                )?;
                tracing::debug!("Order {} pending cancel", client_order_id);
            }
            _ => {
                tracing::debug!(
                    "Order status update for order {}: {}",
                    client_order_id,
                    status_str
                );
            }
        }

        if is_terminal {
            Self::evict_terminal_order_state(client_order_id, status.order_id, orders)?;
        }

        Ok(())
    }

    fn emit_order_canceled(
        order: &TrackedOrder,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        ts_init: UnixNanos,
        exec_sender: &EventSender<ExecutionEvent>,
    ) -> anyhow::Result<()> {
        let event = OrderCanceled::new(
            order.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            UUID4::new(),
            ts_init,
            ts_init,
            false,
            Some(venue_order_id),
            Some(account_id),
            None,
        );
        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Canceled(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order canceled event: {e}"))
    }

    fn emit_order_rejected(
        order: &TrackedOrder,
        account_id: AccountId,
        reason: &str,
        ts_init: UnixNanos,
        exec_sender: &EventSender<ExecutionEvent>,
    ) -> anyhow::Result<()> {
        let event = OrderRejected::new(
            order.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            account_id,
            Ustr::from(reason),
            UUID4::new(),
            ts_init,
            ts_init,
            false,
            false,
        );
        exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Rejected(event)))
            .map_err(|e| anyhow::anyhow!("Failed to send order rejected event: {e}"))
    }

    fn evict_terminal_order_state(
        client_order_id: ClientOrderId,
        order_id: i32,
        orders: &OrderTracker,
    ) -> anyhow::Result<()> {
        let mut state = orders.lock()?;
        if let Some(mut order) = state.active_orders.remove(&order_id) {
            order.pending_cancel = false;
            order.pending_modify = None;
            state.terminal_orders.insert(order_id, order);
        }

        state.order_id_map.remove(&client_order_id);
        state.venue_order_id_map.remove(&order_id);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // Execution handling preserves the stream context.
    pub(super) async fn handle_execution_data(
        exec_data: &ExecutionData,
        orders: &OrderTracker,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
        ib_account: Ustr,
        commission_cache: &Arc<Mutex<CommissionCache>>,
        position_tracker: &PositionTracker,
    ) -> anyhow::Result<()> {
        if !Self::execution_in_account(exec_data, ib_account)? {
            return Ok(());
        }
        let (tracking_order_id, tracked_context, correlated_id) =
            Self::execution_context(exec_data, orders, instrument_provider, account_id)?;
        let client_order_id = correlated_id.unwrap_or_else(|| {
            ClientOrderId::from(
                parse::ib_venue_order_id(exec_data.execution.order_id, exec_data.execution.perm_id)
                    .as_str(),
            )
        });
        let instrument_id = if let Some(context) = tracked_context.as_ref() {
            context.instrument_id
        } else if let Some(cached_id) =
            instrument_provider.get_instrument_id_by_contract_id(exec_data.contract.contract_id)
        {
            cached_id
        } else {
            Self::resolve_contract_instrument_id(instrument_provider, &exec_data.contract)?
        };

        let (commission, commission_currency) = if parse::is_combo_execution(exec_data) {
            (0.0, exec_data.contract.currency.to_string())
        } else {
            let mut cache = commission_cache.lock();
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

        record_own_fill(
            position_tracker,
            &exec_data.execution.execution_id,
            exec_data.contract.contract_id,
            exec_data.execution.side,
            exec_data.execution.shares,
        )
        .await?;

        let is_bag = matches!(
            exec_data.contract.security_type,
            ibapi::contracts::SecurityType::Spread
        );

        let spread_instrument_id = tracked_context
            .as_ref()
            .map(|context| context.instrument_id);
        let is_spread = if let Some(spread_id) = spread_instrument_id {
            if let Some(instrument) = instrument_provider.find(&spread_id) {
                instrument.is_spread()
            } else {
                false
            }
        } else {
            false
        };

        let avg_px = tracked_context.as_ref().and_then(|context| context.avg_px);
        let venue_order_id = parse::ib_venue_order_id(
            tracking_order_id,
            if exec_data.execution.perm_id == 0 {
                tracked_context
                    .as_ref()
                    .map_or(0, |context| context.perm_id)
            } else {
                exec_data.execution.perm_id
            },
        );

        if tracked_context.is_some() {
            Self::emit_order_accepted_for_fill_if_needed(
                tracking_order_id,
                venue_order_id,
                account_id,
                parse_execution_time(&exec_data.execution.time)?,
                orders,
                exec_sender,
            )?;
        }

        if is_bag && is_spread {
            let fill_id = exec_data.execution.execution_id.clone();
            let mut state = orders.lock()?;
            if let Some(order) = state.order_mut(tracking_order_id) {
                if order.spread_fill_ids.contains(&fill_id) {
                    tracing::debug!(
                        "Combo fill {} already processed for order {}, skipping",
                        fill_id,
                        client_order_id,
                    );
                    return Ok(());
                }
                order.spread_fill_ids.insert(fill_id);
            }
        }

        if !is_bag
            && is_spread
            && let Some(spread_id) = spread_instrument_id
            && let Some(context) = tracked_context.as_ref()
        {
            let fill = SpreadFillContext {
                client_order_id,
                spread_instrument_id: spread_id,
                commission,
                commission_currency: &commission_currency,
                ts_init,
                account_id,
            };

            if let Err(e) = Self::handle_spread_execution(
                exec_data,
                &fill,
                instrument_provider,
                exec_sender,
                orders,
                context,
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

        let mut fill_report = parse_execution_to_fill_report(
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

        fill_report.client_order_id = correlated_id;

        if let Some(context) = tracked_context {
            if exec_data.execution.perm_id == 0 && context.perm_id > 0 {
                fill_report.venue_order_id =
                    parse::ib_venue_order_id(tracking_order_id, context.perm_id);
            }
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

    pub(super) fn update_order_avg_price(
        order_id: i32,
        instrument_id: &InstrumentId,
        avg_fill_price: f64,
        filled: f64,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        orders: &OrderTracker,
    ) -> anyhow::Result<()> {
        if filled <= 0.0 || !parse::should_use_avg_fill_price(avg_fill_price, instrument_id) {
            return Ok(());
        }

        let Some(instrument) = instrument_provider.find(instrument_id) else {
            anyhow::bail!(
                "Cannot update IB order {order_id} average price: instrument {instrument_id} is unavailable"
            );
        };

        let price_magnifier = instrument_provider.get_price_magnifier(instrument_id) as f64;
        let converted_avg_price = avg_fill_price * price_magnifier;
        let avg_px = Price::new(converted_avg_price, instrument.price_precision());

        let mut state = orders.lock()?;
        let order = state.active_orders.get_mut(&order_id).with_context(|| {
            format!("Tracked state not found for Interactive Brokers order {order_id}")
        })?;
        order.avg_px = Some(avg_px);

        Ok(())
    }

    pub(super) async fn handle_whatif_order(
        order_data: &ibapi::orders::OrderData,
        orders: &OrderTracker,
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        exec_sender: &EventSender<ExecutionEvent>,
        ts_init: UnixNanos,
        account_id: AccountId,
    ) -> anyhow::Result<()> {
        let Some(client_order_id) = orders
            .lock()?
            .venue_order_id_map
            .get(&order_data.order_id)
            .copied()
        else {
            tracing::debug!(
                "What-if order for unknown order ID: {}",
                order_data.order_id
            );
            return Ok(());
        };

        let instrument_id = Self::get_mapped_instrument_id(order_data.order_id, orders)?
            .map_or_else(
                || Self::resolve_contract_instrument_id(instrument_provider, &order_data.contract),
                Ok,
            )?;

        let (trader_id, strategy_id) =
            Self::get_required_order_actor_ids(order_data.order_id, orders)?;

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

    pub(super) fn resolve_contract_instrument_id(
        instrument_provider: &Arc<InteractiveBrokersInstrumentProvider>,
        contract: &Contract,
    ) -> anyhow::Result<InstrumentId> {
        instrument_provider
            .resolve_instrument_id_for_contract(contract)
            .context("Failed to resolve IBKR contract to instrument ID")
    }
}
