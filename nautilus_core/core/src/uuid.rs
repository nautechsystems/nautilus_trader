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

use crate::buffer::{Buffer36, Buffer};
use std::fmt::{Debug, Display, Formatter, Result};
use uuid::Uuid;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct UUID4 {
    value: Buffer36,
}

impl UUID4 {
    pub fn new() -> UUID4 {
        let uuid = Uuid::new_v4();
        UUID4 {
            value: Buffer36::from_str(uuid.to_string().as_str()),
        }
    }

    pub fn from_bytes(value: Buffer36) -> UUID4 {
        UUID4 { value }
    }

    pub fn from_str(s: &str) -> UUID4 {
        let uuid = Uuid::parse_str(s).unwrap();
        UUID4 {
            value: Buffer36::from_str(uuid.to_string().as_str()),
        }
    }
}

impl Default for UUID4 {
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_str())
    }
}

impl Display for UUID4 {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_str())
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn uuid4_new() -> UUID4 {
    UUID4::new()
}

#[no_mangle]
pub extern "C" fn uuid4_free(uuid4: UUID4) {
    drop(uuid4); // Memory freed here
}

#[no_mangle]
pub extern "C" fn uuid4_from_bytes(value: Buffer36) -> UUID4 {
    UUID4::from_bytes(value)
}

#[no_mangle]
pub extern "C" fn uuid4_to_bytes(uuid: &UUID4) -> Buffer36 {
    uuid.value.clone()
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::buffer::Buffer36;
    use crate::uuid::{uuid4_from_bytes, uuid4_new, UUID4};

    #[test]
    fn test_new() {
        let uuid = UUID4::from_str("2d89666b-1a1e-4a75-b193-4eb3b454c757");

        assert_eq!(uuid.to_string().len(), 36)
    }

    #[test]
    fn test_from_str() {
        let uuid = UUID4::from_str("2d89666b-1a1e-4a75-b193-4eb3b454c757");

        assert_eq!(uuid.to_string().len(), 36);
        assert_eq!(uuid.to_string(), "2d89666b-1a1e-4a75-b193-4eb3b454c757");
    }

    #[test]
    fn test_uuid4_new() {
        let uuid = uuid4_new();

        assert_eq!(uuid.to_string().len(), 36)
    }

    #[test]
    fn test_uuid4_from_bytes() {
        let mut buffer: [u8; 36] = [0; 36];
        buffer.copy_from_slice(
            "2d89666b-1a1e-4a75-b193-4eb3b454c757"
                .to_string()
                .into_bytes()
                .as_slice(),
        );
        let value = Buffer36 {
            data: buffer,
            len: 36,
        };
        let uuid = uuid4_from_bytes(value);

        assert_eq!(uuid.to_string(), "2d89666b-1a1e-4a75-b193-4eb3b454c757")
    }
}
