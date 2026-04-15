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

use crate::messages::{DataEvent, ExecutionEvent};

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
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_replace_data_event_sender_overwrites_previous() {
        std::thread::spawn(|| {
            let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
            let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
            replace_data_event_sender(tx1);
            replace_data_event_sender(tx2);
            let _sender = get_data_event_sender();
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_replace_exec_event_sender_overwrites_previous() {
        std::thread::spawn(|| {
            let (tx1, _rx1) = tokio::sync::mpsc::unbounded_channel();
            let (tx2, _rx2) = tokio::sync::mpsc::unbounded_channel();
            replace_exec_event_sender(tx1);
            replace_exec_event_sender(tx2);
            let _sender = get_exec_event_sender();
        })
        .join()
        .unwrap();
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
    fn test_try_get_exec_event_sender_returns_none_when_unset() {
        let result = std::thread::spawn(try_get_exec_event_sender)
            .join()
            .unwrap();
        assert!(result.is_none());
    }
}
