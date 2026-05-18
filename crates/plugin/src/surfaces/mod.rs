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

//! Plug-point trait surfaces.
//!
//! Each module exposes one plug-point: an author-facing trait
//! (`Plugin<Surface>`), the `#[repr(C)]` vtable the boundary uses, an opaque
//! handle type, and the per-`T` static vtable factory wired through the
//! `Tag<T>` pattern from [`custom_data::custom_data_vtable`] and
//! [`actor::actor_vtable`].
//!
//! The infrastructure modules at the crate root (`boundary`, `host`,
//! `manifest`, `macros`, `panic`, `loader`) are deliberately separated from
//! these per-surface modules so the "how plug-ins work" mechanism stays
//! orthogonal to the "what plug-ins implement" surface set.

pub mod actor;
pub mod custom_data;
pub mod strategy;
