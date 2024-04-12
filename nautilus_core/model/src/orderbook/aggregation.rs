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
    enums::{BookType, OrderSide, RecordFlag},
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

pub(crate) fn update_book_with_quote_tick(
    book: &mut OrderBook,
    quote: &QuoteTick,
) -> Result<(), InvalidBookOperation> {
    if book.book_type != BookType::L1_MBP {
        return Err(InvalidBookOperation::Update(book.book_type));
    };

    let bid = BookOrder::new(
        OrderSide::Buy,
        quote.bid_price,
        quote.bid_size,
        OrderSide::Buy as u64,
    );

    let ask = BookOrder::new(
        OrderSide::Sell,
        quote.ask_price,
        quote.ask_size,
        OrderSide::Sell as u64,
    );

    update_book_bid(book, bid, quote.ts_event);
    update_book_ask(book, ask, quote.ts_event);

    Ok(())
}

pub(crate) fn update_book_with_trade_tick(
    book: &mut OrderBook,
    trade: &TradeTick,
) -> Result<(), InvalidBookOperation> {
    if book.book_type != BookType::L1_MBP {
        return Err(InvalidBookOperation::Update(book.book_type));
    };

    let bid = BookOrder::new(
        OrderSide::Buy,
        trade.price,
        trade.size,
        OrderSide::Buy as u64,
    );

    let ask = BookOrder::new(
        OrderSide::Sell,
        trade.price,
        trade.size,
        OrderSide::Sell as u64,
    );

    update_book_bid(book, bid, trade.ts_event);
    update_book_ask(book, ask, trade.ts_event);

    Ok(())
}

fn update_book_ask(book: &mut OrderBook, order: BookOrder, ts_event: u64) {
    if let Some(top_asks) = book.asks.top() {
        if let Some(top_ask) = top_asks.first() {
            let order_id = top_ask.order_id;
            book.asks.remove(order_id, 0, ts_event);
        }
    }
    book.asks.add(order);
}

fn update_book_bid(book: &mut OrderBook, order: BookOrder, ts_event: u64) {
    if let Some(top_bids) = book.bids.top() {
        if let Some(top_bid) = top_bids.first() {
            let order_id = top_bid.order_id;
            book.bids.remove(order_id, 0, ts_event);
        }
    }
    book.bids.add(order);
}
