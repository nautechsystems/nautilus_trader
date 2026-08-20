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

use std::ffi::c_char;

use nautilus_core::ffi::{abort_on_panic, cvec::CVec, string::str_to_cstr};

use crate::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick},
    enums::{BookType, OrderSide, OrderSideSpecified},
    identifiers::InstrumentId,
    orderbook::{BookLevel, OrderBook, analysis::book_check_integrity, ladder::BookPrice},
    types::{ERROR_PRICE, Price, Quantity, price::PriceRaw},
};

/// Returns an owning pointer to the heap-allocated `OrderBook` which the caller must
/// eventually pass to [`orderbook_drop`].
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_new(
    instrument_id: InstrumentId,
    book_type: BookType,
) -> *mut OrderBook {
    Box::into_raw(Box::new(OrderBook::new(instrument_id, book_type)))
}

/// # Safety
///
/// `book` must be a live owning pointer returned by [`orderbook_new`], and must not
/// be used after this call.
///
/// # Panics
///
/// Panics if `book` is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orderbook_drop(book: *mut OrderBook) {
    abort_on_panic(|| {
        assert!(!book.is_null(), "`book` was NULL");
        // SAFETY: Caller guarantees `book` was allocated by `orderbook_new`
        drop(unsafe { Box::from_raw(book) }); // Memory freed here
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_reset(book: &mut OrderBook) {
    book.reset();
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_instrument_id(book: &OrderBook) -> InstrumentId {
    book.instrument_id
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_book_type(book: &OrderBook) -> BookType {
    book.book_type
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_sequence(book: &OrderBook) -> u64 {
    book.sequence
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_ts_last(book: &OrderBook) -> u64 {
    book.ts_last.into()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_update_count(book: &OrderBook) -> u64 {
    book.update_count
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_add(
    book: &mut OrderBook,
    order: BookOrder,
    flags: u8,
    sequence: u64,
    ts_event: u64,
) {
    book.add(order, flags, sequence, ts_event.into());
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_update(
    book: &mut OrderBook,
    order: BookOrder,
    flags: u8,
    sequence: u64,
    ts_event: u64,
) {
    book.update(order, flags, sequence, ts_event.into());
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_delete(
    book: &mut OrderBook,
    order: BookOrder,
    flags: u8,
    sequence: u64,
    ts_event: u64,
) {
    book.delete(order, flags, sequence, ts_event.into());
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_clear(book: &mut OrderBook, sequence: u64, ts_event: u64) {
    book.clear(sequence, ts_event.into());
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_clear_bids(book: &mut OrderBook, sequence: u64, ts_event: u64) {
    book.clear_bids(sequence, ts_event.into());
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_clear_asks(book: &mut OrderBook, sequence: u64, ts_event: u64) {
    book.clear_asks(sequence, ts_event.into());
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_apply_delta(book: &mut OrderBook, delta: &OrderBookDelta) {
    if let Err(e) = book.apply_delta_unchecked(delta) {
        log::error!("Failed to apply order book delta: {e}");
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_apply_deltas(book: &mut OrderBook, deltas: &OrderBookDeltas) {
    // Clone will actually copy the contents of the `deltas` vec
    if let Err(e) = book.apply_deltas_unchecked(deltas) {
        log::error!("Failed to apply order book deltas: {e}");
    }
}

/// Creates an `OrderBookDeltas` snapshot from the current order book state.
///
/// This is the reverse operation of `orderbook_apply_deltas`: it converts the current book state
/// back into a snapshot format with a `Clear` delta followed by `Add` deltas for all orders.
///
/// # Parameters
///
/// - `book` - The order book to convert.
/// - `ts_event` - UNIX timestamp (nanoseconds) when the book event occurred.
/// - `ts_init` - UNIX timestamp (nanoseconds) when the instance was created.
///
/// # Returns
///
/// An owning pointer to an `OrderBookDeltas` containing a snapshot of the current order book
/// state, which the caller must eventually pass to [`crate::ffi::data::deltas::orderbook_deltas_drop`].
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_to_snapshot_deltas(
    book: &OrderBook,
    ts_event: u64,
    ts_init: u64,
) -> *mut OrderBookDeltas {
    use nautilus_core::UnixNanos;
    Box::into_raw(Box::new(
        book.to_deltas(UnixNanos::from(ts_event), UnixNanos::from(ts_init)),
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_apply_depth(book: &mut OrderBook, depth: &OrderBookDepth10) {
    if let Err(e) = book.apply_depth_unchecked(depth) {
        log::error!("Failed to apply order book depth: {e}");
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_bids(book: &mut OrderBook) -> CVec {
    book.bids
        .levels
        .values()
        .map(|level| Box::into_raw(Box::new(level.clone())))
        .collect::<Vec<*mut BookLevel>>()
        .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_asks(book: &mut OrderBook) -> CVec {
    book.asks
        .levels
        .values()
        .map(|level| Box::into_raw(Box::new(level.clone())))
        .collect::<Vec<*mut BookLevel>>()
        .into()
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_bids_down_to(
    book: &mut OrderBook,
    price_raw: PriceRaw,
    price_prec: u8,
) -> CVec {
    let price = Price::from_raw(price_raw, price_prec);
    let bound = BookPrice::new(price, OrderSideSpecified::Buy);
    book.bids
        .levels
        .range(..=bound)
        .map(|(_, level)| Box::into_raw(Box::new(level.clone())))
        .collect::<Vec<*mut BookLevel>>()
        .into()
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_asks_up_to(
    book: &mut OrderBook,
    price_raw: PriceRaw,
    price_prec: u8,
) -> CVec {
    let price = Price::from_raw(price_raw, price_prec);
    let bound = BookPrice::new(price, OrderSideSpecified::Sell);
    book.asks
        .levels
        .range(..=bound)
        .map(|(_, level)| Box::into_raw(Box::new(level.clone())))
        .collect::<Vec<*mut BookLevel>>()
        .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_has_bid(book: &mut OrderBook) -> u8 {
    u8::from(book.has_bid())
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_has_ask(book: &mut OrderBook) -> u8 {
    u8::from(book.has_ask())
}

/// # Panics
///
/// Panics if there are no bid orders for best bid price.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_best_bid_price(book: &mut OrderBook) -> Price {
    abort_on_panic(|| {
        book.best_bid_price()
            .expect("Error: No bid orders for best bid price")
    })
}

/// # Panics
///
/// Panics if there are no ask orders for best ask price.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_best_ask_price(book: &mut OrderBook) -> Price {
    abort_on_panic(|| {
        book.best_ask_price()
            .expect("Error: No ask orders for best ask price")
    })
}

/// # Panics
///
/// Panics if there are no bid orders for best bid size.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_best_bid_size(book: &mut OrderBook) -> Quantity {
    abort_on_panic(|| {
        book.best_bid_size()
            .expect("Error: No bid orders for best bid size")
    })
}

/// # Panics
///
/// Panics if there are no ask orders for best ask size.
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_best_ask_size(book: &mut OrderBook) -> Quantity {
    abort_on_panic(|| {
        book.best_ask_size()
            .expect("Error: No ask orders for best ask size")
    })
}

/// # Panics
///
/// Panics if unable to calculate spread (requires at least one bid and one ask).
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_spread(book: &mut OrderBook) -> f64 {
    abort_on_panic(|| {
        book.spread()
            .expect("Error: Unable to calculate `spread` (no bid or ask)")
    })
}

/// # Panics
///
/// Panics if unable to calculate midpoint (requires at least one bid and one ask).
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_midpoint(book: &mut OrderBook) -> f64 {
    abort_on_panic(|| {
        book.midpoint()
            .expect("Error: Unable to calculate `midpoint` (no bid or ask)")
    })
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_get_avg_px_for_quantity(
    book: &mut OrderBook,
    qty: Quantity,
    order_side: OrderSide,
) -> f64 {
    book.get_avg_px_for_quantity(qty, order_side)
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_get_worst_px_for_quantity(
    book: &mut OrderBook,
    qty: Quantity,
    order_side: OrderSide,
) -> Price {
    book.get_worst_px_for_quantity(qty, order_side)
        .unwrap_or(ERROR_PRICE)
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_get_quantity_for_price(
    book: &mut OrderBook,
    price: Price,
    order_side: OrderSide,
) -> f64 {
    book.get_quantity_for_price(price, order_side)
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_get_quantity_at_level(
    book: &OrderBook,
    price: Price,
    order_side: OrderSide,
    size_precision: u8,
) -> Quantity {
    book.get_quantity_at_level(price, order_side, size_precision)
}

/// Updates the order book with a quote tick.
///
/// # Panics
///
/// Panics if book type is not `L1_MBP`.
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_update_quote_tick(book: &mut OrderBook, quote: &QuoteTick) {
    book.update_quote_tick(quote).unwrap();
}

/// Updates the order book with a trade tick.
///
/// # Panics
///
/// Panics if book type is not `L1_MBP`.
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_update_trade_tick(book: &mut OrderBook, trade: &TradeTick) {
    book.update_trade_tick(trade).unwrap();
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_simulate_fills(book: &OrderBook, order: BookOrder) -> CVec {
    book.simulate_fills(&order).into()
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_get_all_crossed_levels(
    book: &OrderBook,
    order_side: OrderSide,
    price: Price,
    size_precision: u8,
) -> CVec {
    book.get_all_crossed_levels(order_side, price, size_precision)
        .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_check_integrity(book: &OrderBook) -> u8 {
    u8::from(book_check_integrity(book).is_ok())
}

/// # Safety
///
/// `v` must uniquely own a valid `Vec<(Price, Quantity)>` allocation transferred from Rust.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vec_drop_fills(v: CVec) {
    let data = unsafe { v.into_vec::<(Price, Quantity)>() };
    drop(data); // Memory freed here
}

/// Returns a pretty printed `OrderBook` number of levels per side, as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_pprint_to_cstr(book: &OrderBook, num_levels: usize) -> *const c_char {
    str_to_cstr(&book.pprint(num_levels, None))
}

#[cfg(test)]
mod cvec_tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_empty_fills_drop_returns_without_panic() {
        unsafe { vec_drop_fills(CVec::empty()) };
    }
}
