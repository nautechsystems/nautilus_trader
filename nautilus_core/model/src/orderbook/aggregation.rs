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

use super::book::OrderBook;
use crate::{
    data::{order::BookOrder, quote::QuoteTick, trade::TradeTick},
    enums::{BookType, OrderSide},
    orderbook::book::BookIntegrityError,
};

pub fn book_update_quote_tick(book: &mut OrderBook, quote: &QuoteTick) {
    book_update_bid(
        book,
        BookOrder::from_quote_tick(quote, OrderSide::Buy),
        quote.ts_event,
        0,
    );
    book_update_ask(
        book,
        BookOrder::from_quote_tick(quote, OrderSide::Sell),
        quote.ts_event,
        0,
    );
}

pub fn book_update_trade_tick(book: &mut OrderBook, trade: &TradeTick) {
    book_update_bid(
        book,
        BookOrder::from_trade_tick(trade, OrderSide::Buy),
        trade.ts_event,
        0,
    );
    book_update_ask(
        book,
        BookOrder::from_trade_tick(trade, OrderSide::Sell),
        trade.ts_event,
        0,
    );
}

pub fn book_update_ask(book: &mut OrderBook, order: BookOrder, ts_event: u64, sequence: u64) {
    match book.asks.top() {
        Some(top_asks) => match top_asks.first() {
            Some(top_ask) => {
                let order_id = top_ask.order_id;
                book.asks.remove(order_id, ts_event, sequence);
                book.asks.add(order);
            }
            None => {
                book.asks.add(order);
            }
        },
        None => {
            book.asks.add(order);
        }
    }
}

pub fn book_update_bid(book: &mut OrderBook, order: BookOrder, ts_event: u64, sequence: u64) {
    match book.bids.top() {
        Some(top_bids) => match top_bids.first() {
            Some(top_bid) => {
                let order_id = top_bid.order_id;
                book.bids.remove(order_id, ts_event, sequence);
                book.bids.add(order);
            }
            None => {
                book.bids.add(order);
            }
        },
        None => {
            book.bids.add(order);
        }
    }
}

pub fn book_update_top(book: &mut OrderBook, order: BookOrder, ts_event: u64, sequence: u64) {
    // Because of the way we typically get updates from a L1_MBP order book (bid
    // and ask updates at the same time), its quite probable that the last
    // bid is now the ask price we are trying to insert (or vice versa). We
    // just need to add some extra protection against this if we aren't calling
    // `check_integrity()` on each individual update.
    match order.side {
        OrderSide::Buy => {
            if let Some(best_ask_price) = book.best_ask_price() {
                if order.price > best_ask_price {
                    book.clear_bids(ts_event, sequence);
                }
            }
        }
        OrderSide::Sell => {
            if let Some(best_bid_price) = book.best_bid_price() {
                if order.price < best_bid_price {
                    book.clear_asks(ts_event, sequence);
                }
            }
        }
        _ => panic!("{}", BookIntegrityError::NoOrderSide),
    }
}

pub(crate) fn pre_process_order(book_type: BookType, mut order: BookOrder) -> BookOrder {
    match book_type {
        BookType::L1_MBP => order.order_id = order.side as u64,
        BookType::L2_MBP => order.order_id = order.price.raw as u64,
        BookType::L3_MBO => {} // No pre-processing
    };
    order
}
