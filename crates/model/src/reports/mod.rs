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

//! Status report types for trading operations.
//!
//! This module provides report types for tracking and communicating the status
//! of various trading operations, including order fills, order status, position
//! status, and mass status requests.

pub mod fill;
pub mod mass_status;
pub mod order;
pub mod position;

// Re-exports
pub use fill::FillReport;
pub use mass_status::ExecutionMassStatus;
use nautilus_core::UnixNanos;
pub use order::OrderStatusReport;
pub use position::PositionStatusReport;

use crate::data::HasTsInit;

impl HasTsInit for FillReport {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for OrderStatusReport {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for PositionStatusReport {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for ExecutionMassStatus {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

crate::impl_catalog_path_prefix!(FillReport, "fill_report");
crate::impl_catalog_path_prefix!(OrderStatusReport, "order_status_report");
crate::impl_catalog_path_prefix!(PositionStatusReport, "position_status_report");
crate::impl_catalog_path_prefix!(ExecutionMassStatus, "execution_mass_status");
