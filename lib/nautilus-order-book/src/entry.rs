// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

#[repr(C)]
#[derive(Copy, Clone)]
/// Represents an entry in an order book.
pub struct OrderBookEntry
{
    pub price: f64,
    pub qty: f64,
    pub update_id: u64,
}


impl OrderBookEntry
{
    /// Initialize a new instance of the `OrderBookEntry` structure.
    #[no_mangle]
    pub extern "C" fn new_entry(price: f64, qty: f64, update_id: u64) -> OrderBookEntry {
        return OrderBookEntry { price, qty, update_id };
    }

    /// Update the entry with the given quantity and update identifier.
    #[no_mangle]
    pub extern "C" fn update(&mut self, qty: f64, update_id: u64) {
        self.qty = qty;
        self.update_id = update_id;
    }
}
