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

use nautilus_core::buffer::{Buffer, Buffer32};
use std::fmt::{Debug, Display, Formatter, Result};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
pub struct ClientId {
    pub value: Buffer32,
}

impl From<&str> for ClientId {
    fn from(s: &str) -> ClientId {
        ClientId {
            value: Buffer32::from(s),
        }
    }
}

impl Display for ClientId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_str())
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn client_id_free(client_id: ClientId) {
    drop(client_id); // Memory freed here
}

#[no_mangle]
pub extern "C" fn client_id_from_buffer(value: Buffer32) -> ClientId {
    ClientId { value }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::ClientId;

    #[test]
    fn test_client_id_from_str() {
        let client_id1 = ClientId::from("BINANCE");
        let client_id2 = ClientId::from("FTX");

        assert_eq!(client_id1, client_id1);
        assert_ne!(client_id1, client_id2);
        assert_eq!(client_id1.to_string(), "BINANCE");
    }

    #[test]
    fn test_client_id_as_str() {
        let client_id = ClientId::from("BINANCE");

        assert_eq!(client_id.to_string(), "BINANCE");
    }
}
