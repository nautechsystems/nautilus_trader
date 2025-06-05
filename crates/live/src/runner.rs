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
    msgbus::{self, switchboard::MessagingSwitchboard},
    runner::{DataQueue, RunnerEvent, set_data_event_sender, set_data_evt_queue},
};
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

pub trait Runner {
    fn run(&mut self);
}

pub struct AsyncRunner {
    pub clock: Rc<RefCell<LiveClock>>,
    data_rx: UnboundedReceiver<DataEvent>,
    signal_rx: UnboundedReceiver<()>,
}

impl Debug for AsyncRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(AsyncRunner))
            .field("clock_set", &true)
            .finish()
    }
}

impl AsyncRunner {
    pub fn new(clock: Rc<RefCell<LiveClock>>) -> (Self, UnboundedSender<()>) {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        // Set up the global data event sender for direct access
        set_data_event_sender(data_tx.clone());

        // Also keep the existing AsyncDataQueue for backward compatibility
        set_data_evt_queue(Rc::new(RefCell::new(AsyncDataQueue(data_tx))));

        let runner = Self {
            clock,
            data_rx,
            signal_rx,
        };

        (runner, signal_tx)
    }
}

impl AsyncRunner {
    /// Runs the async runner event loop.
    ///
    /// This method processes data events, time events, and signal events in an async loop.
    /// It will run until a signal is received or the event streams are closed.
    pub async fn run(&mut self) {
        log::info!("Starting AsyncRunner");

        let mut time_event_stream = self.clock.borrow().get_event_stream();

        let data_engine_process = MessagingSwitchboard::data_engine_process();
        let data_engine_response = MessagingSwitchboard::data_engine_response();

        loop {
            // Collect the next event to process, including signal events
            let next_event = tokio::select! {
                Some(resp) = self.data_rx.recv() => RunnerEvent::Data(resp),
                Some(event) = time_event_stream.next() => RunnerEvent::Time(event),
                Some(_) = self.signal_rx.recv() => {
                    tracing::info!("AsyncRunner received signal, shutting down");
                    return; // Signal to stop
                },
                else => return, // Sentinel event ends run
            };

            match next_event {
                RunnerEvent::Time(event) => self.clock.borrow().get_handler(event).run(),
                RunnerEvent::Data(event) => match event {
                    DataEvent::Data(data) => msgbus::send(data_engine_process, &data),
                    DataEvent::Response(resp) => msgbus::send(data_engine_response, &resp),
                },
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
