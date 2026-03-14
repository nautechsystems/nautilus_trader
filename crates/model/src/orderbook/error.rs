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

use nautilus_core::UnixNanos;

use super::ladder::BookPrice;
use crate::{
    enums::{BookType, OrderSide},
    identifiers::InstrumentId,
};

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum InvalidBookOperation {
    #[error("[NT-MD-00101] Invalid book operation: cannot pre-process order for {0} book")]
    PreProcessOrder(BookType),
    #[error("[NT-MD-00102] Invalid book operation: cannot add order for {0} book")]
    Add(BookType),
    #[error("[NT-MD-00103] Invalid book operation: cannot update with tick for {0} book")]
    Update(BookType),
}

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum BookIntegrityError {
    /// A delta referenced an order ID that does not exist in the book.
    /// This typically indicates a gap in the market data feed or
    /// that the book was not properly initialized from a snapshot.
    #[error(
        "[NT-MD-00201] Integrity error: order not found: order_id={0}, sequence={1}, ts_event={2}"
    )]
    OrderNotFound(u64, u64, UnixNanos),
    /// An order in the book has `NoOrderSide`, which should never occur
    /// and indicates internal corruption of the book state.
    #[error("[NT-MD-00202] Integrity error: invalid `NoOrderSide` in book")]
    NoOrderSide,
    /// Could not determine the side (bid/ask) for an order during
    /// a delete or update operation.
    #[error("[NT-MD-00203] Integrity error: order_id={0} not found in book for side resolution")]
    OrderNotFoundForSideResolution(u64),
    /// The best bid price is greater than or equal to the best ask price,
    /// violating the fundamental order book invariant.
    #[error("[NT-MD-00204] Integrity error: orders in cross [{0} {1}]")]
    OrdersCrossed(BookPrice, BookPrice),
    /// An L2_MBP book has more than one order at a single price level,
    /// which violates the L2 aggregated-level constraint.
    #[error(
        "[NT-MD-00205] Integrity error: number of {0} orders at level > 1 for L2_MBP book, was {1}"
    )]
    TooManyOrders(OrderSide, usize),
    /// An L1_MBP book has more than one price level per side,
    /// which violates the L1 top-of-book constraint.
    #[error("[NT-MD-00206] Integrity error: number of {0} levels > 1 for L1_MBP book, was {1}")]
    TooManyLevels(OrderSide, usize),
    /// A delta's instrument ID does not match the book's instrument ID,
    /// indicating the delta was routed to the wrong book.
    #[error("[NT-MD-00207] Integrity error: instrument ID mismatch: book={0}, delta={1}")]
    InstrumentMismatch(InstrumentId, InstrumentId),
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
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_invalid_book_operation_codes() {
        assert!(
            InvalidBookOperation::PreProcessOrder(BookType::L1_MBP)
                .to_string()
                .starts_with("[NT-MD-00101]")
        );
        assert!(
            InvalidBookOperation::Add(BookType::L2_MBP)
                .to_string()
                .starts_with("[NT-MD-00102]")
        );
        assert!(
            InvalidBookOperation::Update(BookType::L3_MBO)
                .to_string()
                .starts_with("[NT-MD-00103]")
        );
    }

    #[rstest]
    fn test_book_integrity_error_codes() {
        assert!(
            BookIntegrityError::NoOrderSide
                .to_string()
                .starts_with("[NT-MD-00202]")
        );
        assert!(
            BookIntegrityError::OrderNotFoundForSideResolution(42)
                .to_string()
                .starts_with("[NT-MD-00203]")
        );
        assert!(
            BookIntegrityError::TooManyOrders(OrderSide::Buy, 5)
                .to_string()
                .starts_with("[NT-MD-00205]")
        );
        assert!(
            BookIntegrityError::TooManyLevels(OrderSide::Sell, 3)
                .to_string()
                .starts_with("[NT-MD-00206]")
        );
    }

    #[rstest]
    fn test_error_message_format() {
        let err = InvalidBookOperation::Add(BookType::L2_MBP);
        assert_eq!(
            err.to_string(),
            "[NT-MD-00102] Invalid book operation: cannot add order for L2_MBP book"
        );
    }
}
