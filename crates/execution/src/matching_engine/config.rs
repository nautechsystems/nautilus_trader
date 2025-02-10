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

/// Configuration for `OrderMatchingEngine` instances.
#[derive(Debug, Clone)]
pub struct OrderMatchingEngineConfig {
    pub bar_execution: bool,
    pub reject_stop_orders: bool,
    pub support_gtd_orders: bool,
    pub support_contingent_orders: bool,
    pub use_position_ids: bool,
    pub use_random_ids: bool,
    pub use_reduce_only: bool,
}

impl OrderMatchingEngineConfig {
    /// Creates a new default [`OrderMatchingEngineConfig`] instance.
    #[must_use]
    pub const fn new(
        bar_execution: bool,
        reject_stop_orders: bool,
        support_gtd_orders: bool,
        support_contingent_orders: bool,
        use_position_ids: bool,
        use_random_ids: bool,
        use_reduce_only: bool,
    ) -> Self {
        Self {
            bar_execution,
            reject_stop_orders,
            support_gtd_orders,
            support_contingent_orders,
            use_position_ids,
            use_random_ids,
            use_reduce_only,
        }
    }
}

#[allow(clippy::derivable_impls)]
impl Default for OrderMatchingEngineConfig {
    /// Creates a new default [`OrderMatchingEngineConfig`] instance.
    fn default() -> Self {
        Self {
            bar_execution: false,
            reject_stop_orders: false,
            support_gtd_orders: false,
            support_contingent_orders: false,
            use_position_ids: false,
            use_random_ids: false,
            use_reduce_only: false,
        }
    }
}
