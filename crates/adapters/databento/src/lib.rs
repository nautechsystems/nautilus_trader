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

//! The [Databento](https://databento.com) integration adapter.

#![warn(rustc::all)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(rustdoc::broken_intra_doc_links)]
// #![deny(clippy::missing_errors_doc)]

// TODO: We still rely on `IntoPy` for now, so temporarily ignore
// these deprecations until fully migrated to `IntoPyObject`.
#![allow(deprecated)]

pub mod common;
pub mod decode;
pub mod enums;
pub mod live;
pub mod loader;
pub mod symbology;
pub mod types;

#[cfg(feature = "python")]
pub mod python;
