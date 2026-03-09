//! Tokio-based channel senders for live trading runtime.
//!
//! This module provides thread-local storage for tokio mpsc channels used in live trading.

use std::cell::OnceCell;

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
        assert!(
            s.set(sender).is_ok(),
            "Data event sender can only be set once"
        );
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
        assert!(
            s.set(sender).is_ok(),
            "Execution event sender can only be set once"
        );
    });
}

thread_local! {
    static DATA_EVENT_SENDER: OnceCell<tokio::sync::mpsc::UnboundedSender<DataEvent>> = const { OnceCell::new() };
    static EXEC_EVENT_SENDER: OnceCell<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>> = const { OnceCell::new() };
}
