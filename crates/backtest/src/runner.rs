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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
    cell::{OnceCell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

use nautilus_common::{
    clock::{Clock, TestClock},
    messages::data::SubscribeCommand,
    runner::{DataEvent, DataQueue, DataResponseQueue, GlobalDataQueue},
};
use nautilus_data::engine::DataEngine;

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

pub type MessageBusCommands = Rc<RefCell<VecDeque<SubscribeCommand>>>;

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

// TODO: Determine how to deduplicate trait
pub trait Runner {
    fn new() -> Self;
    fn run(&mut self, engine: &mut DataEngine);
}

pub struct BacktestRunner {
    pub dq: DataResponseQueue,
    pub clock: Rc<RefCell<TestClock>>,
}

// TODO: Untangle puzzle later
// impl Runner for BacktestRunner {
//     fn new() -> Self {
//         let clock = Rc::new(RefCell::new(TestClock::new()));
//         set_clock(clock.clone());
//
//         let dq = Rc::new(RefCell::new(SyncDataQueue(VecDeque::new())));
//         set_data_queue(dq.clone());
//         Self { dq, clock }
//     }
//
//     fn run(&mut self, engine: &mut DataEngine) {
//         let msgbus_cmd = get_msgbus_cmd();
//
//         while let Some(resp) = self.dq.as_ref().borrow_mut().0.pop_front() {
//             match resp {
//                 DataEvent::Response(resp) => engine.response(resp),
//                 DataEvent::Data(data) => {
//                     while let Some(sub_cmd) = msgbus_cmd.borrow_mut().pop_front() {
//                         engine.execute(sub_cmd);
//                     }
//
//                     // Advance clock time and collect all triggered events and handlers
//                     let handlers: Vec<TimeEventHandlerV2> = {
//                         let mut guard = self.clock.borrow_mut();
//                         guard.advance_to_time_on_heap(data.ts_init());
//                         guard.by_ref().collect()
//                         // drop guard
//                     };
//
//                     // Execute all handlers before processing the data
//                     handlers.into_iter().for_each(TimeEventHandlerV2::run);
//
//                     engine.process_data(data);
//                 }
//             }
//         }
//     }
// }

#[cfg(test)]
#[cfg(feature = "clock_v2")]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use futures::StreamExt;
    use nautilus_common::{
        clock::{LiveClock, TestClock},
        timer::{TimeEvent, TimeEventCallback},
    };
    use rstest::rstest;

    use super::{get_clock, set_clock};

    #[rstest]
    fn test_global_test_clock() {
        let test_clock = Rc::new(RefCell::new(TestClock::new()));
        set_clock(test_clock.clone());

        // component/actor adding an alert
        get_clock().borrow_mut().set_time_alert_ns(
            "hola",
            2.into(),
            Some(TimeEventCallback::Rust(Rc::new(|event: TimeEvent| {}))),
        );

        // runner pulling advancing and pulling from event stream
        test_clock.borrow_mut().advance_to_time_on_heap(3.into());
        assert!(test_clock.borrow_mut().next().is_some());
    }
}
