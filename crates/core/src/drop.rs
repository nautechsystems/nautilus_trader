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

//! Explicit, manually-invocable cleanup hook used to break reference cycles before `Drop`.
//!
//! Many long-lived components register callbacks or handlers that retain strong references back to
//! them, creating reference-count cycles that prevent Rust’s automatic destructor (`Drop`) from
//! running.  The `CleanDrop` trait provides an *object-safe* method, `clean_drop`, that can be
//! called explicitly (e.g. during an orderly shutdown) to release such resources.  Implementations
//! should also call `clean_drop` from their `Drop` impl as a final safety net.
//!
//! Design contract:
//! 1. **Idempotent** – multiple calls must be safe.
//! 2. Perform all externally-observable cleanup here (unregister handlers, abort tasks, clear
//!    callbacks, downgrade `Rc`/`Arc` references, etc.).

/// Trait providing an explicit cleanup method that may be invoked prior to `Drop`.
pub trait CleanDrop {
    /// Perform custom cleanup, releasing external resources and breaking strong reference cycles.
    fn clean_drop(&mut self);
}
