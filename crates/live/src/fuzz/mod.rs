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

//! Shared support for adapter fuzz binaries.

#[doc(hidden)]
pub use libfuzzer_sys::{Corpus, fuzz_target as libfuzzer_target};

// libfuzzer-sys 0.4.13 resolves `Corpus` through an absolute path at the call
// site. Alias this crate so adapter packages only depend on nautilus-live.
#[doc(hidden)]
#[macro_export]
macro_rules! fuzz_target {
    ($($tokens:tt)*) => {
        extern crate nautilus_live as libfuzzer_sys;
        $crate::fuzz::libfuzzer_target!($($tokens)*);
    };
}

pub use crate::fuzz_target;
