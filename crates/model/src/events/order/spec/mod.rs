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

//! Test-only fluent builders for order event types.
//!
//! Each spec mirrors the fields of a production event with sensible defaults, derives
//! [`bon::Builder`], and exposes a `build()` method that funnels through the production
//! constructor so any invariant checks still run on the constructed value.
//!
//! Specs are gated behind the `stubs` feature and must not be referenced from production code.

pub mod accepted;
pub mod cancel_rejected;
pub mod canceled;
pub mod denied;
pub mod emulated;
pub mod expired;
pub mod filled;
pub mod initialized;
pub mod modify_rejected;
pub mod pending_cancel;
pub mod pending_update;
pub mod rejected;
pub mod released;
pub mod submitted;
pub mod triggered;
pub mod updated;

pub use accepted::OrderAcceptedSpec;
pub use cancel_rejected::OrderCancelRejectedSpec;
pub use canceled::OrderCanceledSpec;
pub use denied::OrderDeniedSpec;
pub use emulated::OrderEmulatedSpec;
pub use expired::OrderExpiredSpec;
pub use filled::OrderFilledSpec;
pub use initialized::OrderInitializedSpec;
pub use modify_rejected::OrderModifyRejectedSpec;
pub use pending_cancel::OrderPendingCancelSpec;
pub use pending_update::OrderPendingUpdateSpec;
pub use rejected::OrderRejectedSpec;
pub use released::OrderReleasedSpec;
pub use submitted::OrderSubmittedSpec;
pub use triggered::OrderTriggeredSpec;
pub use updated::OrderUpdatedSpec;
