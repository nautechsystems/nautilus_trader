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

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use futures::StreamExt;
use nautilus_common::{
    clock::{Clock, LiveClock},
    messages::DataEvent,
    runner::{DataQueue, RunnerEvent, get_data_cmd_queue, set_data_evt_queue},
    runtime::get_runtime,
};
use nautilus_data::engine::DataEngine;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub struct AsyncDataQueue(UnboundedSender<DataEvent>);

impl Debug for AsyncDataQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple(stringify!(AsyncDataQueue)).finish()
    }
}

impl DataQueue for AsyncDataQueue {
    fn push(&mut self, event: DataEvent) {
        if let Err(e) = self.0.send(event) {
            log::error!("Unable to send data event to async data channel: {e}");
        }
    }
}

// TODO: Use message bus instead of direct reference to DataEngine
pub trait Runner {
    fn run(&mut self, data_engine: &mut DataEngine);
}

pub struct AsyncRunner {
    pub clock: Rc<RefCell<LiveClock>>,
    data_rx: UnboundedReceiver<DataEvent>,
}

impl Debug for AsyncRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(AsyncRunner))
            .field("clock_set", &true)
            .finish()
    }
}

impl AsyncRunner {
    pub fn new(clock: Rc<RefCell<LiveClock>>) -> Self {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_evt_queue(Rc::new(RefCell::new(AsyncDataQueue(data_tx))));

        Self { clock, data_rx }
    }
}

impl Runner for AsyncRunner {
    fn run(&mut self, data_engine: &mut DataEngine) {
        let mut time_event_stream = self.clock.borrow().get_event_stream();
        let data_cmd_queue = get_data_cmd_queue();

        loop {
            while let Some(cmd) = data_cmd_queue.borrow_mut().pop_front() {
                // TODO: Send to data engine execute endpoint address
                data_engine.execute(&cmd);
            }

            // Collect the next event to process
            let next_event = get_runtime().block_on(async {
                tokio::select! {
                    Some(resp) = self.data_rx.recv() => Some(RunnerEvent::Data(resp)),
                    Some(event) = time_event_stream.next() => Some(RunnerEvent::Timer(event)),
                    else => None,
                }
            });

            // Process the event outside of the async context
            match next_event {
                Some(RunnerEvent::Data(event)) => match event {
                    DataEvent::Response(resp) => data_engine.response(resp),
                    DataEvent::Data(data) => data_engine.process_data(data),
                },
                Some(RunnerEvent::Timer(event)) => self.clock.borrow().get_handler(event).run(),
                None => break, // Sentinel event ends runner
            }
        }
    }
}

#[cfg(test)]
#[cfg(feature = "clock_v2")]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use futures::StreamExt;
    use nautilus_common::{
        clock::LiveClock,
        runner::{get_global_clock, set_global_clock},
        timer::{TimeEvent, TimeEventCallback},
    };

    #[tokio::test]
    async fn test_global_live_clock() {
        let live_clock = Rc::new(RefCell::new(LiveClock::new()));
        set_global_clock(live_clock.clone());
        let alert_time = live_clock.borrow().get_time_ns() + 100;

        // component/actor adding an alert
        let _ = get_global_clock().borrow_mut().set_time_alert_ns(
            "hola",
            alert_time,
            Some(TimeEventCallback::Rust(Rc::new(|_event: TimeEvent| {}))),
            None,
        );

        // runner pulling from event
        assert!(
            live_clock
                .borrow()
                .get_event_stream()
                .next()
                .await
                .is_some()
        );
    }
}
