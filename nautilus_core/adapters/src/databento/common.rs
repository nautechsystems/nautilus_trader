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

use databento::historical::DateTimeRange;
use nautilus_core::time::UnixNanos;
use time::OffsetDateTime;

pub const DATABENTO: &str = "DATABENTO";
pub const ALL_SYMBOLS: &str = "ALL_SYMBOLS";

pub fn get_date_time_range(start: UnixNanos, end: UnixNanos) -> anyhow::Result<DateTimeRange> {
    Ok(DateTimeRange::from((
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(start))?,
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(end))?,
    )))
}
