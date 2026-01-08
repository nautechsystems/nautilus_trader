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

use std::fmt::{Debug, Display};

use nautilus_core::UnixNanos;

/// Trait for latency models used in backtesting.
///
/// Latency models simulate network delays for order operations during backtesting.
/// Implementations can provide static or dynamic (jittered) latency values.
pub trait LatencyModel: Debug {
    /// Returns the latency for order insertion operations.
    fn get_insert_latency(&self) -> UnixNanos;

    /// Returns the latency for order update/modify operations.
    fn get_update_latency(&self) -> UnixNanos;

    /// Returns the latency for order delete/cancel operations.
    fn get_delete_latency(&self) -> UnixNanos;

    /// Returns the base latency component.
    fn get_base_latency(&self) -> UnixNanos;
}

/// Static latency model with fixed latency values.
///
/// Models the latency for different order operations including base network latency
/// and specific operation latencies for insert, update, and delete operations.
///
/// The base latency is automatically added to each operation latency, matching
/// Python's behavior. For example, if `base_latency_nanos = 100ms` and
/// `insert_latency_nanos = 200ms`, the effective insert latency will be 300ms.
#[derive(Debug, Clone)]
pub struct StaticLatencyModel {
    base_latency_nanos: UnixNanos,
    insert_latency_nanos: UnixNanos,
    update_latency_nanos: UnixNanos,
    delete_latency_nanos: UnixNanos,
}

impl StaticLatencyModel {
    /// Creates a new [`StaticLatencyModel`] instance.
    ///
    /// The base latency is added to each operation latency to get the effective latency.
    ///
    /// # Arguments
    ///
    /// * `base_latency_nanos` - Base network latency added to all operations
    /// * `insert_latency_nanos` - Additional latency for order insertion
    /// * `update_latency_nanos` - Additional latency for order updates
    /// * `delete_latency_nanos` - Additional latency for order cancellation
    #[must_use]
    pub fn new(
        base_latency_nanos: UnixNanos,
        insert_latency_nanos: UnixNanos,
        update_latency_nanos: UnixNanos,
        delete_latency_nanos: UnixNanos,
    ) -> Self {
        Self {
            base_latency_nanos,
            insert_latency_nanos: UnixNanos::from(
                base_latency_nanos.as_u64() + insert_latency_nanos.as_u64(),
            ),
            update_latency_nanos: UnixNanos::from(
                base_latency_nanos.as_u64() + update_latency_nanos.as_u64(),
            ),
            delete_latency_nanos: UnixNanos::from(
                base_latency_nanos.as_u64() + delete_latency_nanos.as_u64(),
            ),
        }
    }
}

impl LatencyModel for StaticLatencyModel {
    fn get_insert_latency(&self) -> UnixNanos {
        self.insert_latency_nanos
    }

    fn get_update_latency(&self) -> UnixNanos {
        self.update_latency_nanos
    }

    fn get_delete_latency(&self) -> UnixNanos {
        self.delete_latency_nanos
    }

    fn get_base_latency(&self) -> UnixNanos {
        self.base_latency_nanos
    }
}

impl Display for StaticLatencyModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LatencyModel()")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_static_latency_model() {
        let model = StaticLatencyModel::new(
            UnixNanos::from(1_000_000),
            UnixNanos::from(2_000_000),
            UnixNanos::from(3_000_000),
            UnixNanos::from(4_000_000),
        );

        // Base is added to each operation latency
        assert_eq!(model.get_insert_latency().as_u64(), 3_000_000);
        assert_eq!(model.get_update_latency().as_u64(), 4_000_000);
        assert_eq!(model.get_delete_latency().as_u64(), 5_000_000);
        assert_eq!(model.get_base_latency().as_u64(), 1_000_000);
    }
}
