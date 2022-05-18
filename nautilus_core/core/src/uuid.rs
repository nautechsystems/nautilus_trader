// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::impl_identifier_boundary_api;
use pyo3::ffi;
use std::fmt::{Debug, Display, Formatter, Result};
use uuid::Uuid;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[allow(clippy::box_collection)] // C ABI compatibility
pub struct UUID4 {
    value: Box<String>,
}

impl UUID4 {
    pub fn new() -> UUID4 {
        let uuid = Uuid::new_v4();
        UUID4 {
            value: Box::new(uuid.to_string()),
        }
    }
}

impl From<&str> for UUID4 {
    fn from(s: &str) -> Self {
        let uuid = Uuid::parse_str(s).unwrap();
        UUID4 {
            value: Box::new(uuid.to_string()),
        }
    }
}

impl Default for UUID4 {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn uuid4_new() -> UUID4 {
    UUID4::new()
}

impl_identifier_boundary_api!(UUID4);
