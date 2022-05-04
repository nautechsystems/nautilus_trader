use nautilus_core::{buffer::Buffer64, uuid::UUID4};

#[derive(Clone, Hash, Debug)]
pub struct TimeEvent {
    name: Buffer64,
    pub id: UUID4,
    pub ts_event: u64,
    pub ts_init: u64,
}

impl PartialEq for TimeEvent {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for TimeEvent {
    fn assert_receiver_is_total_eq(&self) {}
}

trait Timer {
    fn pop_event(&self, event_id: UUID4, ts_init: u64) -> TimeEvent;
    fn iterate_next_time(&mut self, now_ns: u64);
    fn cancel(&mut self);
}

pub struct TestTimer {
    name: Buffer64,
    // callback: PyObject,
    interval_ns: u64,
    start_time_ns: u64,
    stop_time_ns: u64,
    next_time_ns: u64,
    is_expired: bool,
}

impl Timer for TestTimer {
    fn pop_event(&self, event_id: UUID4, ts_init: u64) -> TimeEvent {
        TimeEvent {
            name: self.name.clone(),
            id: event_id,
            ts_event: self.next_time_ns,
            ts_init,
        }
    }
    fn iterate_next_time(&mut self, now_ns: u64) {
        self.next_time_ns += self.interval_ns;
        if self.stop_time_ns <= now_ns {
            self.is_expired = true
        }
    }
    fn cancel(&mut self) {
        self.is_expired = true;
    }
}

impl TestTimer {
    fn new(
        name: Buffer64,
        // callback: PyObject,
        interval_ns: u64,
        start_time_ns: u64,
        stop_time_ns: Option<u64>,
    ) -> Self {
        TestTimer {
            name,
            // callback,
            interval_ns,
            start_time_ns,
            stop_time_ns: stop_time_ns.unwrap_or(0),
            next_time_ns: start_time_ns + interval_ns,
            is_expired: false,
        }
    }
    fn list_advance(&mut self, to_time_ns: u64) -> Vec<TimeEvent> {
        self.take_while(|(_, next_time)| to_time_ns >= *next_time)
            .map(|(event, _)| event)
            .collect()
    }
    fn pop_next_event(&mut self) -> TimeEvent {
        self.next().unwrap().0
    }
}

impl Iterator for TestTimer {
    type Item = (TimeEvent, u64);

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_expired {
            None
        } else {
            let event = TimeEvent {
                name: self.name.clone(),
                id: UUID4::new(),
                ts_event: self.next_time_ns,
                ts_init: self.next_time_ns,
            };
            self.next_time_ns += self.interval_ns;
            Some((event, self.next_time_ns))
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::buffer::Buffer64;

    use super::TestTimer;

    #[test]
    fn pop_event() {
        let mut timer = TestTimer::new(Buffer64::from("test"), 0, 1, None);
        assert!(timer.next().is_some());
        assert!(timer.next().is_some());
        timer.is_expired = true;
        assert!(timer.next().is_none());
    }

    #[test]
    fn list_advance() {
        let mut timer = TestTimer::new(Buffer64::from("test"), 1, 0, None);
        let events = timer.list_advance(5);
        assert_eq!(events.len(), 4);
    }
}
