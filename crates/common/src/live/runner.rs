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
        if s.set(sender).is_err() {
            panic!("Data event sender can only be set once");
        }
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
        if s.set(sender).is_err() {
            panic!("Execution event sender can only be set once");
        }
    });
}

thread_local! {
    static DATA_EVENT_SENDER: OnceCell<tokio::sync::mpsc::UnboundedSender<DataEvent>> = const { OnceCell::new() };
    static EXEC_EVENT_SENDER: OnceCell<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>> = const { OnceCell::new() };
}
