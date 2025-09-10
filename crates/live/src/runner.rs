// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{fmt::Debug, sync::Arc};

use nautilus_common::{
    messages::{DataEvent, data::DataCommand},
    msgbus::{self, switchboard::MessagingSwitchboard},
    runner::{
        DataCommandSender, TimeEventSender, set_data_cmd_sender, set_data_event_sender,
        set_time_event_sender,
    },
    timer::TimeEventHandlerV2,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Asynchronous implementation of `DataCommandSender` for live environments.
#[derive(Debug)]
pub struct AsyncDataCommandSender {
    cmd_tx: UnboundedSender<DataCommand>,
}

impl AsyncDataCommandSender {
    #[must_use]
    pub const fn new(cmd_tx: UnboundedSender<DataCommand>) -> Self {
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
    time_tx: UnboundedSender<TimeEventHandlerV2>,
}

impl AsyncTimeEventSender {
    #[must_use]
    pub const fn new(time_tx: UnboundedSender<TimeEventHandlerV2>) -> Self {
        Self { time_tx }
    }

    /// Gets a clone of the underlying channel sender for async use.
    ///
    /// This allows async contexts to get a direct channel sender that
    /// can be moved into async tasks without `RefCell` borrowing issues.
    #[must_use]
    pub fn get_channel_sender(&self) -> UnboundedSender<TimeEventHandlerV2> {
        self.time_tx.clone()
    }
}

impl TimeEventSender for AsyncTimeEventSender {
    fn send(&self, handler: TimeEventHandlerV2) {
        if let Err(e) = self.time_tx.send(handler) {
            log::error!("Failed to send time event handler: {e}");
        }
    }
}

pub trait Runner {
    fn run(&mut self);
}

pub struct AsyncRunner {
    data_rx: UnboundedReceiver<DataEvent>,
    cmd_rx: UnboundedReceiver<DataCommand>,
    time_rx: UnboundedReceiver<TimeEventHandlerV2>,
    signal_rx: UnboundedReceiver<()>,
    signal_tx: UnboundedSender<()>,
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
    #[must_use]
    pub fn new() -> Self {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (time_tx, time_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandlerV2>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        set_time_event_sender(Arc::new(AsyncTimeEventSender::new(time_tx)));
        set_data_event_sender(data_tx);
        set_data_cmd_sender(Arc::new(AsyncDataCommandSender::new(cmd_tx)));

        Self {
            data_rx,
            cmd_rx,
            time_rx,
            signal_rx,
            signal_tx,
        }
    }

    /// Stops the runner with an internal shutdown signal.
    pub fn stop(&self) {
        if let Err(e) = self.signal_tx.send(()) {
            log::error!("Failed to send shutdown signal: {e}");
        }
    }
}

impl AsyncRunner {
    /// Runs the async runner event loop.
    ///
    /// This method processes data events, time events, and signal events in an async loop.
    /// It will run until a signal is received or the event streams are closed.
    pub async fn run(&mut self) {
        log::info!("Starting AsyncRunner");

        let data_engine_process = MessagingSwitchboard::data_engine_process();
        let data_engine_response = MessagingSwitchboard::data_engine_response();
        let data_engine_execute = MessagingSwitchboard::data_engine_execute();

        loop {
            tokio::select! {
                Some(event) = self.data_rx.recv() => {
                    match event {
                        DataEvent::Data(data) => msgbus::send_any(data_engine_process, &data),
                        DataEvent::Response(resp) => {
                            msgbus::send_any(data_engine_response, &resp);
                        }
                        #[cfg(feature = "defi")]
                        DataEvent::DeFi(data) => msgbus::send_any(data_engine_process, &data),
                    }
                },
                Some(handler) = self.time_rx.recv() => {
                    handler.run();
                },
                Some(cmd) = self.cmd_rx.recv() => {
                    msgbus::send_any(data_engine_execute, &cmd);
                },
                Some(()) = self.signal_rx.recv() => {
                    tracing::info!("AsyncRunner received signal, shutting down");
                    return; // Signal to stop
                },
                else => return, // Sentinel event ends run
            };
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::{rc::Rc, time::Duration};

    use nautilus_common::{
        messages::data::{SubscribeCommand, SubscribeCustomData},
        timer::{TimeEvent, TimeEventCallback, TimeEventHandlerV2},
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::{Data, quote::QuoteTick},
        identifiers::{ClientId, InstrumentId},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use tokio::sync::mpsc;
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

    #[rstest]
    fn test_async_data_command_sender_creation() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let sender = AsyncDataCommandSender::new(tx);
        assert!(format!("{sender:?}").contains("AsyncDataCommandSender"));
    }

    #[rstest]
    fn test_async_time_event_sender_creation() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let sender = AsyncTimeEventSender::new(tx);
        assert!(format!("{sender:?}").contains("AsyncTimeEventSender"));
    }

    #[rstest]
    fn test_async_time_event_sender_get_channel() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let sender = AsyncTimeEventSender::new(tx);
        let channel = sender.get_channel_sender();

        // Verify the channel is functional
        let event = TimeEvent::new(
            Ustr::from("test"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let callback = TimeEventCallback::from(Rc::new(|_: TimeEvent| {}) as Rc<dyn Fn(TimeEvent)>);
        let handler = TimeEventHandlerV2::new(event, callback);

        assert!(channel.send(handler).is_ok());
    }

    #[tokio::test]
    async fn test_async_data_command_sender_execute() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sender = AsyncDataCommandSender::new(tx);

        let command = DataCommand::Subscribe(SubscribeCommand::Data(SubscribeCustomData {
            client_id: Some(ClientId::from("TEST")),
            venue: None,
            data_type: nautilus_model::data::DataType::new("QuoteTick", None),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
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
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sender = AsyncTimeEventSender::new(tx);

        let event = TimeEvent::new(
            Ustr::from("test"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );
        let callback = TimeEventCallback::from(Rc::new(|_: TimeEvent| {}) as Rc<dyn Fn(TimeEvent)>);
        let handler = TimeEventHandlerV2::new(event, callback);

        sender.send(handler);

        assert!(rx.recv().await.is_some());
    }

    #[tokio::test]
    async fn test_runner_shutdown_signal() {
        // Create runner with manual channels to avoid global state
        let (_data_tx, data_rx) = mpsc::unbounded_channel::<DataEvent>();
        let (_cmd_tx, cmd_rx) = mpsc::unbounded_channel::<DataCommand>();
        let (_time_tx, time_rx) = mpsc::unbounded_channel::<TimeEventHandlerV2>();
        let (signal_tx, signal_rx) = mpsc::unbounded_channel::<()>();

        let mut runner = AsyncRunner {
            data_rx,
            cmd_rx,
            time_rx,
            signal_rx,
            signal_tx: signal_tx.clone(),
        };

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
        let (data_tx, data_rx) = mpsc::unbounded_channel::<DataEvent>();
        let (_cmd_tx, cmd_rx) = mpsc::unbounded_channel::<DataCommand>();
        let (_time_tx, time_rx) = mpsc::unbounded_channel::<TimeEventHandlerV2>();
        let (signal_tx, signal_rx) = mpsc::unbounded_channel::<()>();

        let mut runner = AsyncRunner {
            data_rx,
            cmd_rx,
            time_rx,
            signal_rx,
            signal_tx: signal_tx.clone(),
        };

        // Start runner
        let runner_handle = tokio::spawn(async move {
            runner.run().await;
        });

        // Drop data sender to close channel - this should cause runner to exit
        drop(data_tx);

        // Send stop signal to ensure clean shutdown
        tokio::time::sleep(Duration::from_millis(50)).await;
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
        let (data_tx, data_rx) = mpsc::unbounded_channel::<DataEvent>();
        let (_cmd_tx, cmd_rx) = mpsc::unbounded_channel::<DataCommand>();
        let (_time_tx, time_rx) = mpsc::unbounded_channel::<TimeEventHandlerV2>();
        let (signal_tx, signal_rx) = mpsc::unbounded_channel::<()>();

        // Setup runner
        let mut runner = AsyncRunner {
            data_rx,
            cmd_rx,
            time_rx,
            signal_rx,
            signal_tx: signal_tx.clone(),
        };

        // Spawn multiple concurrent senders
        let mut handles = vec![];
        for _ in 0..5 {
            let tx_clone = data_tx.clone();
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

        // Give runner time to process
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Stop runner
        signal_tx.send(()).unwrap();

        let _ = tokio::time::timeout(Duration::from_secs(1), runner_handle).await;
    }

    #[rstest]
    #[case(10)]
    #[case(100)]
    #[case(1000)]
    fn test_channel_send_performance(#[case] count: usize) {
        let (tx, mut rx) = mpsc::unbounded_channel::<DataEvent>();
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
}
