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

//! Opaque host-side ABI tokens.
//!
//! The public `nautilus-plugin` crate supplies the token types used to declare
//! the plug-in init symbol while the host implementation remains an internal
//! Nautilus deployment detail.

/// Opaque host service table supplied by the host implementation.
#[repr(C)]
#[derive(Debug)]
pub struct HostVTable {
    _opaque: [u8; 0],
}

/// Opaque per-instance host context supplied by the host implementation.
#[repr(C)]
#[derive(Debug)]
pub struct HostContext {
    _opaque: [u8; 0],
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, size_of};

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn host_vtable_is_opaque_zero_sized_token() {
        assert_eq!(size_of::<HostVTable>(), 0);
        assert_eq!(align_of::<HostVTable>(), 1);
    }

    #[rstest]
    fn host_context_is_opaque_zero_sized_token() {
        assert_eq!(size_of::<HostContext>(), 0);
        assert_eq!(align_of::<HostContext>(), 1);
    }
}
