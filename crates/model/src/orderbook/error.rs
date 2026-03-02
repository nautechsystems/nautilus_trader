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

//! Errors associated with order book operations and integrity.

use nautilus_core::{UnixNanos, impl_error_codes};

use super::ladder::BookPrice;
use crate::{
    enums::{BookType, OrderSide},
    identifiers::InstrumentId,
};

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum InvalidBookOperation {
    #[error("Invalid book operation: cannot pre-process order for {0} book")]
    PreProcessOrder(BookType),
    #[error("Invalid book operation: cannot add order for {0} book")]
    Add(BookType),
    #[error("Invalid book operation: cannot update with tick for {0} book")]
    Update(BookType),
}

impl_error_codes! {
    InvalidBookOperation {
        /// Cannot pre-process an order for the given book type.
        PreProcessOrder(_) => "NT-0201",
        /// Cannot add an order for the given book type.
        Add(_) => "NT-0202",
        /// Cannot update with a tick for the given book type.
        Update(_) => "NT-0203",
    }
}

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum BookIntegrityError {
    #[error("Integrity error: order not found: order_id={0}, sequence={1}, ts_event={2}")]
    OrderNotFound(u64, u64, UnixNanos),
    #[error("Integrity error: invalid `NoOrderSide` in book")]
    NoOrderSide,
    #[error("Integrity error: order_id={0} not found in book for side resolution")]
    OrderNotFoundForSideResolution(u64),
    #[error("Integrity error: orders in cross [{0} {1}]")]
    OrdersCrossed(BookPrice, BookPrice),
    #[error("Integrity error: number of {0} orders at level > 1 for L2_MBP book, was {1}")]
    TooManyOrders(OrderSide, usize),
    #[error("Integrity error: number of {0} levels > 1 for L1_MBP book, was {1}")]
    TooManyLevels(OrderSide, usize),
    #[error("Integrity error: instrument ID mismatch: book={0}, delta={1}")]
    InstrumentMismatch(InstrumentId, InstrumentId),
}

impl_error_codes! {
    BookIntegrityError {
        /// Order was not found in the book during a delta operation.
        OrderNotFound(_, _, _) => "NT-0211",
        /// An order in the book has `NoOrderSide`, indicating corruption.
        NoOrderSide => "NT-0212",
        /// Order not found when resolving its side in the book.
        OrderNotFoundForSideResolution(_) => "NT-0213",
        /// Best bid and ask prices are crossed, violating book invariants.
        OrdersCrossed(_, _) => "NT-0214",
        /// More orders at a price level than allowed for the book type.
        TooManyOrders(_, _) => "NT-0215",
        /// More price levels than allowed for the book type.
        TooManyLevels(_, _) => "NT-0216",
        /// Delta instrument ID does not match the book instrument ID.
        InstrumentMismatch(_, _) => "NT-0217",
    }
}

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum BookViewError {
    #[error("Instrument ID mismatch: book={0}, own_book={1}")]
    InstrumentMismatch(InstrumentId, InstrumentId),

    #[error("Opposite own book must have different instrument ID: book={0}, opposite={1}")]
    OppositeInstrumentMatch(InstrumentId, InstrumentId),
}

#[cfg(test)]
mod tests {
    use nautilus_core::ErrorCode;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_invalid_book_operation_codes() {
        assert_eq!(
            InvalidBookOperation::PreProcessOrder(BookType::L1_MBP).code(),
            "NT-0201"
        );
        assert_eq!(
            InvalidBookOperation::Add(BookType::L2_MBP).code(),
            "NT-0202"
        );
        assert_eq!(
            InvalidBookOperation::Update(BookType::L3_MBO).code(),
            "NT-0203"
        );
    }

    #[rstest]
    fn test_book_integrity_error_codes() {
        assert_eq!(BookIntegrityError::NoOrderSide.code(), "NT-0212");
        assert_eq!(
            BookIntegrityError::OrderNotFoundForSideResolution(42).code(),
            "NT-0213"
        );
        assert_eq!(
            BookIntegrityError::TooManyOrders(OrderSide::Buy, 5).code(),
            "NT-0215"
        );
        assert_eq!(
            BookIntegrityError::TooManyLevels(OrderSide::Sell, 3).code(),
            "NT-0216"
        );
    }

    #[rstest]
    fn test_coded_message_format() {
        let err = InvalidBookOperation::Add(BookType::L2_MBP);
        assert_eq!(
            err.coded_message(),
            "[NT-0202] Invalid book operation: cannot add order for L2_MBP book"
        );
    }
}
