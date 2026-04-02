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

//! Global runtime machinery and thread-local storage.
//!
//! This module provides global access to shared runtime resources including clocks,
//! message queues, and time event channels. It manages thread-local storage for
//! system-wide components that need to be accessible across threads.

use std::{cell::RefCell, fmt::Debug, sync::Arc};

use crate::{
    messages::{data::DataCommand, execution::TradingCommand},
    msgbus::{self, MessagingSwitchboard},
    timer::TimeEventHandler,
};

/// Trait for data command sending that can be implemented for both sync and async runners.
pub trait DataCommandSender {
    /// Executes a data command.
    ///
    /// - **Sync runners** send the command to a queue for synchronous execution.
    /// - **Async runners** send the command to a channel for asynchronous execution.
    fn execute(&self, command: DataCommand);
}

/// Synchronous [`DataCommandSender`] for backtest environments.
///
/// Buffers commands in a thread-local queue for deferred execution,
/// avoiding `RefCell` re-entrancy when sent from event handler callbacks.
#[derive(Debug)]
pub struct SyncDataCommandSender;

impl DataCommandSender for SyncDataCommandSender {
    fn execute(&self, command: DataCommand) {
        DATA_CMD_QUEUE.with(|q| q.borrow_mut().push(command));
    }
}

/// Drain all buffered data commands, dispatching each to the data engine.
pub fn drain_data_cmd_queue() {
    DATA_CMD_QUEUE.with(|q| {
        let commands: Vec<DataCommand> = q.borrow_mut().drain(..).collect();
        let endpoint = MessagingSwitchboard::data_engine_execute();
        for cmd in commands {
            msgbus::send_data_command(endpoint, cmd);
        }
    });
}

/// Returns `true` if the data command queue is empty.
pub fn data_cmd_queue_is_empty() -> bool {
    DATA_CMD_QUEUE.with(|q| q.borrow().is_empty())
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
            .borrow()
            .as_ref()
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
        let mut slot = s.borrow_mut();
        assert!(slot.is_none(), "Data command sender can only be set once");
        *slot = Some(sender);
    });
}

/// Replaces the global data command sender for the current thread.
pub fn replace_data_cmd_sender(sender: Arc<dyn DataCommandSender>) {
    DATA_CMD_SENDER.with(|s| {
        *s.borrow_mut() = Some(sender);
    });
}

/// Trait for time event sending that can be implemented for both sync and async runners.
pub trait TimeEventSender: Debug + Send + Sync {
    /// Sends a time event handler.
    fn send(&self, handler: TimeEventHandler);
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
            .borrow()
            .as_ref()
            .expect("Time event sender should be initialized by runner")
            .clone()
    })
}

/// Attempts to get the global time event sender without panicking.
///
/// Returns `None` if the sender is not initialized (e.g., in test environments).
#[must_use]
pub fn try_get_time_event_sender() -> Option<Arc<dyn TimeEventSender>> {
    TIME_EVENT_SENDER.with(|sender| sender.borrow().as_ref().cloned())
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
        let mut slot = s.borrow_mut();
        assert!(slot.is_none(), "Time event sender can only be set once");
        *slot = Some(sender);
    });
}

/// Replaces the global time event sender for the current thread.
pub fn replace_time_event_sender(sender: Arc<dyn TimeEventSender>) {
    TIME_EVENT_SENDER.with(|s| {
        *s.borrow_mut() = Some(sender);
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

/// Synchronous [`TradingCommandSender`] for backtest environments.
///
/// Buffers commands in a thread-local queue for deferred execution,
/// avoiding `RefCell` re-entrancy when sent from event handler callbacks.
#[derive(Debug)]
pub struct SyncTradingCommandSender;

impl TradingCommandSender for SyncTradingCommandSender {
    fn execute(&self, command: TradingCommand) {
        TRADING_CMD_QUEUE.with(|q| q.borrow_mut().push(command));
    }
}

/// Drain all buffered trading commands, dispatching each to the exec engine.
pub fn drain_trading_cmd_queue() {
    TRADING_CMD_QUEUE.with(|q| {
        let commands: Vec<TradingCommand> = q.borrow_mut().drain(..).collect();
        let endpoint = MessagingSwitchboard::exec_engine_execute();
        for cmd in commands {
            msgbus::send_trading_command(endpoint, cmd);
        }
    });
}

/// Returns `true` if the trading command queue is empty.
pub fn trading_cmd_queue_is_empty() -> bool {
    TRADING_CMD_QUEUE.with(|q| q.borrow().is_empty())
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
            .borrow()
            .as_ref()
            .expect("Trading command sender should be initialized by runner")
            .clone()
    })
}

/// Attempts to get the global trading command sender without panicking.
///
/// Returns `None` if the sender is not initialized (e.g., in test environments).
#[must_use]
pub fn try_get_trading_cmd_sender() -> Option<Arc<dyn TradingCommandSender>> {
    EXEC_CMD_SENDER.with(|sender| sender.borrow().as_ref().cloned())
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
        let mut slot = s.borrow_mut();
        assert!(
            slot.is_none(),
            "Trading command sender can only be set once"
        );
        *slot = Some(sender);
    });
}

/// Replaces the global trading command sender for the current thread.
pub fn replace_exec_cmd_sender(sender: Arc<dyn TradingCommandSender>) {
    EXEC_CMD_SENDER.with(|s| {
        *s.borrow_mut() = Some(sender);
    });
}

thread_local! {
    static TIME_EVENT_SENDER: RefCell<Option<Arc<dyn TimeEventSender>>> = const { RefCell::new(None) };
    static DATA_CMD_SENDER: RefCell<Option<Arc<dyn DataCommandSender>>> = const { RefCell::new(None) };
    static EXEC_CMD_SENDER: RefCell<Option<Arc<dyn TradingCommandSender>>> = const { RefCell::new(None) };
    static DATA_CMD_QUEUE: RefCell<Vec<DataCommand>> = const { RefCell::new(Vec::new()) };
    static TRADING_CMD_QUEUE: RefCell<Vec<TradingCommand>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rstest::rstest;

    use super::*;

    #[derive(Debug)]
    struct NoopTimeEventSender;

    impl TimeEventSender for NoopTimeEventSender {
        fn send(&self, _handler: TimeEventHandler) {}
    }

    #[rstest]
    fn test_replace_data_cmd_sender_overwrites_previous() {
        std::thread::spawn(|| {
            replace_data_cmd_sender(Arc::new(SyncDataCommandSender));
            replace_data_cmd_sender(Arc::new(SyncDataCommandSender));
            let _sender = get_data_cmd_sender();
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_replace_exec_cmd_sender_overwrites_previous() {
        std::thread::spawn(|| {
            replace_exec_cmd_sender(Arc::new(SyncTradingCommandSender));
            replace_exec_cmd_sender(Arc::new(SyncTradingCommandSender));
            let _sender = get_trading_cmd_sender();
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_replace_time_event_sender_overwrites_previous() {
        std::thread::spawn(|| {
            replace_time_event_sender(Arc::new(NoopTimeEventSender));
            replace_time_event_sender(Arc::new(NoopTimeEventSender));
            let _sender = get_time_event_sender();
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_set_data_cmd_sender_panics_on_double_set() {
        let result = std::thread::spawn(|| {
            set_data_cmd_sender(Arc::new(SyncDataCommandSender));
            set_data_cmd_sender(Arc::new(SyncDataCommandSender));
        })
        .join();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_set_exec_cmd_sender_panics_on_double_set() {
        let result = std::thread::spawn(|| {
            set_exec_cmd_sender(Arc::new(SyncTradingCommandSender));
            set_exec_cmd_sender(Arc::new(SyncTradingCommandSender));
        })
        .join();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_set_time_event_sender_panics_on_double_set() {
        let result = std::thread::spawn(|| {
            set_time_event_sender(Arc::new(NoopTimeEventSender));
            set_time_event_sender(Arc::new(NoopTimeEventSender));
        })
        .join();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_try_get_time_event_sender_returns_none_when_unset() {
        let result = std::thread::spawn(try_get_time_event_sender)
            .join()
            .unwrap();
        assert!(result.is_none());
    }

    #[rstest]
    fn test_try_get_trading_cmd_sender_returns_none_when_unset() {
        let is_none = std::thread::spawn(|| try_get_trading_cmd_sender().is_none())
            .join()
            .unwrap();
        assert!(is_none);
    }
}
