// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use futures::StreamExt;
use nautilus_common::{
    clock::{set_clock, Clock, LiveClock, TestClock},
    messages::data::{DataEvent, DataResponse},
    runtime::get_runtime,
    timer::{TimeEvent, TimeEventHandlerV2},
};
use nautilus_model::data::GetTsInit;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::DataEngine;

pub trait Runner {
    type Sender;

    fn new() -> Self;
    fn run(&mut self, engine: &mut DataEngine);
    fn get_sender(&self) -> Self::Sender;
}

pub trait SendResponse {
    fn send(&self, resp: DataResponse);
}

pub type DataResponseQueue = Rc<RefCell<VecDeque<DataEvent>>>;

pub struct BacktestRunner {
    queue: DataResponseQueue,
    pub clock: Rc<RefCell<TestClock>>,
}

impl Runner for BacktestRunner {
    type Sender = DataResponseQueue;

    fn new() -> Self {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        set_clock(clock.clone());

        Self {
            queue: Rc::new(RefCell::new(VecDeque::new())),
            clock,
        }
    }

    fn run(&mut self, engine: &mut DataEngine) {
        while let Some(resp) = self.queue.as_ref().borrow_mut().pop_front() {
            match resp {
                DataEvent::Response(resp) => engine.response(resp),
                DataEvent::Data(data) => {
                    // Advance clock time and collect all triggered events and handlers
                    let handlers: Vec<TimeEventHandlerV2> = {
                        let mut guard = self.clock.borrow_mut();
                        guard.advance_to_time_on_heap(data.ts_init());
                        guard.by_ref().collect()
                        // drop guard
                    };

                    // Execute all handlers before processing the data
                    handlers.into_iter().for_each(TimeEventHandlerV2::run);

                    engine.process_data(data);
                }
            }
        }
    }

    fn get_sender(&self) -> Self::Sender {
        self.queue.clone()
    }
}

pub struct LiveRunner {
    resp_tx: UnboundedSender<DataEvent>,
    resp_rx: UnboundedReceiver<DataEvent>,
    pub clock: Rc<RefCell<LiveClock>>,
}

impl Runner for LiveRunner {
    type Sender = UnboundedSender<DataEvent>;

    fn new() -> Self {
        let (resp_tx, resp_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();

        let clock = Rc::new(RefCell::new(LiveClock::new()));
        set_clock(clock.clone());

        Self {
            resp_tx,
            resp_rx,
            clock,
        }
    }

    fn run(&mut self, engine: &mut DataEngine) {
        let mut time_event_stream = self.clock.borrow().get_event_stream();
        loop {
            // Collect the next event to process
            let next_event = get_runtime().block_on(async {
                tokio::select! {
                    Some(resp) = self.resp_rx.recv() => Some(RunnerEvent::Data(resp)),
                    Some(event) = time_event_stream.next() => Some(RunnerEvent::Timer(event)),
                    else => None,
                }
            });

            // Process the event outside of the async context
            match next_event {
                Some(RunnerEvent::Data(resp)) => match resp {
                    DataEvent::Response(resp) => engine.response(resp),
                    DataEvent::Data(data) => engine.process_data(data),
                },
                Some(RunnerEvent::Timer(event)) => self.clock.borrow().get_handler(event).run(),
                None => break,
            }
        }
    }

    fn get_sender(&self) -> Self::Sender {
        self.resp_tx.clone()
    }
}

// Helper enum to represent different event types
#[allow(clippy::large_enum_variant)]
enum RunnerEvent {
    Data(DataEvent),
    Timer(TimeEvent),
}
