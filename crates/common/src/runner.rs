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
//! system-wide components that need to be accessible across the application.

use std::{
    cell::{OnceCell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

use tokio::sync::mpsc::UnboundedSender;

use crate::{
    clock::Clock,
    messages::{DataEvent, data::DataCommand},
    timer::TimeEvent,
};

pub type GlobalClock = Rc<RefCell<dyn Clock>>;

/// # Panics
///
/// Panics if thread-local storage cannot be accessed or the global clock is uninitialized.
#[must_use]
pub fn get_global_clock() -> Rc<RefCell<dyn Clock>> {
    CLOCK
        .try_with(|clock| {
            clock
                .get()
                .expect("Clock should be initialized by runner")
                .clone()
        })
        .expect("Should be able to access thread local storage")
}

/// # Panics
///
/// Panics if thread-local storage cannot be accessed or the global clock is already set.
pub fn set_global_clock(c: Rc<RefCell<dyn Clock>>) {
    CLOCK
        .try_with(|clock| {
            assert!(clock.set(c).is_ok(), "Global clock already set");
        })
        .expect("Should be able to access thread local clock");
}

pub type DataCommandQueue = Rc<RefCell<VecDeque<DataCommand>>>;

/// Get globally shared message bus command queue
///
/// # Panics
///
/// Panics if thread-local storage cannot be accessed.
#[must_use]
pub fn get_data_cmd_queue() -> DataCommandQueue {
    DATA_CMD_QUEUE
        .try_with(std::clone::Clone::clone)
        .expect("Should be able to access thread local storage")
}

pub trait DataQueue {
    fn push(&mut self, event: DataEvent);
}

pub type GlobalDataQueue = Rc<RefCell<dyn DataQueue>>;

#[derive(Debug)]
pub struct SyncDataQueue(VecDeque<DataEvent>);

impl DataQueue for SyncDataQueue {
    fn push(&mut self, event: DataEvent) {
        self.0.push_back(event);
    }
}

/// # Panics
///
/// Panics if thread-local storage cannot be accessed or the data event queue is uninitialized.
#[must_use]
pub fn get_data_evt_queue() -> Rc<RefCell<dyn DataQueue>> {
    DATA_EVT_QUEUE
        .try_with(|dq| {
            dq.get()
                .expect("Data queue should be initialized by runner")
                .clone()
        })
        .expect("Should be able to access thread local storage")
}

/// # Panics
///
/// Panics if thread-local storage cannot be accessed or the global data event queue is already set.
pub fn set_data_evt_queue(dq: Rc<RefCell<dyn DataQueue>>) {
    DATA_EVT_QUEUE
        .try_with(|deque| {
            assert!(deque.set(dq).is_ok(), "Global data queue already set");
        })
        .expect("Should be able to access thread local storage");
}

/// Sends a data event to the global data event queue.
///
/// This function provides a convenient way for data clients and feed handlers
/// to send data events to the AsyncRunner for processing.
///
/// # Panics
///
/// Panics if thread-local storage cannot be accessed or the data event queue is uninitialized.
pub fn send_data_event(event: DataEvent) {
    get_data_evt_queue().borrow_mut().push(event);
}

/// Sets the global data event sender.
///
/// This should be called by the AsyncRunner when it creates the channel.
///
/// # Panics
///
/// Panics if thread-local storage cannot be accessed or a sender is already set.
pub fn set_data_event_sender(sender: UnboundedSender<DataEvent>) {
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
    static CLOCK: OnceCell<GlobalClock> = OnceCell::new();
    static DATA_EVT_QUEUE: OnceCell<GlobalDataQueue> = OnceCell::new();
    static DATA_CMD_QUEUE: DataCommandQueue = Rc::new(RefCell::new(VecDeque::new()));
    // TODO: Potentially redundant but added to simplify the abstraction layers for now
    static DATA_EVT_SENDER: OnceCell<UnboundedSender<DataEvent>> = const { OnceCell::new() };
}

// Represents different event types for the runner.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum RunnerEvent {
    Time(TimeEvent),
    Data(DataEvent),
}
