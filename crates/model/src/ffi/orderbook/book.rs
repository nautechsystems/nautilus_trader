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

use std::{
    ffi::c_char,
    ops::{Deref, DerefMut},
};

use nautilus_core::ffi::{cvec::CVec, string::str_to_cstr};

use super::level::BookLevel_API;
use crate::{
    data::{
        BookOrder, OrderBookDelta, OrderBookDeltas_API, OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{BookType, OrderSide},
    identifiers::InstrumentId,
    orderbook::{OrderBook, analysis::book_check_integrity},
    types::{Price, Quantity},
};

/// C compatible Foreign Function Interface (FFI) for an underlying `OrderBook`.
///
/// This struct wraps `OrderBook` in a way that makes it compatible with C function
/// calls, enabling interaction with `OrderBook` in a C environment.
///
/// It implements the `Deref` trait, allowing instances of `OrderBook_API` to be
/// dereferenced to `OrderBook`, providing access to `OrderBook`'s methods without
/// having to manually access the underlying `OrderBook` instance.
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct OrderBook_API(Box<OrderBook>);

impl Deref for OrderBook_API {
    type Target = OrderBook;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for OrderBook_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_new(instrument_id: InstrumentId, book_type: BookType) -> OrderBook_API {
    OrderBook_API(Box::new(OrderBook::new(instrument_id, book_type)))
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_drop(book: OrderBook_API) {
    drop(book); // Memory freed here
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_reset(book: &mut OrderBook_API) {
    book.reset();
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_instrument_id(book: &OrderBook_API) -> InstrumentId {
    book.instrument_id
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_book_type(book: &OrderBook_API) -> BookType {
    book.book_type
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_sequence(book: &OrderBook_API) -> u64 {
    book.sequence
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_ts_last(book: &OrderBook_API) -> u64 {
    book.ts_last.into()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_update_count(book: &OrderBook_API) -> u64 {
    book.update_count
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_add(
    book: &mut OrderBook_API,
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
    book: &mut OrderBook_API,
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
    book: &mut OrderBook_API,
    order: BookOrder,
    flags: u8,
    sequence: u64,
    ts_event: u64,
) {
    book.delete(order, flags, sequence, ts_event.into());
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_clear(book: &mut OrderBook_API, sequence: u64, ts_event: u64) {
    book.clear(sequence, ts_event.into());
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_clear_bids(book: &mut OrderBook_API, sequence: u64, ts_event: u64) {
    book.clear_bids(sequence, ts_event.into());
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_clear_asks(book: &mut OrderBook_API, sequence: u64, ts_event: u64) {
    book.clear_asks(sequence, ts_event.into());
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_apply_delta(book: &mut OrderBook_API, delta: &OrderBookDelta) {
    book.apply_delta(delta);
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_apply_deltas(book: &mut OrderBook_API, deltas: &OrderBookDeltas_API) {
    // Clone will actually copy the contents of the `deltas` vec
    book.apply_deltas(deltas.deref());
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_apply_depth(book: &mut OrderBook_API, depth: &OrderBookDepth10) {
    book.apply_depth(depth);
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_bids(book: &mut OrderBook_API) -> CVec {
    book.bids
        .levels
        .values()
        .map(|level| BookLevel_API::new(level.clone()))
        .collect::<Vec<BookLevel_API>>()
        .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_asks(book: &mut OrderBook_API) -> CVec {
    book.asks
        .levels
        .values()
        .map(|level| BookLevel_API::new(level.clone()))
        .collect::<Vec<BookLevel_API>>()
        .into()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_has_bid(book: &mut OrderBook_API) -> u8 {
    u8::from(book.has_bid())
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_has_ask(book: &mut OrderBook_API) -> u8 {
    u8::from(book.has_ask())
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_best_bid_price(book: &mut OrderBook_API) -> Price {
    book.best_bid_price()
        .expect("Error: No bid orders for best bid price")
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_best_ask_price(book: &mut OrderBook_API) -> Price {
    book.best_ask_price()
        .expect("Error: No ask orders for best ask price")
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_best_bid_size(book: &mut OrderBook_API) -> Quantity {
    book.best_bid_size()
        .expect("Error: No bid orders for best bid size")
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_best_ask_size(book: &mut OrderBook_API) -> Quantity {
    book.best_ask_size()
        .expect("Error: No ask orders for best ask size")
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_spread(book: &mut OrderBook_API) -> f64 {
    book.spread()
        .expect("Error: Unable to calculate `spread` (no bid or ask)")
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_midpoint(book: &mut OrderBook_API) -> f64 {
    book.midpoint()
        .expect("Error: Unable to calculate `midpoint` (no bid or ask)")
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_get_avg_px_for_quantity(
    book: &mut OrderBook_API,
    qty: Quantity,
    order_side: OrderSide,
) -> f64 {
    book.get_avg_px_for_quantity(qty, order_side)
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_get_quantity_for_price(
    book: &mut OrderBook_API,
    price: Price,
    order_side: OrderSide,
) -> f64 {
    book.get_quantity_for_price(price, order_side)
}

/// Updates the order book with a quote tick.
///
/// # Panics
///
/// This function panics:
/// - If book type is not `L1_MBP`.
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_update_quote_tick(book: &mut OrderBook_API, quote: &QuoteTick) {
    book.update_quote_tick(quote).unwrap();
}

/// Updates the order book with a trade tick.
///
/// # Panics
///
/// This function panics:
/// - If book type is not `L1_MBP`.
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_update_trade_tick(book: &mut OrderBook_API, trade: &TradeTick) {
    book.update_trade_tick(trade).unwrap();
}

#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn orderbook_simulate_fills(book: &OrderBook_API, order: BookOrder) -> CVec {
    book.simulate_fills(&order).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn orderbook_check_integrity(book: &OrderBook_API) -> u8 {
    u8::from(book_check_integrity(book).is_ok())
}

// TODO: This struct implementation potentially leaks memory
// TODO: Skip clippy check for now since it requires large modification
#[allow(clippy::drop_non_drop)]
#[unsafe(no_mangle)]
pub extern "C" fn vec_fills_drop(v: CVec) {
    let CVec { ptr, len, cap } = v;
    let data: Vec<(Price, Quantity)> =
        unsafe { Vec::from_raw_parts(ptr.cast::<(Price, Quantity)>(), len, cap) };
    drop(data); // Memory freed here
}

/// Returns a pretty printed `OrderBook` number of levels per side, as a C string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn orderbook_pprint_to_cstr(
    book: &OrderBook_API,
    num_levels: usize,
) -> *const c_char {
    str_to_cstr(&book.pprint(num_levels))
}
