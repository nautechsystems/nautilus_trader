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

use super::{book::OrderBook, error::InvalidBookOperation};
use crate::{
    data::{order::BookOrder, quote::QuoteTick, trade::TradeTick},
    enums::{BookType, OrderSide},
};

pub(crate) fn pre_process_order(book_type: BookType, mut order: BookOrder) -> BookOrder {
    match book_type {
        BookType::L1_MBP => order.order_id = order.side as u64,
        BookType::L2_MBP => order.order_id = order.price.raw as u64,
        BookType::L3_MBO => {} // No pre-processing
    };
    order
}

pub fn update_book_with_quote_tick(
    book: &mut OrderBook,
    quote: &QuoteTick,
) -> Result<(), InvalidBookOperation> {
    if book.book_type != BookType::L1_MBP {
        return Err(InvalidBookOperation::Update(book.book_type));
    };

    update_book_bid(
        book,
        BookOrder::from_quote_tick(quote, OrderSide::Buy),
        quote.ts_event,
        0,
    );
    update_book_ask(
        book,
        BookOrder::from_quote_tick(quote, OrderSide::Sell),
        quote.ts_event,
        0,
    );
    Ok(())
}

pub fn update_book_with_trade_tick(
    book: &mut OrderBook,
    trade: &TradeTick,
) -> Result<(), InvalidBookOperation> {
    if book.book_type != BookType::L1_MBP {
        return Err(InvalidBookOperation::Update(book.book_type));
    };

    update_book_bid(
        book,
        BookOrder::from_trade_tick(trade, OrderSide::Buy),
        trade.ts_event,
        0,
    );
    update_book_ask(
        book,
        BookOrder::from_trade_tick(trade, OrderSide::Sell),
        trade.ts_event,
        0,
    );
    Ok(())
}

pub fn update_book_ask(book: &mut OrderBook, order: BookOrder, ts_event: u64, sequence: u64) {
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

pub fn update_book_bid(book: &mut OrderBook, order: BookOrder, ts_event: u64, sequence: u64) {
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
