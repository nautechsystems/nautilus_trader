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
//! `AsyncRunner` owns the seven public tokio mpsc channel pairs, an internal
//! sourced-execution sidecar pair, and a shutdown signal channel. Construction creates the channels without side
//! effects. The sender halves are placed into thread-local storage
//! via [`AsyncRunner::bind_senders`] so that adapters and engine
//! components can resolve them through the `get_*_sender()` accessors
//! in `nautilus_common::runner` and `nautilus_common::live::runner`.
//!
//! Public channel pairs:
//!
//! - **Time events**: timer callbacks dispatched by the clock.
//! - **System events**: system notifications handled by the live node.
//! - **System commands**: control requests handled by the live node.
//! - **Execution events**: fills, order updates, and account state from
//!   execution clients to the execution engine. Opted-in clients use a private
//!   source-bound sidecar lane; legacy clients retain the public execution channel.
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
//!   phases. Use [`AsyncRunner::handle_next_exec_event`] for the execution
//!   branch so both legacy and sourced execution lanes remain serviceable.
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
//! - The legacy and sourced execution lanes each preserve FIFO order. Fair
//!   arbitration prevents either ready lane from starving, but no global order
//!   is defined across the two lanes.

use std::{
    cell::RefCell,
    fmt::Debug,
    future::poll_fn,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use nautilus_common::{
    live::runner::{
        get_exec_event_sender, replace_data_event_sender, replace_exec_event_sender,
        replace_system_command_sender, replace_system_event_sender,
    },
    messages::{
        DataEvent, ExecutionEvent, ExecutionReport, SystemCommand, SystemEvent,
        data::DataCommand,
        execution::{SourcedExecutionReport, TradingCommand},
    },
    msgbus::{self, MessagingSwitchboard},
    runner::{
        DataCommandSender, TimeEventMessage, TimeEventSender, TradingCommandMessage,
        TradingCommandSender, replace_data_cmd_sender, replace_exec_cmd_sender,
        replace_time_event_sender,
    },
};
use nautilus_model::{
    events::{AccountState, OrderEventAny},
    identifiers::ClientId,
};

thread_local! {
    static SOURCED_EXEC_EVENT_SENDER: RefCell<Option<tokio::sync::mpsc::UnboundedSender<SourcedExecutionEvent>>> = const { RefCell::new(None) };
    static INTEGRATED_SOURCED_EXEC_INGRESS: RefCell<Option<SourcedExecutionIngress>> = const { RefCell::new(None) };
}

/// An execution-event sender permanently bound to one execution client.
///
/// Sending through this sink uses only the sourced ingress lane. A closed sourced lane returns
/// the original event to the caller and never falls back to the legacy lane.
#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct SourcedExecutionEventSink {
    client_id: ClientId,
    sender: tokio::sync::mpsc::UnboundedSender<SourcedExecutionEvent>,
    public_exec_sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
}

impl SourcedExecutionEventSink {
    pub(crate) fn new(
        client_id: ClientId,
        sender: tokio::sync::mpsc::UnboundedSender<SourcedExecutionEvent>,
        public_exec_sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
    ) -> Self {
        Self {
            client_id,
            sender,
            public_exec_sender,
        }
    }

    /// Sends an execution event through the sourced ingress lane.
    ///
    /// # Errors
    ///
    /// Returns the original event when the sourced ingress or paired public execution receiver
    /// is closed.
    pub(crate) fn send(
        &self,
        event: ExecutionEvent,
    ) -> Result<(), Box<tokio::sync::mpsc::error::SendError<ExecutionEvent>>> {
        self.send_with_purpose(SourcedExecutionEvent::runtime(self.client_id, event))
    }

    fn send_with_purpose(
        &self,
        event: SourcedExecutionEvent,
    ) -> Result<(), Box<tokio::sync::mpsc::error::SendError<ExecutionEvent>>> {
        if self.public_exec_sender.is_closed() {
            let (_, event) = event.into_parts();
            return Err(Box::new(tokio::sync::mpsc::error::SendError(event)));
        }
        self.sender.send(event).map_err(|e| {
            let (_, event) = e.0.into_parts();
            Box::new(tokio::sync::mpsc::error::SendError(event))
        })
    }

    /// Sends a source-bound startup account barrier through the sourced ingress lane.
    ///
    /// # Errors
    ///
    /// Returns the original account event when the sourced ingress or paired public execution
    /// receiver is closed.
    pub(crate) fn send_bootstrap_account(
        &self,
        state: AccountState,
    ) -> Result<(), Box<tokio::sync::mpsc::error::SendError<ExecutionEvent>>> {
        self.send_with_purpose(SourcedExecutionEvent::bootstrap_account(
            self.client_id,
            state,
        ))
    }
}

/// Returns a sourced execution-event sink bound to `client_id` for the current thread.
///
/// # Panics
///
/// Panics if no async runner has bound the sourced ingress sender on this thread.
#[must_use]
#[doc(hidden)]
pub fn get_sourced_exec_event_sink(client_id: ClientId) -> SourcedExecutionEventSink {
    SOURCED_EXEC_EVENT_SENDER.with(|sender| {
        SourcedExecutionEventSink::new(
            client_id,
            sender
                .borrow()
                .as_ref()
                .expect("sourced execution-event sender is not bound")
                .clone(),
            get_exec_event_sender(),
        )
    })
}

fn replace_sourced_exec_event_sender(
    sender: tokio::sync::mpsc::UnboundedSender<SourcedExecutionEvent>,
) {
    SOURCED_EXEC_EVENT_SENDER.with(|slot| {
        *slot.borrow_mut() = Some(sender);
    });
}

#[allow(
    clippy::large_enum_variant,
    reason = "sourced runtime events are consumed immediately; boxing would add routing allocations"
)]
#[derive(Debug)]
pub(crate) enum SourcedExecutionEvent {
    Runtime {
        client_id: ClientId,
        event: ExecutionEvent,
    },
    BootstrapAccount {
        client_id: ClientId,
        state: AccountState,
    },
}

impl SourcedExecutionEvent {
    pub(crate) const fn runtime(client_id: ClientId, event: ExecutionEvent) -> Self {
        Self::Runtime { client_id, event }
    }

    pub(crate) const fn bootstrap_account(client_id: ClientId, state: AccountState) -> Self {
        Self::BootstrapAccount { client_id, state }
    }

    pub(crate) fn into_parts(self) -> (ClientId, ExecutionEvent) {
        match self {
            Self::Runtime { client_id, event } => (client_id, event),
            Self::BootstrapAccount { client_id, state } => {
                (client_id, ExecutionEvent::Account(state))
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum ExecutionEventIngress {
    Legacy(ExecutionEvent),
    Sourced(SourcedExecutionEvent),
}

#[derive(Debug)]
pub(crate) struct SourcedExecutionIngress {
    receiver: tokio::sync::mpsc::UnboundedReceiver<SourcedExecutionEvent>,
    prefer_sourced: bool,
}

impl SourcedExecutionIngress {
    pub(crate) const fn new(
        receiver: tokio::sync::mpsc::UnboundedReceiver<SourcedExecutionEvent>,
    ) -> Self {
        Self {
            receiver,
            prefer_sourced: false,
        }
    }

    #[cfg(any(test, feature = "node"))]
    pub(crate) fn len(&self) -> usize {
        self.receiver.len()
    }

    /// Receives fairly from the independent legacy and sourced execution lanes.
    ///
    /// Arbitration preserves FIFO within each lane and guarantees bounded progress when both
    /// lanes remain ready, without defining a global order across lanes.
    pub(crate) async fn recv(
        &mut self,
        legacy_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> Option<ExecutionEventIngress> {
        poll_fn(|cx| self.poll_recv(cx, legacy_rx)).await
    }

    fn poll_recv(
        &mut self,
        cx: &mut Context<'_>,
        legacy_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> Poll<Option<ExecutionEventIngress>> {
        if legacy_rx.is_closed() {
            self.receiver.close();
        }

        if self.prefer_sourced {
            let sourced_closed = match Pin::new(&mut self.receiver).poll_recv(cx) {
                Poll::Ready(Some(event)) => {
                    return self.ready(ExecutionEventIngress::Sourced(event));
                }
                Poll::Ready(None) => true,
                Poll::Pending => false,
            };
            let legacy_closed = match Pin::new(legacy_rx).poll_recv(cx) {
                Poll::Ready(Some(event)) => {
                    return self.ready(ExecutionEventIngress::Legacy(event));
                }
                Poll::Ready(None) => true,
                Poll::Pending => false,
            };

            return if sourced_closed && legacy_closed {
                Poll::Ready(None)
            } else {
                Poll::Pending
            };
        }

        let legacy_closed = match Pin::new(&mut *legacy_rx).poll_recv(cx) {
            Poll::Ready(Some(event)) => {
                return self.ready(ExecutionEventIngress::Legacy(event));
            }
            Poll::Ready(None) => true,
            Poll::Pending => false,
        };
        let sourced_closed = match Pin::new(&mut self.receiver).poll_recv(cx) {
            Poll::Ready(Some(event)) => {
                return self.ready(ExecutionEventIngress::Sourced(event));
            }
            Poll::Ready(None) => true,
            Poll::Pending => false,
        };

        if legacy_closed && sourced_closed {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }

    fn ready(&mut self, ingress: ExecutionEventIngress) -> Poll<Option<ExecutionEventIngress>> {
        self.prefer_sourced = matches!(&ingress, ExecutionEventIngress::Legacy(_));
        Poll::Ready(Some(ingress))
    }

    #[cfg(any(test, feature = "node"))]
    pub(crate) fn try_recv(
        &mut self,
        legacy_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> Option<ExecutionEventIngress> {
        let ingress = if self.prefer_sourced {
            self.receiver
                .try_recv()
                .map(ExecutionEventIngress::Sourced)
                .or_else(|_| legacy_rx.try_recv().map(ExecutionEventIngress::Legacy))
                .ok()
        } else {
            legacy_rx
                .try_recv()
                .map(ExecutionEventIngress::Legacy)
                .or_else(|_| self.receiver.try_recv().map(ExecutionEventIngress::Sourced))
                .ok()
        };

        if let Some(ingress) = &ingress {
            self.prefer_sourced = matches!(ingress, ExecutionEventIngress::Legacy(_));
        }

        ingress
    }
}

fn replace_integrated_sourced_exec_ingress(ingress: Option<SourcedExecutionIngress>) {
    INTEGRATED_SOURCED_EXEC_INGRESS.with(|slot| {
        *slot.borrow_mut() = ingress;
    });
}

/// Asynchronous implementation of `DataCommandSender` for live environments.
#[derive(Debug)]
pub struct AsyncDataCommandSender {
    cmd_tx: tokio::sync::mpsc::UnboundedSender<DataCommand>,
}

impl AsyncDataCommandSender {
    #[must_use]
    pub const fn new(cmd_tx: tokio::sync::mpsc::UnboundedSender<DataCommand>) -> Self {
        Self { cmd_tx }
    }
}

impl DataCommandSender for AsyncDataCommandSender {
    fn execute(&self, command: DataCommand) {
        if let Err(e) = self.cmd_tx.send(command) {
            log::error!("Failed to send data command: {e}");
        }
    }
}

/// Asynchronous implementation of `TimeEventSender` for live environments.
#[derive(Debug, Clone)]
pub struct AsyncTimeEventSender {
    time_tx: tokio::sync::mpsc::UnboundedSender<TimeEventMessage>,
}

impl AsyncTimeEventSender {
    #[must_use]
    pub const fn new(time_tx: tokio::sync::mpsc::UnboundedSender<TimeEventMessage>) -> Self {
        Self { time_tx }
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
    cmd_tx: tokio::sync::mpsc::UnboundedSender<TradingCommandMessage>,
}

impl AsyncTradingCommandSender {
    #[must_use]
    pub const fn new(cmd_tx: tokio::sync::mpsc::UnboundedSender<TradingCommandMessage>) -> Self {
        Self { cmd_tx }
    }
}

impl TradingCommandSender for AsyncTradingCommandSender {
    fn execute(&self, message: TradingCommandMessage) {
        if let Err(e) = self.cmd_tx.send(message) {
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
    pub time_evt_rx: tokio::sync::mpsc::UnboundedReceiver<TimeEventMessage>,
    pub system_evt_rx: tokio::sync::mpsc::UnboundedReceiver<SystemEvent>,
    pub system_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SystemCommand>,
    pub exec_evt_rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    pub exec_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<TradingCommandMessage>,
    pub data_evt_rx: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    pub data_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DataCommand>,
}

#[cfg(feature = "node")]
#[allow(
    clippy::large_enum_variant,
    reason = "runner events are consumed immediately; boxing would add routing allocations"
)]
pub(crate) enum PendingRunnerEvent {
    TimeEvent(TimeEventMessage),
    SystemEvent(SystemEvent),
    SystemCommand(SystemCommand),
    ExecEvent(ExecutionEvent),
    SourcedExecEvent(SourcedExecutionEvent),
    ExecCommand(TradingCommandMessage),
    DataEvent(DataEvent),
    DataCommand(DataCommand),
}

pub struct AsyncRunner {
    channels: AsyncRunnerChannels,
    time_evt_tx: tokio::sync::mpsc::UnboundedSender<TimeEventMessage>,
    system_evt_tx: tokio::sync::mpsc::UnboundedSender<SystemEvent>,
    system_cmd_tx: tokio::sync::mpsc::UnboundedSender<SystemCommand>,
    signal_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    signal_tx: tokio::sync::mpsc::UnboundedSender<()>,
    exec_evt_tx: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
    sourced_exec_ingress: SourcedExecutionIngress,
    sourced_exec_evt_tx: tokio::sync::mpsc::UnboundedSender<SourcedExecutionEvent>,
    exec_cmd_tx: tokio::sync::mpsc::UnboundedSender<TradingCommandMessage>,
    data_evt_tx: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    data_cmd_tx: tokio::sync::mpsc::UnboundedSender<DataCommand>,
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
    pub fn new() -> Self {
        use tokio::sync::mpsc::unbounded_channel; // tokio-import-ok

        let (time_evt_tx, time_evt_rx) = unbounded_channel::<TimeEventMessage>();
        let (system_evt_tx, system_evt_rx) = unbounded_channel::<SystemEvent>();
        let (system_cmd_tx, system_cmd_rx) = unbounded_channel::<SystemCommand>();
        let (signal_tx, signal_rx) = unbounded_channel::<()>();
        let (exec_evt_tx, exec_evt_rx) = unbounded_channel::<ExecutionEvent>();
        let (sourced_exec_evt_tx, sourced_exec_evt_rx) =
            unbounded_channel::<SourcedExecutionEvent>();
        let (exec_cmd_tx, exec_cmd_rx) = unbounded_channel::<TradingCommandMessage>();
        let (data_evt_tx, data_evt_rx) = unbounded_channel::<DataEvent>();
        let (data_cmd_tx, data_cmd_rx) = unbounded_channel::<DataCommand>();

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
            sourced_exec_ingress: SourcedExecutionIngress::new(sourced_exec_evt_rx),
            sourced_exec_evt_tx,
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
        replace_integrated_sourced_exec_ingress(None);
        replace_time_event_sender(Arc::new(AsyncTimeEventSender::new(
            self.time_evt_tx.clone(),
        )));
        replace_system_event_sender(self.system_evt_tx.clone());
        replace_system_command_sender(self.system_cmd_tx.clone());
        replace_exec_event_sender(self.exec_evt_tx.clone());
        replace_sourced_exec_event_sender(self.sourced_exec_evt_tx.clone());
        replace_exec_cmd_sender(Arc::new(AsyncTradingCommandSender::new(
            self.exec_cmd_tx.clone(),
        )));
        replace_data_event_sender(self.data_evt_tx.clone());
        replace_data_cmd_sender(Arc::new(AsyncDataCommandSender::new(
            self.data_cmd_tx.clone(),
        )));
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
    /// endpoints (which use thread-local storage). Drive `exec_evt_rx` through
    /// [`Self::handle_next_exec_event`] so the private sourced lane is also serviced.
    #[must_use]
    pub fn take_channels(self) -> AsyncRunnerChannels {
        let Self {
            channels,
            sourced_exec_ingress,
            ..
        } = self;
        replace_integrated_sourced_exec_ingress(Some(sourced_exec_ingress));
        channels
    }

    /// Receives and handles the next execution event for an integrated runner.
    ///
    /// This fairly services both the public legacy receiver and the private sourced sidecar
    /// installed by [`Self::take_channels`]. Poll this future on the same thread that extracted
    /// the channels so the thread-local sourced ingress and message bus remain aligned.
    ///
    /// Returns `false` after both execution lanes close.
    pub async fn handle_next_exec_event(
        exec_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> bool {
        let ingress = poll_fn(|cx| {
            INTEGRATED_SOURCED_EXEC_INGRESS.with(|slot| {
                let mut slot = slot.borrow_mut();
                if let Some(sourced) = slot.as_mut() {
                    sourced.poll_recv(cx, exec_evt_rx)
                } else {
                    match Pin::new(&mut *exec_evt_rx).poll_recv(cx) {
                        Poll::Ready(Some(event)) => {
                            Poll::Ready(Some(ExecutionEventIngress::Legacy(event)))
                        }
                        Poll::Ready(None) => Poll::Ready(None),
                        Poll::Pending => Poll::Pending,
                    }
                }
            })
        })
        .await;

        match ingress {
            Some(ExecutionEventIngress::Legacy(event)) => Self::handle_exec_event(event),
            Some(ExecutionEventIngress::Sourced(event)) => Self::handle_sourced_exec_event(event),
            None => return false,
        }
        true
    }

    /// Consumes the runner and returns its public channels plus the internal sourced sidecar.
    #[must_use]
    #[cfg(any(test, feature = "node"))]
    pub(crate) fn take_channels_with_sourced(
        self,
    ) -> (AsyncRunnerChannels, SourcedExecutionIngress) {
        (self.channels, self.sourced_exec_ingress)
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
                Self::handle_data_event(evt);
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
    pub(crate) fn drain_pending_system_events(&mut self) -> Vec<SystemEvent> {
        let mut events = Vec::new();

        while let Ok(event) = self.channels.system_evt_rx.try_recv() {
            events.push(event);
        }

        events
    }

    #[cfg(feature = "node")]
    pub(crate) fn drain_pending_system_commands(&mut self) -> Vec<SystemCommand> {
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
    pub async fn run(&mut self) {
        self.bind_senders();

        log::info!("AsyncRunner starting");

        loop {
            tokio::select! {
                biased;

                Some(()) = self.signal_rx.recv() => {
                    log::info!("AsyncRunner received signal, shutting down");
                    return;
                },
                Some(handler) = self.channels.time_evt_rx.recv() => {
                    let _ = Self::handle_time_event(handler);
                },
                Some(event) = self.channels.system_evt_rx.recv() => {
                    log::error!("System event {event:?} requires the LiveNode runner");
                },
                Some(command) = self.channels.system_cmd_rx.recv() => {
                    log::error!("System command {command:?} requires the LiveNode runner");
                },
                Some(ingress) = self.sourced_exec_ingress.recv(
                    &mut self.channels.exec_evt_rx,
                ) => {
                    match ingress {
                        ExecutionEventIngress::Legacy(event) => Self::handle_exec_event(event),
                        ExecutionEventIngress::Sourced(event) => {
                            Self::handle_sourced_exec_event(event);
                        }
                    }
                },
                Some(cmd) = self.channels.exec_cmd_rx.recv() => {
                    Self::handle_trading_command(cmd);
                },
                Some(evt) = self.channels.data_evt_rx.recv() => {
                    Self::handle_data_event(evt);
                },
                Some(cmd) = self.channels.data_cmd_rx.recv() => {
                    Self::handle_data_command(cmd);
                },
                else => {
                    log::debug!("AsyncRunner all channels closed, exiting");
                    return;
                }
            };
        }
    }

    /// Handles a time event by running its callback.
    #[inline]
    #[must_use]
    pub fn handle_time_event(message: TimeEventMessage) -> bool {
        message.dispatch()
    }

    /// Handles a data command by sending to the `DataEngine`.
    #[inline]
    pub fn handle_data_command(cmd: DataCommand) {
        msgbus::send_data_command(MessagingSwitchboard::data_engine_execute(), cmd);
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

    /// Dispatches a deferred trading command to its direct endpoint.
    #[inline]
    pub fn handle_trading_command(message: TradingCommandMessage) {
        let mut messages = vec![message];
        while let Some(message) = messages.pop() {
            messages.extend(message.dispatch().into_iter().rev());
        }
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

    /// Handles an execution event from a source-bound ingress lane.
    ///
    /// Only order and fill reports use strict source-aware reconciliation. Every other event
    /// retains its existing legacy dispatch semantics without being requeued.
    #[inline]
    pub(crate) fn handle_sourced_exec_event(event: SourcedExecutionEvent) {
        let (client_id, event) = event.into_parts();
        match event {
            ExecutionEvent::Report(
                report @ (ExecutionReport::Order(_) | ExecutionReport::Fill(_)),
            ) => {
                Self::handle_sourced_exec_report(client_id, report);
            }
            event => Self::handle_exec_event(event),
        }
    }

    #[inline]
    fn handle_sourced_exec_report(client_id: ClientId, report: ExecutionReport) {
        let endpoint = MessagingSwitchboard::exec_engine_reconcile_sourced_execution_report();
        msgbus::send_sourced_execution_report(
            endpoint,
            SourcedExecutionReport::new(client_id, report),
        );
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
            self.sourced_exec_ingress.len(),
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
        processed += poll_execution_channels(
            &mut self.channels.exec_evt_rx,
            &mut self.sourced_exec_ingress,
            pending.3,
            pending.4,
            &mut process,
        );
        processed += poll_channel(
            &mut self.channels.exec_cmd_rx,
            pending.5,
            PendingRunnerEvent::ExecCommand,
            &mut process,
        );
        processed += poll_channel(
            &mut self.channels.data_evt_rx,
            pending.6,
            PendingRunnerEvent::DataEvent,
            &mut process,
        );
        processed += poll_channel(
            &mut self.channels.data_cmd_rx,
            pending.7,
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
            Some(ingress) = self.sourced_exec_ingress.recv(
                &mut self.channels.exec_evt_rx,
            ) => {
                match ingress {
                    ExecutionEventIngress::Legacy(event) => {
                        Some(PendingRunnerEvent::ExecEvent(event))
                    }
                    ExecutionEventIngress::Sourced(event) => {
                        Some(PendingRunnerEvent::SourcedExecEvent(event))
                    }
                }
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
fn poll_execution_channels(
    legacy_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    sourced: &mut SourcedExecutionIngress,
    legacy_pending: usize,
    sourced_pending: usize,
    process: &mut impl FnMut(PendingRunnerEvent),
) -> usize {
    let mut processed = 0;
    let mut legacy_remaining = legacy_pending;
    let mut sourced_remaining = sourced_pending;

    while legacy_remaining > 0 || sourced_remaining > 0 {
        let ingress = if sourced.prefer_sourced {
            let sourced = (sourced_remaining > 0)
                .then(|| sourced.receiver.try_recv().ok())
                .flatten()
                .map(ExecutionEventIngress::Sourced);
            sourced.or_else(|| {
                (legacy_remaining > 0)
                    .then(|| legacy_rx.try_recv().ok())
                    .flatten()
                    .map(ExecutionEventIngress::Legacy)
            })
        } else {
            let legacy = (legacy_remaining > 0)
                .then(|| legacy_rx.try_recv().ok())
                .flatten()
                .map(ExecutionEventIngress::Legacy);
            legacy.or_else(|| {
                (sourced_remaining > 0)
                    .then(|| sourced.receiver.try_recv().ok())
                    .flatten()
                    .map(ExecutionEventIngress::Sourced)
            })
        };
        let Some(ingress) = ingress else {
            break;
        };

        let event = match ingress {
            ExecutionEventIngress::Legacy(event) => {
                legacy_remaining -= 1;
                sourced.prefer_sourced = true;
                PendingRunnerEvent::ExecEvent(event)
            }
            ExecutionEventIngress::Sourced(event) => {
                sourced_remaining -= 1;
                sourced.prefer_sourced = false;
                PendingRunnerEvent::SourcedExecEvent(event)
            }
        };
        process(event);
        processed += 1;
    }

    processed
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
    use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        live::runner::{
            get_data_event_sender, get_exec_event_sender, get_system_command_sender,
            get_system_event_sender, try_get_system_command_sender, try_get_system_event_sender,
        },
        messages::{
            ExecutionEvent, ExecutionReport,
            data::{SubscribeCommand, SubscribeCustomData},
            execution::{CancelAllOrders, TradingCommand},
            system::{ReconnectSocket, SocketState, SocketStateChange},
        },
        msgbus::{TypedHandler, TypedIntoHandler, stubs::get_typed_into_message_saving_handler},
        runner::{
            TimeEventMessage, get_data_cmd_sender, get_time_event_sender, get_trading_cmd_sender,
            replace_exec_cmd_sender, try_get_time_event_sender, try_get_trading_cmd_sender,
        },
        timer::{TimeEvent, TimeEventCallback},
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_execution::engine::{ExecutionEngine, stubs::StubExecutionClient};
    use nautilus_model::{
        data::{Data, DataType, quote::QuoteTick},
        enums::{
            AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, PositionSide,
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
        instruments::{Instrument, stubs::audusd_sim},
        reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
        types::{Money, Price, Quantity},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

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
        time_evt_rx: tokio::sync::mpsc::UnboundedReceiver<TimeEventMessage>,
        data_evt_rx: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        data_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DataCommand>,
        exec_evt_rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        exec_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<TradingCommandMessage>,
        signal_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
        signal_tx: tokio::sync::mpsc::UnboundedSender<()>,
    ) -> AsyncRunner {
        let (time_evt_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (system_evt_tx, system_evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let (system_cmd_tx, system_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (data_evt_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (data_cmd_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (exec_evt_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (sourced_exec_evt_tx, sourced_exec_evt_rx) = tokio::sync::mpsc::unbounded_channel();
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
            sourced_exec_ingress: SourcedExecutionIngress::new(sourced_exec_evt_rx),
            sourced_exec_evt_tx,
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
            .send(TimeEventMessage::new(
                time_event,
                TimeEventCallback::from(|_: TimeEvent| {}),
            ))
            .unwrap();
        exec_evt_tx
            .send(ExecutionEvent::Order(OrderEventAny::Submitted(
                OrderSubmittedSpec::builder()
                    .client_order_id(ClientOrderId::from("O-POLL-001"))
                    .build(),
            )))
            .unwrap();
        exec_cmd_tx
            .send(TradingCommandMessage::new(
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
            ))
            .unwrap();
        data_evt_tx
            .send(DataEvent::Data(Data::Quote(test_quote())))
            .unwrap();
        data_cmd_tx
            .send(DataCommand::Subscribe(SubscribeCommand::Data(
                SubscribeCustomData {
                    client_id: Some(ClientId::from("POLL")),
                    venue: None,
                    data_type: DataType::new("QuoteTick", None, None),
                    command_id: UUID4::new(),
                    ts_init: UnixNanos::from(4),
                    correlation_id: None,
                    params: None,
                },
            )))
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
        runner
            .sourced_exec_evt_tx
            .send(SourcedExecutionEvent::runtime(
                ClientId::from("SOURCE-POLL"),
                ExecutionEvent::Order(OrderEventAny::Submitted(
                    OrderSubmittedSpec::builder()
                        .client_order_id(ClientOrderId::from("O-SOURCED-POLL-001"))
                        .build(),
                )),
            ))
            .unwrap();
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
            PendingRunnerEvent::SourcedExecEvent(_) => {
                processed_by_channel[3] += 1;
                processed_order.push("sourced_exec_event");
            }
            PendingRunnerEvent::ExecCommand(_) => {
                processed_by_channel[4] += 1;
                processed_order.push("exec_command");
            }
            PendingRunnerEvent::DataEvent(_) => {
                processed_by_channel[5] += 1;
                processed_order.push("data_event");
                data_evt_tx
                    .send(DataEvent::Data(Data::Quote(test_quote())))
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

        assert_eq!(first, 9);
        assert_eq!(second, 1);
        assert_eq!(processed_by_channel, [1, 2, 1, 2, 1, 2, 1]);
        assert_eq!(
            &processed_order[..4],
            ["time", "system_event", "system_event", "system_command"]
        );
        let mut execution_lanes = processed_order[4..6].to_vec();
        execution_lanes.sort_unstable();
        assert_eq!(execution_lanes, ["exec_event", "sourced_exec_event"]);
        assert_eq!(
            &processed_order[6..],
            ["exec_command", "data_event", "data_command", "data_event"]
        );
    }

    #[cfg(feature = "node")]
    #[rstest]
    fn test_poll_pending_new_sourced_event_does_not_displace_entry_legacy_events() {
        let mut runner = AsyncRunner::new();
        let sourced_tx = runner.sourced_exec_evt_tx.clone();
        for client_order_id in ["O-LEGACY-SNAPSHOT-001", "O-LEGACY-SNAPSHOT-002"] {
            runner
                .exec_evt_tx
                .send(ExecutionEvent::Order(OrderEventAny::Submitted(
                    OrderSubmittedSpec::builder()
                        .client_order_id(ClientOrderId::from(client_order_id))
                        .build(),
                )))
                .unwrap();
        }

        let mut legacy_seen = 0;
        let first = runner.poll_pending(|event| match event {
            PendingRunnerEvent::ExecEvent(_) => {
                legacy_seen += 1;
                if legacy_seen == 1 {
                    sourced_tx
                        .send(SourcedExecutionEvent::runtime(
                            ClientId::from("SOURCE-REENTRANT"),
                            ExecutionEvent::Order(OrderEventAny::Submitted(
                                OrderSubmittedSpec::builder()
                                    .client_order_id(ClientOrderId::from("O-SOURCED-REENTRANT"))
                                    .build(),
                            )),
                        ))
                        .unwrap();
                }
            }
            _ => panic!("Only entry-snapshot legacy events should be processed"),
        });

        assert_eq!(first, 2);
        assert_eq!(legacy_seen, 2);
        assert_eq!(runner.sourced_exec_ingress.len(), 1);
        assert_eq!(
            runner.poll_pending(|event| {
                assert!(matches!(event, PendingRunnerEvent::SourcedExecEvent(_)));
            }),
            1
        );
    }

    #[cfg(feature = "node")]
    #[rstest]
    fn test_poll_pending_new_legacy_event_does_not_displace_entry_sourced_events() {
        let mut runner = AsyncRunner::new();
        let legacy_tx = runner.exec_evt_tx.clone();
        for client_order_id in ["O-SOURCED-SNAPSHOT-001", "O-SOURCED-SNAPSHOT-002"] {
            runner
                .sourced_exec_evt_tx
                .send(SourcedExecutionEvent::runtime(
                    ClientId::from("SOURCE-SNAPSHOT"),
                    ExecutionEvent::Order(OrderEventAny::Submitted(
                        OrderSubmittedSpec::builder()
                            .client_order_id(ClientOrderId::from(client_order_id))
                            .build(),
                    )),
                ))
                .unwrap();
        }

        let mut sourced_seen = 0;
        let first = runner.poll_pending(|event| match event {
            PendingRunnerEvent::SourcedExecEvent(_) => {
                sourced_seen += 1;
                if sourced_seen == 1 {
                    legacy_tx
                        .send(ExecutionEvent::Order(OrderEventAny::Submitted(
                            OrderSubmittedSpec::builder()
                                .client_order_id(ClientOrderId::from("O-LEGACY-REENTRANT"))
                                .build(),
                        )))
                        .unwrap();
                }
            }
            _ => panic!("Only entry-snapshot sourced events should be processed"),
        });

        assert_eq!(first, 2);
        assert_eq!(sourced_seen, 2);
        assert_eq!(runner.channels.exec_evt_rx.len(), 1);
        assert_eq!(
            runner.poll_pending(|event| {
                assert!(matches!(event, PendingRunnerEvent::ExecEvent(_)));
            }),
            1
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

        runner.system_cmd_tx.send(test_system_command()).unwrap();
        runner.system_evt_tx.send(test_system_event()).unwrap();

        assert!(matches!(
            runner.recv().await,
            Some(PendingRunnerEvent::SystemEvent(_))
        ));
        assert!(matches!(
            runner.recv().await,
            Some(PendingRunnerEvent::SystemCommand(_))
        ));
    }

    #[cfg(feature = "node")]
    #[tokio::test]
    async fn test_recv_fairly_services_ready_execution_lanes_without_starvation() {
        let (_time_evt_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_data_evt_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_data_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel();
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

        for index in 0..2 {
            exec_evt_tx
                .send(ExecutionEvent::Order(OrderEventAny::Submitted(
                    OrderSubmittedSpec::builder()
                        .client_order_id(ClientOrderId::from(format!("O-LEGACY-{index}")))
                        .build(),
                )))
                .unwrap();
            runner
                .sourced_exec_evt_tx
                .send(SourcedExecutionEvent::runtime(
                    ClientId::from("SOURCE-RECV"),
                    ExecutionEvent::Order(OrderEventAny::Submitted(
                        OrderSubmittedSpec::builder()
                            .client_order_id(ClientOrderId::from(format!("O-SOURCED-{index}")))
                            .build(),
                    )),
                ))
                .unwrap();
        }

        let mut legacy_ids = Vec::new();
        let mut sourced_ids = Vec::new();
        let mut ingress_order = Vec::new();

        for _ in 0..4 {
            match runner.recv().await.unwrap() {
                PendingRunnerEvent::ExecEvent(ExecutionEvent::Order(event)) => {
                    ingress_order.push("legacy");
                    legacy_ids.push(event.client_order_id());
                }
                PendingRunnerEvent::SourcedExecEvent(SourcedExecutionEvent::Runtime {
                    event: ExecutionEvent::Order(event),
                    ..
                }) => {
                    ingress_order.push("sourced");
                    sourced_ids.push(event.client_order_id());
                }
                _ => panic!("Unexpected runner event"),
            }
        }

        assert_eq!(
            legacy_ids,
            [
                ClientOrderId::from("O-LEGACY-0"),
                ClientOrderId::from("O-LEGACY-1")
            ]
        );
        assert_eq!(
            sourced_ids,
            [
                ClientOrderId::from("O-SOURCED-0"),
                ClientOrderId::from("O-SOURCED-1")
            ]
        );
        assert_eq!(ingress_order, ["legacy", "sourced", "legacy", "sourced"]);
    }

    #[rstest]
    fn test_sourced_sink_uses_only_source_bound_lane_for_entire_event_stream() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let source_id = ClientId::from("SOURCE-ONLY");
        let sink = get_sourced_exec_event_sink(source_id);
        let account = AccountState::new(
            AccountId::from("SOURCE-ONLY-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );
        let report = PositionStatusReport::new(
            AccountId::from("SOURCE-ONLY-001"),
            InstrumentId::from("EUR/USD.SIM"),
            PositionSide::Long,
            Quantity::from(100_000),
            UnixNanos::from(3),
            UnixNanos::from(4),
            None,
            Some(PositionId::from("P-SOURCE-ONLY")),
            None,
        );

        sink.send(ExecutionEvent::Account(account)).unwrap();
        sink.send(ExecutionEvent::Report(ExecutionReport::Position(Box::new(
            report,
        ))))
        .unwrap();

        let (mut channels, mut sourced) = runner.take_channels_with_sourced();
        let first = sourced.receiver.try_recv().unwrap();
        let second = sourced.receiver.try_recv().unwrap();
        let (first_client_id, first_event) = first.into_parts();
        let (second_client_id, second_event) = second.into_parts();
        assert_eq!(sink.client_id, source_id);
        assert_eq!(first_client_id, source_id);
        assert!(matches!(first_event, ExecutionEvent::Account(_)));
        assert_eq!(second_client_id, source_id);
        assert!(matches!(
            second_event,
            ExecutionEvent::Report(ExecutionReport::Position(_))
        ));
        assert!(channels.exec_evt_rx.try_recv().is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_take_channels_services_prebound_sourced_account_events() {
        msgbus::get_message_bus().borrow_mut().dispose();
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let sink = get_sourced_exec_event_sink(ClientId::from("SOURCE-INTEGRATED"));
        let runtime_event_id = UUID4::new();
        let bootstrap_event_id = UUID4::new();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_handler = received.clone();
        msgbus::register_account_state_endpoint(
            MessagingSwitchboard::portfolio_update_account(),
            TypedHandler::from(move |account: &AccountState| {
                received_handler.borrow_mut().push(account.event_id);
            }),
        );
        let AsyncRunnerChannels {
            time_evt_rx: _,
            system_evt_rx: _,
            system_cmd_rx: _,
            mut exec_evt_rx,
            exec_cmd_rx: _,
            data_evt_rx: _,
            data_cmd_rx: _,
        } = runner.take_channels();

        sink.send(ExecutionEvent::Account(AccountState::new(
            AccountId::from("SOURCE-INTEGRATED-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            runtime_event_id,
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        )))
        .unwrap();
        sink.send_bootstrap_account(AccountState::new(
            AccountId::from("SOURCE-INTEGRATED-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            bootstrap_event_id,
            UnixNanos::from(3),
            UnixNanos::from(4),
            None,
        ))
        .unwrap();

        assert!(AsyncRunner::handle_next_exec_event(&mut exec_evt_rx).await);
        assert!(AsyncRunner::handle_next_exec_event(&mut exec_evt_rx).await);
        assert_eq!(
            received.borrow().as_slice(),
            &[runtime_event_id, bootstrap_event_id]
        );
    }

    #[rstest]
    fn test_bootstrap_send_rejects_closed_public_receiver_before_sourced_enqueue() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let sink = get_sourced_exec_event_sink(ClientId::from("SOURCE-CLOSED-PUBLIC"));
        let (mut channels, mut sourced) = runner.take_channels_with_sourced();
        channels.exec_evt_rx.close();

        let result = sink.send_bootstrap_account(AccountState::new(
            AccountId::from("SOURCE-CLOSED-PUBLIC-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        ));

        assert!(result.is_err());
        assert!(sourced.receiver.try_recv().is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_integrated_runner_dispatches_strict_reports_with_source() {
        msgbus::get_message_bus().borrow_mut().dispose();
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let source_id = ClientId::from("SOURCE-INTEGRATED-STRICT");
        let sink = get_sourced_exec_event_sink(source_id);
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_handler = received.clone();
        msgbus::register_sourced_execution_report_endpoint(
            MessagingSwitchboard::exec_engine_reconcile_sourced_execution_report(),
            TypedIntoHandler::from(move |report: SourcedExecutionReport| {
                let kind = match report.report {
                    ExecutionReport::Order(_) => "order",
                    ExecutionReport::Fill(_) => "fill",
                    _ => panic!("Unexpected non-strict execution report"),
                };
                received_handler.borrow_mut().push((report.client_id, kind));
            }),
        );
        let mut channels = runner.take_channels();
        let account_id = AccountId::from("SOURCE-INTEGRATED-STRICT-001");
        let instrument_id = InstrumentId::from("EUR/USD.SIM");
        let client_order_id = ClientOrderId::from("O-INTEGRATED-STRICT");
        let venue_order_id = VenueOrderId::from("V-INTEGRATED-STRICT");
        let order = OrderStatusReport::new(
            account_id,
            instrument_id,
            Some(client_order_id),
            venue_order_id,
            OrderSide::Buy.into(),
            OrderType::Market,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from(100_000),
            Quantity::from(0),
            UnixNanos::from(1),
            UnixNanos::from(2),
            UnixNanos::from(3),
            None,
        );
        let fill = FillReport::new(
            account_id,
            instrument_id,
            venue_order_id,
            TradeId::from("T-INTEGRATED-STRICT"),
            OrderSide::Buy,
            Quantity::from(100_000),
            Price::from("1.10000"),
            Money::from("1 USD"),
            LiquiditySide::Taker,
            Some(client_order_id),
            None,
            UnixNanos::from(4),
            UnixNanos::from(5),
            None,
        );

        sink.send(ExecutionEvent::Report(ExecutionReport::Order(Box::new(
            order,
        ))))
        .unwrap();
        sink.send(ExecutionEvent::Report(ExecutionReport::Fill(Box::new(
            fill,
        ))))
        .unwrap();

        assert!(AsyncRunner::handle_next_exec_event(&mut channels.exec_evt_rx).await);
        assert!(AsyncRunner::handle_next_exec_event(&mut channels.exec_evt_rx).await);
        assert_eq!(
            received.borrow().as_slice(),
            [(source_id, "order"), (source_id, "fill")]
        );
    }

    #[rstest]
    fn test_sourced_sink_closes_with_integrated_channels() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let sink = get_sourced_exec_event_sink(ClientId::from("SOURCE-INTEGRATED-CLOSED"));
        let channels = runner.take_channels();
        drop(channels);

        let error = sink
            .send(ExecutionEvent::Account(AccountState::new(
                AccountId::from("SOURCE-INTEGRATED-CLOSED-001"),
                AccountType::Cash,
                vec![],
                vec![],
                true,
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
                None,
            )))
            .expect_err("sourced sink must close with the public integrated channels");

        assert!(matches!(error.0, ExecutionEvent::Account(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_integrated_runner_drains_sourced_lane_before_closing() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let sink = get_sourced_exec_event_sink(ClientId::from("SOURCE-INTEGRATED-DRAIN"));
        let mut channels = runner.take_channels();

        sink.send(ExecutionEvent::Account(AccountState::new(
            AccountId::from("SOURCE-INTEGRATED-DRAIN-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        )))
        .unwrap();
        channels.exec_evt_rx.close();

        assert!(AsyncRunner::handle_next_exec_event(&mut channels.exec_evt_rx).await);
        let handled = tokio::time::timeout(
            Duration::from_millis(100),
            AsyncRunner::handle_next_exec_event(&mut channels.exec_evt_rx),
        )
        .await
        .expect("integrated sourced lane should close after draining");
        assert!(!handled);
    }

    #[rstest]
    fn test_sourced_sink_closed_lane_returns_event_without_legacy_fallback() {
        let (sourced_tx, sourced_rx) = tokio::sync::mpsc::unbounded_channel();
        let (legacy_tx, mut legacy_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        drop(sourced_rx);
        let sink =
            SourcedExecutionEventSink::new(ClientId::from("SOURCE-CLOSED"), sourced_tx, legacy_tx);

        let error = sink
            .send(ExecutionEvent::Account(AccountState::new(
                AccountId::from("SOURCE-CLOSED-001"),
                AccountType::Cash,
                vec![],
                vec![],
                true,
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
                None,
            )))
            .expect_err("closed sourced lane must return the original event");

        assert!(matches!(error.0, ExecutionEvent::Account(_)));
        assert!(legacy_rx.try_recv().is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_standalone_runner_dispatches_sourced_non_report_event() {
        msgbus::get_message_bus().borrow_mut().dispose();
        let mut runner = AsyncRunner::new();
        let source_tx = runner.sourced_exec_evt_tx.clone();
        let signal_tx = runner.signal_tx.clone();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_handler = received.clone();
        msgbus::register_account_state_endpoint(
            MessagingSwitchboard::portfolio_update_account(),
            TypedHandler::from(move |account: &AccountState| {
                received_handler.borrow_mut().push(account.account_id);
                signal_tx.send(()).unwrap();
            }),
        );
        source_tx
            .send(SourcedExecutionEvent::runtime(
                ClientId::from("SOURCE-STANDALONE"),
                ExecutionEvent::Account(AccountState::new(
                    AccountId::from("SOURCE-STANDALONE-001"),
                    AccountType::Cash,
                    vec![],
                    vec![],
                    true,
                    UUID4::new(),
                    UnixNanos::from(1),
                    UnixNanos::from(2),
                    None,
                )),
            ))
            .unwrap();

        runner.run().await;

        assert_eq!(
            received.borrow().as_slice(),
            &[AccountId::from("SOURCE-STANDALONE-001")]
        );
    }

    #[rstest]
    fn test_sourced_non_strict_reports_dispatch_directly_to_legacy_endpoint() {
        msgbus::get_message_bus().borrow_mut().dispose();
        let legacy_reports = Rc::new(RefCell::new(Vec::new()));
        let legacy_handler = legacy_reports.clone();
        msgbus::register_execution_report_endpoint(
            MessagingSwitchboard::exec_engine_reconcile_execution_report(),
            TypedIntoHandler::from(move |report: ExecutionReport| {
                legacy_handler.borrow_mut().push(report);
            }),
        );
        let strict_reports = Rc::new(RefCell::new(Vec::new()));
        let strict_handler = strict_reports.clone();
        msgbus::register_sourced_execution_report_endpoint(
            MessagingSwitchboard::exec_engine_reconcile_sourced_execution_report(),
            TypedIntoHandler::from(move |report: SourcedExecutionReport| {
                strict_handler.borrow_mut().push(report);
            }),
        );
        let source_client_id = ClientId::from("SOURCE-NON-STRICT");
        let account_id = AccountId::from("SOURCE-NON-STRICT-001");
        let instrument_id = InstrumentId::from("EUR/USD.SIM");
        let order_report = OrderStatusReport::new(
            account_id,
            instrument_id,
            Some(ClientOrderId::from("O-SOURCE-NON-STRICT")),
            VenueOrderId::from("V-SOURCE-NON-STRICT"),
            OrderSide::Buy.into(),
            OrderType::Market,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from(100_000),
            Quantity::from(0),
            UnixNanos::from(1),
            UnixNanos::from(2),
            UnixNanos::from(3),
            None,
        );
        let position_report = PositionStatusReport::new(
            account_id,
            instrument_id,
            PositionSide::Long,
            Quantity::from(100_000),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
            Some(PositionId::from("P-SOURCE-POSITION")),
            None,
        );
        let mass_status = ExecutionMassStatus::new(
            source_client_id,
            account_id,
            instrument_id.venue,
            UnixNanos::from(4),
            None,
        );

        for report in [
            ExecutionReport::OrderWithFills(Box::new(order_report), Vec::new()),
            ExecutionReport::Position(Box::new(position_report)),
            ExecutionReport::MassStatus(Box::new(mass_status)),
        ] {
            AsyncRunner::handle_sourced_exec_event(SourcedExecutionEvent::runtime(
                source_client_id,
                ExecutionEvent::Report(report),
            ));
        }

        assert!(matches!(
            legacy_reports.borrow().as_slice(),
            [
                ExecutionReport::OrderWithFills(_, _),
                ExecutionReport::Position(_),
                ExecutionReport::MassStatus(_),
            ]
        ));
        assert!(strict_reports.borrow().is_empty());
    }

    #[rstest]
    fn test_async_data_command_sender_creation() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = AsyncDataCommandSender::new(tx);
        assert!(format!("{sender:?}").contains("AsyncDataCommandSender"));
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

        let received = rx.recv().await.unwrap();
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
        let (_data_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (_time_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
            runner.run().await;
        });

        // Send shutdown signal
        signal_tx.send(()).unwrap();

        // Runner should stop quickly
        let result = tokio::time::timeout(Duration::from_millis(100), runner_handle).await;
        assert!(result.is_ok(), "Runner should stop on signal");
    }

    #[tokio::test]
    async fn test_runner_closes_on_channel_drop() {
        let (data_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (_time_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
            runner.run().await;
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
        let (data_evt_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_data_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (_time_evt_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
                    tx_clone.send(DataEvent::Data(Data::Quote(quote))).unwrap();
                    tokio::task::yield_now().await;
                }
            });
            handles.push(handle);
        }

        // Start runner in background
        let runner_handle = tokio::spawn(async move {
            runner.run().await;
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

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
            let clock = Rc::new(RefCell::new(TestClock::new()));
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

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
            let clock = Rc::new(RefCell::new(TestClock::new()));
            let cache = Rc::new(RefCell::new(Cache::default()));
            let exec_engine = Rc::new(RefCell::new(ExecutionEngine::new(clock, cache, None)));
            ExecutionEngine::register_msgbus_handlers(&exec_engine);

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
        let (_data_evt_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_data_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (_time_evt_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
            runner.run().await;
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
            .send(TradingCommandMessage::new(
                MessagingSwitchboard::exec_engine_execute(),
                command,
            ))
            .unwrap();

        tokio::task::yield_now().await;
        signal_tx.send(()).unwrap();

        let result = tokio::time::timeout(Duration::from_millis(100), runner_handle).await;
        assert!(result.is_ok(), "Runner should process command and stop");
    }

    #[tokio::test]
    async fn test_runner_processes_multiple_trading_commands() {
        let (_data_evt_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_data_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (_time_evt_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
            runner.run().await;
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
                .send(TradingCommandMessage::new(
                    MessagingSwitchboard::exec_engine_execute(),
                    command,
                ))
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

    #[derive(Clone, Copy, Debug)]
    enum AmbiguousRuntimeReportKind {
        OrderStatus,
        Fill,
    }

    #[derive(Clone, Copy, Debug)]
    enum RuntimeReportIngressLane {
        Legacy,
        Sourced,
    }

    #[rstest]
    #[case::sourced_order(
        AmbiguousRuntimeReportKind::OrderStatus,
        RuntimeReportIngressLane::Sourced
    )]
    #[case::sourced_fill(AmbiguousRuntimeReportKind::Fill, RuntimeReportIngressLane::Sourced)]
    #[case::legacy_order(
        AmbiguousRuntimeReportKind::OrderStatus,
        RuntimeReportIngressLane::Legacy
    )]
    #[case::legacy_fill(AmbiguousRuntimeReportKind::Fill, RuntimeReportIngressLane::Legacy)]
    fn test_runtime_report_routing_respects_ingress_source_compatibility(
        #[case] report_kind: AmbiguousRuntimeReportKind,
        #[case] ingress_lane: RuntimeReportIngressLane,
    ) {
        std::thread::spawn(move || {
            msgbus::get_message_bus().borrow_mut().dispose();

            let clock = Rc::new(RefCell::new(TestClock::new()));
            let cache = Rc::new(RefCell::new(Cache::default()));
            let exec_engine = Rc::new(RefCell::new(ExecutionEngine::new(
                clock,
                cache.clone(),
                None,
            )));
            let instrument = audusd_sim();
            let account_id = AccountId::from("SHARED-ACCOUNT");
            let venue_router_id = ClientId::from("VENUE-A");
            let source_id = ClientId::from("SOURCE-B");

            let venue_router = StubExecutionClient::new(
                venue_router_id,
                account_id,
                instrument.id().venue,
                OmsType::Netting,
                None,
            );
            let venue_router_registrations = venue_router.registered_external_order_ids();
            let source = StubExecutionClient::new(
                source_id,
                account_id,
                Venue::from("SOURCE-B"),
                OmsType::Netting,
                None,
            )
            .with_handles_all_order_venues();
            let source_registrations = source.registered_external_order_ids();

            {
                let mut engine = exec_engine.borrow_mut();
                engine.register_client(Box::new(venue_router)).unwrap();
                engine.register_client(Box::new(source)).unwrap();
                engine
                    .cache()
                    .borrow_mut()
                    .add_instrument(instrument.clone().into())
                    .unwrap();
            }
            ExecutionEngine::register_msgbus_handlers(&exec_engine);

            let runner = AsyncRunner::new();
            runner.bind_senders();
            let (client_order_id, event) = match report_kind {
                AmbiguousRuntimeReportKind::OrderStatus => {
                    let client_order_id = ClientOrderId::from("O-RUNTIME-ORDER");
                    let report = OrderStatusReport::new(
                        account_id,
                        instrument.id(),
                        Some(client_order_id),
                        VenueOrderId::from("V-RUNTIME-ORDER"),
                        OrderSide::Buy.into(),
                        OrderType::Market,
                        TimeInForce::Gtc,
                        OrderStatus::Accepted,
                        Quantity::from(100_000),
                        Quantity::from(0),
                        UnixNanos::from(1),
                        UnixNanos::from(2),
                        UnixNanos::from(3),
                        None,
                    );
                    (
                        client_order_id,
                        ExecutionEvent::Report(ExecutionReport::Order(Box::new(report))),
                    )
                }
                AmbiguousRuntimeReportKind::Fill => {
                    let client_order_id = ClientOrderId::from("O-RUNTIME-FILL");
                    let report = FillReport::new(
                        account_id,
                        instrument.id(),
                        VenueOrderId::from("V-RUNTIME-FILL"),
                        TradeId::from("T-RUNTIME-FILL"),
                        OrderSide::Buy,
                        Quantity::from(100_000),
                        Price::from("1.10000"),
                        Money::from("1 USD"),
                        LiquiditySide::Taker,
                        Some(client_order_id),
                        None,
                        UnixNanos::from(1),
                        UnixNanos::from(2),
                        None,
                    );
                    (
                        client_order_id,
                        ExecutionEvent::Report(ExecutionReport::Fill(Box::new(report))),
                    )
                }
            };

            match ingress_lane {
                RuntimeReportIngressLane::Sourced => {
                    get_sourced_exec_event_sink(source_id).send(event).unwrap();
                    let (channels, mut sourced) = runner.take_channels_with_sourced();
                    AsyncRunner::handle_sourced_exec_event(
                        sourced
                            .receiver
                            .try_recv()
                            .expect("runtime report should reach the sourced execution-event lane"),
                    );
                    assert!(channels.exec_evt_rx.is_empty());
                }
                RuntimeReportIngressLane::Legacy => {
                    get_exec_event_sender().send(event).unwrap();
                    let (mut channels, sourced) = runner.take_channels_with_sourced();
                    AsyncRunner::handle_exec_event(
                        channels
                            .exec_evt_rx
                            .try_recv()
                            .expect("runtime report should reach the legacy execution-event lane"),
                    );
                    assert_eq!(sourced.len(), 0);
                }
            }

            let origin = cache.borrow().client_id(&client_order_id).copied();
            let venue_router_registered = venue_router_registrations.borrow().clone();
            let source_registered = source_registrations.borrow().clone();
            let expected = match ingress_lane {
                RuntimeReportIngressLane::Sourced => {
                    (Some(source_id), Vec::new(), vec![client_order_id])
                }
                RuntimeReportIngressLane::Legacy => (None, vec![client_order_id], Vec::new()),
            };
            assert_eq!(
                (origin, venue_router_registered, source_registered),
                expected,
                "only the opted-in sourced lane may enforce the emitting execution client",
            );
        })
        .join()
        .unwrap();
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
        let (_data_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (_time_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
            runner.run().await;
        });

        // Use stop via signal_tx directly
        signal_tx.send(()).unwrap();

        let result = tokio::time::timeout(Duration::from_millis(100), runner_handle).await;
        assert!(result.is_ok(), "Runner should stop when stop() is called");
    }

    #[tokio::test]
    async fn test_all_event_types_integration() {
        let (data_evt_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (data_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (time_evt_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
        let (exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
            runner.run().await;
        });

        // Send data event
        let quote = test_quote();
        data_evt_tx
            .send(DataEvent::Data(Data::Quote(quote)))
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
        data_cmd_tx.send(command).unwrap();

        // Send time event
        let event = TimeEvent::new(
            Ustr::from("test"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let callback = TimeEventCallback::from(|_: TimeEvent| {});
        let message = TimeEventMessage::new(event, callback);
        time_evt_tx.send(message).unwrap();

        // Send execution order event
        let order_event = OrderSubmittedSpec::builder()
            .client_order_id(ClientOrderId::from("O-001"))
            .build();
        exec_evt_tx
            .send(ExecutionEvent::Order(OrderEventAny::Submitted(order_event)))
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
            .send(ExecutionEvent::Report(ExecutionReport::Order(Box::new(
                order_status,
            ))))
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
            .send(ExecutionEvent::Report(ExecutionReport::Fill(Box::new(
                fill,
            ))))
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
            .send(ExecutionEvent::Report(ExecutionReport::Position(Box::new(
                position,
            ))))
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
            .send(ExecutionEvent::Account(account_state))
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
        let (_data_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (_time_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
            runner.run().await;
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
        let (data_evt_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (_time_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventMessage>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommandMessage>();
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
                .send(DataEvent::Data(Data::Quote(quote)))
                .unwrap();
        }

        let runner_handle = tokio::spawn(async move {
            runner.run().await;
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
                runner.channels.system_evt_rx.try_recv().unwrap(),
                test_system_event()
            );

            get_system_command_sender()
                .send(test_system_command())
                .unwrap();
            assert_eq!(
                runner.channels.system_cmd_rx.try_recv().unwrap(),
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

            let system_events = runner.drain_pending_system_events();

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

            let system_commands = runner.drain_pending_system_commands();

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
