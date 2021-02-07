// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

use chrono::offset::Utc;
use chrono::Datelike;
use chrono::Timelike;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;


/// Represents a datetime
#[repr(C)]
#[derive(PartialEq, Copy, Clone)]
pub struct DateTime
{
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub microsecond: u32,
}


#[no_mangle]
/// Return the current date time in UTC.
pub extern "C" fn c_utc_now() -> DateTime {
    let utc = Utc::now();
    return DateTime {
        year: utc.year() as u32,
        month: utc.month(),
        day: utc.day(),
        hour: utc.hour(),
        minute: utc.minute(),
        second: utc.second(),
        microsecond: utc.timestamp_subsec_micros(),
    };
}


#[no_mangle]
/// Return the current seconds since the Unix epoch.
pub extern "C" fn c_timestamp() -> f64 {
    return SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_secs_f64();
}


#[no_mangle]
/// Return the current milliseconds since the Unix epoch.
pub extern "C" fn c_timestamp_ms() -> u64 {
    return SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_millis() as u64;
}


#[no_mangle]
/// Return the current microseconds since the Unix epoch.
pub extern "C" fn c_timestamp_us() -> u64 {
    return SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_micros() as u64;
}
