// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use super::{container::OrderBookContainer, level::Level_API};
use crate::{
    data::{
        delta::OrderBookDelta, deltas::OrderBookDeltas_API, depth::OrderBookDepth10,
        order::BookOrder, quote::QuoteTick, trade::TradeTick,
    },
    enums::{BookType, OrderSide},
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

/// Provides a C compatible Foreign Function Interface (FFI) for an underlying `OrderBook`.
///
/// This struct wraps `OrderBook` in a way that makes it compatible with C function
/// calls, enabling interaction with `OrderBook` in a C environment.
///
/// It implements the `Deref` trait, allowing instances of `OrderBook_API` to be
/// dereferenced to `OrderBook`, providing access to `OrderBook`'s methods without
/// having to manually access the underlying `OrderBook` instance.
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct OrderBook_API(Box<OrderBookContainer>);

impl Deref for OrderBook_API {
    type Target = OrderBookContainer;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for OrderBook_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[no_mangle]
pub extern "C" fn orderbook_new(instrument_id: InstrumentId, book_type: BookType) -> OrderBook_API {
    OrderBook_API(Box::new(OrderBookContainer::new(instrument_id, book_type)))
}

#[no_mangle]
pub extern "C" fn orderbook_drop(book: OrderBook_API) {
    drop(book); // Memory freed here
}

#[no_mangle]
pub extern "C" fn orderbook_reset(book: &mut OrderBook_API) {
    book.reset();
}

#[no_mangle]
pub extern "C" fn orderbook_instrument_id(book: &OrderBook_API) -> InstrumentId {
    book.instrument_id
}

#[no_mangle]
pub extern "C" fn orderbook_book_type(book: &OrderBook_API) -> BookType {
    book.book_type
}

#[no_mangle]
pub extern "C" fn orderbook_sequence(book: &OrderBook_API) -> u64 {
    book.sequence()
}

#[no_mangle]
pub extern "C" fn orderbook_ts_last(book: &OrderBook_API) -> u64 {
    book.ts_last()
}

#[no_mangle]
pub extern "C" fn orderbook_count(book: &OrderBook_API) -> u64 {
    book.count()
}

#[no_mangle]
pub extern "C" fn orderbook_add(
    book: &mut OrderBook_API,
    order: BookOrder,
    ts_event: u64,
    sequence: u64,
) {
    book.add(order, ts_event, sequence);
}

#[no_mangle]
pub extern "C" fn orderbook_update(
    book: &mut OrderBook_API,
    order: BookOrder,
    ts_event: u64,
    sequence: u64,
) {
    book.update(order, ts_event, sequence);
}

#[no_mangle]
pub extern "C" fn orderbook_delete(
    book: &mut OrderBook_API,
    order: BookOrder,
    ts_event: u64,
    sequence: u64,
) {
    book.delete(order, ts_event, sequence);
}

#[no_mangle]
pub extern "C" fn orderbook_clear(book: &mut OrderBook_API, ts_event: u64, sequence: u64) {
    book.clear(ts_event, sequence);
}

#[no_mangle]
pub extern "C" fn orderbook_clear_bids(book: &mut OrderBook_API, ts_event: u64, sequence: u64) {
    book.clear_bids(ts_event, sequence);
}

#[no_mangle]
pub extern "C" fn orderbook_clear_asks(book: &mut OrderBook_API, ts_event: u64, sequence: u64) {
    book.clear_asks(ts_event, sequence);
}

#[no_mangle]
pub extern "C" fn orderbook_apply_delta(book: &mut OrderBook_API, delta: OrderBookDelta) {
    book.apply_delta(delta);
}

#[no_mangle]
pub extern "C" fn orderbook_apply_deltas(book: &mut OrderBook_API, deltas: &OrderBookDeltas_API) {
    // Clone will actually copy the contents of the `deltas` vec
    book.apply_deltas(deltas.deref().clone());
}

#[no_mangle]
pub extern "C" fn orderbook_apply_depth(book: &mut OrderBook_API, depth: OrderBookDepth10) {
    book.apply_depth(depth);
}

#[no_mangle]
pub extern "C" fn orderbook_bids(book: &mut OrderBook_API) -> CVec {
    book.bids()
        .iter()
        .map(|l| Level_API::new(l.to_owned().clone()))
        .collect::<Vec<Level_API>>()
        .into()
}

#[no_mangle]
pub extern "C" fn orderbook_asks(book: &mut OrderBook_API) -> CVec {
    book.asks()
        .iter()
        .map(|l| Level_API::new(l.to_owned().clone()))
        .collect::<Vec<Level_API>>()
        .into()
}

#[no_mangle]
pub extern "C" fn orderbook_has_bid(book: &mut OrderBook_API) -> u8 {
    u8::from(book.has_bid())
}

#[no_mangle]
pub extern "C" fn orderbook_has_ask(book: &mut OrderBook_API) -> u8 {
    u8::from(book.has_ask())
}

#[no_mangle]
pub extern "C" fn orderbook_best_bid_price(book: &mut OrderBook_API) -> Price {
    book.best_bid_price()
        .expect("Error: No bid orders for best bid price")
}

#[no_mangle]
pub extern "C" fn orderbook_best_ask_price(book: &mut OrderBook_API) -> Price {
    book.best_ask_price()
        .expect("Error: No ask orders for best ask price")
}

#[no_mangle]
pub extern "C" fn orderbook_best_bid_size(book: &mut OrderBook_API) -> Quantity {
    book.best_bid_size()
        .expect("Error: No bid orders for best bid size")
}

#[no_mangle]
pub extern "C" fn orderbook_best_ask_size(book: &mut OrderBook_API) -> Quantity {
    book.best_ask_size()
        .expect("Error: No ask orders for best ask size")
}

#[no_mangle]
pub extern "C" fn orderbook_spread(book: &mut OrderBook_API) -> f64 {
    book.spread()
        .expect("Error: Unable to calculate `spread` (no bid or ask)")
}

#[no_mangle]
pub extern "C" fn orderbook_midpoint(book: &mut OrderBook_API) -> f64 {
    book.midpoint()
        .expect("Error: Unable to calculate `midpoint` (no bid or ask)")
}

#[no_mangle]
pub extern "C" fn orderbook_get_avg_px_for_quantity(
    book: &mut OrderBook_API,
    qty: Quantity,
    order_side: OrderSide,
) -> f64 {
    book.get_avg_px_for_quantity(qty, order_side)
}

#[no_mangle]
pub extern "C" fn orderbook_get_quantity_for_price(
    book: &mut OrderBook_API,
    price: Price,
    order_side: OrderSide,
) -> f64 {
    book.get_quantity_for_price(price, order_side)
}

#[no_mangle]
pub extern "C" fn orderbook_update_quote_tick(book: &mut OrderBook_API, tick: &QuoteTick) {
    book.update_quote_tick(tick);
}

#[no_mangle]
pub extern "C" fn orderbook_update_trade_tick(book: &mut OrderBook_API, tick: &TradeTick) {
    book.update_trade_tick(tick);
}

#[no_mangle]
pub extern "C" fn orderbook_simulate_fills(book: &OrderBook_API, order: BookOrder) -> CVec {
    book.simulate_fills(&order).into()
}

#[no_mangle]
pub extern "C" fn orderbook_check_integrity(book: &OrderBook_API) -> u8 {
    u8::from(book.check_integrity().is_ok())
}

// TODO: This struct implementation potentially leaks memory
// TODO: Skip clippy check for now since it requires large modification
#[allow(clippy::drop_non_drop)]
#[no_mangle]
pub extern "C" fn vec_fills_drop(v: CVec) {
    let CVec { ptr, len, cap } = v;
    let data: Vec<(Price, Quantity)> =
        unsafe { Vec::from_raw_parts(ptr.cast::<(Price, Quantity)>(), len, cap) };
    drop(data); // Memory freed here
}

/// Returns a pretty printed `OrderBook` number of levels per side, as a C string pointer.
#[no_mangle]
pub extern "C" fn orderbook_pprint_to_cstr(
    book: &OrderBook_API,
    num_levels: usize,
) -> *const c_char {
    str_to_cstr(&book.pprint(num_levels))
}
