// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::ffi::CStr;

use nautilus_common::clock::{test_clock_new, test_clock_set_time_alert_ns};

// #[test]
// fn test_clock_advance() {
//     unsafe {
//         let mut clock = test_clock_new();
//         let timer_name = "test-timer-001";
//         let name_ptr = CStr::from_bytes_with_nul_unchecked(timer_name.as_bytes()).as_ptr();
//
//         test_clock_set_time_alert_ns(&mut clock, name_ptr, 2_000);
//         assert_eq!(clock.timers.len(), 1);
//         assert_eq!(clock.timers.keys().next().unwrap().as_str(), timer_name);
//
//         let events = clock.advance_time(3_000, true);
//
//         assert!(clock.timers.values().next().unwrap().is_expired);
//         assert_eq!(events.len(), 1);
//         assert_eq!(
//             events.first().unwrap().name.to_string(),
//             String::from_str(timer_name).unwrap()
//         );
//     }
// }

#[test]
fn test_clock_event_callback() {
    unsafe {
        let mut test_clock = test_clock_new();
        let timer_name = "test-timer-001";
        let name_ptr = CStr::from_bytes_with_nul_unchecked(timer_name.as_bytes()).as_ptr();
        test_clock_set_time_alert_ns(&mut test_clock, name_ptr, 2_000);
        let events = test_clock.advance_time(3_000, true);
        assert_eq!(events.len(), 1); // TODO
    }
}
