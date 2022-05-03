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

use nautilus_core::buffer::{Buffer, Buffer36};
use std::fmt::{Debug, Display, Formatter, Result};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
pub struct ClientOrderId {
    pub value: Buffer36,
}

impl From<&str> for ClientOrderId {
    fn from(s: &str) -> ClientOrderId {
        ClientOrderId {
            value: Buffer36::from(s),
        }
    }
}

impl Display for ClientOrderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_str())
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn client_order_id_free(client_order_id: ClientOrderId) {
    drop(client_order_id); // Memory freed here
}

#[no_mangle]
pub extern "C" fn client_order_id_from_buffer(value: Buffer36) -> ClientOrderId {
    ClientOrderId { value }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::ClientOrderId;

    #[test]
    fn test_client_id_from_str() {
        let client_order_id1 = ClientOrderId::from("O-20200814-102234-001-001-1");
        let client_order_id2 = ClientOrderId::from("O-20200814-102234-001-001-2");

        assert_eq!(client_order_id1, client_order_id1);
        assert_ne!(client_order_id1, client_order_id2);
        assert_eq!(client_order_id1.to_string(), "O-20200814-102234-001-001-1");
    }

    #[test]
    fn test_client_id_as_str() {
        let client_order_id = ClientOrderId::from("O-20200814-102234-001-001-1");

        assert_eq!(client_order_id.to_string(), "O-20200814-102234-001-001-1");
    }
}
