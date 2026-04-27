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

pub mod filled;

pub use filled::OrderFilledSpec;
