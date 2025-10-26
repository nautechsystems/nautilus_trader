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

//! Global runtime machinery and thread-local storage.
//!
//! This module provides global access to shared runtime resources including clocks,
//! message queues, and time event channels. It manages thread-local storage for
//! system-wide components that need to be accessible across threads.

use std::{cell::OnceCell, fmt::Debug, sync::Arc};

use crate::{
    messages::{DataEvent, ExecutionEvent, data::DataCommand, execution::TradingCommand},
    msgbus::{self, switchboard::MessagingSwitchboard},
    timer::TimeEventHandlerV2,
};

/// Trait for data command sending that can be implemented for both sync and async runners.
pub trait DataCommandSender {
    /// Executes a data command.
    ///
    /// - **Sync runners** send the command to a queue for synchronous execution.
    /// - **Async runners** send the command to a channel for asynchronous execution.
    fn execute(&self, command: DataCommand);
}

/// Synchronous implementation of DataCommandSender for backtest environments.
#[derive(Debug)]
pub struct SyncDataCommandSender;

impl DataCommandSender for SyncDataCommandSender {
    fn execute(&self, command: DataCommand) {
        // TODO: Placeholder, we still need to queue and drain even for sync
        let endpoint = MessagingSwitchboard::data_engine_execute();
        msgbus::send_any(endpoint, &command);
    }
}

/// Gets the global data command sender.
///
/// # Panics
///
/// Panics if the sender is uninitialized.
#[must_use]
pub fn get_data_cmd_sender() -> Arc<dyn DataCommandSender> {
    DATA_CMD_SENDER.with(|sender| {
        sender
            .get()
            .expect("Data command sender should be initialized by runner")
            .clone()
    })
}

/// Sets the global data command sender.
///
/// This should be called by the runner when it initializes.
/// Can only be called once per thread.
///
/// # Panics
///
/// Panics if a sender has already been set.
pub fn set_data_cmd_sender(sender: Arc<dyn DataCommandSender>) {
    DATA_CMD_SENDER.with(|s| {
        if s.set(sender).is_err() {
            panic!("Data command sender can only be set once");
        }
    });
}

/// Trait for time event sending that can be implemented for both sync and async runners.
pub trait TimeEventSender: Debug + Send + Sync {
    /// Sends a time event handler.
    fn send(&self, handler: TimeEventHandlerV2);
}

/// Gets the global time event sender.
///
/// # Panics
///
/// Panics if the sender is uninitialized.
#[must_use]
pub fn get_time_event_sender() -> Arc<dyn TimeEventSender> {
    TIME_EVENT_SENDER.with(|sender| {
        sender
            .get()
            .expect("Time event sender should be initialized by runner")
            .clone()
    })
}

/// Attempts to get the global time event sender without panicking.
///
/// Returns `None` if the sender is not initialized (e.g., in test environments).
#[must_use]
pub fn try_get_time_event_sender() -> Option<Arc<dyn TimeEventSender>> {
    TIME_EVENT_SENDER.with(|sender| sender.get().cloned())
}

/// Sets the global time event sender.
///
/// Can only be called once per thread.
///
/// # Panics
///
/// Panics if a sender has already been set.
pub fn set_time_event_sender(sender: Arc<dyn TimeEventSender>) {
    TIME_EVENT_SENDER.with(|s| {
        if s.set(sender).is_err() {
            panic!("Time event sender can only be set once");
        }
    });
}

/// Gets the global data event sender.
///
/// # Panics
///
/// Panics if the sender is uninitialized.
#[must_use]
pub fn get_data_event_sender() -> tokio::sync::mpsc::UnboundedSender<DataEvent> {
    DATA_EVENT_SENDER.with(|sender| {
        sender
            .get()
            .expect("Data event sender should be initialized by runner")
            .clone()
    })
}

/// Sets the global data event sender.
///
/// Can only be called once per thread.
///
/// # Panics
///
/// Panics if a sender has already been set.
pub fn set_data_event_sender(sender: tokio::sync::mpsc::UnboundedSender<DataEvent>) {
    DATA_EVENT_SENDER.with(|s| {
        if s.set(sender).is_err() {
            panic!("Data event sender can only be set once");
        }
    });
}

/// Trait for trading command sending that can be implemented for both sync and async runners.
pub trait TradingCommandSender {
    /// Executes a trading command.
    ///
    /// - **Sync runners** send the command to a queue for synchronous execution.
    /// - **Async runners** send the command to a channel for asynchronous execution.
    fn execute(&self, command: TradingCommand);
}

/// Gets the global execution event sender.
///
/// # Panics
///
/// Panics if the sender is uninitialized.
#[must_use]
pub fn get_exec_event_sender() -> tokio::sync::mpsc::UnboundedSender<ExecutionEvent> {
    EXEC_EVENT_SENDER.with(|sender| {
        sender
            .get()
            .expect("Execution event sender should be initialized by runner")
            .clone()
    })
}

/// Sets the global execution event sender.
///
/// Can only be called once per thread.
///
/// # Panics
///
/// Panics if a sender has already been set.
pub fn set_exec_event_sender(sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>) {
    EXEC_EVENT_SENDER.with(|s| {
        if s.set(sender).is_err() {
            panic!("Execution event sender can only be set once");
        }
    });
}

/// Gets the global trading command sender.
///
/// # Panics
///
/// Panics if the sender is uninitialized.
#[must_use]
pub fn get_trading_cmd_sender() -> Arc<dyn TradingCommandSender> {
    EXEC_CMD_SENDER.with(|sender| {
        sender
            .get()
            .expect("Trading command sender should be initialized by runner")
            .clone()
    })
}

/// Sets the global trading command sender.
///
/// This should be called by the runner when it initializes.
/// Can only be called once per thread.
///
/// # Panics
///
/// Panics if a sender has already been set.
pub fn set_exec_cmd_sender(sender: Arc<dyn TradingCommandSender>) {
    EXEC_CMD_SENDER.with(|s| {
        if s.set(sender).is_err() {
            panic!("Trading command sender can only be set once");
        }
    });
}

// TODO: We can refine this for the synch runner later, data event sender won't be required
thread_local! {
    static TIME_EVENT_SENDER: OnceCell<Arc<dyn TimeEventSender>> = const { OnceCell::new() };
    static DATA_EVENT_SENDER: OnceCell<tokio::sync::mpsc::UnboundedSender<DataEvent>> = const { OnceCell::new() };
    static DATA_CMD_SENDER: OnceCell<Arc<dyn DataCommandSender>> = const { OnceCell::new() };
    static EXEC_EVENT_SENDER: OnceCell<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>> = const { OnceCell::new() };
    static EXEC_CMD_SENDER: OnceCell<Arc<dyn TradingCommandSender>> = const { OnceCell::new() };
}
