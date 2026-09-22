// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Interactive Brokers market data stream processing.

use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use futures_util::Stream;
use ibapi::{
    ConnectivityStatus, Error, Notice,
    contracts::tick_types::TickType,
    market_data::{
        IgnoreSize, SmartDepth, TradingHours,
        historical::{
            Bar as HistoricalBar, BarSize as HistoricalBarSize, HistoricalBarUpdate,
            WhatToShow as HistoricalWhatToShow,
        },
        realtime::{
            Bar as RealtimeBar, MarketDepths, TickGeneric, TickPrice, TickPriceSize, TickSize,
            TickTypes, WhatToShow as RealtimeWhatToShow,
        },
    },
    prelude::StreamExt,
    subscriptions::{Subscription, SubscriptionItem},
};
use nautilus_common::{live::sender::EventSender, messages::DataEvent};
use nautilus_core::{UnixNanos, time::AtomicTime};
use nautilus_live::task::TaskSlot;
use nautilus_model::{
    data::{Bar, BarType, BookOrder, Data, OrderBookDelta, QuoteTick, option_chain::OptionGreeks},
    enums::{BookAction, OrderSide},
    identifiers::{ClientId, InstrumentId},
    types::{Price, Quantity},
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    config::InteractiveBrokersDataClientConfig,
    data::{
        cache::{OptionGreeksCache, QuoteCache, is_sentinel_price},
        convert::{bar_type_to_ib_bar_size, ib_bar_to_nautilus_bar, ib_timestamp_to_unix_nanos},
        parse::{
            log_tick_parse_error, parse_index_price, parse_market_depth_operation,
            parse_option_open_interest, parse_quote_tick, parse_trade_tick,
        },
    },
    data_types::InteractiveBrokersSubscriptionIdle,
};

enum StreamAction {
    Continue,
    Stop,
    Resubscribe,
}

struct StreamContext {
    stream_config: StreamConfig,
    client: Arc<ibapi::Client>,
    data_sender: EventSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    data_farm_state: Arc<DataFarmConnectionState>,
    recovery_scope: DataFarmRecoveryScope,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
}

type StreamFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy)]
pub(super) struct StreamConfig {
    client_id: ClientId,
    all_last_trades: bool,
    idle_timeout_secs: Option<u64>,
}

impl StreamConfig {
    pub(super) fn new(client_id: ClientId, config: &InteractiveBrokersDataClientConfig) -> Self {
        Self {
            client_id,
            all_last_trades: config.all_last_trades,
            idle_timeout_secs: config.subscription_idle_timeout_secs,
        }
    }
}

impl StreamContext {
    fn monitor(
        &self,
        subscription: impl Into<String>,
        farm_generation: u64,
    ) -> SubscriptionMonitor {
        let timeout = self
            .stream_config
            .idle_timeout_secs
            .map(Duration::from_secs);
        SubscriptionMonitor {
            client_id: self.stream_config.client_id,
            instrument_id: self.instrument_id,
            subscription: subscription.into(),
            timeout,
            deadline: timeout.and_then(|timeout| tokio::time::Instant::now().checked_add(timeout)),
            last_data_received_ns: None,
            data_sender: self.data_sender.clone(),
            clock: self.clock,
            cancellation_token: self.cancellation_token.clone(),
            data_farm_state: Arc::clone(&self.data_farm_state),
            recovery_scope: self.recovery_scope,
            farm_generation,
        }
    }
}

struct SubscriptionMonitor {
    client_id: ClientId,
    instrument_id: InstrumentId,
    subscription: String,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    last_data_received_ns: Option<u64>,
    data_sender: EventSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    data_farm_state: Arc<DataFarmConnectionState>,
    recovery_scope: DataFarmRecoveryScope,
    farm_generation: u64,
}

impl SubscriptionMonitor {
    async fn wait_for_idle(&self) {
        match self.deadline {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => std::future::pending::<()>().await,
        }
    }

    fn received_data(&mut self) {
        if let Some(timeout) = self.timeout {
            self.last_data_received_ns = Some(self.clock.get_time_ns().as_u64());
            self.deadline = tokio::time::Instant::now().checked_add(timeout);
        }
    }

    fn notify_idle(&mut self) -> bool {
        if self.deadline.take().is_none() || self.cancellation_token.is_cancelled() {
            return true;
        }
        let state = self.data_farm_state.state.lock();
        if state.recovery(self.recovery_scope).recovery_generation != self.farm_generation {
            return true;
        }
        drop(state);
        let Some(timeout) = self.timeout else {
            return true;
        };
        let now = self.clock.get_time_ns();
        let event = InteractiveBrokersSubscriptionIdle {
            client_id: self.client_id,
            instrument_id: self.instrument_id,
            subscription: self.subscription.clone(),
            idle_timeout_secs: timeout.as_secs(),
            last_data_received_ns: self.last_data_received_ns,
            ts_event: now,
            ts_init: now,
        };
        tracing::warn!(
            "IB {} subscription for {} has received no market data for {}s",
            self.subscription,
            self.instrument_id,
            timeout.as_secs(),
        );
        self.data_sender
            .send(DataEvent::Data(Data::from(event)))
            .is_ok()
    }
}

async fn run_stream<S, Subscribe, Process>(
    context: &StreamContext,
    mut subscribe_fn: Subscribe,
    mut process_fn: Process,
) -> anyhow::Result<()>
where
    S: CancellableStream,
    Subscribe: for<'a> FnMut(&'a StreamContext) -> StreamFuture<'a, anyhow::Result<S>>,
    Process: for<'a> FnMut(
        &'a mut S,
        &'a StreamContext,
        u64,
    ) -> StreamFuture<'a, anyhow::Result<StreamAction>>,
{
    let mut farm_generation = if context.recovery_scope == DataFarmRecoveryScope::MarketData {
        context.data_farm_state.recovery_generation()
    } else {
        context
            .data_farm_state
            .recovery_generation_for(context.recovery_scope)
    };

    loop {
        if !wait_for_connection_or_cancel(context).await {
            return Ok(());
        }

        let mut subscription = match subscribe_fn(context).await {
            Ok(subscription) => subscription,
            Err(e) => {
                tracing::warn!(
                    "Failed to establish IB stream for {}: {e}",
                    context.instrument_id,
                );
                tokio::select! {
                    () = context.cancellation_token.cancelled() => return Ok(()),
                    () = tokio::time::sleep(HISTORICAL_BAR_RETRY_DELAY) => continue,
                }
            }
        };

        match process_fn(&mut subscription, context, farm_generation).await? {
            StreamAction::Stop | StreamAction::Continue => return Ok(()),
            StreamAction::Resubscribe => {
                subscription.cancel().await;
                farm_generation = if context.recovery_scope == DataFarmRecoveryScope::MarketData {
                    context.data_farm_state.recovery_generation()
                } else {
                    context
                        .data_farm_state
                        .recovery_generation_for(context.recovery_scope)
                };

                if context.client.is_connected() {
                    tokio::select! {
                        () = context.cancellation_token.cancelled() => return Ok(()),
                        () = tokio::time::sleep(HISTORICAL_BAR_RETRY_DELAY) => {}
                    }
                }
            }
        }
    }
}

// ibapi 4.2 bounds `Subscription<T>` by a crate-private decoder trait, so generic code
// cannot name `Subscription<T>`; each streamed item type opts in here instead.
trait CancellableStream: Send + 'static {
    async fn cancel(&mut self);
}

macro_rules! impl_cancellable_stream {
    ($($item:ty),+ $(,)?) => {
        $(
            impl CancellableStream for Subscription<$item> {
                async fn cancel(&mut self) {
                    Subscription::cancel(self).await;
                }
            }
        )+
    };
}

impl_cancellable_stream!(
    HistoricalBarUpdate,
    TickTypes,
    ibapi::market_data::realtime::BidAsk,
    ibapi::market_data::realtime::Trade,
    RealtimeBar,
    MarketDepths,
);

async fn wait_for_connection_or_cancel(context: &StreamContext) -> bool {
    while !context.client.is_connected() {
        tokio::select! {
            () = context.cancellation_token.cancelled() => return false,
            () = tokio::time::sleep(HISTORICAL_BAR_RETRY_DELAY) => {}
        }
    }

    !context.cancellation_token.is_cancelled()
}

trait IntoSubscriptionTick {
    fn into_subscription_item(self) -> SubscriptionItem<TickTypes>;
}

impl IntoSubscriptionTick for TickTypes {
    fn into_subscription_item(self) -> SubscriptionItem<TickTypes> {
        SubscriptionItem::Data(self)
    }
}

impl IntoSubscriptionTick for SubscriptionItem<TickTypes> {
    fn into_subscription_item(self) -> SubscriptionItem<TickTypes> {
        self
    }
}

const SUBSCRIPTION_DISCONNECTED_CODE: i32 = 10182;
const DATA_FARM_RECOVERY_HISTORY_LIMIT: usize = 1_024;
const HISTORICAL_BAR_MIN_COUNT: i64 = 300;
const HISTORICAL_BAR_RETRY_DELAY: Duration = Duration::from_secs(1);
const IB_GENERIC_TICK_OPTION_OPEN_INTEREST: &str = "101";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DataFarmKind {
    MarketData,
    HistoricalData,
    SecurityDefinition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DataFarmRecoveryScope {
    MarketData,
    HistoricalData,
    SecurityDefinition,
    HistoricalBars,
}

impl From<DataFarmKind> for DataFarmRecoveryScope {
    fn from(kind: DataFarmKind) -> Self {
        match kind {
            DataFarmKind::MarketData => Self::MarketData,
            DataFarmKind::HistoricalData => Self::HistoricalData,
            DataFarmKind::SecurityDefinition => Self::SecurityDefinition,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DataFarmIdentity {
    kind: DataFarmKind,
    name: Option<String>,
}

impl DataFarmIdentity {
    fn from_notice(notice: &Notice) -> Option<Self> {
        let kind = match notice.code {
            2103 | 2104 => DataFarmKind::MarketData,
            2105 | 2106 => DataFarmKind::HistoricalData,
            2157 | 2158 => DataFarmKind::SecurityDefinition,
            _ => return None,
        };
        let name = notice
            .message
            .rsplit_once(':')
            .map(|(_, name)| name.trim())
            .filter(|name| !name.is_empty())
            .map(str::to_owned);

        Some(Self { kind, name })
    }
}

#[derive(Debug, Default)]
struct DataFarmRecoveryState {
    recoveries: VecDeque<(u64, UnixNanos)>,
    recovery_generation: u64,
}

impl DataFarmRecoveryState {
    fn record_recovery(&mut self, degraded_since_ns: UnixNanos) {
        self.recovery_generation = self.recovery_generation.wrapping_add(1);
        self.recoveries
            .push_back((self.recovery_generation, degraded_since_ns));

        if self.recoveries.len() > DATA_FARM_RECOVERY_HISTORY_LIMIT {
            let (_, pruned_since_ns) = self.recoveries.pop_front().unwrap();
            if let Some((_, retained_since_ns)) = self.recoveries.front_mut() {
                // Preserve the earliest replay boundary for streams lagging behind the history
                *retained_since_ns = (*retained_since_ns).min(pruned_since_ns);
            }
        }
    }
}

#[derive(Debug, Default)]
struct DataFarmState {
    degraded_farms: HashMap<DataFarmIdentity, UnixNanos>,
    degraded_scopes: AHashMap<DataFarmRecoveryScope, UnixNanos>,
    market_data: DataFarmRecoveryState,
    historical_data: DataFarmRecoveryState,
    security_definition: DataFarmRecoveryState,
    historical_bars: DataFarmRecoveryState,
}

impl DataFarmState {
    fn recovery(&self, scope: DataFarmRecoveryScope) -> &DataFarmRecoveryState {
        match scope {
            DataFarmRecoveryScope::MarketData => &self.market_data,
            DataFarmRecoveryScope::HistoricalData => &self.historical_data,
            DataFarmRecoveryScope::SecurityDefinition => &self.security_definition,
            DataFarmRecoveryScope::HistoricalBars => &self.historical_bars,
        }
    }

    fn recovery_mut(&mut self, scope: DataFarmRecoveryScope) -> &mut DataFarmRecoveryState {
        match scope {
            DataFarmRecoveryScope::MarketData => &mut self.market_data,
            DataFarmRecoveryScope::HistoricalData => &mut self.historical_data,
            DataFarmRecoveryScope::SecurityDefinition => &mut self.security_definition,
            DataFarmRecoveryScope::HistoricalBars => &mut self.historical_bars,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct DataFarmConnectionState {
    state: Mutex<DataFarmState>,
    recovery_notify: tokio::sync::Notify,
}

impl DataFarmConnectionState {
    fn recovery_pending(&self, notice: &Notice) -> bool {
        let Some(farm) = DataFarmIdentity::from_notice(notice) else {
            return false;
        };

        if notice.connectivity_status() != Some(ConnectivityStatus::Ok) {
            return false;
        }

        let state = self.state.lock();
        state.degraded_farms.contains_key(&farm)
            || state
                .degraded_scopes
                .contains_key(&DataFarmRecoveryScope::from(farm.kind))
    }

    pub(super) fn handle_notice(&self, notice: &Notice, clock: &'static AtomicTime) {
        let Some(farm) = DataFarmIdentity::from_notice(notice) else {
            return;
        };

        match notice.connectivity_status() {
            Some(ConnectivityStatus::Broken) => {
                self.mark_farm_degraded(farm, clock.get_time_ns());
                tracing::debug!(
                    "IB data farm degraded by notice {} - {}; waiting for farm OK before resubscribe",
                    notice.code,
                    notice.message
                );
            }
            Some(ConnectivityStatus::Ok) if self.mark_ok(&farm) => {
                tracing::info!(
                    "IB data farm recovered by notice {} - {}; resubscribing data feeds",
                    notice.code,
                    notice.message
                );
            }
            _ => {}
        }
    }

    pub(super) fn mark_degraded(&self, generation: u64, degraded_since_ns: UnixNanos) {
        self.mark_degraded_for(
            DataFarmRecoveryScope::MarketData,
            generation,
            degraded_since_ns,
        );
    }

    fn mark_degraded_for(
        &self,
        scope: DataFarmRecoveryScope,
        generation: u64,
        degraded_since_ns: UnixNanos,
    ) {
        let mut state = self.state.lock();

        if state.recovery(scope).recovery_generation != generation {
            return;
        }

        state
            .degraded_scopes
            .entry(scope)
            .and_modify(|current| *current = (*current).min(degraded_since_ns))
            .or_insert(degraded_since_ns);
    }

    fn mark_farm_degraded(&self, farm: DataFarmIdentity, degraded_since_ns: UnixNanos) {
        let mut state = self.state.lock();

        state
            .degraded_farms
            .entry(farm)
            .and_modify(|current| *current = (*current).min(degraded_since_ns))
            .or_insert(degraded_since_ns);
    }

    fn mark_ok(&self, farm: &DataFarmIdentity) -> bool {
        let mut state = self.state.lock();

        let family_scope = DataFarmRecoveryScope::from(farm.kind);
        let farm_degraded_since_ns = state.degraded_farms.remove(farm);
        let scope_degraded_since_ns = state.degraded_scopes.remove(&family_scope);
        let family_degraded_since_ns =
            earliest_data_loss_ns(farm_degraded_since_ns, scope_degraded_since_ns);
        let historical_bars_recovery_since_ns = match farm.kind {
            DataFarmKind::MarketData | DataFarmKind::HistoricalData => earliest_data_loss_ns(
                family_degraded_since_ns,
                state
                    .degraded_scopes
                    .remove(&DataFarmRecoveryScope::HistoricalBars),
            ),
            DataFarmKind::SecurityDefinition => None,
        };

        if let Some(degraded_since_ns) = family_degraded_since_ns {
            state
                .recovery_mut(family_scope)
                .record_recovery(degraded_since_ns);
        }

        if let Some(degraded_since_ns) = historical_bars_recovery_since_ns {
            state
                .recovery_mut(DataFarmRecoveryScope::HistoricalBars)
                .record_recovery(degraded_since_ns);
        }

        if family_degraded_since_ns.is_none() && historical_bars_recovery_since_ns.is_none() {
            return false;
        }
        drop(state);
        self.recovery_notify.notify_waiters();
        true
    }

    fn recovery_generation(&self) -> u64 {
        self.recovery_generation_for(DataFarmRecoveryScope::MarketData)
    }

    fn recovery_generation_for(&self, scope: DataFarmRecoveryScope) -> u64 {
        self.state.lock().recovery(scope).recovery_generation
    }

    fn recovery_since_ns_after_for(
        &self,
        scope: DataFarmRecoveryScope,
        generation: u64,
    ) -> Option<UnixNanos> {
        self.state
            .lock()
            .recovery(scope)
            .recoveries
            .iter()
            .filter_map(|(recovery_generation, degraded_since_ns)| {
                (*recovery_generation > generation).then_some(*degraded_since_ns)
            })
            .min()
    }

    async fn wait_for_recovery_after(&self, generation: u64) {
        self.wait_for_recovery_after_for(DataFarmRecoveryScope::MarketData, generation)
            .await;
    }

    async fn wait_for_recovery_after_for(&self, scope: DataFarmRecoveryScope, generation: u64) {
        loop {
            let notified = self.recovery_notify.notified();

            if self.recovery_generation_for(scope) != generation {
                return;
            }
            notified.await;
        }
    }
}

fn is_subscription_disconnected_error(error: &Error) -> bool {
    matches!(error, Error::ConnectionReset)
        || matches!(error, Error::Notice(notice) if notice.code == SUBSCRIPTION_DISCONNECTED_CODE)
}

fn earliest_data_loss_ns(first: Option<UnixNanos>, second: Option<UnixNanos>) -> Option<UnixNanos> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (Some(first), None) => Some(first),
        (None, Some(second)) => Some(second),
        (None, None) => None,
    }
}

async fn wait_for_data_farm_recovery_or_cancel(
    data_farm_state: &DataFarmConnectionState,
    generation: u64,
    cancellation_token: &CancellationToken,
) -> bool {
    wait_for_data_farm_recovery_or_cancel_for(
        data_farm_state,
        DataFarmRecoveryScope::MarketData,
        generation,
        cancellation_token,
    )
    .await
}

async fn wait_for_data_farm_recovery_or_cancel_for(
    data_farm_state: &DataFarmConnectionState,
    scope: DataFarmRecoveryScope,
    generation: u64,
    cancellation_token: &CancellationToken,
) -> bool {
    tokio::select! {
        () = data_farm_state.wait_for_recovery_after_for(scope, generation) => true,
        () = cancellation_token.cancelled() => false,
    }
}

pub(super) fn resolve_historical_bar_start_ns(
    start_ns: Option<UnixNanos>,
    now_ns: UnixNanos,
) -> UnixNanos {
    start_ns.unwrap_or(now_ns)
}

pub(super) fn resolve_historical_bar_replay_start_ns(
    first_start_ns: UnixNanos,
    last_disconnection_ns: Option<UnixNanos>,
) -> UnixNanos {
    match last_disconnection_ns {
        Some(last_disconnection_ns) if last_disconnection_ns > first_start_ns => {
            last_disconnection_ns
        }
        _ => first_start_ns,
    }
}

pub(super) fn calculate_historical_bar_subscription_duration(
    bar_type: BarType,
    start_ns: UnixNanos,
    now_ns: UnixNanos,
) -> ibapi::market_data::historical::Duration {
    use ibapi::market_data::historical::ToDuration;

    let bar_seconds = bar_type.spec().timedelta().as_secs().max(1);
    let requested_seconds =
        ((now_ns.as_u64().saturating_sub(start_ns.as_u64())) / 1_000_000_000) as i64;
    let minimum_seconds = bar_seconds.saturating_mul(HISTORICAL_BAR_MIN_COUNT);
    let duration_seconds = requested_seconds.max(minimum_seconds).max(bar_seconds);

    if duration_seconds >= 86_400 {
        let duration_days = ((duration_seconds + 86_399) / 86_400).min(i32::MAX as i64) as i32;
        duration_days.days()
    } else {
        let duration_seconds = duration_seconds.min(i32::MAX as i64) as i32;
        duration_seconds.seconds()
    }
}

fn should_emit_historical_bar(bar: &Bar, start_ns: UnixNanos) -> bool {
    bar.ts_init >= start_ns
}

pub(super) async fn monitor_data_farm_notices(
    client: Arc<ibapi::Client>,
    data_farm_state: Arc<DataFarmConnectionState>,
    market_data_type: Option<ibapi::market_data::MarketDataType>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    is_connected: Arc<std::sync::atomic::AtomicBool>,
) -> anyhow::Result<()> {
    let mut notices = client
        .notice_stream()
        .context("Failed to subscribe to IB notice stream")?;

    loop {
        tokio::select! {
            () = cancellation_token.cancelled() => return Ok(()),
            notice = notices.next() => {
                let Some(notice) = notice else {
                    if !cancellation_token.is_cancelled() {
                        tracing::warn!(
                            "IB data farm notice stream ended; data client is disconnected",
                        );
                        is_connected.store(false, std::sync::atomic::Ordering::Relaxed);
                    }
                    return Ok(());
                };

                if let Some(market_data_type) = market_data_type
                    && data_farm_state.recovery_pending(&notice)
                {
                    if let Err(e) = client
                        .switch_market_data_type(market_data_type)
                        .await
                    {
                        is_connected.store(false, std::sync::atomic::Ordering::Relaxed);
                        return Err(e).context(
                            "Failed to restore IB market data type after reconnect; data client is disconnected",
                        );
                    }
                    tracing::info!("Restored IB market data type to {market_data_type:?}");
                }
                data_farm_state.handle_notice(&notice, clock);
            }
        }
    }
}

#[derive(Debug)]
struct HistoricalBarStreamState {
    first_start_ns: UnixNanos,
    replay_start_ns: UnixNanos,
    last_disconnection_ns: Option<UnixNanos>,
    had_connection: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_historical_bars_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    bar_type: BarType,
    what_to_show: HistoricalWhatToShow,
    price_precision: u8,
    size_precision: u8,
    use_rth: bool,
    start_ns: Option<UnixNanos>,
    data_sender: EventSender<DataEvent>,
    handle_revised_bars: bool,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    data_farm_state: Arc<DataFarmConnectionState>,
    stream_config: StreamConfig,
) -> anyhow::Result<()> {
    tracing::debug!("Starting historical bars subscription for {}", bar_type);
    let first_start_ns = resolve_historical_bar_start_ns(start_ns, clock.get_time_ns());
    let stream_state = Arc::new(tokio::sync::Mutex::new(HistoricalBarStreamState {
        first_start_ns,
        replay_start_ns: first_start_ns,
        last_disconnection_ns: None,
        had_connection: false,
    }));
    let context = StreamContext {
        stream_config,
        client,
        data_sender,
        clock,
        cancellation_token,
        data_farm_state,
        recovery_scope: DataFarmRecoveryScope::HistoricalBars,
        instrument_id: bar_type.instrument_id(),
        price_precision,
        size_precision,
    };
    let trading_hours = if use_rth {
        TradingHours::Regular
    } else {
        TradingHours::Extended
    };

    run_stream(
        &context,
        |context| {
            let contract = contract.clone();
            let stream_state = Arc::clone(&stream_state);
            Box::pin(async move {
                let replay_start_ns = {
                    let mut state = stream_state.lock().await;
                    state.replay_start_ns = resolve_historical_bar_replay_start_ns(
                        state.first_start_ns,
                        state.last_disconnection_ns,
                    );
                    state.replay_start_ns
                };
                let duration = calculate_historical_bar_subscription_duration(
                    bar_type,
                    replay_start_ns,
                    context.clock.get_time_ns(),
                );
                context
                    .client
                    .historical_data(&contract, bar_type_to_historical_bar_size(bar_type)?)
                    .duration(duration)
                    .what_to_show(what_to_show)
                    .trading_hours(trading_hours)
                    .stream()
                    .await
                    .context("Failed to create historical bars subscription")
            })
        },
        |subscription, context, farm_generation| {
            let stream_state = Arc::clone(&stream_state);
            Box::pin(process_historical_bar_stream(
                subscription,
                context,
                farm_generation,
                bar_type,
                handle_revised_bars,
                stream_state,
                context.monitor(format!("bars:{bar_type}"), farm_generation),
            ))
        },
    )
    .await
}

async fn process_historical_bar_stream<S>(
    subscription: &mut S,
    context: &StreamContext,
    farm_generation: u64,
    bar_type: BarType,
    handle_revised_bars: bool,
    stream_state: Arc<tokio::sync::Mutex<HistoricalBarStreamState>>,
    mut monitor: SubscriptionMonitor,
) -> anyhow::Result<StreamAction>
where
    S: CancellableStream
        + Stream<Item = Result<SubscriptionItem<HistoricalBarUpdate>, Error>>
        + Unpin,
{
    loop {
        tokio::select! {
            biased;
            () = context.cancellation_token.cancelled() => return Ok(StreamAction::Stop),
            () = context.data_farm_state.wait_for_recovery_after_for(
                DataFarmRecoveryScope::HistoricalBars,
                farm_generation,
            ) => {
                record_historical_bar_recovery(
                    context,
                    farm_generation,
                    &stream_state,
                )
                .await;
                return Ok(StreamAction::Resubscribe);
            }
            () = monitor.wait_for_idle() => {
                if !monitor.notify_idle() {
                    return Ok(StreamAction::Stop);
                }
            }
            update = subscription.next() => {
                match update {
                    Some(Ok(SubscriptionItem::Data(HistoricalBarUpdate::Historical(data)))) => {
                        if !data.bars.is_empty() { monitor.received_data(); }
                        let replay_start_ns = mark_historical_bar_connected(&stream_state).await;

                        for ib_bar in &data.bars {
                            let bar = ib_bar_to_nautilus_bar(
                                ib_bar,
                                bar_type,
                                context.price_precision,
                                context.size_precision,
                            )?;

                            if should_emit_historical_bar(&bar, replay_start_ns)
                                && context.data_sender.send(DataEvent::Data(Data::Bar(bar))).is_err()
                            {
                                return Ok(StreamAction::Stop);
                            }
                        }
                    }
                    Some(Ok(SubscriptionItem::Data(HistoricalBarUpdate::Update(ib_bar)))) => {
                        monitor.received_data();
                        let replay_start_ns = mark_historical_bar_connected(&stream_state).await;

                        if handle_revised_bars {
                            let bar = ib_bar_to_nautilus_bar(
                                &ib_bar,
                                bar_type,
                                context.price_precision,
                                context.size_precision,
                            )?;

                            if should_emit_historical_bar(&bar, replay_start_ns)
                                && context.data_sender.send(DataEvent::Data(Data::Bar(bar))).is_err()
                            {
                                return Ok(StreamAction::Stop);
                            }
                        }
                    }
                    Some(Ok(SubscriptionItem::Data(HistoricalBarUpdate::End { .. }))) => {}
                    Some(Ok(SubscriptionItem::Notice(notice))) => {
                        context.data_farm_state.handle_notice(&notice, context.clock);
                    }
                    Some(Err(e)) if is_subscription_disconnected_error(&e) => {
                        context.data_farm_state.mark_degraded_for(
                            DataFarmRecoveryScope::HistoricalBars,
                            farm_generation,
                            context.clock.get_time_ns(),
                        );

                        if !wait_for_data_farm_recovery_or_cancel_for(
                            &context.data_farm_state,
                            DataFarmRecoveryScope::HistoricalBars,
                            farm_generation,
                            &context.cancellation_token,
                        )
                        .await
                        {
                            return Ok(StreamAction::Stop);
                        }
                        record_historical_bar_recovery(
                            context,
                            farm_generation,
                            &stream_state,
                        )
                        .await;
                        return Ok(StreamAction::Resubscribe);
                    }
                    Some(Err(e)) => {
                        tracing::warn!(
                            "Historical bars subscription ended for {}: {:?}",
                            bar_type,
                            e,
                        );
                        mark_historical_bar_disconnected(context, &stream_state).await;
                        return Ok(StreamAction::Resubscribe);
                    }
                    None => {
                        tracing::warn!(
                            "Historical bars subscription ended unexpectedly for {}",
                            bar_type,
                        );
                        mark_historical_bar_disconnected(context, &stream_state).await;
                        return Ok(StreamAction::Resubscribe);
                    }
                }
            }
        }
    }
}

async fn mark_historical_bar_connected(
    stream_state: &tokio::sync::Mutex<HistoricalBarStreamState>,
) -> UnixNanos {
    let mut state = stream_state.lock().await;
    state.had_connection = true;
    state.replay_start_ns
}

async fn mark_historical_bar_disconnected(
    context: &StreamContext,
    stream_state: &tokio::sync::Mutex<HistoricalBarStreamState>,
) {
    let mut state = stream_state.lock().await;
    if state.had_connection {
        state.last_disconnection_ns = Some(context.clock.get_time_ns());
    }
}

async fn record_historical_bar_recovery(
    context: &StreamContext,
    farm_generation: u64,
    stream_state: &tokio::sync::Mutex<HistoricalBarStreamState>,
) {
    let recovered_since = context
        .data_farm_state
        .recovery_since_ns_after_for(DataFarmRecoveryScope::HistoricalBars, farm_generation);
    let mut state = stream_state.lock().await;
    state.last_disconnection_ns =
        earliest_data_loss_ns(state.last_disconnection_ns, recovered_since);
}

fn bar_type_to_historical_bar_size(bar_type: BarType) -> anyhow::Result<HistoricalBarSize> {
    bar_type_to_ib_bar_size(&bar_type)
}

async fn process_market_data_stream<S, Process>(
    subscription: &mut S,
    context: &StreamContext,
    farm_generation: u64,
    mut process_fn: Process,
    mut monitor: SubscriptionMonitor,
) -> anyhow::Result<StreamAction>
where
    S: CancellableStream + Stream<Item = Result<SubscriptionItem<TickTypes>, Error>> + Unpin,
    Process: for<'a> FnMut(
        Result<SubscriptionItem<TickTypes>, Error>,
        &'a StreamContext,
    ) -> StreamFuture<'a, anyhow::Result<StreamAction>>,
{
    loop {
        tokio::select! {
            biased;
            () = context.cancellation_token.cancelled() => return Ok(StreamAction::Stop),
            () = context.data_farm_state.wait_for_recovery_after(farm_generation) => {
                return Ok(StreamAction::Resubscribe);
            }
            () = monitor.wait_for_idle() => {
                if !monitor.notify_idle() {
                    return Ok(StreamAction::Stop);
                }
            }
            tick_result = subscription.next() => {
                let Some(tick_result) = tick_result else {
                    tracing::warn!(
                        "IB market data stream ended unexpectedly for {}",
                        context.instrument_id,
                    );
                    return Ok(StreamAction::Resubscribe);
                };

                if let Ok(SubscriptionItem::Data(tick)) = &tick_result
                    && !matches!(tick, TickTypes::SnapshotEnd | TickTypes::RequestParameters(_) | TickTypes::MarketDataType(_))
                {
                    monitor.received_data();
                }

                if let Err(e) = &tick_result
                    && is_subscription_disconnected_error(e)
                {
                    context
                        .data_farm_state
                        .mark_degraded(farm_generation, context.clock.get_time_ns());
                    tracing::warn!(
                        "IB market data stream disconnected for {}; waiting for data farm recovery: {:?}",
                        context.instrument_id,
                        e,
                    );

                    if !wait_for_data_farm_recovery_or_cancel(
                        &context.data_farm_state,
                        farm_generation,
                        &context.cancellation_token,
                    )
                    .await
                    {
                        return Ok(StreamAction::Stop);
                    }
                    return Ok(StreamAction::Resubscribe);
                }

                if let Ok(SubscriptionItem::Notice(notice)) = &tick_result {
                    context.data_farm_state.handle_notice(notice, context.clock);
                }

                match process_fn(tick_result, context).await? {
                    StreamAction::Continue => {}
                    action => return Ok(action),
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_quote_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: EventSender<DataEvent>,
    quote_cache: Arc<tokio::sync::Mutex<QuoteCache>>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    ignore_size_updates: bool,
    data_farm_state: Arc<DataFarmConnectionState>,
    stream_config: StreamConfig,
) -> anyhow::Result<()> {
    tracing::debug!("Starting quote subscription for {}", instrument_id);
    let context = StreamContext {
        stream_config,
        client,
        data_sender,
        clock,
        cancellation_token,
        data_farm_state,
        recovery_scope: DataFarmRecoveryScope::MarketData,
        instrument_id,
        price_precision,
        size_precision,
    };

    run_stream(
        &context,
        |context| {
            let contract = contract.clone();
            Box::pin(async move {
                context
                    .client
                    .market_data(&contract)
                    .streaming()
                    .subscribe()
                    .await
                    .context("Failed to create market data subscription")
            })
        },
        |subscription, context, farm_generation| {
            let quote_cache = Arc::clone(&quote_cache);
            Box::pin(async move {
                process_market_data_stream(
                    subscription,
                    context,
                    farm_generation,
                    |tick_result, context| {
                        let quote_cache = Arc::clone(&quote_cache);
                        Box::pin(async move {
                            process_quote_tick_result(
                                tick_result,
                                context.instrument_id,
                                context.price_precision,
                                context.size_precision,
                                &context.data_sender,
                                &quote_cache,
                                context.clock,
                                ignore_size_updates,
                            )
                            .await
                        })
                    },
                    context.monitor("quotes", farm_generation),
                )
                .await
            })
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)] // Stream state is supplied explicitly to the task.
pub(super) async fn handle_option_greeks_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    data_sender: EventSender<DataEvent>,
    option_greeks_cache: Arc<tokio::sync::Mutex<OptionGreeksCache>>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    data_farm_state: Arc<DataFarmConnectionState>,
    stream_config: StreamConfig,
) -> anyhow::Result<()> {
    tracing::debug!("Starting option greeks subscription for {}", instrument_id);
    let context = StreamContext {
        stream_config,
        client,
        data_sender,
        clock,
        cancellation_token,
        data_farm_state,
        recovery_scope: DataFarmRecoveryScope::MarketData,
        instrument_id,
        price_precision: 0,
        size_precision: 0,
    };

    run_stream(
        &context,
        |context| {
            let contract = contract.clone();
            Box::pin(async move {
                context
                    .client
                    .market_data(&contract)
                    .generic_ticks(&[IB_GENERIC_TICK_OPTION_OPEN_INTEREST])
                    .streaming()
                    .subscribe()
                    .await
                    .context("Failed to create option greeks market data subscription")
            })
        },
        |subscription, context, farm_generation| {
            let option_greeks_cache = Arc::clone(&option_greeks_cache);
            Box::pin(async move {
                process_market_data_stream(
                    subscription,
                    context,
                    farm_generation,
                    |tick_result, context| {
                        let option_greeks_cache = Arc::clone(&option_greeks_cache);
                        Box::pin(async move {
                            process_option_greeks_tick_result(
                                tick_result,
                                context.instrument_id,
                                &context.data_sender,
                                &option_greeks_cache,
                                context.clock,
                            )
                            .await
                        })
                    },
                    context.monitor("option_greeks", farm_generation),
                )
                .await
            })
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_index_price_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    price_precision: u8,
    price_magnifier: i32,
    data_sender: EventSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    data_farm_state: Arc<DataFarmConnectionState>,
    stream_config: StreamConfig,
) -> anyhow::Result<()> {
    tracing::debug!("Starting index price subscription for {}", instrument_id);
    let context = StreamContext {
        stream_config,
        client,
        data_sender,
        clock,
        cancellation_token,
        data_farm_state,
        recovery_scope: DataFarmRecoveryScope::MarketData,
        instrument_id,
        price_precision,
        size_precision: 0,
    };

    run_stream(
        &context,
        |context| {
            let contract = contract.clone();
            Box::pin(async move {
                context
                    .client
                    .market_data(&contract)
                    .streaming()
                    .subscribe()
                    .await
                    .context("Failed to create index market data subscription")
            })
        },
        move |subscription, context, farm_generation| {
            Box::pin(process_market_data_stream(
                subscription,
                context,
                farm_generation,
                move |tick_result, context| {
                    Box::pin(async move {
                        process_index_price_tick_result(
                            tick_result,
                            context.instrument_id,
                            context.price_precision,
                            price_magnifier,
                            &context.data_sender,
                            context.clock,
                        )
                        .await
                    })
                },
                context.monitor("index_prices", farm_generation),
            ))
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_tick_by_tick_quote_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: EventSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    price_magnifier: f64,
    data_farm_state: Arc<DataFarmConnectionState>,
    stream_config: StreamConfig,
) -> anyhow::Result<()> {
    tracing::debug!(
        "Starting tick-by-tick quote subscription for {}",
        instrument_id
    );

    let context = StreamContext {
        stream_config,
        client,
        data_sender,
        clock,
        cancellation_token,
        data_farm_state,
        recovery_scope: DataFarmRecoveryScope::MarketData,
        instrument_id,
        price_precision,
        size_precision,
    };

    run_stream(
        &context,
        |context| {
            let contract = contract.clone();
            Box::pin(async move {
                context
                    .client
                    .tick_by_tick(&contract, 0)
                    .bid_ask(IgnoreSize::No)
                    .await
                    .context("Failed to create tick-by-tick bid/ask subscription")
            })
        },
        move |subscription, context, farm_generation| {
            Box::pin(process_tick_by_tick_quote_stream(
                subscription,
                context,
                farm_generation,
                price_magnifier,
                context.monitor("quotes", farm_generation),
            ))
        },
    )
    .await
}

async fn process_tick_by_tick_quote_stream<S>(
    subscription: &mut S,
    context: &StreamContext,
    farm_generation: u64,
    price_magnifier: f64,
    mut monitor: SubscriptionMonitor,
) -> anyhow::Result<StreamAction>
where
    S: CancellableStream
        + Stream<Item = Result<SubscriptionItem<ibapi::market_data::realtime::BidAsk>, Error>>
        + Unpin,
{
    loop {
        tokio::select! {
            biased;
            () = context.cancellation_token.cancelled() => return Ok(StreamAction::Stop),
            () = context.data_farm_state.wait_for_recovery_after(farm_generation) => {
                return Ok(StreamAction::Resubscribe);
            }
            () = monitor.wait_for_idle() => {
                if !monitor.notify_idle() {
                    return Ok(StreamAction::Stop);
                }
            }
            tick_result = subscription.next() => {
                match tick_result {
                    Some(Ok(SubscriptionItem::Data(bid_ask))) => {
                        monitor.received_data();

                        if is_sentinel_price(bid_ask.bid_price, Some(bid_ask.bid_size))
                            || is_sentinel_price(bid_ask.ask_price, Some(bid_ask.ask_size))
                            || !bid_ask.bid_size.is_finite()
                            || bid_ask.bid_size <= 0.0
                            || !bid_ask.ask_size.is_finite()
                            || bid_ask.ask_size <= 0.0
                        {
                            tracing::debug!(
                                "Ignoring incomplete IB quote for {}: bid={}@{}, ask={}@{}",
                                context.instrument_id,
                                bid_ask.bid_size,
                                bid_ask.bid_price,
                                bid_ask.ask_size,
                                bid_ask.ask_price,
                            );
                            continue;
                        }
                        let ts_event = ib_timestamp_to_unix_nanos(&bid_ask.time);
                        let ts_init = context.clock.get_time_ns();
                        let quote_tick = parse_quote_tick(
                            context.instrument_id,
                            Some(bid_ask.bid_price * price_magnifier),
                            Some(bid_ask.ask_price * price_magnifier),
                            Some(bid_ask.bid_size),
                            Some(bid_ask.ask_size),
                            context.price_precision,
                            context.size_precision,
                            ts_event,
                            ts_init,
                        );

                        match quote_tick {
                            Ok(quote_tick) => {
                                if context
                                    .data_sender
                                    .send(DataEvent::Data(Data::Quote(quote_tick)))
                                    .is_err()
                                {
                                    return Ok(StreamAction::Stop);
                                }
                            }
                            Err(e) => log_tick_parse_error("quote", context.instrument_id, &e),
                        }
                    }
                    Some(Ok(SubscriptionItem::Notice(notice))) => {
                        context.data_farm_state.handle_notice(&notice, context.clock);
                        tracing::debug!(
                            "IB tick-by-tick quote notice for {}: {} - {}",
                            context.instrument_id,
                            notice.code,
                            notice.message,
                        );
                    }
                    Some(Err(e)) if is_subscription_disconnected_error(&e) => {
                        context
                            .data_farm_state
                            .mark_degraded(farm_generation, context.clock.get_time_ns());
                        if !wait_for_data_farm_recovery_or_cancel(
                            &context.data_farm_state,
                            farm_generation,
                            &context.cancellation_token,
                        )
                        .await
                        {
                            return Ok(StreamAction::Stop);
                        }
                        return Ok(StreamAction::Resubscribe);
                    }
                    Some(Err(e)) => anyhow::bail!("Subscription error: {e:?}"),
                    None => {
                        tracing::warn!(
                            "IB tick-by-tick quote stream ended unexpectedly for {}; resubscribing",
                            context.instrument_id,
                        );

                        return Ok(StreamAction::Resubscribe);
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_trade_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: EventSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    data_farm_state: Arc<DataFarmConnectionState>,
    stream_config: StreamConfig,
) -> anyhow::Result<()> {
    tracing::debug!("Starting trade subscription for {}", instrument_id);
    let context = StreamContext {
        stream_config,
        client,
        data_sender,
        clock,
        cancellation_token,
        data_farm_state,
        recovery_scope: DataFarmRecoveryScope::MarketData,
        instrument_id,
        price_precision,
        size_precision,
    };

    run_stream(
        &context,
        |context| {
            let contract = contract.clone();
            Box::pin(async move {
                let builder = context.client.tick_by_tick(&contract, 0);
                let subscription = if context.stream_config.all_last_trades {
                    builder.all_last().await
                } else {
                    builder.last().await
                };
                subscription.context("Failed to create tick-by-tick trade subscription")
            })
        },
        |subscription, context, farm_generation| {
            Box::pin(process_trade_stream(
                subscription,
                context.instrument_id,
                context.price_precision,
                context.size_precision,
                &context.data_sender,
                context.clock,
                &context.cancellation_token,
                &context.data_farm_state,
                farm_generation,
                context.monitor("trades", farm_generation),
            ))
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_realtime_bars_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    bar_type: BarType,
    bar_type_str: String,
    instrument_id: InstrumentId,
    what_to_show: RealtimeWhatToShow,
    price_precision: u8,
    size_precision: u8,
    data_sender: EventSender<DataEvent>,
    clock: &'static AtomicTime,
    last_bars: Arc<tokio::sync::Mutex<AHashMap<String, RealtimeBar>>>,
    bar_timeout_tasks: Arc<tokio::sync::Mutex<AHashMap<String, TaskSlot<()>>>>,
    handle_revised_bars: bool,
    use_rth: bool,
    cancellation_token: CancellationToken,
    data_farm_state: Arc<DataFarmConnectionState>,
    stream_config: StreamConfig,
) -> anyhow::Result<()> {
    tracing::debug!("Starting bars subscription for {}", bar_type);
    let trading_hours = if use_rth {
        TradingHours::Regular
    } else {
        TradingHours::Extended
    };

    let context = StreamContext {
        stream_config,
        client,
        data_sender,
        clock,
        cancellation_token,
        data_farm_state,
        recovery_scope: DataFarmRecoveryScope::MarketData,
        instrument_id,
        price_precision,
        size_precision,
    };

    run_stream(
        &context,
        |context| {
            let contract = contract.clone();
            Box::pin(async move {
                context
                    .client
                    .realtime_bars(&contract)
                    .what_to_show(what_to_show)
                    .trading_hours(trading_hours)
                    .subscribe()
                    .await
                    .context("Failed to create realtime bars subscription")
            })
        },
        |subscription, context, farm_generation| {
            let bar_type_str = bar_type_str.clone();
            let last_bars = Arc::clone(&last_bars);
            let bar_timeout_tasks = Arc::clone(&bar_timeout_tasks);
            Box::pin(async move {
                process_realtime_bar_stream(
                    subscription,
                    bar_type,
                    &bar_type_str,
                    context.price_precision,
                    context.size_precision,
                    &context.data_sender,
                    &last_bars,
                    &bar_timeout_tasks,
                    handle_revised_bars,
                    &context.cancellation_token,
                    &context.data_farm_state,
                    farm_generation,
                    context.clock,
                    context.monitor(format!("bars:{bar_type}"), farm_generation),
                )
                .await
            })
        },
    )
    .await
}

async fn update_revised_bar_tracking(
    bar_type_str: &str,
    bar: RealtimeBar,
    last_bars: &Arc<tokio::sync::Mutex<AHashMap<String, RealtimeBar>>>,
    bar_timeout_tasks: &Arc<tokio::sync::Mutex<AHashMap<String, TaskSlot<()>>>>,
) {
    last_bars.lock().await.insert(bar_type_str.to_string(), bar);

    if let Some(mut existing) = bar_timeout_tasks.lock().await.remove(bar_type_str) {
        existing.abort();
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_trade_stream<S>(
    subscription: &mut S,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: &EventSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: &CancellationToken,
    data_farm_state: &DataFarmConnectionState,
    farm_generation: u64,
    mut monitor: SubscriptionMonitor,
) -> anyhow::Result<StreamAction>
where
    S: CancellableStream
        + Stream<Item = Result<SubscriptionItem<ibapi::market_data::realtime::Trade>, Error>>
        + Unpin,
{
    loop {
        tokio::select! {
            biased;
            () = cancellation_token.cancelled() => {
                tracing::debug!("Trade subscription cancelled for {}", instrument_id);
                subscription.cancel().await;
                return Ok(StreamAction::Stop);
            }
            () = data_farm_state.wait_for_recovery_after(farm_generation) => {
                subscription.cancel().await;
                return Ok(StreamAction::Resubscribe);
            }
            () = monitor.wait_for_idle() => {
                if !monitor.notify_idle() {
                    return Ok(StreamAction::Stop);
                }
            }
            tick_result = subscription.next() => {
                match tick_result {
                    Some(Ok(SubscriptionItem::Data(tick))) => {
                        monitor.received_data();

                        if is_sentinel_price(tick.price, Some(tick.size))
                            || !tick.size.is_finite()
                            || tick.size <= 0.0
                        {
                            tracing::debug!(
                                "Ignoring IB trade sentinel for {}: price={}, size={}",
                                instrument_id,
                                tick.price,
                                tick.size
                            );
                            continue;
                        }
                        let ts_event = ib_timestamp_to_unix_nanos(&tick.time);
                        let ts_init = clock.get_time_ns();

                        match parse_trade_tick(
                            instrument_id,
                            tick.price,
                            tick.size,
                            price_precision,
                            size_precision,
                            ts_event,
                            ts_init,
                            None,
                        ) {
                            Ok(trade_tick) => {
                                if data_sender.send(DataEvent::Data(Data::Trade(trade_tick))).is_err() {
                                    return Ok(StreamAction::Stop);
                                }
                            }
                            Err(e) => log_tick_parse_error("trade", instrument_id, &e),
                        }
                    }
                    Some(Ok(SubscriptionItem::Notice(notice))) => {
                        data_farm_state.handle_notice(&notice, clock);
                        tracing::debug!(
                            "IB trade notice for {}: {} - {}",
                            instrument_id,
                            notice.code,
                            notice.message
                        );
                    }
                    Some(Err(e)) if is_subscription_disconnected_error(&e) => {
                        data_farm_state.mark_degraded(farm_generation, clock.get_time_ns());
                        tracing::warn!(
                            "Trade subscription disconnected for {}; waiting for data farm recovery: {:?}",
                            instrument_id,
                            e
                        );

                        if !wait_for_data_farm_recovery_or_cancel(
                            data_farm_state,
                            farm_generation,
                            cancellation_token,
                        )
                        .await
                        {
                            subscription.cancel().await;
                            return Ok(StreamAction::Stop);
                        }
                        return Ok(StreamAction::Resubscribe);
                    }
                    Some(Err(e)) => {
                        tracing::error!("Trade subscription error for {}: {:?}", instrument_id, e);
                        anyhow::bail!("Subscription error: {e:?}");
                    }
                    None => {
                        tracing::warn!(
                            "IB trade stream ended unexpectedly for {}; resubscribing",
                            instrument_id,
                        );

                        return Ok(StreamAction::Resubscribe);
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_realtime_bar_stream<S>(
    subscription: &mut S,
    bar_type: BarType,
    bar_type_str: &str,
    price_precision: u8,
    size_precision: u8,
    data_sender: &EventSender<DataEvent>,
    last_bars: &Arc<tokio::sync::Mutex<AHashMap<String, RealtimeBar>>>,
    bar_timeout_tasks: &Arc<tokio::sync::Mutex<AHashMap<String, TaskSlot<()>>>>,
    handle_revised_bars: bool,
    cancellation_token: &CancellationToken,
    data_farm_state: &DataFarmConnectionState,
    farm_generation: u64,
    clock: &'static AtomicTime,
    mut monitor: SubscriptionMonitor,
) -> anyhow::Result<StreamAction>
where
    S: CancellableStream + Stream<Item = Result<SubscriptionItem<RealtimeBar>, Error>> + Unpin,
{
    loop {
        tokio::select! {
            biased;
            () = cancellation_token.cancelled() => {
                tracing::debug!("Bars subscription cancelled for {}", bar_type);
                subscription.cancel().await;
                return Ok(StreamAction::Stop);
            }
            () = data_farm_state.wait_for_recovery_after(farm_generation) => {
                subscription.cancel().await;
                return Ok(StreamAction::Resubscribe);
            }
            () = monitor.wait_for_idle() => {
                if !monitor.notify_idle() {
                    return Ok(StreamAction::Stop);
                }
            }
            bar_result = subscription.next() => {
                match bar_result {
                    Some(Ok(SubscriptionItem::Data(bar))) => {
                        monitor.received_data();
                        let parsed_bar = ib_bar_to_nautilus_bar(
                            &HistoricalBar {
                                date: bar.date.into(),
                                open: bar.open,
                                high: bar.high,
                                low: bar.low,
                                close: bar.close,
                                volume: bar.volume,
                                wap: bar.wap,
                                count: bar.count,
                            },
                            bar_type,
                            price_precision,
                            size_precision,
                        )?;

                        if data_sender.send(DataEvent::Data(Data::Bar(parsed_bar))).is_err() {
                            return Ok(StreamAction::Stop);
                        }

                        if handle_revised_bars {
                            update_revised_bar_tracking(
                                bar_type_str,
                                bar,
                                last_bars,
                                bar_timeout_tasks,
                            )
                            .await;
                        }
                    }
                    Some(Ok(SubscriptionItem::Notice(notice))) => {
                        data_farm_state.handle_notice(&notice, clock);
                        tracing::debug!(
                            "IB realtime bar notice for {}: {} - {}",
                            bar_type,
                            notice.code,
                            notice.message
                        );
                    }
                    Some(Err(e)) if is_subscription_disconnected_error(&e) => {
                        data_farm_state.mark_degraded(farm_generation, clock.get_time_ns());
                        tracing::warn!(
                            "Realtime bar subscription disconnected for {}; waiting for data farm recovery: {:?}",
                            bar_type,
                            e
                        );

                        if !wait_for_data_farm_recovery_or_cancel(
                            data_farm_state,
                            farm_generation,
                            cancellation_token,
                        )
                        .await
                        {
                            subscription.cancel().await;
                            return Ok(StreamAction::Stop);
                        }
                        return Ok(StreamAction::Resubscribe);
                    }
                    Some(Err(e)) => {
                        tracing::error!("Bars subscription error for {}: {:?}", bar_type, e);
                        anyhow::bail!("Subscription error: {e:?}");
                    }
                    None => {
                        tracing::warn!(
                            "IB realtime bar stream ended unexpectedly for {}; resubscribing",
                            bar_type,
                        );

                        return Ok(StreamAction::Resubscribe);
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_market_depth_stream<S>(
    subscription: &mut S,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: &EventSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: &CancellationToken,
    data_farm_state: &DataFarmConnectionState,
    farm_generation: u64,
    mut monitor: SubscriptionMonitor,
) -> anyhow::Result<StreamAction>
where
    S: CancellableStream + Stream<Item = Result<SubscriptionItem<MarketDepths>, Error>> + Unpin,
{
    let mut sequence: u64 = 0;
    let mut l2_order_ids = AHashMap::new();
    let mut next_l2_order_id = 1_u64;

    // Each (re)established stream re-sends the full depth, so clear any levels
    // left in the engine book by a previous stream before applying its deltas.
    if !send_depth_stream_clear(instrument_id, sequence, data_sender, clock) {
        return Ok(StreamAction::Stop);
    }

    loop {
        tokio::select! {
            biased;
            () = cancellation_token.cancelled() => {
                subscription.cancel().await;
                return Ok(StreamAction::Stop);
            }
            () = data_farm_state.wait_for_recovery_after(farm_generation) => {
                subscription.cancel().await;
                return Ok(StreamAction::Resubscribe);
            }
            () = monitor.wait_for_idle() => {
                if !monitor.notify_idle() {
                    return Ok(StreamAction::Stop);
                }
            }
            depth_result = subscription.next() => {
                match depth_result {
                    Some(Ok(SubscriptionItem::Data(MarketDepths::MarketDepth(depth)))) => {
                        monitor.received_data();
                        let ts_event = clock.get_time_ns();
                        let ts_init = ts_event;
                        let order_side = if depth.side == 1 { OrderSide::Buy } else { OrderSide::Sell };
                        let Some(action) = parse_market_depth_operation(depth.operation) else {
                            tracing::warn!(
                                "Ignoring unknown IB depth operation {} for {}",
                                depth.operation,
                                instrument_id
                            );
                            continue;
                        };
                        sequence += 1;
                        let price = Price::new(depth.price, price_precision);
                        let size = Quantity::new(depth.size, size_precision);
                        let order_id = depth.position as u64;
                        let order = BookOrder::new(order_side, price, size, order_id);
                        let delta = OrderBookDelta::new(
                            instrument_id,
                            action,
                            order,
                            0,
                            sequence,
                            ts_event,
                            ts_init,
                        );

                        if data_sender.send(DataEvent::Data(Data::BookDelta(delta))).is_err() {
                            return Ok(StreamAction::Stop);
                        }
                    }
                    Some(Ok(SubscriptionItem::Data(MarketDepths::MarketDepthL2(depth)))) => {
                        monitor.received_data();
                        let ts_event = clock.get_time_ns();
                        let ts_init = ts_event;
                        let order_side = if depth.side == 1 { OrderSide::Buy } else { OrderSide::Sell };
                        let Some(action) = parse_market_depth_operation(depth.operation) else {
                            tracing::warn!(
                                "Ignoring unknown IB L2 depth operation {} for {}",
                                depth.operation,
                                instrument_id
                            );
                            continue;
                        };
                        sequence += 1;
                        let price = Price::new(depth.price, price_precision);
                        let size = Quantity::new(depth.size, size_precision);
                        let order_key = (depth.position, depth.market_maker.clone());
                        let order_id = *l2_order_ids.entry(order_key.clone()).or_insert_with(|| {
                            let order_id = next_l2_order_id;
                            next_l2_order_id += 1;
                            order_id
                        });
                        let order = BookOrder::new(order_side, price, size, order_id);
                        let delta = OrderBookDelta::new(
                            instrument_id,
                            action,
                            order,
                            0,
                            sequence,
                            ts_event,
                            ts_init,
                        );

                        if data_sender.send(DataEvent::Data(Data::BookDelta(delta))).is_err() {
                            return Ok(StreamAction::Stop);
                        }

                        if action == BookAction::Delete {
                            l2_order_ids.remove(&order_key);
                        }
                    }
                    Some(Ok(SubscriptionItem::Notice(notice))) => {
                        data_farm_state.handle_notice(&notice, clock);
                        tracing::debug!(
                            "IB market depth notice for {}: {} - {}",
                            instrument_id,
                            notice.code,
                            notice.message
                        );
                    }
                    Some(Err(e)) if is_subscription_disconnected_error(&e) => {
                        data_farm_state.mark_degraded(farm_generation, clock.get_time_ns());
                        tracing::warn!(
                            "Market depth subscription disconnected for {}; waiting for data farm recovery: {:?}",
                            instrument_id,
                            e
                        );

                        if !wait_for_data_farm_recovery_or_cancel(
                            data_farm_state,
                            farm_generation,
                            cancellation_token,
                        )
                        .await
                        {
                            subscription.cancel().await;
                            return Ok(StreamAction::Stop);
                        }
                        return Ok(StreamAction::Resubscribe);
                    }
                    Some(Err(e)) => anyhow::bail!("Subscription error: {e:?}"),
                    None => {
                        tracing::warn!(
                            "IB market depth stream ended unexpectedly for {}; resubscribing",
                            instrument_id,
                        );

                        return Ok(StreamAction::Resubscribe);
                    }
                }
            }
        }
    }
}

fn send_depth_stream_clear(
    instrument_id: InstrumentId,
    sequence: u64,
    data_sender: &EventSender<DataEvent>,
    clock: &'static AtomicTime,
) -> bool {
    let ts_clear = clock.get_time_ns();
    let clear = OrderBookDelta::clear(instrument_id, sequence, ts_clear, ts_clear);
    data_sender
        .send(DataEvent::Data(Data::BookDelta(clear)))
        .is_ok()
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_market_depth_subscription(
    client: Arc<ibapi::Client>,
    contract: ibapi::contracts::Contract,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    depth_rows: i32,
    is_smart_depth: bool,
    data_sender: EventSender<DataEvent>,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
    data_farm_state: Arc<DataFarmConnectionState>,
    stream_config: StreamConfig,
) -> anyhow::Result<()> {
    let context = StreamContext {
        stream_config,
        client,
        data_sender,
        clock,
        cancellation_token,
        data_farm_state,
        recovery_scope: DataFarmRecoveryScope::MarketData,
        instrument_id,
        price_precision,
        size_precision,
    };

    run_stream(
        &context,
        |context| {
            let contract = contract.clone();
            Box::pin(async move {
                let subscription = context
                    .client
                    .market_depth(&contract, depth_rows)
                    .smart_depth(if is_smart_depth {
                        SmartDepth::Yes
                    } else {
                        SmartDepth::No
                    })
                    .subscribe()
                    .await
                    .context("Failed to create market depth subscription")?;
                let ts_clear = context.clock.get_time_ns();
                let clear = OrderBookDelta::clear(context.instrument_id, 0, ts_clear, ts_clear);
                context
                    .data_sender
                    .send(DataEvent::Data(Data::BookDelta(clear)))
                    .map_err(|e| anyhow::anyhow!("Failed to send order book clear: {e}"))?;
                Ok(subscription)
            })
        },
        |subscription, context, farm_generation| {
            Box::pin(process_market_depth_stream(
                subscription,
                context.instrument_id,
                context.price_precision,
                context.size_precision,
                &context.data_sender,
                context.clock,
                &context.cancellation_token,
                &context.data_farm_state,
                farm_generation,
                context.monitor("order_book", farm_generation),
            ))
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn process_quote_tick_result<I, E>(
    tick_result: Result<I, E>,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    data_sender: &EventSender<DataEvent>,
    quote_cache: &Arc<tokio::sync::Mutex<QuoteCache>>,
    clock: &'static AtomicTime,
    ignore_size_updates: bool,
) -> anyhow::Result<StreamAction>
where
    I: IntoSubscriptionTick,
    E: Debug,
{
    match tick_result.map(IntoSubscriptionTick::into_subscription_item) {
        Ok(SubscriptionItem::Data(TickTypes::Price(price))) => {
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;

            let quote = {
                let mut cache = quote_cache.lock().await;
                update_quote_from_price_tick(
                    &mut cache,
                    instrument_id,
                    &price,
                    price_precision,
                    size_precision,
                    ts_event,
                    ts_init,
                )
            };

            Ok(send_quote_tick(quote, data_sender, instrument_id))
        }
        Ok(SubscriptionItem::Data(TickTypes::Size(size))) => {
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;

            let quote = {
                let mut cache = quote_cache.lock().await;
                update_quote_from_size_tick(
                    &mut cache,
                    instrument_id,
                    &size,
                    price_precision,
                    size_precision,
                    ts_event,
                    ts_init,
                    ignore_size_updates,
                )
            };

            Ok(send_quote_tick(quote, data_sender, instrument_id))
        }
        Ok(SubscriptionItem::Data(TickTypes::PriceSize(price_size))) => {
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;

            let quote = {
                let mut cache = quote_cache.lock().await;
                update_quote_from_price_size_tick(
                    &mut cache,
                    instrument_id,
                    &price_size,
                    price_precision,
                    size_precision,
                    ts_event,
                    ts_init,
                )
            };

            Ok(send_quote_tick(quote, data_sender, instrument_id))
        }
        Ok(SubscriptionItem::Notice(notice)) => {
            tracing::debug!(
                "IB notice for {}: {} - {}",
                instrument_id,
                notice.code,
                notice.message
            );

            if notice.code == 162 {
                tracing::debug!("Market data subscription cancelled for {}", instrument_id);
                return Ok(StreamAction::Stop);
            }
            Ok(StreamAction::Continue)
        }
        Ok(SubscriptionItem::Data(TickTypes::SnapshotEnd)) => {
            tracing::debug!("Snapshot end received for {}", instrument_id);
            Ok(StreamAction::Continue)
        }
        Ok(SubscriptionItem::Data(_)) => Ok(StreamAction::Continue),
        Err(e) => {
            tracing::error!("Subscription error for {}: {:?}", instrument_id, e);
            anyhow::bail!("Subscription error: {e:?}");
        }
    }
}

async fn process_option_greeks_tick_result<I, E>(
    tick_result: Result<I, E>,
    instrument_id: InstrumentId,
    data_sender: &EventSender<DataEvent>,
    option_greeks_cache: &Arc<tokio::sync::Mutex<OptionGreeksCache>>,
    clock: &'static AtomicTime,
) -> anyhow::Result<StreamAction>
where
    I: IntoSubscriptionTick,
    E: Debug,
{
    match tick_result.map(IntoSubscriptionTick::into_subscription_item) {
        Ok(SubscriptionItem::Data(TickTypes::OptionComputation(computation))) => {
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;

            let greeks = {
                let mut cache = option_greeks_cache.lock().await;
                cache.update_from_computation(instrument_id, &computation, ts_event, ts_init)
            };

            Ok(send_option_greeks(greeks, data_sender, instrument_id))
        }
        Ok(SubscriptionItem::Data(TickTypes::Generic(TickGeneric { tick_type, value }))) => {
            process_option_open_interest_tick(
                instrument_id,
                tick_type,
                value,
                data_sender,
                option_greeks_cache,
                clock,
            )
            .await
        }
        Ok(SubscriptionItem::Data(TickTypes::Size(TickSize { tick_type, size }))) => {
            process_option_open_interest_tick(
                instrument_id,
                tick_type,
                size,
                data_sender,
                option_greeks_cache,
                clock,
            )
            .await
        }
        Ok(SubscriptionItem::Notice(notice)) => {
            tracing::debug!(
                "IB option greeks notice for {}: {} - {}",
                instrument_id,
                notice.code,
                notice.message
            );

            if notice.code == 162 {
                tracing::debug!("Option greeks subscription cancelled for {}", instrument_id);
                return Ok(StreamAction::Stop);
            }
            Ok(StreamAction::Continue)
        }
        Ok(SubscriptionItem::Data(TickTypes::SnapshotEnd)) => {
            tracing::debug!("Option greeks snapshot end received for {}", instrument_id);
            Ok(StreamAction::Continue)
        }
        Ok(SubscriptionItem::Data(_)) => Ok(StreamAction::Continue),
        Err(e) => {
            tracing::error!(
                "Option greeks subscription error for {}: {:?}",
                instrument_id,
                e
            );
            anyhow::bail!("Subscription error: {e:?}");
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_index_price_tick_result<I, E>(
    tick_result: Result<I, E>,
    instrument_id: InstrumentId,
    price_precision: u8,
    price_magnifier: i32,
    data_sender: &EventSender<DataEvent>,
    clock: &'static AtomicTime,
) -> anyhow::Result<StreamAction>
where
    I: IntoSubscriptionTick,
    E: Debug,
{
    match tick_result.map(IntoSubscriptionTick::into_subscription_item) {
        Ok(SubscriptionItem::Data(TickTypes::Price(price)))
            if matches!(price.tick_type, TickType::Last) =>
        {
            if is_sentinel_price(price.price, None) {
                tracing::debug!(
                    "Ignoring IB index price sentinel for {}: {}",
                    instrument_id,
                    price.price
                );
                return Ok(StreamAction::Continue);
            }
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;
            let index_price = parse_index_price(
                instrument_id,
                price.price,
                price_precision,
                price_magnifier,
                ts_event,
                ts_init,
            )?;

            if data_sender
                .send(DataEvent::Data(Data::IndexPrice(index_price)))
                .is_err()
            {
                return Ok(StreamAction::Stop);
            }
            Ok(StreamAction::Continue)
        }
        Ok(SubscriptionItem::Data(TickTypes::PriceSize(price_size)))
            if matches!(price_size.price_tick_type, TickType::Last) =>
        {
            if is_sentinel_price(price_size.price, Some(price_size.size)) {
                tracing::debug!(
                    "Ignoring IB index price sentinel for {}: {}",
                    instrument_id,
                    price_size.price
                );
                return Ok(StreamAction::Continue);
            }
            let ts_event = clock.get_time_ns();
            let ts_init = ts_event;
            let index_price = parse_index_price(
                instrument_id,
                price_size.price,
                price_precision,
                price_magnifier,
                ts_event,
                ts_init,
            )?;

            if data_sender
                .send(DataEvent::Data(Data::IndexPrice(index_price)))
                .is_err()
            {
                return Ok(StreamAction::Stop);
            }
            Ok(StreamAction::Continue)
        }
        Ok(SubscriptionItem::Notice(_)) => Ok(StreamAction::Continue),
        Ok(SubscriptionItem::Data(_)) => Ok(StreamAction::Continue),
        Err(e) => {
            tracing::error!(
                "Index price subscription stream error for {}: {:?}",
                instrument_id,
                e
            );
            anyhow::bail!("Subscription error: {e:?}");
        }
    }
}

fn update_quote_from_price_tick(
    cache: &mut QuoteCache,
    instrument_id: InstrumentId,
    price: &TickPrice,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Option<QuoteTick> {
    match price.tick_type {
        TickType::Bid | TickType::DelayedBid => cache.update_bid_price(
            instrument_id,
            price.price,
            None,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        TickType::Ask | TickType::DelayedAsk => cache.update_ask_price(
            instrument_id,
            price.price,
            None,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        TickType::Last | TickType::DelayedLast => None,
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn update_quote_from_size_tick(
    cache: &mut QuoteCache,
    instrument_id: InstrumentId,
    size: &TickSize,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    ignore_size_updates: bool,
) -> Option<QuoteTick> {
    match size.tick_type {
        TickType::BidSize | TickType::DelayedBidSize => cache.update_bid_size_with_filter(
            instrument_id,
            size.size,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
            ignore_size_updates,
        ),
        TickType::AskSize | TickType::DelayedAskSize => cache.update_ask_size_with_filter(
            instrument_id,
            size.size,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
            ignore_size_updates,
        ),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn update_quote_from_price_size_tick(
    cache: &mut QuoteCache,
    instrument_id: InstrumentId,
    price_size: &TickPriceSize,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Option<QuoteTick> {
    let quote = match price_size.price_tick_type {
        TickType::Bid | TickType::DelayedBid => cache.update_bid_price(
            instrument_id,
            price_size.price,
            Some(price_size.size),
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        TickType::Ask | TickType::DelayedAsk => cache.update_ask_price(
            instrument_id,
            price_size.price,
            Some(price_size.size),
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        TickType::Last | TickType::DelayedLast => None,
        _ => None,
    };

    if price_size.size <= 0.0 {
        return quote;
    }

    match price_size.price_tick_type {
        TickType::Bid | TickType::DelayedBid => cache.update_bid_size(
            instrument_id,
            price_size.size,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        TickType::Ask | TickType::DelayedAsk => cache.update_ask_size(
            instrument_id,
            price_size.size,
            price_precision,
            size_precision,
            ts_event,
            ts_init,
        ),
        _ => quote,
    }
}

fn send_quote_tick(
    quote: Option<QuoteTick>,
    data_sender: &EventSender<DataEvent>,
    instrument_id: InstrumentId,
) -> StreamAction {
    if let Some(quote_tick) = quote
        && data_sender
            .send(DataEvent::Data(Data::Quote(quote_tick)))
            .is_err()
    {
        tracing::warn!(
            "Data channel closed, stopping subscription for {}",
            instrument_id
        );
        return StreamAction::Stop;
    }

    StreamAction::Continue
}

fn send_option_greeks(
    greeks: Option<OptionGreeks>,
    data_sender: &EventSender<DataEvent>,
    instrument_id: InstrumentId,
) -> StreamAction {
    if let Some(option_greeks) = greeks
        && data_sender
            .send(DataEvent::Data(Data::OptionGreeks(option_greeks)))
            .is_err()
    {
        tracing::warn!(
            "Data channel closed, stopping option greeks subscription for {}",
            instrument_id
        );
        return StreamAction::Stop;
    }

    StreamAction::Continue
}

async fn process_option_open_interest_tick(
    instrument_id: InstrumentId,
    tick_type: TickType,
    value: f64,
    data_sender: &EventSender<DataEvent>,
    option_greeks_cache: &Arc<tokio::sync::Mutex<OptionGreeksCache>>,
    clock: &'static AtomicTime,
) -> anyhow::Result<StreamAction> {
    let Some(open_interest) = parse_option_open_interest(&tick_type, value) else {
        return Ok(StreamAction::Continue);
    };

    let ts_event = clock.get_time_ns();
    let ts_init = ts_event;
    let greeks = {
        let mut cache = option_greeks_cache.lock().await;
        cache.update_open_interest(instrument_id, open_interest, ts_event, ts_init)
    };

    Ok(send_option_greeks(greeks, data_sender, instrument_id))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use ahash::AHashMap;
    use ibapi::{
        Error, Notice,
        contracts::{OptionComputation, tick_types::TickType},
        market_data::realtime::{
            Bar as RealtimeBar, MarketDepth, MarketDepthL2, MarketDepths, TickAttribute,
            TickGeneric, TickPrice, TickPriceSize, TickSize, TickTypes, Trade, TradeAttribute,
        },
        subscriptions::SubscriptionItem,
    };
    use nautilus_common::messages::DataEvent;
    use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_live::task::TaskSlot;
    use nautilus_model::{
        data::{BarType, Data},
        enums::BookAction,
        identifiers::{InstrumentId, Symbol, Venue},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use tokio_util::sync::CancellationToken;

    use super::{
        AtomicTime, CancellableStream, ClientId, DataFarmConnectionState, DataFarmIdentity,
        DataFarmKind, DataFarmRecoveryScope, InteractiveBrokersSubscriptionIdle, OptionGreeksCache,
        QuoteCache, StreamAction, StreamConfig, SubscriptionMonitor,
        process_index_price_tick_result, process_market_depth_stream,
        process_option_greeks_tick_result, process_quote_tick_result, process_realtime_bar_stream,
        process_trade_stream, send_quote_tick, update_quote_from_price_tick,
        update_revised_bar_tracking,
    };
    use crate::stubs::ChannelSubscription;

    impl<T: Send + 'static> CancellableStream for ChannelSubscription<T> {
        async fn cancel(&mut self) {
            self.close();
        }
    }

    fn instrument_id() -> InstrumentId {
        InstrumentId::new(Symbol::from("SPX"), Venue::from("CBOE"))
    }

    fn minute_bar_type() -> BarType {
        BarType::from("SPX.CBOE-1-MINUTE-LAST-EXTERNAL")
    }

    fn notice(code: i32, message: &str) -> Notice {
        Notice {
            request_id: None,
            code,
            message: message.to_string(),
            error_time: None,
            advanced_order_reject_json: String::new(),
        }
    }

    #[rstest]
    fn test_resolve_historical_bar_start_ns_uses_current_time_when_missing() {
        let now_ns = UnixNanos::from(2_000);

        let start_ns = super::resolve_historical_bar_start_ns(None, now_ns);

        assert_eq!(start_ns, now_ns);
    }

    #[rstest]
    fn test_resolve_historical_bar_replay_start_ns_uses_last_disconnect_when_later() {
        let first_start_ns = UnixNanos::from(1_000);
        let last_disconnection_ns = UnixNanos::from(1_500);

        let replay_start_ns = super::resolve_historical_bar_replay_start_ns(
            first_start_ns,
            Some(last_disconnection_ns),
        );

        assert_eq!(replay_start_ns, last_disconnection_ns);
    }

    #[rstest]
    fn test_resolve_historical_bar_replay_start_ns_uses_first_start_when_no_prior_disconnection() {
        let first_start_ns = UnixNanos::from(1_000);

        let replay_start_ns = super::resolve_historical_bar_replay_start_ns(first_start_ns, None);

        assert_eq!(replay_start_ns, first_start_ns);
    }

    #[rstest]
    fn test_resolve_historical_bar_replay_start_ns_uses_first_start_when_disconnection_before_start()
     {
        let first_start_ns = UnixNanos::from(1_000);
        let last_disconnection_ns = UnixNanos::from(500);

        let replay_start_ns = super::resolve_historical_bar_replay_start_ns(
            first_start_ns,
            Some(last_disconnection_ns),
        );

        assert_eq!(replay_start_ns, first_start_ns);
    }

    #[rstest]
    fn test_calculate_historical_bar_subscription_duration_requests_at_least_300_bars() {
        use ibapi::market_data::historical::ToDuration;

        let duration = super::calculate_historical_bar_subscription_duration(
            minute_bar_type(),
            UnixNanos::from(9_000_000_000),
            UnixNanos::from(10_000_000_000),
        );

        assert_eq!(duration, 18_000.seconds());
    }

    #[rstest]
    fn test_data_farm_state_records_recovery_generation() {
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();

        data_farm_state.handle_notice(
            &notice(2103, "Market data farm connection is broken:usfarm"),
            clock,
        );
        data_farm_state.handle_notice(
            &notice(2104, "Market data farm connection is OK:usfarm"),
            clock,
        );

        assert_eq!(data_farm_state.recovery_generation(), 1);
        assert!(
            data_farm_state
                .recovery_since_ns_after_for(DataFarmRecoveryScope::MarketData, 0)
                .is_some()
        );
    }

    #[rstest]
    fn test_data_farm_state_reports_pending_market_data_recovery() {
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();
        let recovered = notice(2104, "Market data farm connection is OK:usfarm");

        assert!(!data_farm_state.recovery_pending(&recovered));
        data_farm_state.mark_degraded(data_farm_state.recovery_generation(), UnixNanos::from(10));
        assert!(data_farm_state.recovery_pending(&recovered));

        data_farm_state.handle_notice(&recovered, clock);
        assert!(!data_farm_state.recovery_pending(&recovered));
    }

    #[tokio::test]
    async fn test_historical_bar_recovery_tracks_market_data_recovery() {
        let clock = get_atomic_clock_realtime();
        let data_farm_state = Arc::new(DataFarmConnectionState::default());
        let initial_generation =
            data_farm_state.recovery_generation_for(DataFarmRecoveryScope::HistoricalBars);
        let state_for_waiter = Arc::clone(&data_farm_state);
        let waiter = tokio::spawn(async move {
            state_for_waiter
                .wait_for_recovery_after_for(
                    DataFarmRecoveryScope::HistoricalBars,
                    initial_generation,
                )
                .await;
            state_for_waiter.recovery_generation_for(DataFarmRecoveryScope::HistoricalBars)
        });

        data_farm_state.handle_notice(
            &notice(2103, "Market data farm connection is broken:usfarm"),
            clock,
        );
        data_farm_state.handle_notice(
            &notice(2104, "Market data farm connection is OK:usfarm"),
            clock,
        );

        let observed_generation = tokio::time::timeout(Duration::from_millis(100), waiter)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(observed_generation, 1);
        assert!(
            data_farm_state
                .recovery_since_ns_after_for(
                    DataFarmRecoveryScope::HistoricalBars,
                    initial_generation,
                )
                .is_some()
        );
    }

    #[tokio::test]
    async fn test_historical_bar_recovery_tracks_historical_data_recovery() {
        let clock = get_atomic_clock_realtime();
        let data_farm_state = Arc::new(DataFarmConnectionState::default());
        let initial_generation =
            data_farm_state.recovery_generation_for(DataFarmRecoveryScope::HistoricalBars);
        let state_for_waiter = Arc::clone(&data_farm_state);
        let waiter = tokio::spawn(async move {
            state_for_waiter
                .wait_for_recovery_after_for(
                    DataFarmRecoveryScope::HistoricalBars,
                    initial_generation,
                )
                .await;
            state_for_waiter.recovery_generation_for(DataFarmRecoveryScope::HistoricalBars)
        });

        data_farm_state.handle_notice(
            &notice(2105, "HMDS data farm connection is broken:ushmds"),
            clock,
        );
        data_farm_state.handle_notice(
            &notice(2106, "HMDS data farm connection is OK:ushmds"),
            clock,
        );

        let observed_generation = tokio::time::timeout(Duration::from_millis(100), waiter)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(data_farm_state.recovery_generation(), 0);
        assert_eq!(
            data_farm_state.recovery_generation_for(DataFarmRecoveryScope::HistoricalData),
            1
        );
        assert_eq!(observed_generation, 1);
    }

    #[rstest]
    fn test_historical_bar_recovery_ignores_stale_subscription_degradation() {
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();
        let initial_generation =
            data_farm_state.recovery_generation_for(DataFarmRecoveryScope::HistoricalBars);

        data_farm_state.mark_degraded_for(
            DataFarmRecoveryScope::HistoricalBars,
            initial_generation,
            UnixNanos::from(10),
        );
        data_farm_state.handle_notice(
            &notice(2104, "Market data farm connection is OK:usfarm"),
            clock,
        );
        data_farm_state.mark_degraded_for(
            DataFarmRecoveryScope::HistoricalBars,
            initial_generation,
            UnixNanos::from(20),
        );
        data_farm_state.handle_notice(
            &notice(2106, "HMDS data farm connection is OK:ushmds"),
            clock,
        );

        let state = data_farm_state.state.lock();
        assert_eq!(state.historical_bars.recovery_generation, 1);
        assert_eq!(
            state.historical_bars.recoveries.front(),
            Some(&(1, UnixNanos::from(10)))
        );
        assert_eq!(state.market_data.recovery_generation, 0);
        assert_eq!(state.historical_data.recovery_generation, 0);
        assert!(state.degraded_scopes.is_empty());
    }

    #[rstest]
    fn test_historical_bar_recovery_preserves_earliest_cross_family_boundary() {
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();

        data_farm_state.mark_farm_degraded(
            DataFarmIdentity {
                kind: DataFarmKind::MarketData,
                name: Some(String::from("usfarm")),
            },
            UnixNanos::from(20),
        );
        data_farm_state.mark_farm_degraded(
            DataFarmIdentity {
                kind: DataFarmKind::HistoricalData,
                name: Some(String::from("ushmds")),
            },
            UnixNanos::from(10),
        );
        data_farm_state.handle_notice(
            &notice(2104, "Market data farm connection is OK:usfarm"),
            clock,
        );
        let market_recovery_generation =
            data_farm_state.recovery_generation_for(DataFarmRecoveryScope::HistoricalBars);
        data_farm_state.handle_notice(
            &notice(2106, "HMDS data farm connection is OK:ushmds"),
            clock,
        );

        assert_eq!(market_recovery_generation, 1);
        assert_eq!(
            data_farm_state.recovery_generation_for(DataFarmRecoveryScope::HistoricalBars),
            2
        );
        assert_eq!(
            data_farm_state.recovery_since_ns_after_for(DataFarmRecoveryScope::HistoricalBars, 0),
            Some(UnixNanos::from(10))
        );
        assert_eq!(
            data_farm_state.recovery_since_ns_after_for(
                DataFarmRecoveryScope::HistoricalBars,
                market_recovery_generation,
            ),
            Some(UnixNanos::from(10))
        );
    }

    #[rstest]
    fn test_security_definition_recovery_does_not_advance_historical_bars() {
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();

        data_farm_state.handle_notice(
            &notice(2157, "Sec-def data farm connection is broken:secdefil"),
            clock,
        );
        data_farm_state.handle_notice(
            &notice(2158, "Sec-def data farm connection is OK:secdefil"),
            clock,
        );

        assert_eq!(
            data_farm_state.recovery_generation_for(DataFarmRecoveryScope::SecurityDefinition),
            1
        );
        assert_eq!(
            data_farm_state.recovery_generation_for(DataFarmRecoveryScope::HistoricalBars),
            0
        );
        assert_eq!(
            data_farm_state.recovery_since_ns_after_for(DataFarmRecoveryScope::HistoricalBars, 0),
            None
        );
    }

    #[rstest]
    fn test_data_farm_state_recovers_each_reported_farm_independently() {
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();

        data_farm_state.handle_notice(
            &notice(2103, "Market data farm connection is broken:usfarm.nj"),
            clock,
        );
        data_farm_state.handle_notice(
            &notice(2103, "Market data farm connection is broken:cashfarm"),
            clock,
        );
        data_farm_state.handle_notice(
            &notice(2105, "HMDS data farm connection is broken:ushmds"),
            clock,
        );

        data_farm_state.handle_notice(&notice(2104, "Market data farm connection is OK"), clock);
        assert_eq!(data_farm_state.recovery_generation(), 0);

        data_farm_state.handle_notice(
            &notice(2104, "Market data farm connection is OK:usfarm.nj"),
            clock,
        );
        assert_eq!(data_farm_state.recovery_generation(), 1);
        assert_eq!(
            data_farm_state.recovery_generation_for(DataFarmRecoveryScope::HistoricalData),
            0
        );

        data_farm_state.handle_notice(
            &notice(2106, "HMDS data farm connection is OK:ushmds"),
            clock,
        );
        assert_eq!(data_farm_state.recovery_generation(), 1);
        assert_eq!(
            data_farm_state.recovery_generation_for(DataFarmRecoveryScope::HistoricalData),
            1
        );

        data_farm_state.handle_notice(
            &notice(2104, "Market data farm connection is OK:cashfarm"),
            clock,
        );
        assert_eq!(data_farm_state.recovery_generation(), 2);
    }

    #[rstest]
    fn test_data_farm_state_preserves_each_farm_degradation_time() {
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();

        data_farm_state.mark_farm_degraded(
            DataFarmIdentity {
                kind: DataFarmKind::MarketData,
                name: Some(String::from("usfarm.nj")),
            },
            UnixNanos::from(20),
        );
        data_farm_state.mark_farm_degraded(
            DataFarmIdentity {
                kind: DataFarmKind::MarketData,
                name: Some(String::from("cashfarm")),
            },
            UnixNanos::from(10),
        );
        data_farm_state.handle_notice(
            &notice(2104, "Market data farm connection is OK:usfarm.nj"),
            clock,
        );
        data_farm_state.handle_notice(
            &notice(2104, "Market data farm connection is OK:cashfarm"),
            clock,
        );
        let latest_generation = data_farm_state.recovery_generation();

        assert_eq!(latest_generation, 2);
        assert_eq!(
            data_farm_state.recovery_since_ns_after_for(DataFarmRecoveryScope::MarketData, 0),
            Some(UnixNanos::from(10))
        );
        assert_eq!(
            data_farm_state.recovery_since_ns_after_for(DataFarmRecoveryScope::MarketData, 1),
            Some(UnixNanos::from(10))
        );
    }

    #[rstest]
    fn test_data_farm_state_ignores_stale_subscription_degradation() {
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();
        let initial_generation = data_farm_state.recovery_generation();

        data_farm_state.mark_degraded(initial_generation, UnixNanos::from(10));
        data_farm_state.handle_notice(
            &notice(2104, "Market data farm connection is OK:usfarm.nj"),
            clock,
        );
        data_farm_state.mark_degraded(initial_generation, UnixNanos::from(20));

        let state = data_farm_state.state.lock();
        assert_eq!(state.market_data.recovery_generation, 1);
        assert!(state.degraded_farms.is_empty());
        assert!(state.degraded_scopes.is_empty());
    }

    #[tokio::test]
    async fn test_data_farm_state_preserves_recovery_during_resubscription() {
        let clock = get_atomic_clock_realtime();
        let data_farm_state = Arc::new(DataFarmConnectionState::default());
        let initial_generation = data_farm_state.recovery_generation();

        data_farm_state.handle_notice(
            &notice(2103, "Market data farm connection is broken:usfarm.nj"),
            clock,
        );
        data_farm_state.handle_notice(
            &notice(2104, "Market data farm connection is OK:usfarm.nj"),
            clock,
        );
        data_farm_state
            .wait_for_recovery_after(initial_generation)
            .await;
        let resubscription_generation = data_farm_state.recovery_generation();

        let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
        let state_for_resubscription = Arc::clone(&data_farm_state);

        let resubscription_task = tokio::spawn(async move {
            release_receiver.await.unwrap();
            state_for_resubscription
                .wait_for_recovery_after(resubscription_generation)
                .await;
            state_for_resubscription.recovery_generation()
        });

        data_farm_state.handle_notice(
            &notice(2103, "Market data farm connection is broken:cashfarm"),
            clock,
        );
        data_farm_state.handle_notice(
            &notice(2104, "Market data farm connection is OK:cashfarm"),
            clock,
        );
        release_sender.send(()).unwrap();
        let observed_generation = resubscription_task.await.unwrap();

        assert_eq!(observed_generation, 2);
    }

    #[tokio::test]
    async fn test_process_index_price_tick_result_emits_index_update_from_last_price() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();

        let action = process_index_price_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Last,
                price: 452525.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            100,
            &sender.clone().into(),
            clock,
        )
        .await
        .unwrap();

        assert!(matches!(action, StreamAction::Continue));

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::IndexPrice(index)) => {
                assert_eq!(index.value.as_f64(), 4525.25);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_waits_for_sizes_after_bid_and_ask_prices() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        let bid_action = process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Bid,
                price: 100.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();
        let ask_action = process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Ask,
                price: 101.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();

        assert!(matches!(bid_action, StreamAction::Continue));
        assert!(matches!(ask_action, StreamAction::Continue));

        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_waits_for_delayed_sizes() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        let bid_action = process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::DelayedBid,
                price: 312.44,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();
        let ask_action = process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::DelayedAsk,
                price: 312.45,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();

        assert!(matches!(bid_action, StreamAction::Continue));
        assert!(matches!(ask_action, StreamAction::Continue));

        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_emits_quote_from_delayed_size_ticks() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        for tick in [
            TickTypes::Price(TickPrice {
                tick_type: TickType::DelayedBid,
                price: 10.0,
                attributes: TickAttribute::default(),
            }),
            TickTypes::Price(TickPrice {
                tick_type: TickType::DelayedAsk,
                price: 11.0,
                attributes: TickAttribute::default(),
            }),
            TickTypes::Size(TickSize {
                tick_type: TickType::DelayedBidSize,
                size: 3.0,
            }),
            TickTypes::Size(TickSize {
                tick_type: TickType::DelayedAskSize,
                size: 4.0,
            }),
        ] {
            process_quote_tick_result(
                Ok::<_, &'static str>(tick),
                instrument_id,
                2,
                0,
                &sender.clone().into(),
                &quote_cache,
                clock,
                false,
            )
            .await
            .unwrap();
        }

        let mut last_quote = None;

        while let Ok(event) = receiver.try_recv() {
            if let DataEvent::Data(Data::Quote(quote)) = event {
                last_quote = Some(quote);
            }
        }
        let quote = last_quote.expect("expected at least one quote from delayed ticks");
        assert_eq!(quote.bid_price.as_decimal(), dec!(10));
        assert_eq!(quote.ask_price.as_decimal(), dec!(11));
        assert_eq!(quote.bid_size.as_decimal(), dec!(3));
        assert_eq!(quote.ask_size.as_decimal(), dec!(4));
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_emits_quote_from_delayed_price_size_ticks() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        for tick in [
            TickPriceSize {
                price_tick_type: TickType::DelayedBid,
                price: 99.5,
                attributes: TickAttribute::default(),
                size_tick_type: TickType::DelayedBidSize,
                size: 7.0,
            },
            TickPriceSize {
                price_tick_type: TickType::DelayedAsk,
                price: 100.5,
                attributes: TickAttribute::default(),
                size_tick_type: TickType::DelayedAskSize,
                size: 9.0,
            },
        ] {
            let action = process_quote_tick_result(
                Ok::<_, &'static str>(TickTypes::PriceSize(tick)),
                instrument_id,
                2,
                0,
                &sender.clone().into(),
                &quote_cache,
                clock,
                false,
            )
            .await
            .unwrap();

            assert!(matches!(action, StreamAction::Continue));
        }

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::Quote(quote)) => {
                assert_eq!(quote.bid_price.as_decimal(), dec!(99.5));
                assert_eq!(quote.bid_size.as_decimal(), dec!(7));
                assert_eq!(quote.ask_price.as_decimal(), dec!(100.5));
                assert_eq!(quote.ask_size.as_decimal(), dec!(9));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_emits_quote_from_price_size_tick() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::PriceSize(TickPriceSize {
                price_tick_type: TickType::Bid,
                price: 99.5,
                attributes: TickAttribute::default(),
                size_tick_type: TickType::BidSize,
                size: 7.0,
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();
        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::PriceSize(TickPriceSize {
                price_tick_type: TickType::Ask,
                price: 100.5,
                attributes: TickAttribute::default(),
                size_tick_type: TickType::AskSize,
                size: 9.0,
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::Quote(quote)) => {
                assert_eq!(quote.bid_price.as_f64(), 99.5);
                assert_eq!(quote.bid_size.as_f64(), 7.0);
                assert_eq!(quote.ask_price.as_f64(), 100.5);
                assert_eq!(quote.ask_size.as_f64(), 9.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_quote_sentinel_clears_side_and_suppresses_emission() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        for (tick_type, size_type, price, size) in [
            (TickType::Bid, TickType::BidSize, 99.5, 7.0),
            (TickType::Ask, TickType::AskSize, 100.5, 9.0),
        ] {
            process_quote_tick_result(
                Ok::<_, &'static str>(TickTypes::PriceSize(TickPriceSize {
                    price_tick_type: tick_type,
                    price,
                    attributes: TickAttribute::default(),
                    size_tick_type: size_type,
                    size,
                })),
                instrument_id,
                2,
                0,
                &sender.clone().into(),
                &quote_cache,
                clock,
                false,
            )
            .await
            .unwrap();
        }
        assert!(matches!(
            receiver.try_recv().unwrap(),
            DataEvent::Data(Data::Quote(_))
        ));

        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Bid,
                price: -1.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();
        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Ask,
                price: 101.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();

        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_size_only_update_respects_filter() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Bid,
                price: 100.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            true,
        )
        .await
        .unwrap();
        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Ask,
                price: 101.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            true,
        )
        .await
        .unwrap();

        assert!(receiver.try_recv().is_err());

        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Size(TickSize {
                tick_type: TickType::BidSize,
                size: 12.0,
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            true,
        )
        .await
        .unwrap();
        process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Size(TickSize {
                tick_type: TickType::AskSize,
                size: 13.0,
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            true,
        )
        .await
        .unwrap();

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::Quote(quote)) => {
                assert_eq!(quote.bid_size.as_f64(), 12.0);
                assert_eq!(quote.ask_size.as_f64(), 13.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let action = process_quote_tick_result(
            Ok::<_, &'static str>(TickTypes::Size(TickSize {
                tick_type: TickType::BidSize,
                size: 14.0,
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            true,
        )
        .await
        .unwrap();

        assert!(matches!(action, StreamAction::Continue));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_notice_162_stops() {
        let instrument_id = instrument_id();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        let action = process_quote_tick_result(
            Ok::<_, &'static str>(SubscriptionItem::Notice(Notice {
                request_id: None,
                code: 162,
                message: String::from("Market data subscription cancelled"),
                error_time: None,
                advanced_order_reject_json: String::new(),
            })),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            false,
        )
        .await
        .unwrap();

        assert!(matches!(action, StreamAction::Stop));
    }

    #[tokio::test]
    async fn test_process_index_price_tick_result_ignores_non_last_ticks() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();

        let action = process_index_price_tick_result(
            Ok::<_, &'static str>(TickTypes::Price(TickPrice {
                tick_type: TickType::Bid,
                price: 4500.0,
                attributes: TickAttribute::default(),
            })),
            instrument_id,
            2,
            1,
            &sender.clone().into(),
            clock,
        )
        .await
        .unwrap();

        assert!(matches!(action, StreamAction::Continue));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_process_quote_tick_result_bubbles_subscription_error() {
        let instrument_id = instrument_id();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let quote_cache = Arc::new(tokio::sync::Mutex::new(QuoteCache::new()));

        let result = process_quote_tick_result(
            Err::<TickTypes, _>("boom"),
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            &quote_cache,
            clock,
            false,
        )
        .await;

        let error = result.err().unwrap();
        assert_eq!(error.to_string(), "Subscription error: \"boom\"");
    }

    #[tokio::test]
    async fn test_process_option_greeks_tick_result_merges_partial_ticks() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let greeks_cache = Arc::new(tokio::sync::Mutex::new(OptionGreeksCache::new()));

        let bid_action = process_option_greeks_tick_result(
            Ok::<_, &'static str>(TickTypes::OptionComputation(OptionComputation {
                field: TickType::BidOption,
                implied_volatility: Some(0.24),
                underlying_price: Some(155.0),
                ..Default::default()
            })),
            instrument_id,
            &sender.clone().into(),
            &greeks_cache,
            clock,
        )
        .await
        .unwrap();
        assert!(matches!(bid_action, StreamAction::Continue));
        assert!(receiver.try_recv().is_err());

        let ask_action = process_option_greeks_tick_result(
            Ok::<_, &'static str>(TickTypes::OptionComputation(OptionComputation {
                field: TickType::AskOption,
                implied_volatility: Some(0.26),
                underlying_price: Some(155.0),
                ..Default::default()
            })),
            instrument_id,
            &sender.clone().into(),
            &greeks_cache,
            clock,
        )
        .await
        .unwrap();
        assert!(matches!(ask_action, StreamAction::Continue));
        assert!(receiver.try_recv().is_err());

        let oi_action = process_option_greeks_tick_result(
            Ok::<_, &'static str>(TickTypes::Generic(TickGeneric {
                tick_type: TickType::OptionCallOpenInterest,
                value: 1000.0,
            })),
            instrument_id,
            &sender.clone().into(),
            &greeks_cache,
            clock,
        )
        .await
        .unwrap();
        assert!(matches!(oi_action, StreamAction::Continue));
        assert!(receiver.try_recv().is_err());

        let model_action = process_option_greeks_tick_result(
            Ok::<_, &'static str>(TickTypes::OptionComputation(OptionComputation {
                field: TickType::ModelOption,
                implied_volatility: Some(0.25),
                delta: Some(0.55),
                gamma: Some(0.02),
                vega: Some(0.15),
                theta: Some(-0.05),
                underlying_price: Some(155.0),
                ..Default::default()
            })),
            instrument_id,
            &sender.clone().into(),
            &greeks_cache,
            clock,
        )
        .await
        .unwrap();

        assert!(matches!(model_action, StreamAction::Continue));

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::OptionGreeks(greeks)) => {
                assert_eq!(greeks.delta, 0.55);
                assert_eq!(greeks.gamma, 0.02);
                assert_eq!(greeks.vega, 0.15);
                assert_eq!(greeks.theta, -0.05);
                assert_eq!(greeks.rho, 0.0);
                assert_eq!(greeks.mark_iv, Some(0.25));
                assert_eq!(greeks.bid_iv, Some(0.24));
                assert_eq!(greeks.ask_iv, Some(0.26));
                assert_eq!(greeks.underlying_price, Some(155.0));
                assert_eq!(greeks.open_interest, Some(1000.0));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_process_option_greeks_tick_result_notice_162_stops() {
        let instrument_id = instrument_id();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let clock = get_atomic_clock_realtime();
        let greeks_cache = Arc::new(tokio::sync::Mutex::new(OptionGreeksCache::new()));

        let action = process_option_greeks_tick_result(
            Ok::<_, &'static str>(SubscriptionItem::Notice(Notice {
                request_id: None,
                code: 162,
                message: String::from("Market data subscription cancelled"),
                error_time: None,
                advanced_order_reject_json: String::new(),
            })),
            instrument_id,
            &sender.clone().into(),
            &greeks_cache,
            clock,
        )
        .await
        .unwrap();

        assert!(matches!(action, StreamAction::Stop));
    }

    #[tokio::test]
    async fn test_update_revised_bar_tracking_replaces_bar_and_clears_timeout_task() {
        let bar_type = String::from("AAPL.SMART-5-SECOND-LAST-EXTERNAL");
        let last_bars = Arc::new(tokio::sync::Mutex::new(AHashMap::new()));
        let bar_timeout_tasks = Arc::new(tokio::sync::Mutex::new(AHashMap::new()));

        let stale_task = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });

        bar_timeout_tasks
            .lock()
            .await
            .insert(bar_type.clone(), TaskSlot::from_handle(stale_task));

        let bar = RealtimeBar {
            date: time::OffsetDateTime::UNIX_EPOCH,
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.5,
            volume: 10.0,
            wap: 100.25,
            count: 2,
        };

        update_revised_bar_tracking(&bar_type, bar, &last_bars, &bar_timeout_tasks).await;
        tokio::task::yield_now().await;

        let last_bars_guard = last_bars.lock().await;
        let stored_bar = last_bars_guard.get(&bar_type).unwrap();
        assert_eq!(stored_bar.close, 100.5);
        assert!(!bar_timeout_tasks.lock().await.contains_key(&bar_type));
    }

    #[tokio::test]
    async fn test_process_trade_stream_emits_trade_tick() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (trade_sender, trade_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut subscription = ChannelSubscription::new(trade_receiver);
        let cancellation_token = CancellationToken::new();
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();

        trade_sender
            .send(Ok(Trade {
                tick_type: String::from("Last"),
                time: time::OffsetDateTime::UNIX_EPOCH,
                price: 4500.25,
                size: 3.0,
                trade_attribute: TradeAttribute {
                    past_limit: false,
                    unreported: false,
                },
                exchange: String::from("CBOE"),
                special_conditions: String::new(),
            }))
            .unwrap();
        drop(trade_sender);

        process_trade_stream(
            &mut subscription,
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            clock,
            &cancellation_token,
            &data_farm_state,
            data_farm_state.recovery_generation(),
            disabled_monitor(),
        )
        .await
        .unwrap();

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::Trade(trade)) => {
                assert_eq!(trade.price.as_f64(), 4500.25);
                assert_eq!(trade.size.as_f64(), 3.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_process_trade_stream_10182_waits_for_farm_ok_and_resubscribes() {
        let instrument_id = instrument_id();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (trade_sender, trade_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut subscription = ChannelSubscription::new(trade_receiver);
        let cancellation_token = CancellationToken::new();
        let clock = get_atomic_clock_realtime();
        let data_farm_state = Arc::new(DataFarmConnectionState::default());
        let farm_generation = data_farm_state.recovery_generation();

        trade_sender
            .send(Err(Error::Notice(notice(
                10182,
                "Failed to request live updates (disconnected).",
            ))))
            .unwrap();

        let state_for_recovery = Arc::clone(&data_farm_state);

        let recovery_task = tokio::spawn(async move {
            loop {
                if state_for_recovery
                    .state
                    .lock()
                    .degraded_scopes
                    .contains_key(&DataFarmRecoveryScope::MarketData)
                {
                    state_for_recovery.handle_notice(
                        &notice(2104, "Market data farm connection is OK:usfarm"),
                        clock,
                    );
                    break;
                }
                tokio::task::yield_now().await;
            }
        });

        let action = process_trade_stream(
            &mut subscription,
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            clock,
            &cancellation_token,
            &data_farm_state,
            farm_generation,
            disabled_monitor(),
        )
        .await
        .unwrap();
        recovery_task.await.unwrap();

        assert!(matches!(action, StreamAction::Resubscribe));
        assert_eq!(data_farm_state.recovery_generation(), 1);
    }

    #[tokio::test]
    async fn test_process_realtime_bar_stream_emits_bar_and_tracks_revision() {
        let bar_type = BarType::from("SPX.CBOE-5-SECOND-LAST-EXTERNAL");
        let bar_type_str = bar_type.to_string();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (bar_sender, bar_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut subscription = ChannelSubscription::new(bar_receiver);
        let cancellation_token = CancellationToken::new();
        let last_bars = Arc::new(tokio::sync::Mutex::new(AHashMap::new()));
        let timeout_tasks = Arc::new(tokio::sync::Mutex::new(AHashMap::new()));
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();

        bar_sender
            .send(Ok(RealtimeBar {
                date: time::OffsetDateTime::UNIX_EPOCH,
                open: 100.0,
                high: 101.0,
                low: 99.5,
                close: 100.5,
                volume: 10.0,
                wap: 100.25,
                count: 2,
            }))
            .unwrap();
        drop(bar_sender);

        process_realtime_bar_stream(
            &mut subscription,
            bar_type,
            &bar_type_str,
            2,
            0,
            &sender.clone().into(),
            &last_bars,
            &timeout_tasks,
            true,
            &cancellation_token,
            &data_farm_state,
            data_farm_state.recovery_generation(),
            clock,
            disabled_monitor(),
        )
        .await
        .unwrap();

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::Bar(bar)) => {
                assert_eq!(bar.close.as_f64(), 100.5);
                assert_eq!(bar.volume.as_f64(), 10.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(last_bars.lock().await.contains_key(&bar_type_str));
    }

    #[tokio::test]
    async fn test_process_market_depth_stream_emits_deltas_with_sequence() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (depth_sender, depth_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut subscription = ChannelSubscription::new(depth_receiver);
        let cancellation_token = CancellationToken::new();
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();

        depth_sender
            .send(Ok(MarketDepths::MarketDepth(MarketDepth {
                position: 1,
                operation: 0,
                side: 1,
                price: 100.0,
                size: 5.0,
            })))
            .unwrap();
        depth_sender
            .send(Ok(MarketDepths::MarketDepthL2(MarketDepthL2 {
                position: 2,
                market_maker: String::from("MM1"),
                operation: 1,
                side: 0,
                price: 101.0,
                size: 7.0,
                smart_depth: true,
            })))
            .unwrap();
        depth_sender
            .send(Ok(MarketDepths::MarketDepthL2(MarketDepthL2 {
                position: 2,
                market_maker: String::from("M1M"),
                operation: 0,
                side: 0,
                price: 101.5,
                size: 8.0,
                smart_depth: true,
            })))
            .unwrap();
        drop(depth_sender);

        process_market_depth_stream(
            &mut subscription,
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            clock,
            &cancellation_token,
            &data_farm_state,
            data_farm_state.recovery_generation(),
            disabled_monitor(),
        )
        .await
        .unwrap();

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::BookDelta(delta)) => {
                assert_eq!(delta.action, BookAction::Clear);
                assert_eq!(delta.sequence, 0);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::BookDelta(delta)) => {
                assert_eq!(delta.sequence, 1);
                assert_eq!(delta.order.price.as_f64(), 100.0);
                assert_eq!(delta.order.size.as_f64(), 5.0);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        let first_l2_order_id = match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::BookDelta(delta)) => {
                assert_eq!(delta.sequence, 2);
                assert_eq!(delta.order.price.as_f64(), 101.0);
                assert_eq!(delta.order.size.as_f64(), 7.0);
                delta.order.order_id
            }
            other => panic!("unexpected event: {other:?}"),
        };

        match receiver.recv().await.unwrap() {
            DataEvent::Data(Data::BookDelta(delta)) => {
                assert_eq!(delta.sequence, 3);
                assert_eq!(delta.order.price.as_f64(), 101.5);
                assert_eq!(delta.order.size.as_f64(), 8.0);
                assert_ne!(delta.order.order_id, first_l2_order_id);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_process_market_depth_stream_clears_before_resubscribed_deltas() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let cancellation_token = CancellationToken::new();
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();

        for _ in 0..2 {
            let (depth_sender, depth_receiver) = tokio::sync::mpsc::unbounded_channel();
            let mut subscription = ChannelSubscription::new(depth_receiver);
            depth_sender
                .send(Ok(MarketDepths::MarketDepth(MarketDepth {
                    position: 1,
                    operation: 0,
                    side: 1,
                    price: 100.0,
                    size: 5.0,
                })))
                .unwrap();
            drop(depth_sender);

            let action = process_market_depth_stream(
                &mut subscription,
                instrument_id,
                2,
                0,
                &sender.clone().into(),
                clock,
                &cancellation_token,
                &data_farm_state,
                data_farm_state.recovery_generation(),
                disabled_monitor(),
            )
            .await
            .unwrap();
            assert!(matches!(action, StreamAction::Resubscribe));
        }

        for _ in 0..2 {
            match receiver.recv().await.unwrap() {
                DataEvent::Data(Data::BookDelta(delta)) => {
                    assert_eq!(delta.action, BookAction::Clear);
                    assert_eq!(delta.sequence, 0);
                }
                other => panic!("unexpected event: {other:?}"),
            }

            match receiver.recv().await.unwrap() {
                DataEvent::Data(Data::BookDelta(delta)) => {
                    assert_eq!(delta.action, BookAction::Add);
                    assert_eq!(delta.sequence, 1);
                    assert_eq!(delta.order.price.as_f64(), 100.0);
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn test_process_trade_stream_emits_negative_price_and_skips_sentinel() {
        let instrument_id = instrument_id();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (trade_sender, trade_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut subscription = ChannelSubscription::new(trade_receiver);
        let cancellation_token = CancellationToken::new();
        let clock = get_atomic_clock_realtime();
        let data_farm_state = DataFarmConnectionState::default();

        for (price, size) in [(-1.0, 0.0), (-1.0, 3.0), (-1.25, 2.0)] {
            trade_sender
                .send(Ok(Trade {
                    tick_type: String::from("Last"),
                    time: time::OffsetDateTime::UNIX_EPOCH,
                    price,
                    size,
                    trade_attribute: TradeAttribute {
                        past_limit: false,
                        unreported: false,
                    },
                    exchange: String::from("CBOE"),
                    special_conditions: String::new(),
                }))
                .unwrap();
        }
        drop(trade_sender);

        process_trade_stream(
            &mut subscription,
            instrument_id,
            2,
            0,
            &sender.clone().into(),
            clock,
            &cancellation_token,
            &data_farm_state,
            data_farm_state.recovery_generation(),
            disabled_monitor(),
        )
        .await
        .unwrap();

        let mut trades = Vec::new();

        while let Ok(event) = receiver.try_recv() {
            match event {
                DataEvent::Data(Data::Trade(trade)) => {
                    trades.push((trade.price.as_f64(), trade.size.as_f64()));
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert_eq!(trades, vec![(-1.0, 3.0), (-1.25, 2.0)]);
    }

    #[rstest]
    fn test_send_quote_tick_returns_continue_for_none() {
        let instrument_id = instrument_id();
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        assert!(matches!(
            send_quote_tick(None, &sender.clone().into(), instrument_id),
            StreamAction::Continue
        ));
    }

    #[rstest]
    fn test_update_quote_from_price_tick_ignores_last() {
        let instrument_id = instrument_id();
        let mut cache = QuoteCache::new();
        let quote = update_quote_from_price_tick(
            &mut cache,
            instrument_id,
            &TickPrice {
                tick_type: TickType::Last,
                price: 100.0,
                attributes: TickAttribute::default(),
            },
            2,
            0,
            nautilus_core::UnixNanos::new(1),
            nautilus_core::UnixNanos::new(1),
        );
        assert!(quote.is_none());
    }
    fn idle_monitor(
        timeout_secs: Option<u64>,
    ) -> (
        SubscriptionMonitor,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (data_sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let timeout = timeout_secs.map(Duration::from_secs);
        let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(101_u64))));
        let monitor = SubscriptionMonitor {
            client_id: ClientId::from("IB-DATA-17"),
            instrument_id: instrument_id(),
            subscription: "trades".to_string(),
            timeout,
            deadline: timeout.and_then(|timeout| tokio::time::Instant::now().checked_add(timeout)),
            last_data_received_ns: None,
            data_sender: data_sender.into(),
            clock,
            cancellation_token: CancellationToken::new(),
            data_farm_state: Arc::new(DataFarmConnectionState::default()),
            recovery_scope: DataFarmRecoveryScope::MarketData,
            farm_generation: 0,
        };
        (monitor, receiver)
    }

    fn disabled_monitor() -> SubscriptionMonitor {
        idle_monitor(None).0
    }

    fn receive_idle(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) -> InteractiveBrokersSubscriptionIdle {
        let DataEvent::Data(Data::Custom(custom)) = receiver.try_recv().unwrap() else {
            panic!("Expected a subscription idle custom event");
        };
        custom
            .data
            .as_any()
            .downcast_ref::<InteractiveBrokersSubscriptionIdle>()
            .unwrap()
            .clone()
    }

    #[tokio::test(start_paused = true)]
    async fn idle_subscription_emits_once_and_rearms_after_data() {
        let (mut monitor, mut receiver) = idle_monitor(Some(5));
        let started = tokio::time::Instant::now();
        tokio::time::advance(Duration::from_secs(5)).await;
        monitor.clock.set_time(UnixNanos::from(501_u64));
        monitor.wait_for_idle().await;
        assert!(monitor.notify_idle());
        assert_eq!(
            receive_idle(&mut receiver),
            InteractiveBrokersSubscriptionIdle {
                client_id: ClientId::from("IB-DATA-17"),
                instrument_id: instrument_id(),
                subscription: "trades".to_string(),
                idle_timeout_secs: 5,
                last_data_received_ns: None,
                ts_event: UnixNanos::from(501_u64),
                ts_init: UnixNanos::from(501_u64),
            }
        );
        assert!(monitor.notify_idle());
        assert!(receiver.is_empty());
        monitor.clock.set_time(UnixNanos::from(601_u64));
        monitor.received_data();
        assert_eq!(monitor.deadline, Some(started + Duration::from_secs(10)));
        tokio::time::advance(Duration::from_secs(5)).await;
        monitor.clock.set_time(UnixNanos::from(1001_u64));
        monitor.wait_for_idle().await;
        assert!(monitor.notify_idle());
        assert_eq!(
            receive_idle(&mut receiver),
            InteractiveBrokersSubscriptionIdle {
                client_id: ClientId::from("IB-DATA-17"),
                instrument_id: instrument_id(),
                subscription: "trades".to_string(),
                idle_timeout_secs: 5,
                last_data_received_ns: Some(601),
                ts_event: UnixNanos::from(1001_u64),
                ts_init: UnixNanos::from(1001_u64),
            }
        );
        assert!(receiver.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn only_data_postpones_the_idle_deadline() {
        let (mut first, mut first_events) = idle_monitor(Some(5));
        let (mut second, mut second_events) = idle_monitor(Some(5));
        second.subscription = "quotes".to_string();
        let started = tokio::time::Instant::now();
        tokio::time::advance(Duration::from_secs(2)).await;
        first.received_data();
        second
            .data_farm_state
            .handle_notice(&notice(2108, "Market data farm is inactive"), second.clock);
        assert_eq!(first.deadline, Some(started + Duration::from_secs(7)));
        assert_eq!(second.deadline, Some(started + Duration::from_secs(5)));
        tokio::time::advance(Duration::from_secs(3)).await;
        second.wait_for_idle().await;
        assert!(second.notify_idle());
        assert_eq!(receive_idle(&mut second_events).subscription, "quotes");
        assert!(first_events.try_recv().is_err());
        assert!(second_events.is_empty());
    }

    #[rstest]
    #[case("cancel")]
    #[case("recovered")]
    #[tokio::test(start_paused = true)]
    async fn idle_notification_is_suppressed_during_recovery(#[case] state: &str) {
        let (mut monitor, receiver) = idle_monitor(Some(5));
        tokio::time::advance(Duration::from_secs(5)).await;

        match state {
            "cancel" => monitor.cancellation_token.cancel(),
            "recovered" => {
                monitor.data_farm_state.handle_notice(
                    &notice(2103, "Market data farm connection is broken:usfarm"),
                    monitor.clock,
                );
                monitor.data_farm_state.handle_notice(
                    &notice(2104, "Market data farm connection is OK:usfarm"),
                    monitor.clock,
                );
            }
            _ => unreachable!(),
        }
        monitor.wait_for_idle().await;
        assert!(monitor.notify_idle());
        assert!(receiver.is_empty());
        assert_eq!(monitor.deadline, None);
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_idle_detection_stays_disabled() {
        let (mut monitor, receiver) = idle_monitor(None);
        monitor.received_data();
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(monitor.notify_idle());
        assert_eq!(monitor.deadline, None);
        assert_eq!(monitor.last_data_received_ns, None);
        assert!(receiver.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn trade_farm_notices_do_not_hide_idle_subscription() {
        let (monitor, mut receiver) = idle_monitor(Some(5));
        let (_trade_sender, trade_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut subscription = ChannelSubscription::new(trade_receiver);
        let cancellation = monitor.cancellation_token.clone();
        let task_cancellation = cancellation.clone();
        let sender = monitor.data_sender.clone();
        let clock = monitor.clock;
        let farm = Arc::clone(&monitor.data_farm_state);
        let notices = Arc::clone(&farm);

        let task = tokio::spawn(async move {
            process_trade_stream(
                &mut subscription,
                instrument_id(),
                2,
                0,
                &sender,
                clock,
                &task_cancellation,
                &farm,
                0,
                monitor,
            )
            .await
            .unwrap()
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(2)).await;

        for _ in 0..1000 {
            notices.handle_notice(&notice(2108, "Market data farm is inactive"), clock);
        }
        tokio::time::advance(Duration::from_secs(3)).await;
        tokio::task::yield_now().await;
        let event = receive_idle(&mut receiver);
        cancellation.cancel();
        let action = task.await.unwrap();
        assert_eq!(
            event,
            InteractiveBrokersSubscriptionIdle {
                client_id: ClientId::from("IB-DATA-17"),
                instrument_id: instrument_id(),
                subscription: "trades".to_string(),
                idle_timeout_secs: 5,
                last_data_received_ns: None,
                ts_event: UnixNanos::from(101_u64),
                ts_init: UnixNanos::from(101_u64),
            }
        );
        assert!(matches!(action, StreamAction::Stop));
        assert!(receiver.is_empty());
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn stream_config_preserves_trade_selection_and_idle_interval(#[case] all_last_trades: bool) {
        let config = crate::config::InteractiveBrokersDataClientConfig {
            all_last_trades,
            subscription_idle_timeout_secs: Some(17),
            ..Default::default()
        };
        let stream = StreamConfig::new(ClientId::from("IB-DATA-17"), &config);
        assert_eq!(stream.client_id, ClientId::from("IB-DATA-17"));
        assert_eq!(stream.all_last_trades, all_last_trades);
        assert_eq!(stream.idle_timeout_secs, Some(17));
    }

    #[tokio::test(start_paused = true)]
    async fn farm_notice_does_not_mask_subscription_inactivity() {
        let (mut monitor, mut receiver) = idle_monitor(Some(5));
        monitor
            .data_farm_state
            .mark_degraded(0, UnixNanos::from(17_u64));
        tokio::time::advance(Duration::from_secs(5)).await;
        monitor.wait_for_idle().await;
        assert!(monitor.notify_idle());
        let event = receive_idle(&mut receiver);
        assert_eq!(event.idle_timeout_secs, 5);
        assert_eq!(event.last_data_received_ns, None);
        assert_eq!(monitor.deadline, None);
    }
}
