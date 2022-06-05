// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::time::{Timedelta, Timestamp};
use nautilus_core::uuid::UUID4;

/// Index of name string in global string store
pub type NameID = u64;

#[derive(Clone, Hash, Debug)]
/// Represents a time event occurring at the event timestamp.
pub struct TimeEvent {
    /// The event name.
    pub name: NameID,
    /// The event ID.
    pub id: UUID4,
    /// The UNIX timestamp (nanoseconds) when the time event occurred.
    pub ts_event: Timestamp,
    /// The UNIX timestamp (nanoseconds) when the object was initialized.
    pub ts_init: Timestamp,
}

pub trait Timer {
    fn pop_event(&self, event_id: UUID4, ts_init: Timestamp) -> TimeEvent;
    fn iterate_next_time(&mut self, ts_now: Timestamp);
    fn cancel(&mut self);
}

#[allow(dead_code)]
pub struct TestTimer {
    name: NameID,
    interval_ns: Timedelta,
    start_time_ns: Timestamp,
    stop_time_ns: Option<Timestamp>,
    pub next_time_ns: Timestamp,
    pub is_expired: bool,
}

impl TestTimer {
    pub fn new(
        name: NameID,
        interval_ns: Timedelta,
        start_time_ns: Timestamp,
        stop_time_ns: Option<Timestamp>,
    ) -> Self {
        TestTimer {
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            next_time_ns: start_time_ns + interval_ns as u64,
            is_expired: false,
        }
    }
    pub fn advance(&mut self, to_time_ns: Timestamp) -> impl Iterator<Item = TimeEvent> + '_ {
        self.take_while(move |(_, next_time_ns)| to_time_ns >= *next_time_ns)
            .map(|(event, _)| event)
    }
    pub fn pop_next_event(&mut self) -> TimeEvent {
        self.next().unwrap().0
    }
}

impl Iterator for TestTimer {
    type Item = (TimeEvent, Timestamp);

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_expired {
            None
        } else {
            let item = (
                TimeEvent {
                    name: self.name,
                    id: UUID4::new(),
                    ts_event: self.next_time_ns,
                    ts_init: self.next_time_ns,
                },
                self.next_time_ns,
            );

            // if current next event time has exceeded
            // stop time expire timer
            if let Some(stop_time_ns) = self.stop_time_ns {
                if self.next_time_ns >= stop_time_ns {
                    self.is_expired = true;
                }
            }

            self.next_time_ns += self.interval_ns as u64;

            Some(item)
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::{TestTimer, TimeEvent};

    #[test]
    fn test_pop_event() {
        let mut timer = TestTimer::new(0, 0, 1, None);
        assert!(timer.next().is_some());
        assert!(timer.next().is_some());
        timer.is_expired = true;
        assert!(timer.next().is_none());
    }

    #[test]
    fn test_advance() {
        let mut timer = TestTimer::new(0, 1, 0, None);
        let events: Vec<TimeEvent> = timer.advance(5).collect();
        assert_eq!(events.len(), 5);
    }

    #[test]
    fn test_advance_stop() {
        let mut timer = TestTimer::new(0, 1, 0, Some(5));
        let events: Vec<TimeEvent> = timer.advance(10).collect();
        assert_eq!(events.len(), 5);
    }
}
