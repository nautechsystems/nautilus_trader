use std::collections::HashMap;
use super::timer::TestTimer;

struct TestClock {
   time_ns: u64,
   is_test_clock: bool,
   timers: HashMap<String, TestTimer>,
   timer_counter: u64,
   next_event_name: Option<String>,
   next_event_time: Option<u64>,
   next_event_time_ns: u64
}

impl TestClock {
    fn new(initial_ns: u64) -> TestClock {
        TestClock {
            time_ns: initial_ns,
            is_test_clock: true,
            timers: HashMap::new(),
            timer_counter: 0,
            next_event_name: None,
            next_event_time: None,
            next_event_time_ns: 0,
        }
    }
}