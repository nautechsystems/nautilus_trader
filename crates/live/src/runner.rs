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
//! `AsyncRunner` owns five tokio mpsc channel pairs plus a shutdown
//! signal channel. Construction creates the channels without side
//! effects. The sender halves are placed into thread-local storage
//! via [`AsyncRunner::bind_senders`] so that adapters and engine
//! components can resolve them through the `get_*_sender()` accessors
//! in `nautilus_common::runner` and `nautilus_common::live::runner`.
//!
//! Channel pairs:
//!
//! - **Time events**: timer callbacks dispatched by the clock.
//! - **Data commands**: subscribe/unsubscribe requests to data clients.
//! - **Data events**: market data from adapters to the data engine.
//! - **Trading commands**: order actions to execution clients.
//! - **Execution events**: fills, order updates, and account state from
//!   execution clients to the execution engine.
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
//!   thread. `bind_senders` unconditionally replaces the previous
//!   contents, so the last caller wins.

use std::{fmt::Debug, sync::Arc};

use nautilus_common::{
    live::runner::{replace_data_event_sender, replace_exec_event_sender},
    messages::{
        DataEvent, ExecutionEvent, ExecutionReport, data::DataCommand, execution::TradingCommand,
    },
    msgbus::{self, MessagingSwitchboard},
    runner::{
        DataCommandSender, TimeEventSender, TradingCommandSender, replace_data_cmd_sender,
        replace_exec_cmd_sender, replace_time_event_sender,
    },
    timer::TimeEventHandler,
};
use nautilus_model::events::OrderEventAny;

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
    time_tx: tokio::sync::mpsc::UnboundedSender<TimeEventHandler>,
}

impl AsyncTimeEventSender {
    #[must_use]
    pub const fn new(time_tx: tokio::sync::mpsc::UnboundedSender<TimeEventHandler>) -> Self {
        Self { time_tx }
    }

    /// Gets a clone of the underlying channel sender for async use.
    ///
    /// This allows async contexts to get a direct channel sender that
    /// can be moved into async tasks without `RefCell` borrowing issues.
    #[must_use]
    pub fn get_channel_sender(&self) -> tokio::sync::mpsc::UnboundedSender<TimeEventHandler> {
        self.time_tx.clone()
    }
}

impl TimeEventSender for AsyncTimeEventSender {
    fn send(&self, handler: TimeEventHandler) {
        if let Err(e) = self.time_tx.send(handler) {
            log::error!("Failed to send time event handler: {e}");
        }
    }
}

/// Asynchronous implementation of `TradingCommandSender` for live environments.
#[derive(Debug)]
pub struct AsyncTradingCommandSender {
    cmd_tx: tokio::sync::mpsc::UnboundedSender<TradingCommand>,
}

impl AsyncTradingCommandSender {
    #[must_use]
    pub const fn new(cmd_tx: tokio::sync::mpsc::UnboundedSender<TradingCommand>) -> Self {
        Self { cmd_tx }
    }
}

impl TradingCommandSender for AsyncTradingCommandSender {
    fn execute(&self, command: TradingCommand) {
        if let Err(e) = self.cmd_tx.send(command) {
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
    pub time_evt_rx: tokio::sync::mpsc::UnboundedReceiver<TimeEventHandler>,
    pub data_evt_rx: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    pub data_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DataCommand>,
    pub exec_evt_rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    pub exec_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<TradingCommand>,
}

pub struct AsyncRunner {
    channels: AsyncRunnerChannels,
    time_evt_tx: tokio::sync::mpsc::UnboundedSender<TimeEventHandler>,
    data_cmd_tx: tokio::sync::mpsc::UnboundedSender<DataCommand>,
    data_evt_tx: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    exec_cmd_tx: tokio::sync::mpsc::UnboundedSender<TradingCommand>,
    exec_evt_tx: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
    signal_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    signal_tx: tokio::sync::mpsc::UnboundedSender<()>,
}

/// Handle for stopping the AsyncRunner from another context.
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

        let (time_evt_tx, time_evt_rx) = unbounded_channel::<TimeEventHandler>();
        let (data_cmd_tx, data_cmd_rx) = unbounded_channel::<DataCommand>();
        let (data_evt_tx, data_evt_rx) = unbounded_channel::<DataEvent>();
        let (exec_cmd_tx, exec_cmd_rx) = unbounded_channel::<TradingCommand>();
        let (exec_evt_tx, exec_evt_rx) = unbounded_channel::<ExecutionEvent>();
        let (signal_tx, signal_rx) = unbounded_channel::<()>();

        Self {
            channels: AsyncRunnerChannels {
                time_evt_rx,
                data_evt_rx,
                data_cmd_rx,
                exec_evt_rx,
                exec_cmd_rx,
            },
            time_evt_tx,
            data_cmd_tx,
            data_evt_tx,
            exec_cmd_tx,
            exec_evt_tx,
            signal_rx,
            signal_tx,
        }
    }

    /// Binds this runner's channel senders to thread-local storage.
    ///
    /// Call before creating clients that read from TLS (e.g., in the builder),
    /// and again before entering the event loop to reclaim ownership if another
    /// runner was constructed on this thread in the interim.
    pub fn bind_senders(&self) {
        replace_time_event_sender(Arc::new(AsyncTimeEventSender::new(
            self.time_evt_tx.clone(),
        )));
        replace_data_cmd_sender(Arc::new(AsyncDataCommandSender::new(
            self.data_cmd_tx.clone(),
        )));
        replace_data_event_sender(self.data_evt_tx.clone());
        replace_exec_cmd_sender(Arc::new(AsyncTradingCommandSender::new(
            self.exec_cmd_tx.clone(),
        )));
        replace_exec_event_sender(self.exec_evt_tx.clone());
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

    /// Runs the async runner event loop.
    ///
    /// This method processes data events, time events, execution events, and signal events in an async loop.
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
                    Self::handle_time_event(handler);
                },
                Some(cmd) = self.channels.data_cmd_rx.recv() => {
                    Self::handle_data_command(cmd);
                },
                Some(evt) = self.channels.data_evt_rx.recv() => {
                    Self::handle_data_event(evt);
                },
                Some(cmd) = self.channels.exec_cmd_rx.recv() => {
                    Self::handle_exec_command(cmd);
                },
                Some(evt) = self.channels.exec_evt_rx.recv() => {
                    Self::handle_exec_event(evt);
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
    pub fn handle_time_event(handler: TimeEventHandler) {
        handler.run();
    }

    /// Handles a data command by sending to the DataEngine.
    #[inline]
    pub fn handle_data_command(cmd: DataCommand) {
        msgbus::send_data_command(MessagingSwitchboard::data_engine_execute(), cmd);
    }

    /// Handles a data event by sending to the appropriate DataEngine endpoint.
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

    /// Handles an execution command by sending to the ExecEngine.
    #[inline]
    pub fn handle_exec_command(cmd: TradingCommand) {
        msgbus::send_trading_command(MessagingSwitchboard::exec_engine_execute(), cmd);
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nautilus_common::{
        live::runner::{get_data_event_sender, get_exec_event_sender},
        messages::{
            ExecutionEvent, ExecutionReport,
            data::{SubscribeCommand, SubscribeCustomData},
            execution::{CancelAllOrders, TradingCommand},
        },
        runner::{
            get_data_cmd_sender, get_time_event_sender, get_trading_cmd_sender,
            try_get_time_event_sender, try_get_trading_cmd_sender,
        },
        timer::{TimeEvent, TimeEventCallback, TimeEventHandler},
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::{Data, DataType, quote::QuoteTick},
        enums::{
            AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSideSpecified,
            TimeInForce,
        },
        events::{
            OrderAccepted, OrderAcceptedBatch, OrderCanceled, OrderCanceledBatch, OrderEvent,
            OrderEventAny, OrderSubmitted, OrderSubmittedBatch, account::state::AccountState,
        },
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId,
            TraderId, VenueOrderId,
        },
        reports::{FillReport, OrderStatusReport, PositionStatusReport},
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

    // Test helper to create AsyncRunner with manual channels.
    // Sender halves are dummies (not connected to the test receivers) since
    // these tests exercise the event loop, not TLS binding.
    fn create_test_runner(
        time_evt_rx: tokio::sync::mpsc::UnboundedReceiver<TimeEventHandler>,
        data_evt_rx: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        data_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<DataCommand>,
        exec_evt_rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        exec_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<TradingCommand>,
        signal_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
        signal_tx: tokio::sync::mpsc::UnboundedSender<()>,
    ) -> AsyncRunner {
        let (time_evt_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (data_cmd_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (data_evt_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (exec_cmd_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (exec_evt_tx, _) = tokio::sync::mpsc::unbounded_channel();

        AsyncRunner {
            channels: AsyncRunnerChannels {
                time_evt_rx,
                data_evt_rx,
                data_cmd_rx,
                exec_evt_rx,
                exec_cmd_rx,
            },
            time_evt_tx,
            data_cmd_tx,
            data_evt_tx,
            exec_cmd_tx,
            exec_evt_tx,
            signal_rx,
            signal_tx,
        }
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

    #[rstest]
    fn test_async_time_event_sender_get_channel() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = AsyncTimeEventSender::new(tx);
        let channel = sender.get_channel_sender();

        // Verify the channel is functional
        let event = TimeEvent::new(
            Ustr::from("test"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let callback = TimeEventCallback::from(|_: TimeEvent| {});
        let handler = TimeEventHandler::new(event, callback);

        assert!(channel.send(handler).is_ok());
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
        let handler = TimeEventHandler::new(event, callback);

        sender.send(handler);

        assert!(rx.recv().await.is_some());
    }

    #[tokio::test]
    async fn test_runner_shutdown_signal() {
        // Create runner with manual channels to avoid global state
        let (_data_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (_time_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommand>();
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
        let (_time_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommand>();
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
            tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommand>();
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

    #[tokio::test]
    async fn test_async_trading_command_sender_execute() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommand>();
        let sender = AsyncTradingCommandSender::new(tx);

        let command = TradingCommand::CancelAllOrders(CancelAllOrders::new(
            TraderId::from("TRADER-001"),
            None,
            StrategyId::from("S-001"),
            InstrumentId::from("EUR/USD.SIM"),
            OrderSide::Buy,
            UUID4::new(),
            UnixNanos::default(),
            None,
        ));

        sender.execute(command);

        let received = rx.recv().await;
        assert!(received.is_some());
        assert!(matches!(
            received.unwrap(),
            TradingCommand::CancelAllOrders(_)
        ));
    }

    #[tokio::test]
    async fn test_runner_processes_trading_commands() {
        let (_data_evt_tx, data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_data_cmd_tx, data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (_time_evt_tx, time_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (exec_cmd_tx, exec_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommand>();
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
            OrderSide::Buy,
            UUID4::new(),
            UnixNanos::default(),
            None,
        ));
        exec_cmd_tx.send(command).unwrap();

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
            tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (exec_cmd_tx, exec_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommand>();
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
                OrderSide::Buy,
                UUID4::new(),
                UnixNanos::default(),
                None,
            ));
            exec_cmd_tx.send(command).unwrap();
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

        let event = OrderSubmitted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("EUR/USD.SIM"),
            ClientOrderId::from("O-001"),
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );

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
            OrderSide::Buy,
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
            PositionSideSpecified::Long,
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
        let (_time_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommand>();
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
        let (time_evt_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommand>();
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
        let handler = TimeEventHandler::new(event, callback);
        time_evt_tx.send(handler).unwrap();

        // Send execution order event
        let order_event = OrderSubmitted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("EUR/USD.SIM"),
            ClientOrderId::from("O-001"),
            AccountId::from("SIM-001"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        exec_evt_tx
            .send(ExecutionEvent::Order(OrderEventAny::Submitted(order_event)))
            .unwrap();

        // Send execution report (OrderStatus)
        let order_status = OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("EUR/USD.SIM"),
            Some(ClientOrderId::from("O-001")),
            VenueOrderId::from("V-001"),
            OrderSide::Buy,
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
            PositionSideSpecified::Long,
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
        let (_time_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommand>();
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

        let runner_task = tokio::spawn(async move {
            runner.run().await;
        });

        // Use handle to stop
        handle.stop();

        let result = tokio::time::timeout(Duration::from_millis(100), runner_task).await;
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
        let (_time_tx, time_evt_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_exec_evt_tx, exec_evt_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, exec_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<TradingCommand>();
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

        let runner_task = tokio::spawn(async move {
            runner.run().await;
        });

        // Yield to let runner enter event loop before stop signal
        tokio::task::yield_now().await;
        handle.stop();

        let result = tokio::time::timeout(Duration::from_millis(200), runner_task).await;
        assert!(result.is_ok(), "Runner should process events and stop");
    }

    #[rstest]
    fn test_new_does_not_bind_tls() {
        std::thread::spawn(|| {
            let _runner = AsyncRunner::new();
            assert!(try_get_time_event_sender().is_none());
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

            get_trading_cmd_sender().execute(TradingCommand::CancelAllOrders(
                CancelAllOrders::new(
                    TraderId::from("TRADER-001"),
                    None,
                    StrategyId::from("S-001"),
                    InstrumentId::from("EUR/USD.SIM"),
                    OrderSide::Buy,
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                ),
            ));
            assert!(runner.channels.exec_cmd_rx.try_recv().is_ok());

            let event = TimeEvent::new(
                Ustr::from("test"),
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
            );
            let callback = TimeEventCallback::from(|_: TimeEvent| {});
            get_time_event_sender().send(TimeEventHandler::new(event, callback));
            assert!(runner.channels.time_evt_rx.try_recv().is_ok());

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
            OrderSubmitted::new(
                TraderId::from("TRADER-001"),
                StrategyId::from("S-001"),
                InstrumentId::from("EUR/USD.SIM"),
                ClientOrderId::from("O-001"),
                AccountId::from("SIM-001"),
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
            ),
            OrderSubmitted::new(
                TraderId::from("TRADER-001"),
                StrategyId::from("S-001"),
                InstrumentId::from("EUR/USD.SIM"),
                ClientOrderId::from("O-002"),
                AccountId::from("SIM-001"),
                UUID4::new(),
                UnixNanos::from(3),
                UnixNanos::from(4),
            ),
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
            OrderAccepted::new(
                TraderId::from("TRADER-001"),
                StrategyId::from("S-001"),
                InstrumentId::from("EUR/USD.SIM"),
                ClientOrderId::from("O-001"),
                VenueOrderId::from("V-001"),
                AccountId::from("SIM-001"),
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
                false,
            ),
            OrderAccepted::new(
                TraderId::from("TRADER-001"),
                StrategyId::from("S-001"),
                InstrumentId::from("EUR/USD.SIM"),
                ClientOrderId::from("O-002"),
                VenueOrderId::from("V-002"),
                AccountId::from("SIM-001"),
                UUID4::new(),
                UnixNanos::from(3),
                UnixNanos::from(4),
                false,
            ),
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
            OrderCanceled::new(
                TraderId::from("TRADER-001"),
                StrategyId::from("S-001"),
                InstrumentId::from("EUR/USD.SIM"),
                ClientOrderId::from("O-001"),
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
                false,
                None,
                Some(AccountId::from("SIM-001")),
            ),
            OrderCanceled::new(
                TraderId::from("TRADER-001"),
                StrategyId::from("S-001"),
                InstrumentId::from("EUR/USD.SIM"),
                ClientOrderId::from("O-002"),
                UUID4::new(),
                UnixNanos::from(3),
                UnixNanos::from(4),
                false,
                None,
                Some(AccountId::from("SIM-001")),
            ),
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
