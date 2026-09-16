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

//! Async event loop runner for live and sandbox trading nodes.
//!
//! `AsyncRunner` owns seven tokio mpsc channel pairs plus a shutdown
//! signal channel. Construction creates the channels without side
//! effects. The sender halves are placed into thread-local storage
//! via [`AsyncRunner::bind_senders`] so that adapters and engine
//! components can resolve them through the `get_*_sender()` accessors
//! in `nautilus_common::runner` and `nautilus_common::live::runner`.
//!
//! Channel pairs:
//!
//! - **Time events**: timer callbacks dispatched by the clock.
//! - **System events**: system notifications handled by the live node.
//! - **System commands**: control requests handled by the live node.
//! - **Execution events**: fills, order updates, and account state from
//!   execution clients to the execution engine.
//! - **Trading commands**: deferred order actions routed to their direct endpoint.
//! - **Data events**: market data from adapters to the data engine.
//! - **Data commands**: subscribe/unsubscribe requests to data clients.
//!
//! Both `AsyncRunner::run` and `LiveNode::run` use a `biased;` select with
//! system and execution branches polled ahead of data branches. Within each
//! channel pair, events are polled before commands.
//!
//! The runner can drive the event loop in two ways:
//!
//! - **Standalone**: call [`AsyncRunner::run`], which binds senders and
//!   enters a `tokio::select!` loop internally.
//! - **Integrated**: call [`AsyncRunner::take_channels`] to extract the
//!   receivers and run the `select!` loop directly inside `LiveNode::run`,
//!   where it is interleaved with startup, reconciliation, and shutdown
//!   phases.
//!
//! # Invariants
//!
//! - `bind_senders` must be called before any code that reads from TLS.
//!   This includes adapter constructors, clock initialization, and
//!   execution client start methods. Every path from construction to
//!   the event loop must bind before the first TLS read.
//! - The event loop and all TLS consumers must execute on the same
//!   thread. Senders are cloneable and `Send`, but the `RefCell`-backed
//!   TLS slots are not accessible from other threads.
//! - Only one runner at a time should own the TLS slots on a given
//!   thread. `bind_senders` overwrites any existing TLS contents on the
//!   thread, so the last caller wins.

use std::{
    fmt::Debug,
    sync::Arc,
    thread::{self, ThreadId},
};

use nautilus_common::{
    live::{
        dispatch::DispatchMessage,
        runner::{
            replace_data_event_sender, replace_exec_event_sender, replace_system_command_sender,
            replace_system_event_sender,
        },
        sender::{DispatchSender, EventSender},
    },
    messages::{
        DataEvent, ExecutionEvent, ExecutionReport, SystemCommand, SystemEvent, data::DataCommand,
        execution::TradingCommand,
    },
    msgbus::{self, MessagingSwitchboard},
    runner::{
        DataCommandSender, TimeEventMessage, TimeEventSender, TradingCommandMessage,
        TradingCommandSender, replace_data_cmd_sender, replace_exec_cmd_sender,
        replace_time_event_sender,
    },
};
use nautilus_model::events::OrderEventAny;

use crate::dispatch::drain_callbacks;
#[cfg(feature = "node")]
use crate::node::{LiveNodeHandle, NodeState};

/// Asynchronous implementation of `DataCommandSender` for live environments.
#[derive(Debug)]
pub struct AsyncDataCommandSender {
    cmd_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<DataCommand>>,
    owner: ThreadId,
    #[cfg(feature = "node")]
    node_handle: Option<LiveNodeHandle>,
}

impl AsyncDataCommandSender {
    /// Creates a sender owned by the calling thread.
    ///
    /// Construct it on the runtime thread so command sends capture callback roots.
    #[must_use]
    pub fn new(cmd_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<DataCommand>>) -> Self {
        Self {
            cmd_tx,
            owner: thread::current().id(),
            #[cfg(feature = "node")]
            node_handle: None,
        }
    }
}

impl DataCommandSender for AsyncDataCommandSender {
    fn execute(&self, command: DataCommand) {
        if let Err(e) = self.cmd_tx.send(DispatchMessage::new(command, self.owner)) {
            // Disposal releases retained subscriptions after the node drops its receivers
            #[cfg(feature = "node")]
            if self
                .node_handle
                .as_ref()
                .is_some_and(|handle| handle.state() == NodeState::Stopped)
            {
                return;
            }

            log::error!("Failed to send data command: {e}");
        }
    }
}

/// Asynchronous implementation of `TimeEventSender` for live environments.
#[derive(Debug, Clone)]
pub struct AsyncTimeEventSender {
    time_tx: DispatchSender<TimeEventMessage>,
}

impl AsyncTimeEventSender {
    /// Creates a sender owned by the calling runtime thread.
    #[must_use]
    pub fn new(
        time_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<TimeEventMessage>>,
    ) -> Self {
        Self {
            time_tx: DispatchSender::new(time_tx),
        }
    }
}

impl TimeEventSender for AsyncTimeEventSender {
    fn send(&self, message: TimeEventMessage) {
        if let Err(e) = self.time_tx.send(message) {
            log::error!("Failed to send time event message: {e}");
        }
    }
}

/// Asynchronous implementation of `TradingCommandSender` for live environments.
#[derive(Debug)]
pub struct AsyncTradingCommandSender {
    cmd_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<TradingCommandMessage>>,
    owner: ThreadId,
}

impl AsyncTradingCommandSender {
    /// Creates a sender owned by the calling thread.
    ///
    /// Construct it on the runtime thread so command sends capture callback roots.
    #[must_use]
    pub fn new(
        cmd_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<TradingCommandMessage>>,
    ) -> Self {
        Self {
            cmd_tx,
            owner: thread::current().id(),
        }
    }
}

impl TradingCommandSender for AsyncTradingCommandSender {
    fn execute(&self, message: TradingCommandMessage) {
        if let Err(e) = self.cmd_tx.send(DispatchMessage::new(message, self.owner)) {
            log::error!("Failed to send trading command: {e}");
        }
    }
}

pub trait Runner {
    fn run(&mut self);
}

/// Channel receivers for the async event loop.
///
/// These can be extracted from `AsyncRunner` via `take_channels()` to drive
/// the event loop directly on the same thread as the msgbus endpoints.
#[derive(Debug)]
pub struct AsyncRunnerChannels {
    pub time_evt_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<TimeEventMessage>>,
    pub system_evt_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<SystemEvent>>,
    pub system_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<SystemCommand>>,
    pub exec_evt_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<ExecutionEvent>>,
    pub exec_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<TradingCommandMessage>>,
    pub data_evt_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<DataEvent>>,
    pub data_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<DataCommand>>,
}

#[cfg(feature = "node")]
#[allow(
    clippy::large_enum_variant,
    reason = "runner events are consumed immediately; boxing would add routing allocations"
)]
pub(crate) enum PendingRunnerEvent {
    TimeEvent(DispatchMessage<TimeEventMessage>),
    SystemEvent(DispatchMessage<SystemEvent>),
    SystemCommand(DispatchMessage<SystemCommand>),
    ExecEvent(DispatchMessage<ExecutionEvent>),
    ExecCommand(DispatchMessage<TradingCommandMessage>),
    DataEvent(DispatchMessage<DataEvent>),
    DataCommand(DispatchMessage<DataCommand>),
}

pub struct AsyncRunner {
    channels: AsyncRunnerChannels,
    time_evt_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<TimeEventMessage>>,
    system_evt_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<SystemEvent>>,
    system_cmd_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<SystemCommand>>,
    signal_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    signal_tx: tokio::sync::mpsc::UnboundedSender<()>,
    exec_evt_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<ExecutionEvent>>,
    exec_cmd_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<TradingCommandMessage>>,
    data_evt_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<DataEvent>>,
    data_cmd_tx: tokio::sync::mpsc::UnboundedSender<DispatchMessage<DataCommand>>,
}

/// Handle for stopping the `AsyncRunner` from another context.
#[derive(Clone, Debug)]
pub struct AsyncRunnerHandle {
    signal_tx: tokio::sync::mpsc::UnboundedSender<()>,
}

impl AsyncRunnerHandle {
    /// Signals the runner to stop.
    pub fn stop(&self) {
        if let Err(e) = self.signal_tx.send(()) {
            log::error!("Failed to send shutdown signal: {e}");
        }
    }
}

impl Default for AsyncRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for AsyncRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(AsyncRunner)).finish()
    }
}

impl AsyncRunner {
    /// Creates a new [`AsyncRunner`] instance.
    ///
    /// Creates channels but does not bind senders to thread-local storage.
    /// Call [`bind_senders`](Self::bind_senders) before creating clients that
    /// read from TLS, and again before entering the event loop.
    #[must_use]
    #[rustfmt::skip]
    pub fn new() -> Self {
        use tokio::sync::mpsc::unbounded_channel; // tokio-import-ok

        let (time_evt_tx, time_evt_rx) = unbounded_channel::<DispatchMessage<TimeEventMessage>>();
        let (system_evt_tx, system_evt_rx) = unbounded_channel::<DispatchMessage<SystemEvent>>();
        let (system_cmd_tx, system_cmd_rx) = unbounded_channel::<DispatchMessage<SystemCommand>>();
        let (signal_tx, signal_rx) = unbounded_channel::<()>();
        let (exec_evt_tx, exec_evt_rx) = unbounded_channel::<DispatchMessage<ExecutionEvent>>();
        let (exec_cmd_tx, exec_cmd_rx) = unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
        let (data_evt_tx, data_evt_rx) = unbounded_channel::<DispatchMessage<DataEvent>>();
        let (data_cmd_tx, data_cmd_rx) = unbounded_channel::<DispatchMessage<DataCommand>>();

        Self {
            channels: AsyncRunnerChannels {
                time_evt_rx,
                system_evt_rx,
                system_cmd_rx,
                exec_evt_rx,
                exec_cmd_rx,
                data_evt_rx,
                data_cmd_rx,
            },
            time_evt_tx,
            system_evt_tx,
            system_cmd_tx,
            signal_rx,
            signal_tx,
            exec_evt_tx,
            exec_cmd_tx,
            data_evt_tx,
            data_cmd_tx,
        }
    }

    /// Binds this runner's channel senders to thread-local storage.
    ///
    /// Call before creating clients that read from TLS (e.g., in the builder),
    /// and again before entering the event loop to reclaim ownership if another
    /// runner was constructed on this thread in the interim.
    pub fn bind_senders(&self) {
        self.bind_senders_with_data_sender(AsyncDataCommandSender::new(self.data_cmd_tx.clone()));
    }

    #[cfg(feature = "node")]
    pub(crate) fn bind_senders_for_node(&self, handle: LiveNodeHandle) {
        self.bind_senders_with_data_sender(AsyncDataCommandSender {
            cmd_tx: self.data_cmd_tx.clone(),
            owner: thread::current().id(),
            node_handle: Some(handle),
        });
    }

    #[rustfmt::skip]
    fn bind_senders_with_data_sender(&self, sender: AsyncDataCommandSender) {
        replace_time_event_sender(Arc::new(AsyncTimeEventSender::new(self.time_evt_tx.clone())));
        replace_system_event_sender(EventSender::new(self.system_evt_tx.clone()));
        replace_system_command_sender(DispatchSender::new(self.system_cmd_tx.clone()));
        replace_exec_event_sender(EventSender::new(self.exec_evt_tx.clone()));
        replace_exec_cmd_sender(Arc::new(AsyncTradingCommandSender::new(self.exec_cmd_tx.clone())));
        replace_data_event_sender(EventSender::new(self.data_evt_tx.clone()));
        replace_data_cmd_sender(Arc::new(sender));
    }

    /// Stops the runner with an internal shutdown signal.
    pub fn stop(&self) {
        if let Err(e) = self.signal_tx.send(()) {
            log::error!("Failed to send shutdown signal: {e}");
        }
    }

    /// Returns a handle that can be used to stop the runner from another context.
    #[must_use]
    pub fn handle(&self) -> AsyncRunnerHandle {
        AsyncRunnerHandle {
            signal_tx: self.signal_tx.clone(),
        }
    }

    /// Consumes the runner and returns the channel receivers for direct event loop driving.
    ///
    /// This is used when the event loop needs to run on the same thread as the msgbus
    /// endpoints (which use thread-local storage).
    #[must_use]
    pub fn take_channels(self) -> AsyncRunnerChannels {
        self.channels
    }

    /// Flushes all pending data events and commands from the channels.
    ///
    /// Loops until both data channels are empty, processing each item
    /// into the cache immediately. Used in `start()` where channels are
    /// not extracted.
    pub fn flush_pending_data(&mut self) {
        let mut total = 0;

        loop {
            let mut progressed = false;

            // Events drain before commands here even though the runtime select
            // prefers the opposite for everything-else: `LiveNode::start()`
            // calls this after `connect_data_clients()` to push queued
            // `DataEvent::Instrument` items into the cache. A pending
            // subscription command (e.g. `SubscribeBars`) processed before the
            // matching instrument lands would be rejected by the data engine.
            while let Ok(evt) = self.channels.data_evt_rx.try_recv() {
                Self::dispatch_data_event(evt);
                progressed = true;
                total += 1;
            }

            while let Ok(cmd) = self.channels.data_cmd_rx.try_recv() {
                Self::handle_data_command(cmd);
                progressed = true;
                total += 1;
            }

            if !progressed {
                break;
            }
        }

        if total > 0 {
            log::debug!("Flushed {total} pending data events/commands");
        }
    }

    #[cfg(feature = "node")]
    pub(crate) fn drain_pending_system_events(&mut self) -> Vec<DispatchMessage<SystemEvent>> {
        let mut events = Vec::new();

        while let Ok(event) = self.channels.system_evt_rx.try_recv() {
            events.push(event);
        }

        events
    }

    #[cfg(feature = "node")]
    pub(crate) fn drain_pending_system_commands(&mut self) -> Vec<DispatchMessage<SystemCommand>> {
        let mut commands = Vec::new();

        while let Ok(command) = self.channels.system_cmd_rx.try_recv() {
            commands.push(command);
        }

        commands
    }

    /// Runs the async runner event loop.
    ///
    /// This method processes time, system, execution, and data events in an async loop.
    /// It will run until a signal is received or the event streams are closed.
    ///
    /// # Errors
    ///
    /// Returns a callback dispatch failure, retaining pending channel messages and the failure latch.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        self.bind_senders();

        log::info!("AsyncRunner starting");

        loop {
            let callbacks_pending = drain_callbacks().await?;

            tokio::select! {
                biased;

                Some(()) = self.signal_rx.recv() => {
                    log::info!("AsyncRunner received signal, shutting down");
                    return Ok(());
                },
                () = std::future::ready(()), if callbacks_pending => {},
                Some(handler) = self.channels.time_evt_rx.recv() => {
                    let _ = Self::handle_time_event(handler);
                },
                Some(event) = self.channels.system_evt_rx.recv() => {
                    log::error!("System event {event} requires the LiveNode runner");
                },
                Some(command) = self.channels.system_cmd_rx.recv() => {
                    log::error!("System command {command} requires the LiveNode runner");
                },
                Some(evt) = self.channels.exec_evt_rx.recv() => {
                    Self::dispatch_exec_event(evt);
                },
                Some(cmd) = self.channels.exec_cmd_rx.recv() => {
                    Self::handle_trading_command(cmd);
                },
                Some(evt) = self.channels.data_evt_rx.recv() => {
                    Self::dispatch_data_event(evt);
                },
                Some(cmd) = self.channels.data_cmd_rx.recv() => {
                    Self::handle_data_command(cmd);
                },
                else => {
                    log::debug!("AsyncRunner all channels closed, exiting");
                    return Ok(());
                }
            };
        }
    }

    /// Handles a time event under its originating callback root.
    ///
    /// # Panics
    ///
    /// Panics if a rooted message is processed outside its owner thread.
    #[inline]
    #[must_use]
    pub fn handle_time_event(message: DispatchMessage<TimeEventMessage>) -> bool {
        message.dispatch(TimeEventMessage::dispatch)
    }

    /// Handles a data command under its captured callback root by sending to the `DataEngine`.
    ///
    /// # Panics
    ///
    /// Panics if a rooted command is processed outside its owner thread.
    #[inline]
    pub fn handle_data_command(cmd: DispatchMessage<DataCommand>) {
        cmd.dispatch(|cmd| {
            msgbus::send_data_command(MessagingSwitchboard::data_engine_execute(), cmd);
        });
    }

    /// Dispatches a received data event under its originating callback root.
    ///
    /// # Panics
    ///
    /// Panics if a rooted message is processed outside its owner thread.
    pub fn dispatch_data_event(event: DispatchMessage<DataEvent>) {
        event.dispatch(Self::handle_data_event);
    }

    /// Dispatches a received execution event under its originating callback root.
    ///
    /// # Panics
    ///
    /// Panics if a rooted message is processed outside its owner thread.
    pub fn dispatch_exec_event(event: DispatchMessage<ExecutionEvent>) {
        event.dispatch(Self::handle_exec_event);
    }

    /// Handles a data event by sending to the appropriate `DataEngine` endpoint.
    #[inline]
    pub fn handle_data_event(event: DataEvent) {
        match event {
            DataEvent::Data(data) => {
                msgbus::send_data(MessagingSwitchboard::data_engine_process_data(), data);
            }
            DataEvent::Instrument(data) => {
                msgbus::send_any(MessagingSwitchboard::data_engine_process(), &data);
            }
            DataEvent::Response(resp) => {
                msgbus::send_data_response(MessagingSwitchboard::data_engine_response(), resp);
            }
            DataEvent::FundingRate(funding_rate) => {
                msgbus::send_any(MessagingSwitchboard::data_engine_process(), &funding_rate);
            }
            DataEvent::InstrumentStatus(status) => {
                msgbus::send_any(MessagingSwitchboard::data_engine_process(), &status);
            }
            DataEvent::OptionGreeks(greeks) => {
                msgbus::send_any(MessagingSwitchboard::data_engine_process(), &greeks);
            }
            #[cfg(feature = "defi")]
            DataEvent::DeFi(data) => {
                msgbus::send_defi_data(MessagingSwitchboard::data_engine_process_defi_data(), data);
            }
        }
    }

    /// Dispatches an internal execution command directly to the execution engine.
    #[inline]
    pub fn handle_exec_command(cmd: TradingCommand) {
        msgbus::send_trading_command(MessagingSwitchboard::exec_engine_execute(), cmd);
    }

    /// Dispatches a trading command and its deferred children under their captured callback roots.
    ///
    /// # Panics
    ///
    /// Panics if a rooted command is processed outside its owner thread.
    #[inline]
    pub fn handle_trading_command(message: DispatchMessage<TradingCommandMessage>) {
        message.dispatch_trading(|_| {});
    }

    /// Handles an execution event by sending to the appropriate engine endpoint.
    #[inline]
    pub fn handle_exec_event(event: ExecutionEvent) {
        match event {
            ExecutionEvent::Order(order_event) => {
                msgbus::send_order_event(MessagingSwitchboard::exec_engine_process(), order_event);
            }
            ExecutionEvent::OrderSubmittedBatch(batch) => {
                for submitted in batch {
                    msgbus::send_order_event(
                        MessagingSwitchboard::exec_engine_process(),
                        OrderEventAny::Submitted(submitted),
                    );
                }
            }
            ExecutionEvent::OrderAcceptedBatch(batch) => {
                for accepted in batch {
                    msgbus::send_order_event(
                        MessagingSwitchboard::exec_engine_process(),
                        OrderEventAny::Accepted(accepted),
                    );
                }
            }
            ExecutionEvent::OrderCanceledBatch(batch) => {
                for canceled in batch {
                    msgbus::send_order_event(
                        MessagingSwitchboard::exec_engine_process(),
                        OrderEventAny::Canceled(canceled),
                    );
                }
            }
            ExecutionEvent::Report(report) => {
                Self::handle_exec_report(report);
            }
            ExecutionEvent::Account(ref account) => {
                msgbus::send_account_state(
                    MessagingSwitchboard::portfolio_update_account(),
                    account,
                );
            }
        }
    }

    #[inline]
    pub fn handle_exec_report(report: ExecutionReport) {
        let endpoint = MessagingSwitchboard::exec_engine_reconcile_execution_report();
        msgbus::send_execution_report(endpoint, report);
    }
}

#[cfg(feature = "node")]
impl AsyncRunner {
    pub(crate) fn poll_pending(&mut self, mut process: impl FnMut(PendingRunnerEvent)) -> usize {
        self.bind_senders();

        let pending = (
            self.channels.time_evt_rx.len(),
            self.channels.system_evt_rx.len(),
            self.channels.system_cmd_rx.len(),
            self.channels.exec_evt_rx.len(),
            self.channels.exec_cmd_rx.len(),
            self.channels.data_evt_rx.len(),
            self.channels.data_cmd_rx.len(),
        );

        let mut processed = 0;
        processed += poll_channel(
            &mut self.channels.time_evt_rx,
            pending.0,
            PendingRunnerEvent::TimeEvent,
            &mut process,
        );
        processed += poll_channel(
            &mut self.channels.system_evt_rx,
            pending.1,
            PendingRunnerEvent::SystemEvent,
            &mut process,
        );
        processed += poll_channel(
            &mut self.channels.system_cmd_rx,
            pending.2,
            PendingRunnerEvent::SystemCommand,
            &mut process,
        );
        processed += poll_channel(
            &mut self.channels.exec_evt_rx,
            pending.3,
            PendingRunnerEvent::ExecEvent,
            &mut process,
        );
        processed += poll_channel(
            &mut self.channels.exec_cmd_rx,
            pending.4,
            PendingRunnerEvent::ExecCommand,
            &mut process,
        );
        processed += poll_channel(
            &mut self.channels.data_evt_rx,
            pending.5,
            PendingRunnerEvent::DataEvent,
            &mut process,
        );
        processed += poll_channel(
            &mut self.channels.data_cmd_rx,
            pending.6,
            PendingRunnerEvent::DataCommand,
            &mut process,
        );
        processed
    }

    pub(crate) async fn recv(&mut self) -> Option<PendingRunnerEvent> {
        tokio::select! {
            biased;

            Some(message) = self.channels.time_evt_rx.recv() => {
                Some(PendingRunnerEvent::TimeEvent(message))
            }
            Some(event) = self.channels.system_evt_rx.recv() => {
                Some(PendingRunnerEvent::SystemEvent(event))
            }
            Some(command) = self.channels.system_cmd_rx.recv() => {
                Some(PendingRunnerEvent::SystemCommand(command))
            }
            Some(event) = self.channels.exec_evt_rx.recv() => {
                Some(PendingRunnerEvent::ExecEvent(event))
            }
            Some(command) = self.channels.exec_cmd_rx.recv() => {
                Some(PendingRunnerEvent::ExecCommand(command))
            }
            Some(event) = self.channels.data_evt_rx.recv() => {
                Some(PendingRunnerEvent::DataEvent(event))
            }
            Some(command) = self.channels.data_cmd_rx.recv() => {
                Some(PendingRunnerEvent::DataCommand(command))
            }
            else => None,
        }
    }
}

#[cfg(feature = "node")]
fn poll_channel<T>(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<T>,
    pending: usize,
    event: impl Fn(T) -> PendingRunnerEvent,
    process: &mut impl FnMut(PendingRunnerEvent),
) -> usize {
    let mut processed = 0;

    for _ in 0..pending {
        let Ok(message) = receiver.try_recv() else {
            break;
        };

        process(event(message));
        processed += 1;
    }

    processed
}

#[cfg(test)]
mod tests {
    #[cfg(not(all(feature = "simulation", madsim)))]
    use std::num::NonZeroU64;
    use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

    #[cfg(not(all(feature = "simulation", madsim)))]
    use nautilus_common::live::LiveTimer;
    use nautilus_common::{
        actor,
        cache::Cache,
        clock::VirtualClock,
        live::{
            dispatch::DispatchMessage,
            runner::{
                get_data_event_sender, get_exec_event_sender, get_system_command_sender,
                get_system_event_sender, try_get_system_command_sender,
                try_get_system_event_sender,
            },
        },
        messages::{
            ExecutionEvent, ExecutionReport,
            data::{SubscribeCommand, SubscribeCustomData},
            execution::{CancelAllOrders, QueryAccount, TradingCommand},
            system::{ReconnectSocket, SocketState, SocketStateChange},
        },
        msgbus::{TypedIntoHandler, stubs::get_typed_into_message_saving_handler},
        runner::{
            SyncTradingCommandSender, TimeEventMessage, drain_trading_cmd_queue,
            get_data_cmd_sender, get_time_event_sender, get_trading_cmd_sender,
            replace_exec_cmd_sender, try_get_time_event_sender, try_get_trading_cmd_sender,
        },
        timer::{TimeEvent, TimeEventCallback},
    };
    #[cfg(not(all(feature = "simulation", madsim)))]
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_execution::engine::ExecutionEngine;
    use nautilus_model::{
        data::{Data, DataType, quote::QuoteTick},
        enums::{
            AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide,
            TimeInForce,
        },
        events::{
            OrderAcceptedBatch, OrderCanceledBatch, OrderEvent, OrderEventAny, OrderSubmittedBatch,
            account::state::AccountState,
            order::spec::{OrderAcceptedSpec, OrderCanceledSpec, OrderSubmittedSpec},
        },
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId,
            TraderId, Venue, VenueOrderId,
        },
        reports::{FillReport, OrderStatusReport, PositionStatusReport},
        types::{Money, Price, Quantity},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn test_runner_callback_failure_stops_before_next_command(#[case] before_run: bool) {
        actor::clear_callbacks().unwrap();
        let mut runner = AsyncRunner::new();
        let received = Rc::new(RefCell::new(Vec::new()));
        let observed = received.clone();
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_execute(),
            TypedIntoHandler::from(move |command: TradingCommand| {
                observed.borrow_mut().push(command.ts_init());
                crate::dispatch::tests::latch_callback_failure();
            }),
        );

        let sender = AsyncTradingCommandSender::new(runner.exec_cmd_tx.clone());
        let seed_endpoint = ustr::Ustr::from("test.callback-root-seed");
        msgbus::register_trading_command_endpoint(
            seed_endpoint.into(),
            TypedIntoHandler::from(move |command: TradingCommand| {
                sender.execute(TradingCommandMessage::new(
                    MessagingSwitchboard::risk_engine_execute(),
                    command,
                ));
            }),
        );

        for timestamp in [17, 23] {
            SyncTradingCommandSender.execute(TradingCommandMessage::new(
                seed_endpoint.into(),
                TradingCommand::QueryAccount(QueryAccount::new(
                    "TRADER-001".into(),
                    None,
                    "SIM-001".into(),
                    UUID4::new(),
                    timestamp.into(),
                    None,
                    None,
                )),
            ));
        }

        drain_trading_cmd_queue();
        let first = runner.channels.exec_cmd_rx.try_recv().unwrap();
        let second = runner.channels.exec_cmd_rx.try_recv().unwrap();
        assert!(first.is_rooted());
        assert!(second.is_rooted());
        runner.exec_cmd_tx.send(first).unwrap();
        runner.exec_cmd_tx.send(second).unwrap();

        if before_run {
            crate::dispatch::tests::latch_callback_failure();
        }

        let error = runner.run().await.unwrap_err();

        assert_eq!(
            error.downcast_ref::<actor::CallbackDispatchError>(),
            Some(&actor::CallbackDispatchError::DeliveryUnwound)
        );
        assert_eq!(
            *received.borrow(),
            if before_run {
                vec![]
            } else {
                vec![UnixNanos::from(17)]
            }
        );
        assert_eq!(
            runner.channels.exec_cmd_rx.len(),
            if before_run { 2 } else { 1 }
        );
        assert!(!runner.exec_cmd_tx.is_closed());
        assert_eq!(
            actor::callback_failure(),
            Some(actor::CallbackDispatchError::DeliveryUnwound)
        );
        drop(runner);
        assert_eq!(actor::clear_callbacks(), Ok(()));
    }

    #[tokio::test]
    async fn test_runner_stop_preserves_messages_and_channel_only_scheduling() {
        let mut runner = AsyncRunner::new();
        let received = Rc::new(RefCell::new(Vec::new()));
        let stop = runner.signal_tx.clone();

        for timestamp in 1..=65 {
            let received = received.clone();
            let stop = stop.clone();
            runner
                .time_evt_tx
                .send(
                    TimeEventMessage::new(
                        TimeEvent::new(
                            "runner-resume".into(),
                            UUID4::new(),
                            timestamp.into(),
                            timestamp.into(),
                        ),
                        TimeEventCallback::RustLocal(Rc::new(move |_| {
                            received.borrow_mut().push(timestamp);
                            if timestamp == 65 {
                                stop.send(()).unwrap();
                            }
                        })),
                    )
                    .into(),
                )
                .unwrap();
        }

        stop.send(()).unwrap();
        runner.run().await.unwrap();

        assert!(received.borrow().is_empty());
        assert_eq!(runner.channels.time_evt_rx.len(), 65);
        assert!(!runner.time_evt_tx.is_closed());

        let (result, at_yield) = tokio::join!(biased;
            runner.run(),
            async { received.borrow().clone() },
        );
        result.unwrap();

        let expected: Vec<u64> = (1..=65).collect();
        assert_eq!(at_yield, expected);
        assert_eq!(*received.borrow(), expected);
        assert_eq!(runner.channels.time_evt_rx.len(), 0);
        assert!(!runner.time_evt_tx.is_closed());
    }

    // Test fixture for creating test quotes
    fn test_quote() -> QuoteTick {
        QuoteTick {
            instrument_id: InstrumentId::from("EUR/USD.SIM"),
            bid_price: Price::from("1.10000"),
            ask_price: Price::from("1.10001"),
            bid_size: Quantity::from(1_000_000),
            ask_size: Quantity::from(1_000_000),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    fn test_system_event() -> SystemEvent {
        SystemEvent::SocketState(SocketStateChange::new(
            ClientId::from("BINANCE"),
            Some(Venue::from("BINANCE")),
            Ustr::from("binance-futures-market-streams"),
            SocketState::Connected,
        ))
    }

    fn test_system_command() -> SystemCommand {
        SystemCommand::ReconnectSocket(ReconnectSocket::new(
            TraderId::from("TRADER-001"),
            ClientId::from("POLYMARKET"),
            Ustr::from("polymarket-market-streams"),
            UnixNanos::from(3),
        ))
    }

    // Test fixture to create AsyncRunner with manual channels.
    // Sender halves are dummies (not connected to the test receivers) since
    // these tests exercise the event loop, not TLS binding.
    fn create_test_runner(
        time_evt_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<TimeEventMessage>>,
        data_evt_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<DataEvent>>,
        data_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<DataCommand>>,
        exec_evt_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<ExecutionEvent>>,
        exec_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DispatchMessage<TradingCommandMessage>>,
        signal_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
        signal_tx: tokio::sync::mpsc::UnboundedSender<()>,
    ) -> AsyncRunner {
        let (time_evt_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (system_evt_tx, system_evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let (system_cmd_tx, system_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (data_evt_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (data_cmd_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (exec_evt_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (exec_cmd_tx, _) = tokio::sync::mpsc::unbounded_channel();

        AsyncRunner {
            channels: AsyncRunnerChannels {
                time_evt_rx,
                system_evt_rx,
                system_cmd_rx,
                exec_evt_rx,
                exec_cmd_rx,
                data_evt_rx,
                data_cmd_rx,
            },
            time_evt_tx,
            system_evt_tx,
            system_cmd_tx,
            exec_evt_tx,
            exec_cmd_tx,
            data_evt_tx,
            data_cmd_tx,
            signal_rx,
            signal_tx,
        }
    }

    #[cfg(feature = "node")]
    #[rstest]
    fn test_poll_pending_processes_entry_snapshot_across_channels() {
        let (time_evt_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let (data_evt_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let (data_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let (exec_cmd_tx, exec_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel();

        let time_event = TimeEvent::new(
            Ustr::from("test"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        time_evt_tx
            .send(
                (TimeEventMessage::new(time_event, TimeEventCallback::from(|_: TimeEvent| {})))
                    .into(),
            )
            .unwrap();
        exec_evt_tx
            .send(
                (ExecutionEvent::Order(OrderEventAny::Submitted(
                    OrderSubmittedSpec::builder()
                        .client_order_id(ClientOrderId::from("O-POLL-001"))
                        .build(),
                )))
                .into(),
            )
            .unwrap();
        exec_cmd_tx
            .send(
                TradingCommandMessage::new(
                    MessagingSwitchboard::exec_engine_execute(),
                    TradingCommand::CancelAllOrders(CancelAllOrders::new(
                        TraderId::from("TRADER-001"),
                        None,
                        StrategyId::from("S-POLL-001"),
                        InstrumentId::from("EUR/USD.SIM"),
                        Some(OrderSide::Buy),
                        UUID4::new(),
                        UnixNanos::from(3),
                        None,
                        None,
                    )),
                )
                .into(),
            )
            .unwrap();
        data_evt_tx
            .send((DataEvent::Data(Data::Quote(test_quote()))).into())
            .unwrap();
        data_cmd_tx
            .send(
                DataCommand::Subscribe(SubscribeCommand::Data(SubscribeCustomData {
                    client_id: Some(ClientId::from("POLL")),
                    venue: None,
                    data_type: DataType::new("QuoteTick", None, None),
                    command_id: UUID4::new(),
                    ts_init: UnixNanos::from(4),
                    correlation_id: None,
                    params: None,
                }))
                .into(),
            )
            .unwrap();

        let mut runner = create_test_runner(
            time_evt_rx,
            data_evt_rx,
            data_cmd_rx,
            exec_evt_rx,
            exec_cmd_rx,
            signal_rx,
            signal_tx,
        );
        runner.bind_senders();
        get_system_command_sender()
            .send(test_system_command())
            .unwrap();
        get_system_event_sender().send(test_system_event()).unwrap();
        get_system_event_sender().send(test_system_event()).unwrap();
        let mut processed_by_channel = [0; 7];
        let mut processed_order = Vec::new();

        let first = runner.poll_pending(|event| match event {
            PendingRunnerEvent::TimeEvent(_) => {
                processed_by_channel[0] += 1;
                processed_order.push("time");
            }
            PendingRunnerEvent::SystemEvent(_) => {
                processed_by_channel[1] += 1;
                processed_order.push("system_event");
            }
            PendingRunnerEvent::SystemCommand(_) => {
                processed_by_channel[2] += 1;
                processed_order.push("system_command");
            }
            PendingRunnerEvent::ExecEvent(_) => {
                processed_by_channel[3] += 1;
                processed_order.push("exec_event");
            }
            PendingRunnerEvent::ExecCommand(_) => {
                processed_by_channel[4] += 1;
                processed_order.push("exec_command");
            }
            PendingRunnerEvent::DataEvent(_) => {
                processed_by_channel[5] += 1;
                processed_order.push("data_event");
                data_evt_tx
                    .send((DataEvent::Data(Data::Quote(test_quote()))).into())
                    .unwrap();
            }
            PendingRunnerEvent::DataCommand(_) => {
                processed_by_channel[6] += 1;
                processed_order.push("data_command");
            }
        });

        let second = runner.poll_pending(|event| match event {
            PendingRunnerEvent::DataEvent(_) => {
                processed_by_channel[5] += 1;
                processed_order.push("data_event");
            }
            _ => panic!("Unexpected runner event"),
        });

        assert_eq!(first, 8);
        assert_eq!(second, 1);
        assert_eq!(processed_by_channel, [1, 2, 1, 1, 1, 2, 1]);
        assert_eq!(
            processed_order,
            [
                "time",
                "system_event",
                "system_event",
                "system_command",
                "exec_event",
                "exec_command",
                "data_event",
                "data_command",
                "data_event",
            ]
        );
    }

    #[cfg(feature = "node")]
    #[tokio::test]
    async fn test_recv_processes_system_event_before_command() {
        let (_time_evt_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_data_evt_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_data_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_exec_cmd_tx, exec_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut runner = create_test_runner(
            time_evt_rx,
            data_evt_rx,
            data_cmd_rx,
            exec_evt_rx,
            exec_cmd_rx,
            signal_rx,
            signal_tx,
        );

        runner
            .system_cmd_tx
            .send((test_system_command()).into())
            .unwrap();
        runner
            .system_evt_tx
            .send((test_system_event()).into())
            .unwrap();

        assert!(matches!(
            runner.recv().await,
            Some(PendingRunnerEvent::SystemEvent(_))
        ));
        assert!(matches!(
            runner.recv().await,
            Some(PendingRunnerEvent::SystemCommand(_))
        ));
    }

    #[rstest]
    fn test_async_data_command_sender_creation() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = AsyncDataCommandSender::new(tx);
        assert!(format!("{sender:?}").contains("AsyncDataCommandSender"));
    }

    #[cfg(feature = "node")]
    #[rstest]
    fn test_data_command_sender_shutdown_logging() {
        struct ErrorCapture(std::sync::Mutex<Vec<String>>);

        impl log::Log for ErrorCapture {
            fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
                metadata.level() == log::Level::Error
                    && metadata.target() == "nautilus_live::runner"
            }

            fn log(&self, record: &log::Record<'_>) {
                if self.enabled(record.metadata()) {
                    self.0.lock().unwrap().push(record.args().to_string());
                }
            }

            fn flush(&self) {}
        }

        static ERRORS: ErrorCapture = ErrorCapture(std::sync::Mutex::new(Vec::new()));
        log::set_logger(&ERRORS).unwrap();
        log::set_max_level(log::LevelFilter::Error);

        let runner = AsyncRunner::new();
        let handle = LiveNodeHandle::new();
        handle.set_starting();
        runner.bind_senders_for_node(handle.clone());
        let sender = get_data_cmd_sender();
        let mut channels = runner.take_channels();

        let command = DataCommand::Subscribe(SubscribeCommand::Data(SubscribeCustomData {
            client_id: Some(ClientId::from("TEST")),
            venue: None,
            data_type: DataType::new("QuoteTick", None, None),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            correlation_id: None,
            params: None,
        }));

        // Stopping and final draining must still deliver commands while the receiver is alive
        for stopped in [false, true] {
            if stopped {
                handle.set_stopped();
            } else {
                handle.set_shutting_down();
            }

            sender.execute(command.clone());
            assert_eq!(
                channels
                    .data_cmd_rx
                    .try_recv()
                    .unwrap()
                    .dispatch(|command| command),
                command
            );
        }

        drop(channels);
        sender.execute(command.clone());
        assert_eq!(*ERRORS.0.lock().unwrap(), Vec::<String>::new());

        // A stopped previous node must not hide an unexpected closure in its replacement
        let runner = AsyncRunner::new();
        let handle = LiveNodeHandle::new();
        runner.bind_senders_for_node(handle.clone());
        let sender = get_data_cmd_sender();
        drop(runner);

        for shutting_down in [false, true] {
            if shutting_down {
                handle.set_shutting_down();
            } else {
                handle.set_starting();
            }

            sender.execute(command.clone());
        }

        assert_eq!(
            *ERRORS.0.lock().unwrap(),
            vec!["Failed to send data command: channel closed"; 2],
        );

        ERRORS.0.lock().unwrap().clear();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);
        AsyncDataCommandSender::new(tx).execute(command);
        assert_eq!(
            *ERRORS.0.lock().unwrap(),
            vec!["Failed to send data command: channel closed"],
        );
    }

    #[rstest]
    fn test_async_time_event_sender_creation() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = AsyncTimeEventSender::new(tx);
        assert!(format!("{sender:?}").contains("AsyncTimeEventSender"));
    }

    #[tokio::test]
    async fn test_async_data_command_sender_execute() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = AsyncDataCommandSender::new(tx);

        let command = DataCommand::Subscribe(SubscribeCommand::Data(SubscribeCustomData {
            client_id: Some(ClientId::from("TEST")),
            venue: None,
            data_type: DataType::new("QuoteTick", None, None),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            correlation_id: None,
            params: None,
        }));

        sender.execute(command.clone());

        let received = rx.recv().await.unwrap().dispatch(|command| command);
        match (received, command) {
            (
                DataCommand::Subscribe(SubscribeCommand::Data(r)),
                DataCommand::Subscribe(SubscribeCommand::Data(c)),
            ) => {
                assert_eq!(r.client_id, c.client_id);
                assert_eq!(r.data_type, c.data_type);
            }
            _ => panic!("Command mismatch"),
        }
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_async_command_senders_capture_owner_root(#[case] rooted: bool) {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let mut channels = runner.take_channels();

        let data = DataCommand::Subscribe(SubscribeCommand::Data(SubscribeCustomData {
            client_id: Some(ClientId::from("ROOT")),
            venue: None,
            data_type: DataType::new("QuoteTick", None, None),
            command_id: UUID4::new(),
            ts_init: UnixNanos::from(7),
            correlation_id: None,
            params: None,
        }));

        let trading = TradingCommand::CancelAllOrders(CancelAllOrders::new(
            TraderId::from("TRADER-001"),
            None,
            StrategyId::from("ROOT-001"),
            InstrumentId::from("EUR/USD.SIM"),
            Some(OrderSide::Sell),
            UUID4::new(),
            UnixNanos::from(11),
            None,
            None,
        ));
        let expected_data = data.clone();
        let expected_trading = trading.clone();
        let trigger = trading.clone();

        let send = move || {
            get_data_cmd_sender().execute(data.clone());
            get_trading_cmd_sender().execute(TradingCommandMessage::new(
                MessagingSwitchboard::exec_engine_execute(),
                trading.clone(),
            ));
        };

        if rooted {
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::risk_engine_execute(),
                TypedIntoHandler::from(move |_| send()),
            );
            SyncTradingCommandSender.execute(TradingCommandMessage::new(
                MessagingSwitchboard::risk_engine_execute(),
                trigger,
            ));
            drain_trading_cmd_queue();
        } else {
            send();
        }

        let data = channels.data_cmd_rx.try_recv().unwrap();
        let trading = channels.exec_cmd_rx.try_recv().unwrap();

        let results = thread::spawn(move || {
            let data = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                data.dispatch(|command| command)
            }));

            let trading = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                trading.dispatch(|message| (message.endpoint(), message.command().clone()))
            }));

            (data, trading)
        })
        .join()
        .unwrap();

        if rooted {
            for error in [results.0.unwrap_err(), results.1.unwrap_err()] {
                let message = error.downcast_ref::<String>().unwrap();
                assert!(message.contains("command context dispatched outside its owner thread"));
            }
        } else {
            assert_eq!(results.0.unwrap(), expected_data);
            assert_eq!(
                results.1.unwrap(),
                (
                    MessagingSwitchboard::exec_engine_execute(),
                    expected_trading
                ),
            );
        }

        assert!(channels.data_cmd_rx.is_empty());
        assert!(channels.exec_cmd_rx.is_empty());
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_event_senders_capture_owner_root(
        #[case] rooted: bool,
        #[values(false, true)] dispatch: bool,
    ) {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let mut channels = runner.take_channels();
        let expected_quote = test_quote();
        let expected_order = OrderSubmittedSpec::builder()
            .client_order_id(ClientOrderId::from("EVENT-017"))
            .build();
        let order = expected_order.clone();

        let send = move || {
            get_data_event_sender()
                .send(DataEvent::Data(Data::Quote(expected_quote)))
                .unwrap();
            get_exec_event_sender()
                .send(ExecutionEvent::Order(OrderEventAny::Submitted(
                    order.clone(),
                )))
                .unwrap();
        };

        if rooted {
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::risk_engine_execute(),
                TypedIntoHandler::from(move |_| send()),
            );
            SyncTradingCommandSender.execute(TradingCommandMessage::new(
                MessagingSwitchboard::risk_engine_execute(),
                TradingCommand::QueryAccount(QueryAccount::new(
                    "TRADER-001".into(),
                    None,
                    "SIM-001".into(),
                    UUID4::new(),
                    19.into(),
                    None,
                    None,
                )),
            ));
            drain_trading_cmd_queue();
        } else {
            send();
        }

        if dispatch {
            msgbus::register_data_endpoint(
                MessagingSwitchboard::data_engine_process_data(),
                TypedIntoHandler::from(move |data| {
                    assert_eq!(data, Data::Quote(expected_quote));
                    get_data_event_sender().send(DataEvent::Data(data)).unwrap();
                }),
            );

            let order = expected_order.clone();
            msgbus::register_order_event_endpoint(
                MessagingSwitchboard::exec_engine_process(),
                TypedIntoHandler::from(move |event| {
                    assert_eq!(event, OrderEventAny::Submitted(order.clone()));
                    get_exec_event_sender()
                        .send(ExecutionEvent::Order(event))
                        .unwrap();
                }),
            );

            AsyncRunner::dispatch_data_event(channels.data_evt_rx.try_recv().unwrap());
            AsyncRunner::dispatch_exec_event(channels.exec_evt_rx.try_recv().unwrap());
        }

        let data = channels.data_evt_rx.try_recv().unwrap();
        let exec = channels.exec_evt_rx.try_recv().unwrap();

        let (data, exec) = thread::spawn(move || {
            (
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    data.dispatch(|event| event)
                })),
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    exec.dispatch(|event| event)
                })),
            )
        })
        .join()
        .unwrap();

        if rooted || dispatch {
            for error in [data.unwrap_err(), exec.unwrap_err()] {
                assert!(
                    error
                        .downcast_ref::<String>()
                        .unwrap()
                        .contains("command context dispatched outside its owner thread")
                );
            }
        } else {
            let DataEvent::Data(Data::Quote(quote)) = data.unwrap() else {
                panic!("expected quote")
            };

            let ExecutionEvent::Order(OrderEventAny::Submitted(order)) = exec.unwrap() else {
                panic!("expected submitted order")
            };

            assert_eq!(quote, expected_quote);
            assert_eq!(order, expected_order);
        }

        assert!(channels.data_evt_rx.is_empty());
        assert!(channels.exec_evt_rx.is_empty());
    }

    #[rstest]
    fn test_system_and_time_senders_capture_owner_root(#[values(false, true)] rooted: bool) {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let mut channels = runner.take_channels();
        let event = TimeEvent::new("system-time".into(), UUID4::new(), 17.into(), 23.into());
        let observed = Rc::new(RefCell::new(Vec::new()));
        let received = observed.clone();
        let expected_event = event.clone();

        let callback = TimeEventCallback::RustLocal(Rc::new(move |event| {
            received.borrow_mut().push(event);
            get_system_command_sender()
                .send(test_system_command())
                .unwrap();
        }));

        let send = move || {
            get_system_event_sender().send(test_system_event()).unwrap();
            get_system_command_sender()
                .send(test_system_command())
                .unwrap();
            get_time_event_sender().send(TimeEventMessage::new(event.clone(), callback.clone()));
        };

        if rooted {
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::risk_engine_execute(),
                TypedIntoHandler::from(move |_| send()),
            );
            SyncTradingCommandSender.execute(TradingCommandMessage::new(
                MessagingSwitchboard::risk_engine_execute(),
                TradingCommand::QueryAccount(QueryAccount::new(
                    "TRADER-001".into(),
                    None,
                    "SIM-001".into(),
                    UUID4::new(),
                    31.into(),
                    None,
                    None,
                )),
            ));
            drain_trading_cmd_queue();
        } else {
            send();
        }

        let system_event = channels.system_evt_rx.try_recv().unwrap();
        let system_command = channels.system_cmd_rx.try_recv().unwrap();
        let time_event = channels.time_evt_rx.try_recv().unwrap();
        assert_eq!(system_event.is_rooted(), rooted);
        assert_eq!(system_command.is_rooted(), rooted);
        assert_eq!(time_event.is_rooted(), rooted);
        assert!(AsyncRunner::handle_time_event(time_event));
        let child = channels.system_cmd_rx.try_recv().unwrap();

        assert_eq!(system_event.dispatch(|event| event), test_system_event());
        assert_eq!(
            system_command.dispatch(|command| command),
            test_system_command()
        );
        assert_eq!(*observed.borrow(), vec![expected_event]);
        assert!(child.is_rooted());
        assert_eq!(child.dispatch(|command| command), test_system_command());
        assert!(channels.time_evt_rx.is_empty());
        assert!(channels.system_evt_rx.is_empty());
        assert!(channels.system_cmd_rx.is_empty());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn test_live_timer_started_under_root_emits_independent_event() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let mut channels = runner.take_channels();
        let now = get_atomic_clock_realtime().get_time_ns();
        let name = Ustr::from("ROOTED_TIMER");
        let owner = thread::current().id();
        let observed = Rc::new(RefCell::new(Vec::new()));
        let received = observed.clone();

        let callback = TimeEventCallback::RustLocal(Rc::new(move |event| {
            received
                .borrow_mut()
                .push((event.name, event.ts_event, thread::current().id()));
        }));

        let timer = Rc::new(RefCell::new(None));
        let started = timer.clone();
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_execute(),
            TypedIntoHandler::from(move |_| {
                get_system_command_sender()
                    .send(test_system_command())
                    .unwrap();

                let mut timer = LiveTimer::new(
                    name,
                    NonZeroU64::new(1_000_000).unwrap(),
                    now,
                    Some(now),
                    callback.clone(),
                    true,
                    Some(get_time_event_sender()),
                );
                timer.start();
                *started.borrow_mut() = Some(timer);
            }),
        );

        SyncTradingCommandSender.execute(TradingCommandMessage::new(
            MessagingSwitchboard::risk_engine_execute(),
            TradingCommand::QueryAccount(QueryAccount::new(
                "TRADER-001".into(),
                None,
                "SIM-001".into(),
                UUID4::new(),
                31.into(),
                None,
                None,
            )),
        ));
        drain_trading_cmd_queue();
        let witness = channels.system_cmd_rx.try_recv().unwrap();
        let message = tokio::time::timeout(Duration::from_secs(2), channels.time_evt_rx.recv())
            .await
            .unwrap()
            .unwrap();
        timer.borrow_mut().take().unwrap().cancel();

        assert!(witness.is_rooted());
        assert!(!message.is_rooted());
        assert!(observed.borrow().is_empty());
        assert!(AsyncRunner::handle_time_event(message));
        assert_eq!(*observed.borrow(), vec![(name, now, owner)]);
        assert!(channels.time_evt_rx.is_empty());
    }

    #[tokio::test]
    async fn test_async_time_event_sender_send() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = AsyncTimeEventSender::new(tx);

        let event = TimeEvent::new(
            Ustr::from("test"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let callback = TimeEventCallback::from(|_: TimeEvent| {});
        let message = TimeEventMessage::new(event, callback);

        sender.send(message);

        assert!(rx.recv().await.is_some());
    }

    #[tokio::test]
    async fn test_runner_shutdown_signal() {
        // Create runner with manual channels to avoid global state
        let (_data_tx, data_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataEvent>>();
        let (_cmd_tx, data_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataCommand>>();
        let (_time_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TimeEventMessage>>();
        let (_exec_evt_tx, exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<ExecutionEvent>>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let mut runner = create_test_runner(
            time_evt_rx,
            data_evt_rx,
            data_cmd_rx,
            exec_evt_rx,
            exec_cmd_rx,
            signal_rx,
            signal_tx.clone(),
        );

        // Start runner
        let runner_handle = tokio::spawn(async move {
            runner.run().await.unwrap();
        });

        // Send shutdown signal
        signal_tx.send(()).unwrap();

        // Runner should stop quickly
        let result = tokio::time::timeout(Duration::from_millis(100), runner_handle).await;
        assert!(result.is_ok(), "Runner should stop on signal");
    }

    #[tokio::test]
    async fn test_runner_closes_on_channel_drop() {
        let (data_tx, data_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataEvent>>();
        let (_cmd_tx, data_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataCommand>>();
        let (_time_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TimeEventMessage>>();
        let (_exec_evt_tx, exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<ExecutionEvent>>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let mut runner = create_test_runner(
            time_evt_rx,
            data_evt_rx,
            data_cmd_rx,
            exec_evt_rx,
            exec_cmd_rx,
            signal_rx,
            signal_tx.clone(),
        );

        // Start runner
        let runner_handle = tokio::spawn(async move {
            runner.run().await.unwrap();
        });

        drop(data_tx);

        // Yield to let runner enter event loop before stop signal
        tokio::task::yield_now().await;
        signal_tx.send(()).ok();

        // Runner should stop when channels close or on signal
        let result = tokio::time::timeout(Duration::from_millis(200), runner_handle).await;
        assert!(
            result.is_ok(),
            "Runner should stop when channels close or on signal"
        );
    }

    #[tokio::test]
    async fn test_concurrent_event_sending() {
        let (data_evt_tx, data_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataEvent>>();
        let (_data_cmd_tx, data_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataCommand>>();
        let (_time_evt_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TimeEventMessage>>();
        let (_exec_evt_tx, exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<ExecutionEvent>>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        // Setup runner
        let mut runner = create_test_runner(
            time_evt_rx,
            data_evt_rx,
            data_cmd_rx,
            exec_evt_rx,
            exec_cmd_rx,
            signal_rx,
            signal_tx.clone(),
        );

        // Spawn multiple concurrent senders
        let mut handles = vec![];

        for _ in 0..5 {
            let tx_clone = data_evt_tx.clone();

            let handle = tokio::spawn(async move {
                for _ in 0..20 {
                    let quote = test_quote();
                    tx_clone
                        .send(DataEvent::Data(Data::Quote(quote)).into())
                        .unwrap();
                    tokio::task::yield_now().await;
                }
            });

            handles.push(handle);
        }

        // Start runner in background
        let runner_handle = tokio::spawn(async move {
            runner.run().await.unwrap();
        });

        // Wait for all senders
        for handle in handles {
            handle.await.unwrap();
        }

        // Yield to let runner enter event loop before stop signal
        tokio::task::yield_now().await;
        signal_tx.send(()).unwrap();

        let _ = tokio::time::timeout(Duration::from_millis(200), runner_handle).await;
    }

    #[rstest]
    #[case(10)]
    #[case(100)]
    #[case(1000)]
    fn test_channel_send_performance(#[case] count: usize) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let quote = test_quote();

        // Send events
        for _ in 0..count {
            tx.send(DataEvent::Data(Data::Quote(quote))).unwrap();
        }

        // Verify all received
        let mut received = 0;
        while rx.try_recv().is_ok() {
            received += 1;
        }

        assert_eq!(received, count);
    }

    #[rstest]
    fn test_async_trading_command_sender_creation() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = AsyncTradingCommandSender::new(tx);
        assert!(format!("{sender:?}").contains("AsyncTradingCommandSender"));
    }

    #[rstest]
    fn test_async_trading_command_sender_preserves_target_endpoints() {
        std::thread::spawn(|| {
            msgbus::get_message_bus().borrow_mut().dispose();
            let (risk_handler, risk_saving_handler) =
                get_typed_into_message_saving_handler::<TradingCommand>(Some(Ustr::from(
                    "RiskEngine.execute",
                )));
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::risk_engine_execute(),
                risk_handler,
            );
            let (exec_handler, exec_saving_handler) =
                get_typed_into_message_saving_handler::<TradingCommand>(Some(Ustr::from(
                    "ExecEngine.execute",
                )));
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::exec_engine_execute(),
                exec_handler,
            );

            let (tx, mut rx) =
                tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
            let sender = AsyncTradingCommandSender::new(tx);
            sender.execute(TradingCommandMessage::new(
                MessagingSwitchboard::risk_engine_execute(),
                TradingCommand::CancelAllOrders(CancelAllOrders::new(
                    TraderId::from("TRADER-001"),
                    None,
                    StrategyId::from("RISK-001"),
                    InstrumentId::from("EUR/USD.SIM"),
                    Some(OrderSide::Buy),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                )),
            ));
            sender.execute(TradingCommandMessage::new(
                MessagingSwitchboard::exec_engine_execute(),
                TradingCommand::CancelAllOrders(CancelAllOrders::new(
                    TraderId::from("TRADER-001"),
                    None,
                    StrategyId::from("EXEC-001"),
                    InstrumentId::from("EUR/USD.SIM"),
                    Some(OrderSide::Sell),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                )),
            ));

            AsyncRunner::handle_trading_command(rx.try_recv().unwrap());
            AsyncRunner::handle_trading_command(rx.try_recv().unwrap());

            let risk_commands = risk_saving_handler.get_messages();
            let exec_commands = exec_saving_handler.get_messages();
            assert!(rx.try_recv().is_err());
            assert_eq!(risk_commands.len(), 1);
            assert_eq!(
                risk_commands[0].strategy_id(),
                Some(StrategyId::from("RISK-001"))
            );
            assert_eq!(exec_commands.len(), 1);
            assert_eq!(
                exec_commands[0].strategy_id(),
                Some(StrategyId::from("EXEC-001"))
            );
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_async_runner_preserves_deferred_follow_up_order() {
        std::thread::spawn(|| {
            msgbus::get_message_bus().borrow_mut().dispose();
            let clock = Rc::new(RefCell::new(VirtualClock::new()));
            let cache = Rc::new(RefCell::new(Cache::default()));
            let exec_engine = Rc::new(RefCell::new(ExecutionEngine::new(clock, cache, None)));
            ExecutionEngine::register_msgbus_handlers(&exec_engine);
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::risk_engine_execute(),
                TypedIntoHandler::from(|command: TradingCommand| {
                    msgbus::send_trading_command(
                        MessagingSwitchboard::exec_engine_queue_execute(),
                        command,
                    );
                }),
            );

            let (exec_handler, exec_saving_handler) =
                get_typed_into_message_saving_handler::<TradingCommand>(Some(Ustr::from(
                    "ExecEngine.execute",
                )));
            msgbus::register_trading_command_endpoint(
                MessagingSwitchboard::exec_engine_execute(),
                exec_handler,
            );

            let (tx, mut rx) =
                tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
            let sender = Arc::new(AsyncTradingCommandSender::new(tx));
            replace_exec_cmd_sender(sender.clone());
            sender.execute(TradingCommandMessage::new(
                MessagingSwitchboard::risk_engine_execute(),
                TradingCommand::CancelAllOrders(CancelAllOrders::new(
                    TraderId::from("TRADER-001"),
                    None,
                    StrategyId::from("FIRST-001"),
                    InstrumentId::from("EUR/USD.SIM"),
                    Some(OrderSide::Buy),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                )),
            ));
            sender.execute(TradingCommandMessage::new(
                MessagingSwitchboard::exec_engine_execute(),
                TradingCommand::CancelAllOrders(CancelAllOrders::new(
                    TraderId::from("TRADER-001"),
                    None,
                    StrategyId::from("SECOND-001"),
                    InstrumentId::from("EUR/USD.SIM"),
                    Some(OrderSide::Sell),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                )),
            ));

            AsyncRunner::handle_trading_command(rx.try_recv().unwrap());
            AsyncRunner::handle_trading_command(rx.try_recv().unwrap());

            let commands = exec_saving_handler.get_messages();
            let strategy_ids = commands
                .iter()
                .map(TradingCommand::strategy_id)
                .collect::<Vec<_>>();
            assert!(rx.try_recv().is_err());
            assert_eq!(commands.len(), 2);
            assert_eq!(
                strategy_ids,
                vec![
                    Some(StrategyId::from("FIRST-001")),
                    Some(StrategyId::from("SECOND-001"))
                ]
            );
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_async_runner_dispatches_deferred_exec_command_once() {
        std::thread::spawn(|| {
            msgbus::get_message_bus().borrow_mut().dispose();
            let clock = Rc::new(RefCell::new(VirtualClock::new()));
            let cache = Rc::new(RefCell::new(Cache::default()));
            let exec_engine = Rc::new(RefCell::new(ExecutionEngine::new(clock, cache, None)));
            ExecutionEngine::register_msgbus_handlers(&exec_engine);

            let (tx, mut rx) =
                tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
            replace_exec_cmd_sender(Arc::new(AsyncTradingCommandSender::new(tx)));
            let command = TradingCommand::CancelAllOrders(CancelAllOrders::new(
                TraderId::from("TRADER-001"),
                None,
                StrategyId::from("EXEC-001"),
                InstrumentId::from("EUR/USD.SIM"),
                Some(OrderSide::Buy),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ));

            msgbus::send_trading_command(
                MessagingSwitchboard::exec_engine_queue_execute(),
                command,
            );
            assert_eq!(exec_engine.borrow().command_count(), 0);

            AsyncRunner::handle_trading_command(rx.try_recv().unwrap());

            assert!(rx.try_recv().is_err());
            assert_eq!(exec_engine.borrow().command_count(), 1);
        })
        .join()
        .unwrap();
    }

    #[tokio::test]
    async fn test_runner_processes_trading_commands() {
        let (_data_evt_tx, data_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataEvent>>();
        let (_data_cmd_tx, data_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataCommand>>();
        let (_time_evt_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TimeEventMessage>>();
        let (_exec_evt_tx, exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<ExecutionEvent>>();
        let (exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let mut runner = create_test_runner(
            time_evt_rx,
            data_evt_rx,
            data_cmd_rx,
            exec_evt_rx,
            exec_cmd_rx,
            signal_rx,
            signal_tx.clone(),
        );

        let runner_handle = tokio::spawn(async move {
            runner.run().await.unwrap();
        });

        let command = TradingCommand::CancelAllOrders(CancelAllOrders::new(
            TraderId::from("TRADER-001"),
            None,
            StrategyId::from("S-001"),
            InstrumentId::from("EUR/USD.SIM"),
            Some(OrderSide::Buy),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None, // correlation_id
        ));
        exec_cmd_tx
            .send(
                TradingCommandMessage::new(MessagingSwitchboard::exec_engine_execute(), command)
                    .into(),
            )
            .unwrap();

        tokio::task::yield_now().await;
        signal_tx.send(()).unwrap();

        let result = tokio::time::timeout(Duration::from_millis(100), runner_handle).await;
        assert!(result.is_ok(), "Runner should process command and stop");
    }

    #[tokio::test]
    async fn test_runner_processes_multiple_trading_commands() {
        let (_data_evt_tx, data_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataEvent>>();
        let (_data_cmd_tx, data_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataCommand>>();
        let (_time_evt_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TimeEventMessage>>();
        let (_exec_evt_tx, exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<ExecutionEvent>>();
        let (exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let mut runner = create_test_runner(
            time_evt_rx,
            data_evt_rx,
            data_cmd_rx,
            exec_evt_rx,
            exec_cmd_rx,
            signal_rx,
            signal_tx.clone(),
        );

        let runner_handle = tokio::spawn(async move {
            runner.run().await.unwrap();
        });

        for i in 0..10 {
            let strategy_id = format!("S-{i:03}");
            let command = TradingCommand::CancelAllOrders(CancelAllOrders::new(
                TraderId::from("TRADER-001"),
                None,
                StrategyId::from(strategy_id.as_str()),
                InstrumentId::from("EUR/USD.SIM"),
                Some(OrderSide::Buy),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None, // correlation_id
            ));
            exec_cmd_tx
                .send(
                    TradingCommandMessage::new(
                        MessagingSwitchboard::exec_engine_execute(),
                        command,
                    )
                    .into(),
                )
                .unwrap();
        }

        tokio::task::yield_now().await;
        signal_tx.send(()).unwrap();

        let result = tokio::time::timeout(Duration::from_millis(100), runner_handle).await;
        assert!(
            result.is_ok(),
            "Runner should process all commands and stop"
        );
    }

    #[tokio::test]
    async fn test_execution_event_order_channel() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();

        let event = OrderSubmittedSpec::builder()
            .client_order_id(ClientOrderId::from("O-001"))
            .build();

        tx.send(ExecutionEvent::Order(OrderEventAny::Submitted(event)))
            .unwrap();

        let received = rx.recv().await.unwrap();
        match received {
            ExecutionEvent::Order(OrderEventAny::Submitted(e)) => {
                assert_eq!(e.client_order_id(), ClientOrderId::from("O-001"));
            }
            _ => panic!("Expected OrderSubmitted event"),
        }
    }

    #[tokio::test]
    async fn test_execution_report_order_status_channel() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();

        let report = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("EUR/USD.SIM"),
            Some(ClientOrderId::from("O-001")),
            VenueOrderId::from("V-001"),
            OrderSide::Buy.into(),
            OrderType::Market,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from(100_000),
            Quantity::from(100_000),
            UnixNanos::from(1),
            UnixNanos::from(2),
            UnixNanos::from(3),
            None,
        );

        tx.send(ExecutionEvent::Report(ExecutionReport::Order(Box::new(
            report,
        ))))
        .unwrap();

        let received = rx.recv().await.unwrap();
        match received {
            ExecutionEvent::Report(ExecutionReport::Order(r)) => {
                assert_eq!(r.venue_order_id.as_str(), "V-001");
                assert_eq!(r.order_status, OrderStatus::Accepted);
            }
            _ => panic!("Expected OrderStatusReport"),
        }
    }

    #[tokio::test]
    async fn test_execution_report_fill() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();

        let report = FillReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("EUR/USD.SIM"),
            VenueOrderId::from("V-001"),
            TradeId::from("T-001"),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.10000"),
            Money::from("10 USD"),
            LiquiditySide::Taker,
            Some(ClientOrderId::from("O-001")),
            None,
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );

        tx.send(ExecutionEvent::Report(ExecutionReport::Fill(Box::new(
            report,
        ))))
        .unwrap();

        let received = rx.recv().await.unwrap();
        match received {
            ExecutionEvent::Report(ExecutionReport::Fill(r)) => {
                assert_eq!(r.venue_order_id.as_str(), "V-001");
                assert_eq!(r.trade_id.to_string(), "T-001");
            }
            _ => panic!("Expected FillReport"),
        }
    }

    #[tokio::test]
    async fn test_execution_report_position() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();

        let report = PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("EUR/USD.SIM"),
            PositionSide::Long,
            Quantity::from(100_000),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
            Some(PositionId::from("P-001")),
            None,
        );

        tx.send(ExecutionEvent::Report(ExecutionReport::Position(Box::new(
            report,
        ))))
        .unwrap();

        let received = rx.recv().await.unwrap();
        match received {
            ExecutionEvent::Report(ExecutionReport::Position(r)) => {
                assert_eq!(r.venue_position_id.unwrap().as_str(), "P-001");
            }
            _ => panic!("Expected PositionStatusReport"),
        }
    }

    #[tokio::test]
    async fn test_execution_event_account() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();

        let account_state = AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );

        tx.send(ExecutionEvent::Account(account_state)).unwrap();

        let received = rx.recv().await.unwrap();
        match received {
            ExecutionEvent::Account(r) => {
                assert_eq!(r.account_id.as_str(), "SIM-001");
            }
            _ => panic!("Expected AccountState"),
        }
    }

    #[tokio::test]
    async fn test_runner_stop_method() {
        let (_data_tx, data_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataEvent>>();
        let (_cmd_tx, data_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataCommand>>();
        let (_time_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TimeEventMessage>>();
        let (_exec_evt_tx, exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<ExecutionEvent>>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let mut runner = create_test_runner(
            time_evt_rx,
            data_evt_rx,
            data_cmd_rx,
            exec_evt_rx,
            exec_cmd_rx,
            signal_rx,
            signal_tx.clone(),
        );

        let runner_handle = tokio::spawn(async move {
            runner.run().await.unwrap();
        });

        // Use stop via signal_tx directly
        signal_tx.send(()).unwrap();

        let result = tokio::time::timeout(Duration::from_millis(100), runner_handle).await;
        assert!(result.is_ok(), "Runner should stop when stop() is called");
    }

    #[tokio::test]
    async fn test_all_event_types_integration() {
        let (data_evt_tx, data_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataEvent>>();
        let (data_cmd_tx, data_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataCommand>>();
        let (time_evt_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TimeEventMessage>>();
        let (exec_evt_tx, exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<ExecutionEvent>>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let mut runner = create_test_runner(
            time_evt_rx,
            data_evt_rx,
            data_cmd_rx,
            exec_evt_rx,
            exec_cmd_rx,
            signal_rx,
            signal_tx.clone(),
        );

        let runner_handle = tokio::spawn(async move {
            runner.run().await.unwrap();
        });

        // Send data event
        let quote = test_quote();
        data_evt_tx
            .send((DataEvent::Data(Data::Quote(quote))).into())
            .unwrap();

        // Send data command
        let command = DataCommand::Subscribe(SubscribeCommand::Data(SubscribeCustomData {
            client_id: Some(ClientId::from("TEST")),
            venue: None,
            data_type: DataType::new("QuoteTick", None, None),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            correlation_id: None,
            params: None,
        }));

        data_cmd_tx.send(command.into()).unwrap();

        // Send time event
        let event = TimeEvent::new(
            Ustr::from("test"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let callback = TimeEventCallback::from(|_: TimeEvent| {});
        let message = TimeEventMessage::new(event, callback);
        time_evt_tx.send((message).into()).unwrap();

        // Send execution order event
        let order_event = OrderSubmittedSpec::builder()
            .client_order_id(ClientOrderId::from("O-001"))
            .build();
        exec_evt_tx
            .send((ExecutionEvent::Order(OrderEventAny::Submitted(order_event))).into())
            .unwrap();

        // Send execution report (OrderStatus)
        let order_status = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("EUR/USD.SIM"),
            Some(ClientOrderId::from("O-001")),
            VenueOrderId::from("V-001"),
            OrderSide::Buy.into(),
            OrderType::Market,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from(100_000),
            Quantity::from(100_000),
            UnixNanos::from(1),
            UnixNanos::from(2),
            UnixNanos::from(3),
            None,
        );
        exec_evt_tx
            .send((ExecutionEvent::Report(ExecutionReport::Order(Box::new(order_status)))).into())
            .unwrap();

        // Send execution report (Fill)
        let fill = FillReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("EUR/USD.SIM"),
            VenueOrderId::from("V-001"),
            TradeId::from("T-001"),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.10000"),
            Money::from("10 USD"),
            LiquiditySide::Taker,
            Some(ClientOrderId::from("O-001")),
            None,
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );
        exec_evt_tx
            .send((ExecutionEvent::Report(ExecutionReport::Fill(Box::new(fill)))).into())
            .unwrap();

        // Send execution report (Position)
        let position = PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("EUR/USD.SIM"),
            PositionSide::Long,
            Quantity::from(100_000),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
            Some(PositionId::from("P-001")),
            None,
        );
        exec_evt_tx
            .send((ExecutionEvent::Report(ExecutionReport::Position(Box::new(position)))).into())
            .unwrap();

        // Send account event
        let account_state = AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );
        exec_evt_tx
            .send((ExecutionEvent::Account(account_state)).into())
            .unwrap();

        // Yield to let runner enter event loop before stop signal
        tokio::task::yield_now().await;
        signal_tx.send(()).unwrap();

        let result = tokio::time::timeout(Duration::from_millis(200), runner_handle).await;
        assert!(
            result.is_ok(),
            "Runner should process all event types and stop cleanly"
        );
    }

    #[tokio::test]
    async fn test_runner_handle_stops_runner() {
        let (_data_tx, data_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataEvent>>();
        let (_cmd_tx, data_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataCommand>>();
        let (_time_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TimeEventMessage>>();
        let (_exec_evt_tx, exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<ExecutionEvent>>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let mut runner = create_test_runner(
            time_evt_rx,
            data_evt_rx,
            data_cmd_rx,
            exec_evt_rx,
            exec_cmd_rx,
            signal_rx,
            signal_tx.clone(),
        );

        // Get handle before moving runner
        let handle = runner.handle();

        let runner_handle = tokio::spawn(async move {
            runner.run().await.unwrap();
        });

        // Use handle to stop
        handle.stop();

        let result = tokio::time::timeout(Duration::from_millis(100), runner_handle).await;
        assert!(result.is_ok(), "Runner should stop via handle");
    }

    #[tokio::test]
    async fn test_runner_handle_is_cloneable() {
        let (signal_tx, _signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let handle = AsyncRunnerHandle { signal_tx };

        let handle2 = handle.clone();

        // Both handles should be able to send stop signals
        assert!(handle.signal_tx.send(()).is_ok());
        assert!(handle2.signal_tx.send(()).is_ok());
    }

    #[tokio::test]
    async fn test_runner_processes_events_before_stop() {
        let (data_evt_tx, data_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataEvent>>();
        let (_cmd_tx, data_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<DataCommand>>();
        let (_time_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TimeEventMessage>>();
        let (_exec_evt_tx, exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<ExecutionEvent>>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<DispatchMessage<TradingCommandMessage>>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let mut runner = create_test_runner(
            time_evt_rx,
            data_evt_rx,
            data_cmd_rx,
            exec_evt_rx,
            exec_cmd_rx,
            signal_rx,
            signal_tx.clone(),
        );

        let handle = runner.handle();

        // Send events before starting runner
        for _ in 0..10 {
            let quote = test_quote();
            data_evt_tx
                .send((DataEvent::Data(Data::Quote(quote))).into())
                .unwrap();
        }

        let runner_handle = tokio::spawn(async move {
            runner.run().await.unwrap();
        });

        // Yield to let runner enter event loop before stop signal
        tokio::task::yield_now().await;
        handle.stop();

        let result = tokio::time::timeout(Duration::from_millis(200), runner_handle).await;
        assert!(result.is_ok(), "Runner should process events and stop");
    }

    #[rstest]
    fn test_new_does_not_bind_tls() {
        std::thread::spawn(|| {
            let _runner = AsyncRunner::new();
            assert!(try_get_time_event_sender().is_none());
            assert!(try_get_system_command_sender().is_none());
            assert!(try_get_system_event_sender().is_none());
            assert!(try_get_trading_cmd_sender().is_none());
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_bind_senders_routes_to_runner_channels() {
        std::thread::spawn(|| {
            let mut runner = AsyncRunner::new();
            runner.bind_senders();

            get_data_cmd_sender().execute(DataCommand::Subscribe(SubscribeCommand::Data(
                SubscribeCustomData {
                    client_id: Some(ClientId::from("TEST")),
                    venue: None,
                    data_type: DataType::new("test", None, None),
                    command_id: UUID4::new(),
                    ts_init: UnixNanos::default(),
                    correlation_id: None,
                    params: None,
                },
            )));

            assert!(runner.channels.data_cmd_rx.try_recv().is_ok());

            get_trading_cmd_sender().execute(TradingCommandMessage::new(
                MessagingSwitchboard::exec_engine_execute(),
                TradingCommand::CancelAllOrders(CancelAllOrders::new(
                    TraderId::from("TRADER-001"),
                    None,
                    StrategyId::from("S-001"),
                    InstrumentId::from("EUR/USD.SIM"),
                    Some(OrderSide::Buy),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None, // correlation_id
                )),
            ));
            assert!(runner.channels.exec_cmd_rx.try_recv().is_ok());

            let event = TimeEvent::new(
                Ustr::from("test"),
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
            );
            let callback = TimeEventCallback::from(|_: TimeEvent| {});
            get_time_event_sender().send(TimeEventMessage::new(event, callback));
            assert!(runner.channels.time_evt_rx.try_recv().is_ok());

            get_system_event_sender().send(test_system_event()).unwrap();
            assert_eq!(
                runner
                    .channels
                    .system_evt_rx
                    .try_recv()
                    .unwrap()
                    .dispatch(|value| value),
                test_system_event()
            );

            get_system_command_sender()
                .send(test_system_command())
                .unwrap();
            assert_eq!(
                runner
                    .channels
                    .system_cmd_rx
                    .try_recv()
                    .unwrap()
                    .dispatch(|value| value),
                test_system_command()
            );

            get_data_event_sender()
                .send(DataEvent::Data(Data::Quote(test_quote())))
                .unwrap();
            assert!(runner.channels.data_evt_rx.try_recv().is_ok());

            let account = AccountState::new(
                AccountId::from("SIM-001"),
                AccountType::Cash,
                vec![],
                vec![],
                true,
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
                None,
            );
            get_exec_event_sender()
                .send(ExecutionEvent::Account(account))
                .unwrap();
            assert!(runner.channels.exec_evt_rx.try_recv().is_ok());
        })
        .join()
        .unwrap();
    }

    #[cfg(feature = "node")]
    #[rstest]
    fn test_drain_pending_system_events_keeps_data_events_separate() {
        std::thread::spawn(|| {
            let mut runner = AsyncRunner::new();
            runner.bind_senders();
            let system_event = test_system_event();

            get_system_event_sender().send(system_event).unwrap();
            get_data_event_sender()
                .send(DataEvent::Data(Data::Quote(test_quote())))
                .unwrap();

            let system_events = runner
                .drain_pending_system_events()
                .into_iter()
                .map(|message| message.dispatch(|value| value))
                .collect::<Vec<_>>();

            assert_eq!(system_events, vec![system_event]);
            assert!(runner.channels.system_evt_rx.try_recv().is_err());
            assert!(runner.channels.data_evt_rx.try_recv().is_ok());
        })
        .join()
        .unwrap();
    }

    #[cfg(feature = "node")]
    #[rstest]
    fn test_drain_pending_system_commands_keeps_events_separate() {
        std::thread::spawn(|| {
            let mut runner = AsyncRunner::new();
            runner.bind_senders();
            let system_command = test_system_command();

            get_system_command_sender().send(system_command).unwrap();
            get_system_event_sender().send(test_system_event()).unwrap();

            let system_commands = runner
                .drain_pending_system_commands()
                .into_iter()
                .map(|message| message.dispatch(|value| value))
                .collect::<Vec<_>>();

            assert_eq!(system_commands, vec![system_command]);
            assert!(runner.channels.system_cmd_rx.try_recv().is_err());
            assert!(runner.channels.system_evt_rx.try_recv().is_ok());
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_bind_senders_reclaims_tls_from_previous_runner() {
        std::thread::spawn(|| {
            let mut runner1 = AsyncRunner::new();
            runner1.bind_senders();

            let mut runner2 = AsyncRunner::new();
            runner2.bind_senders();

            get_data_cmd_sender().execute(DataCommand::Subscribe(SubscribeCommand::Data(
                SubscribeCustomData {
                    client_id: Some(ClientId::from("TEST")),
                    venue: None,
                    data_type: DataType::new("test", None, None),
                    command_id: UUID4::new(),
                    ts_init: UnixNanos::default(),
                    correlation_id: None,
                    params: None,
                },
            )));

            assert!(runner2.channels.data_cmd_rx.try_recv().is_ok());
            assert!(runner1.channels.data_cmd_rx.try_recv().is_err());
        })
        .join()
        .unwrap();
    }

    #[tokio::test]
    async fn test_execution_event_order_submitted_batch_channel() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();

        let events = vec![
            OrderSubmittedSpec::builder()
                .client_order_id(ClientOrderId::from("O-001"))
                .build(),
            OrderSubmittedSpec::builder()
                .client_order_id(ClientOrderId::from("O-002"))
                .build(),
        ];

        let batch = OrderSubmittedBatch::new(events);
        tx.send(ExecutionEvent::OrderSubmittedBatch(batch)).unwrap();

        let received = rx.recv().await.unwrap();
        match received {
            ExecutionEvent::OrderSubmittedBatch(b) => {
                assert_eq!(b.len(), 2);
                assert_eq!(b.events[0].client_order_id, ClientOrderId::from("O-001"));
                assert_eq!(b.events[1].client_order_id, ClientOrderId::from("O-002"));
            }
            _ => panic!("Expected OrderSubmittedBatch event"),
        }
    }

    #[tokio::test]
    async fn test_execution_event_order_accepted_batch_channel() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();

        let events = vec![
            OrderAcceptedSpec::builder()
                .client_order_id(ClientOrderId::from("O-001"))
                .build(),
            OrderAcceptedSpec::builder()
                .client_order_id(ClientOrderId::from("O-002"))
                .build(),
        ];

        let batch = OrderAcceptedBatch::new(events);
        tx.send(ExecutionEvent::OrderAcceptedBatch(batch)).unwrap();

        let received = rx.recv().await.unwrap();
        match received {
            ExecutionEvent::OrderAcceptedBatch(b) => {
                assert_eq!(b.len(), 2);
                assert_eq!(b.events[0].client_order_id, ClientOrderId::from("O-001"));
                assert_eq!(b.events[1].client_order_id, ClientOrderId::from("O-002"));
            }
            _ => panic!("Expected OrderAcceptedBatch event"),
        }
    }

    #[tokio::test]
    async fn test_execution_event_order_canceled_batch_channel() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();

        let events = vec![
            OrderCanceledSpec::builder()
                .client_order_id(ClientOrderId::from("O-001"))
                .build(),
            OrderCanceledSpec::builder()
                .client_order_id(ClientOrderId::from("O-002"))
                .build(),
        ];

        let batch = OrderCanceledBatch::new(events);
        tx.send(ExecutionEvent::OrderCanceledBatch(batch)).unwrap();

        let received = rx.recv().await.unwrap();
        match received {
            ExecutionEvent::OrderCanceledBatch(b) => {
                assert_eq!(b.len(), 2);
                assert_eq!(b.events[0].client_order_id, ClientOrderId::from("O-001"));
                assert_eq!(b.events[1].client_order_id, ClientOrderId::from("O-002"));
            }
            _ => panic!("Expected OrderCanceledBatch event"),
        }
    }
}
