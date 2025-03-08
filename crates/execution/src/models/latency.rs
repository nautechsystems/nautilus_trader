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

use std::fmt::Display;

use nautilus_core::UnixNanos;

pub struct LatencyModel {
    pub base_latency_nanos: UnixNanos,
    pub insert_latency_nanos: UnixNanos,
    pub update_latency_nanos: UnixNanos,
    pub delete_latency_nanos: UnixNanos,
}

impl LatencyModel {
    #[must_use]
    pub const fn new(
        base_latency_nanos: UnixNanos,
        insert_latency_nanos: UnixNanos,
        update_latency_nanos: UnixNanos,
        delete_latency_nanos: UnixNanos,
    ) -> Self {
        Self {
            base_latency_nanos,
            insert_latency_nanos,
            update_latency_nanos,
            delete_latency_nanos,
        }
    }
}

impl Display for LatencyModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LatencyModel()")
    }
}
