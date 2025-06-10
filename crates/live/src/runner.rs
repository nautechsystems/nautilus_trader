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

use std::{
    cell::{OnceCell, RefCell},
    fmt::Debug,
    rc::Rc,
};

use futures::StreamExt;
use nautilus_common::{
    clock::{Clock, LiveClock},
    messages::{DataEvent, data::DataCommand},
    msgbus::{self, switchboard::MessagingSwitchboard},
    runner::{
        DataCommandSender, DataQueue, RunnerEvent, set_data_cmd_sender, set_data_event_queue,
    },
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub struct AsyncDataQueue(UnboundedSender<DataEvent>);

impl Debug for AsyncDataQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple(stringify!(AsyncDataQueue)).finish()
    }
}

impl DataQueue for AsyncDataQueue {
    fn push(&mut self, event: DataEvent) {
        if let Err(e) = self.0.send(event) {
            log::error!("Unable to send data event to async data channel: {e}");
        }
    }
}

/// Asynchronous implementation of DataCommandSender for live environments.
#[derive(Debug)]
pub struct AsyncDataCommandSender {
    cmd_tx: UnboundedSender<DataCommand>,
}

impl AsyncDataCommandSender {
    pub fn new(cmd_tx: UnboundedSender<DataCommand>) -> Self {
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

/// Sets the global data event sender.
///
/// This should be called by the AsyncRunner when it creates the channel.
///
/// # Panics
///
/// Panics if thread-local storage cannot be accessed or a sender is already set.
pub fn set_data_evt_sender(sender: UnboundedSender<DataEvent>) {
    DATA_EVT_SENDER
        .try_with(|s| {
            assert!(s.set(sender).is_ok(), "Data event sender already set");
        })
        .expect("Should be able to access thread local storage");
}

/// Gets a cloned data event sender.
///
/// This allows data clients to send events directly to the AsyncRunner
/// without going through shared mutable state.
///
/// # Panics
///
/// Panics if thread-local storage cannot be accessed or the sender is uninitialized.
#[must_use]
pub fn get_data_event_sender() -> UnboundedSender<DataEvent> {
    DATA_EVT_SENDER
        .try_with(|s| {
            s.get()
                .expect("Data event sender should be initialized by AsyncRunner")
                .clone()
        })
        .expect("Should be able to access thread local storage")
}

thread_local! {
    static DATA_EVT_SENDER: OnceCell<UnboundedSender<DataEvent>> = const { OnceCell::new() };
}

pub trait Runner {
    fn run(&mut self);
}

pub struct AsyncRunner {
    pub clock: Rc<RefCell<LiveClock>>,
    data_rx: UnboundedReceiver<DataEvent>,
    cmd_rx: UnboundedReceiver<DataCommand>,
    signal_rx: UnboundedReceiver<()>,
    signal_tx: UnboundedSender<()>,
}

impl Debug for AsyncRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(AsyncRunner))
            .field("clock_set", &true)
            .finish()
    }
}

impl AsyncRunner {
    pub fn new(clock: Rc<RefCell<LiveClock>>) -> Self {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        set_data_evt_sender(data_tx.clone());
        set_data_cmd_sender(Rc::new(RefCell::new(AsyncDataCommandSender::new(cmd_tx))));

        // TODO: Deprecated and can be removed?
        set_data_event_queue(Rc::new(RefCell::new(AsyncDataQueue(data_tx))));

        Self {
            clock,
            data_rx,
            cmd_rx,
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

        let mut time_event_stream = self.clock.borrow().get_event_stream();

        let data_engine_process = MessagingSwitchboard::data_engine_process();
        let data_engine_response = MessagingSwitchboard::data_engine_response();
        let data_engine_execute = MessagingSwitchboard::data_engine_execute();

        loop {
            // Collect the next message to process, including signal events
            let next_msg = tokio::select! {
                Some(resp) = self.data_rx.recv() => Some(RunnerEvent::Data(resp)),
                Some(event) = time_event_stream.next() => Some(RunnerEvent::Time(event)),
                Some(cmd) = self.cmd_rx.recv() => {
                    msgbus::send_any(data_engine_execute, &cmd);
                    None // TODO: Refactor this
                },
                Some(()) = self.signal_rx.recv() => {
                    tracing::info!("AsyncRunner received signal, shutting down");
                    return; // Signal to stop
                },
                else => return, // Sentinel event ends run
            };

            if let Some(msg) = next_msg {
                match msg {
                    RunnerEvent::Time(event) => self.clock.borrow().get_handler(event).run(),
                    RunnerEvent::Data(event) => {
                        #[cfg(feature = "defi")]
                        match event {
                            DataEvent::Data(data) => msgbus::send_any(data_engine_process, &data),
                            DataEvent::Response(resp) => {
                                msgbus::send_any(data_engine_response, &resp)
                            }
                            DataEvent::DeFi(data) => {
                                msgbus::send_any(data_engine_process, &data);
                            }
                        }
                        #[cfg(not(feature = "defi"))]
                        match event {
                            DataEvent::Data(data) => msgbus::send_any(data_engine_process, &data),
                            DataEvent::Response(resp) => {
                                msgbus::send_any(data_engine_response, &resp)
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[cfg(feature = "clock_v2")]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use futures::StreamExt;
    use nautilus_common::{
        clock::LiveClock,
        runner::{get_global_clock, set_global_clock},
        timer::{TimeEvent, TimeEventCallback},
    };

    #[tokio::test]
    async fn test_global_live_clock() {
        let live_clock = Rc::new(RefCell::new(LiveClock::new()));
        set_global_clock(live_clock.clone());
        let alert_time = live_clock.borrow().get_time_ns() + 100;

        // component/actor adding an alert
        let _ = get_global_clock().borrow_mut().set_time_alert_ns(
            "hola",
            alert_time,
            Some(TimeEventCallback::Rust(Rc::new(|_event: TimeEvent| {}))),
            None,
        );

        // runner pulling from event
        assert!(
            live_clock
                .borrow()
                .get_event_stream()
                .next()
                .await
                .is_some()
        );
    }
}
