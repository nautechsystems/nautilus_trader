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

#[cfg(test)]
mod tests {
    use nautilus_core::time::c_timestamp;
    use nautilus_core::time::c_timestamp_ms;
    use nautilus_core::time::c_timestamp_us;
    use nautilus_core::time::c_utc_now;

    #[test]
    fn c_utc_now_returns_expected_struct() {
        let date_time = c_utc_now();
        assert!(date_time.year > 0)
    }

    #[test]
    fn c_timestamp_returns_expected_struct() {
        let result = c_timestamp();
        assert!(result > 1610000000.0)
    }

    #[test]
    fn c_timestamp_ms_returns_expected_struct() {
        let result = c_timestamp_ms();
        assert!(result > 1610000000000)
    }

    #[test]
    fn c_timestamp_us_returns_expected_struct() {
        let result = c_timestamp_us();
        assert!(result > 1610000000000000)
    }
}
