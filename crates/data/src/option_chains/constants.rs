// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Module-level constants for option chain rebalancing behavior.

/// Hysteresis band for ATM rebalancing (0.0..=1.0).
/// Price must cross this fraction of the gap to the next strike before ATM shifts.
/// E.g., 0.6 means price must move 60% past the midpoint between two strikes.
pub const DEFAULT_REBALANCE_HYSTERESIS: f64 = 0.6;

/// Minimum time between rebalances in nanoseconds (5 seconds).
pub const DEFAULT_REBALANCE_COOLDOWN_NS: u64 = 5_000_000_000;
