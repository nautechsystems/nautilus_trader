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

pub mod bar;
pub mod delta;
pub mod deltas;
pub mod depth;
pub mod order;
pub mod prices;
pub mod quote;
pub mod trade;

// TODO: https://blog.rust-lang.org/2024/03/30/i128-layout-update.html
// i128 and u128 is now FFI compatible. However, since the clippy lint
// hasn't been removed yet. We'll suppress with #[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]

/// Clones a data instance.
// FFI wrapper for cloning Data instances
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn data_clone(data: &crate::data::Data) -> crate::data::Data {
    data.clone()
}
