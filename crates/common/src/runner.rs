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
    collections::VecDeque,
    rc::Rc,
};

use nautilus_model::data::Data;

use crate::{
    clock::Clock,
    messages::data::{DataResponse, SubscribeCommand},
    timer::TimeEvent,
};

pub trait DataQueue {
    fn push(&mut self, event: DataEvent);
}

pub type GlobalDataQueue = Rc<RefCell<dyn DataQueue>>;

// TODO: Refine this to reduce disparity between enum sizes
#[allow(clippy::large_enum_variant)]
pub enum DataEvent {
    Response(DataResponse),
    Data(Data),
}

pub struct SyncDataQueue(VecDeque<DataEvent>);

impl DataQueue for SyncDataQueue {
    fn push(&mut self, event: DataEvent) {
        self.0.push_back(event);
    }
}

#[must_use]
pub fn get_data_queue() -> Rc<RefCell<dyn DataQueue>> {
    DATA_QUEUE
        .try_with(|dq| {
            dq.get()
                .expect("Data queue should be initialized by runner")
                .clone()
        })
        .expect("Should be able to access thread local storage")
}

pub fn set_data_queue(dq: Rc<RefCell<dyn DataQueue>>) {
    DATA_QUEUE
        .try_with(|deque| {
            assert!(deque.set(dq).is_ok(), "Global data queue already set");
        })
        .expect("Should be able to access thread local storage");
}

pub type GlobalClock = Rc<RefCell<dyn Clock>>;

#[must_use]
pub fn get_clock() -> Rc<RefCell<dyn Clock>> {
    CLOCK
        .try_with(|clock| {
            clock
                .get()
                .expect("Clock should be initialized by runner")
                .clone()
        })
        .expect("Should be able to access thread local storage")
}

pub fn set_clock(c: Rc<RefCell<dyn Clock>>) {
    CLOCK
        .try_with(|clock| {
            assert!(clock.set(c).is_ok(), "Global clock already set");
        })
        .expect("Should be able to access thread local clock");
}

pub type MessageBusCommands = Rc<RefCell<VecDeque<SubscribeCommand>>>; // TODO: Use DataCommand?

/// Get globally shared message bus command queue
#[must_use]
pub fn get_msgbus_cmd() -> MessageBusCommands {
    MSGBUS_CMD
        .try_with(std::clone::Clone::clone)
        .expect("Should be able to access thread local storage")
}

thread_local! {
    static CLOCK: OnceCell<GlobalClock> = OnceCell::new();
    static DATA_QUEUE: OnceCell<GlobalDataQueue> = OnceCell::new();
    static MSGBUS_CMD: MessageBusCommands = Rc::new(RefCell::new(VecDeque::new()));
}

pub trait SendResponse {
    fn send(&self, resp: DataResponse);
}

pub type DataResponseQueue = Rc<RefCell<SyncDataQueue>>;

// Represents different event types for the runner.
#[allow(clippy::large_enum_variant)]
pub enum RunnerEvent {
    Data(DataEvent),
    Timer(TimeEvent),
}
