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

//! Functions related to normalizing and processing top-of-book events.

use crate::{
    data::order::BookOrder,
    enums::{BookType, RecordFlag},
};

pub(crate) fn pre_process_order(book_type: BookType, mut order: BookOrder, flags: u8) -> BookOrder {
    match book_type {
        BookType::L1_MBP => order.order_id = order.side as u64,
        BookType::L2_MBP => order.order_id = order.price.raw as u64,
        BookType::L3_MBO => {
            if flags == 0 {
            } else if RecordFlag::F_TOB.matches(flags) {
                order.order_id = order.side as u64;
            } else if RecordFlag::F_MBP.matches(flags) {
                order.order_id = order.price.raw as u64;
            }
        }
    };
    order
}
