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
        DataCommandSender, RunnerEvent, TimeEventSender, set_data_cmd_sender,
        set_data_event_sender, set_time_event_sender,
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
            // Collect the next message to process, including signal events
            let next_msg = tokio::select! {
                Some(resp) = self.data_rx.recv() => Some(RunnerEvent::Data(resp)),
                Some(handler) = self.time_rx.recv() => Some(RunnerEvent::Time(handler)),
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

            tracing::debug!("Received {next_msg:?}");

            if let Some(msg) = next_msg {
                match msg {
                    RunnerEvent::Time(handler) => handler.run(),
                    RunnerEvent::Data(event) => {
                        #[cfg(feature = "defi")]
                        match event {
                            DataEvent::Data(data) => msgbus::send_any(data_engine_process, &data),
                            DataEvent::Response(resp) => {
                                msgbus::send_any(data_engine_response, &resp);
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
