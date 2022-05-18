use nautilus_core::uuid::UUID4;

/// Index of name string in global string store
pub type NameID = u64;
/// Unix time stamp in nanoseconds
pub type TimeNS = u64;

#[derive(Clone, Hash, Debug)]
/// Represents a time event occurring at the event timestamp.
pub struct TimeEvent {
    /// The event name.
    pub name: NameID,
    /// The event ID.
    pub id: UUID4,
    /// The UNIX timestamp (nanoseconds) when the time event occurred.
    pub ts_event: TimeNS,
    /// The UNIX timestamp (nanoseconds) when the object was initialized.
    pub ts_init: TimeNS,
}

pub trait Timer {
    fn pop_event(&self, event_id: UUID4, ts_init: TimeNS) -> TimeEvent;
    fn iterate_next_time(&mut self, now_ns: TimeNS);
    fn cancel(&mut self);
}

#[allow(dead_code)]
pub struct TestTimer {
    name: NameID,
    interval_ns: TimeNS,
    start_time_ns: TimeNS,
    stop_time_ns: Option<TimeNS>,
    pub next_time_ns: TimeNS,
    pub is_expired: bool,
}

impl TestTimer {
    pub fn new(
        name: NameID,
        interval_ns: TimeNS,
        start_time_ns: TimeNS,
        stop_time_ns: Option<TimeNS>,
    ) -> Self {
        TestTimer {
            name,
            interval_ns,
            start_time_ns,
            stop_time_ns,
            next_time_ns: start_time_ns + interval_ns,
            is_expired: false,
        }
    }
    pub fn advance(&mut self, to_time_ns: TimeNS) -> impl Iterator<Item = TimeEvent> + '_ {
        self.take_while(move |(_, next_time)| to_time_ns >= *next_time)
            .map(|(event, _)| event)
    }
    pub fn pop_next_event(&mut self) -> TimeEvent {
        self.next().unwrap().0
    }
}

impl Iterator for TestTimer {
    type Item = (TimeEvent, TimeNS);

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

            self.next_time_ns += self.interval_ns;

            Some(item)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TestTimer, TimeEvent};

    #[test]
    fn pop_event() {
        let mut timer = TestTimer::new(0, 0, 1, None);
        assert!(timer.next().is_some());
        assert!(timer.next().is_some());
        timer.is_expired = true;
        assert!(timer.next().is_none());
    }

    #[test]
    fn advance() {
        let mut timer = TestTimer::new(0, 1, 0, None);
        let events: Vec<TimeEvent> = timer.advance(5).collect();
        assert_eq!(events.len(), 5);
    }

    #[test]
    fn advance_stop() {
        let mut timer = TestTimer::new(0, 1, 0, Some(5));
        let events: Vec<TimeEvent> = timer.advance(10).collect();
        assert_eq!(events.len(), 5);
    }
}
