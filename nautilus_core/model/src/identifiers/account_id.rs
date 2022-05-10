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
pub struct AccountId {
    value: Buffer36,
}

impl From<&str> for AccountId {
    fn from(s: &str) -> AccountId {
        AccountId {
            value: Buffer36::from(s),
        }
    }
}

impl Display for AccountId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_str())
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn account_id_free(account_id: AccountId) {
    drop(account_id); // Memory freed here
}

#[no_mangle]
pub extern "C" fn account_id_from_buffer(value: Buffer36) -> AccountId {
    AccountId { value }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::AccountId;

    #[test]
    fn test_account_id_from_str() {
        let account_id1 = AccountId::from("123456789");
        let account_id2 = AccountId::from("234567890");

        assert_eq!(account_id1, account_id1);
        assert_ne!(account_id1, account_id2);
    }

    #[test]
    fn test_account_id_as_str() {
        let account_id = AccountId::from("1234567890");

        assert_eq!(account_id.to_string(), "1234567890");
    }
}
