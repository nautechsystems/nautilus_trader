// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

pub mod clock;
#[cfg(feature = "ffi")]
pub mod clock_api;
pub mod enums;
pub mod logging;
#[cfg(feature = "ffi")]
pub mod logging_api;
pub mod msgbus;
pub mod testing;
pub mod timer;
#[cfg(feature = "ffi")]
pub mod timer_api;

#[cfg(feature = "test")]
pub mod stubs {
    use crate::{clock::stubs::*, logging::stubs::*};
}
