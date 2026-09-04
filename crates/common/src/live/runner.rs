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

//! Tokio-based channel senders for live trading runtime.
//!
//! This module provides thread-local storage for tokio mpsc channels used in live trading.

use std::cell::RefCell;

use crate::messages::{DataEvent, ExecutionEvent, SystemCommand, SystemEvent};

/// Gets the global data event sender.
///
/// # Panics
///
/// Panics if the sender is uninitialized.
#[must_use]
pub fn get_data_event_sender() -> tokio::sync::mpsc::UnboundedSender<DataEvent> {
    DATA_EVENT_SENDER.with(|sender| {
        sender
            .borrow()
            .as_ref()
            .expect("Data event sender should be initialized by runner")
            .clone()
    })
}

/// Attempts to get the global data event sender without panicking.
///
/// Returns `None` if the sender is not initialized (e.g., in Python/v1 bridge environments
/// before a runner or adapter bridge has registered a sender).
#[must_use]
pub fn try_get_data_event_sender() -> Option<tokio::sync::mpsc::UnboundedSender<DataEvent>> {
    DATA_EVENT_SENDER.with(|sender| sender.borrow().as_ref().cloned())
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
        let mut slot = s.borrow_mut();
        assert!(slot.is_none(), "Data event sender can only be set once");
        *slot = Some(sender);
    });
}

/// Replaces the global data event sender for the current thread.
pub fn replace_data_event_sender(sender: tokio::sync::mpsc::UnboundedSender<DataEvent>) {
    DATA_EVENT_SENDER.with(|s| {
        *s.borrow_mut() = Some(sender);
    });
}

/// Gets the global system event sender.
///
/// # Panics
///
/// Panics if the sender is uninitialized.
#[must_use]
pub fn get_system_event_sender() -> tokio::sync::mpsc::UnboundedSender<SystemEvent> {
    SYSTEM_EVENT_SENDER.with(|sender| {
        sender
            .borrow()
            .as_ref()
            .expect("System event sender should be initialized by runner")
            .clone()
    })
}

/// Attempts to get the global system event sender without panicking.
///
/// Returns `None` if the sender is not initialized (e.g., in test environments).
#[must_use]
pub fn try_get_system_event_sender() -> Option<tokio::sync::mpsc::UnboundedSender<SystemEvent>> {
    SYSTEM_EVENT_SENDER.with(|sender| sender.borrow().as_ref().cloned())
}

/// Sets the global system event sender.
///
/// Can only be called once per thread.
///
/// # Panics
///
/// Panics if a sender has already been set.
pub fn set_system_event_sender(sender: tokio::sync::mpsc::UnboundedSender<SystemEvent>) {
    SYSTEM_EVENT_SENDER.with(|s| {
        let mut slot = s.borrow_mut();
        assert!(slot.is_none(), "System event sender can only be set once");
        *slot = Some(sender);
    });
}

/// Replaces the global system event sender for the current thread.
pub fn replace_system_event_sender(sender: tokio::sync::mpsc::UnboundedSender<SystemEvent>) {
    SYSTEM_EVENT_SENDER.with(|s| {
        *s.borrow_mut() = Some(sender);
    });
}

/// Gets the global system command sender.
///
/// # Panics
///
/// Panics if the sender is uninitialized.
#[must_use]
pub fn get_system_command_sender() -> tokio::sync::mpsc::UnboundedSender<SystemCommand> {
    SYSTEM_COMMAND_SENDER.with(|sender| {
        sender
            .borrow()
            .as_ref()
            .expect("System command sender should be initialized by runner")
            .clone()
    })
}

/// Attempts to get the global system command sender without panicking.
///
/// Returns `None` if the sender is not initialized.
#[must_use]
pub fn try_get_system_command_sender() -> Option<tokio::sync::mpsc::UnboundedSender<SystemCommand>>
{
    SYSTEM_COMMAND_SENDER.with(|sender| sender.borrow().as_ref().cloned())
}

/// Sets the global system command sender.
///
/// Can only be called once per thread.
///
/// # Panics
///
/// Panics if a sender has already been set.
pub fn set_system_command_sender(sender: tokio::sync::mpsc::UnboundedSender<SystemCommand>) {
    SYSTEM_COMMAND_SENDER.with(|s| {
        let mut slot = s.borrow_mut();
        assert!(slot.is_none(), "System command sender can only be set once");
        *slot = Some(sender);
    });
}

/// Replaces the global system command sender for the current thread.
pub fn replace_system_command_sender(sender: tokio::sync::mpsc::UnboundedSender<SystemCommand>) {
    SYSTEM_COMMAND_SENDER.with(|s| {
        *s.borrow_mut() = Some(sender);
    });
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
            .borrow()
            .as_ref()
            .expect("Execution event sender should be initialized by runner")
            .clone()
    })
}

/// Attempts to get the global execution event sender without panicking.
///
/// Returns `None` if the sender is not initialized (e.g., in test environments).
#[must_use]
pub fn try_get_exec_event_sender() -> Option<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>> {
    EXEC_EVENT_SENDER.with(|sender| sender.borrow().as_ref().cloned())
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
        let mut slot = s.borrow_mut();
        assert!(
            slot.is_none(),
            "Execution event sender can only be set once"
        );
        *slot = Some(sender);
    });
}

/// Replaces the global execution event sender for the current thread.
pub fn replace_exec_event_sender(sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>) {
    EXEC_EVENT_SENDER.with(|s| {
        *s.borrow_mut() = Some(sender);
    });
}

thread_local! {
    static DATA_EVENT_SENDER: RefCell<Option<tokio::sync::mpsc::UnboundedSender<DataEvent>>> = const { RefCell::new(None) };
    static EXEC_EVENT_SENDER: RefCell<Option<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>>> = const { RefCell::new(None) };
    static SYSTEM_EVENT_SENDER: RefCell<Option<tokio::sync::mpsc::UnboundedSender<SystemEvent>>> = const { RefCell::new(None) };
    static SYSTEM_COMMAND_SENDER: RefCell<Option<tokio::sync::mpsc::UnboundedSender<SystemCommand>>> = const { RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_replace_data_event_sender_overwrites_previous() {
        assert_sender_replaced(replace_data_event_sender, get_data_event_sender);
    }

    #[rstest]
    fn test_replace_exec_event_sender_overwrites_previous() {
        assert_sender_replaced(replace_exec_event_sender, get_exec_event_sender);
    }

    #[rstest]
    fn test_replace_system_event_sender_overwrites_previous() {
        assert_sender_replaced(replace_system_event_sender, get_system_event_sender);
    }

    #[rstest]
    fn test_replace_system_command_sender_overwrites_previous() {
        assert_sender_replaced(replace_system_command_sender, get_system_command_sender);
    }

    #[rstest]
    fn test_event_senders_are_thread_local() {
        assert_sender_thread_local(replace_data_event_sender, get_data_event_sender);
        assert_sender_thread_local(replace_exec_event_sender, get_exec_event_sender);
        assert_sender_thread_local(replace_system_event_sender, get_system_event_sender);
        assert_sender_thread_local(replace_system_command_sender, get_system_command_sender);
    }

    #[rstest]
    fn test_set_data_event_sender_panics_on_double_set() {
        let result = std::thread::spawn(|| {
            let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
            let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
            set_data_event_sender(tx1);
            set_data_event_sender(tx2);
        })
        .join();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_set_exec_event_sender_panics_on_double_set() {
        let result = std::thread::spawn(|| {
            let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
            let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
            set_exec_event_sender(tx1);
            set_exec_event_sender(tx2);
        })
        .join();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_set_system_event_sender_panics_on_double_set() {
        let result = std::thread::spawn(|| {
            let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
            let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
            set_system_event_sender(tx1);
            set_system_event_sender(tx2);
        })
        .join();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_set_system_command_sender_panics_on_double_set() {
        let result = std::thread::spawn(|| {
            let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
            let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
            set_system_command_sender(tx1);
            set_system_command_sender(tx2);
        })
        .join();
        assert!(result.is_err());
    }

    #[rstest]
    fn test_try_get_exec_event_sender_returns_none_when_unset() {
        let result = std::thread::spawn(try_get_exec_event_sender)
            .join()
            .unwrap();
        assert!(result.is_none());
    }

    #[rstest]
    fn test_try_get_system_event_sender_returns_none_when_unset() {
        let result = std::thread::spawn(try_get_system_event_sender)
            .join()
            .unwrap();
        assert!(result.is_none());
    }

    #[rstest]
    fn test_try_get_system_command_sender_returns_none_when_unset() {
        let result = std::thread::spawn(try_get_system_command_sender)
            .join()
            .unwrap();
        assert!(result.is_none());
    }

    fn assert_sender_replaced<T: Send + 'static>(
        replace: fn(tokio::sync::mpsc::UnboundedSender<T>),
        get: fn() -> tokio::sync::mpsc::UnboundedSender<T>,
    ) {
        std::thread::spawn(move || {
            let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
            let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();

            replace(tx1.clone());
            replace(tx2.clone());
            let sender = get();

            assert!(!sender.same_channel(&tx1));
            assert!(sender.same_channel(&tx2));
        })
        .join()
        .expect("sender replacement test thread should join");
    }

    fn assert_sender_thread_local<T: Send + 'static>(
        replace: fn(tokio::sync::mpsc::UnboundedSender<T>),
        get: fn() -> tokio::sync::mpsc::UnboundedSender<T>,
    ) {
        let barrier = Arc::new(Barrier::new(2));
        let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
        let expected1 = tx1.clone();
        let expected2 = tx2.clone();

        let barrier1 = Arc::clone(&barrier);

        let thread1 = std::thread::spawn(move || {
            replace(tx1);
            barrier1.wait();
            assert!(get().same_channel(&expected1));
        });

        let thread2 = std::thread::spawn(move || {
            replace(tx2);
            barrier.wait();
            assert!(get().same_channel(&expected2));
        });

        thread1
            .join()
            .expect("first sender isolation test thread should join");
        thread2
            .join()
            .expect("second sender isolation test thread should join");
    }
}
