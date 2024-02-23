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

use crate::{
    data::{
        delta::OrderBookDelta, deltas::OrderBookDeltas, depth::OrderBookDepth10, order::BookOrder,
        quote::QuoteTick, trade::TradeTick,
    },
    enums::{BookType, OrderSide},
    identifiers::instrument_id::InstrumentId,
    orderbook::{
        book::BookIntegrityError, book_mbo::OrderBookMbo, book_mbp::OrderBookMbp, level::Level,
    },
    types::{price::Price, quantity::Quantity},
};

pub struct OrderBookContainer {
    pub instrument_id: InstrumentId,
    pub book_type: BookType,
    mbo: Option<OrderBookMbo>,
    mbp: Option<OrderBookMbp>,
}

const L3_MBO_NOT_INITILIZED: &str = "L3_MBO book not initialized";
const L2_MBP_NOT_INITILIZED: &str = "L2_MBP book not initialized";
const L1_MBP_NOT_INITILIZED: &str = "L1_MBP book not initialized";

impl OrderBookContainer {
    #[must_use]
    pub fn new(instrument_id: InstrumentId, book_type: BookType) -> Self {
        let (mbo, mbp) = match book_type {
            BookType::L3_MBO => (Some(OrderBookMbo::new(instrument_id)), None),
            BookType::L2_MBP => (None, Some(OrderBookMbp::new(instrument_id, false))),
            BookType::L1_MBP => (None, Some(OrderBookMbp::new(instrument_id, true))),
        };

        Self {
            instrument_id,
            book_type,
            mbo,
            mbp,
        }
    }

    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[must_use]
    pub fn book_type(&self) -> BookType {
        self.book_type
    }

    #[must_use]
    pub fn sequence(&self) -> u64 {
        match self.book_type {
            BookType::L3_MBO => self.mbo.as_ref().expect(L3_MBO_NOT_INITILIZED).sequence,
            BookType::L2_MBP => self.mbp.as_ref().expect(L2_MBP_NOT_INITILIZED).sequence,
            BookType::L1_MBP => self.mbp.as_ref().expect(L1_MBP_NOT_INITILIZED).sequence,
        }
    }

    #[must_use]
    pub fn ts_last(&self) -> u64 {
        match self.book_type {
            BookType::L3_MBO => self.mbo.as_ref().expect(L3_MBO_NOT_INITILIZED).ts_last,
            BookType::L2_MBP => self.mbp.as_ref().expect(L2_MBP_NOT_INITILIZED).ts_last,
            BookType::L1_MBP => self.mbp.as_ref().expect(L1_MBP_NOT_INITILIZED).ts_last,
        }
    }

    #[must_use]
    pub fn count(&self) -> u64 {
        match self.book_type {
            BookType::L3_MBO => self.mbo.as_ref().expect(L3_MBO_NOT_INITILIZED).count,
            BookType::L2_MBP => self.mbp.as_ref().expect(L2_MBP_NOT_INITILIZED).count,
            BookType::L1_MBP => self.mbp.as_ref().expect(L1_MBP_NOT_INITILIZED).count,
        }
    }

    pub fn reset(&mut self) {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo_mut().reset(),
            BookType::L2_MBP => self.get_mbp_mut().reset(),
            BookType::L1_MBP => self.get_mbp_mut().reset(),
        };
    }

    pub fn add(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo_mut().add(order, ts_event, sequence),
            BookType::L2_MBP => self.get_mbp_mut().add(order, ts_event, sequence),
            BookType::L1_MBP => panic!("Invalid operation for L1_MBP book: `add`"),
        };
    }

    pub fn update(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo_mut().update(order, ts_event, sequence),
            BookType::L2_MBP => self.get_mbp_mut().update(order, ts_event, sequence),
            BookType::L1_MBP => self.get_mbp_mut().update(order, ts_event, sequence),
        };
    }

    pub fn update_quote_tick(&mut self, quote: &QuoteTick) {
        match self.book_type {
            BookType::L3_MBO => panic!("Invalid operation for L3_MBO book: `update_quote_tick`"),
            BookType::L2_MBP => self.get_mbp_mut().update_quote_tick(quote),
            BookType::L1_MBP => self.get_mbp_mut().update_quote_tick(quote),
        };
    }

    pub fn update_trade_tick(&mut self, trade: &TradeTick) {
        match self.book_type {
            BookType::L3_MBO => panic!("Invalid operation for L3_MBO book: `update_trade_tick`"),
            BookType::L2_MBP => self.get_mbp_mut().update_trade_tick(trade),
            BookType::L1_MBP => self.get_mbp_mut().update_trade_tick(trade),
        };
    }

    pub fn delete(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo_mut().delete(order, ts_event, sequence),
            BookType::L2_MBP => self.get_mbp_mut().delete(order, ts_event, sequence),
            BookType::L1_MBP => self.get_mbp_mut().delete(order, ts_event, sequence),
        };
    }

    pub fn clear(&mut self, ts_event: u64, sequence: u64) {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo_mut().clear(ts_event, sequence),
            BookType::L2_MBP => self.get_mbp_mut().clear(ts_event, sequence),
            BookType::L1_MBP => self.get_mbp_mut().clear(ts_event, sequence),
        };
    }

    pub fn clear_bids(&mut self, ts_event: u64, sequence: u64) {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo_mut().clear_bids(ts_event, sequence),
            BookType::L2_MBP => self.get_mbp_mut().clear_bids(ts_event, sequence),
            BookType::L1_MBP => self.get_mbp_mut().clear_bids(ts_event, sequence),
        };
    }

    pub fn clear_asks(&mut self, ts_event: u64, sequence: u64) {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo_mut().clear_asks(ts_event, sequence),
            BookType::L2_MBP => self.get_mbp_mut().clear_asks(ts_event, sequence),
            BookType::L1_MBP => self.get_mbp_mut().clear_asks(ts_event, sequence),
        };
    }

    pub fn apply_delta(&mut self, delta: OrderBookDelta) {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo_mut().apply_delta(delta),
            BookType::L2_MBP => self.get_mbp_mut().apply_delta(delta),
            BookType::L1_MBP => self.get_mbp_mut().apply_delta(delta),
        };
    }

    pub fn apply_deltas(&mut self, deltas: OrderBookDeltas) {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo_mut().apply_deltas(deltas),
            BookType::L2_MBP => self.get_mbp_mut().apply_deltas(deltas),
            BookType::L1_MBP => self.get_mbp_mut().apply_deltas(deltas),
        };
    }

    pub fn apply_depth(&mut self, depth: OrderBookDepth10) {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo_mut().apply_depth(depth),
            BookType::L2_MBP => self.get_mbp_mut().apply_depth(depth),
            BookType::L1_MBP => panic!("Invalid operation for L1_MBP book: `apply_depth`"),
        };
    }

    #[must_use]
    pub fn bids(&self) -> Vec<&Level> {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().bids().collect(),
            BookType::L2_MBP => self.get_mbp().bids().collect(),
            BookType::L1_MBP => self.get_mbp().bids().collect(),
        }
    }

    #[must_use]
    pub fn asks(&self) -> Vec<&Level> {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().asks().collect(),
            BookType::L2_MBP => self.get_mbp().asks().collect(),
            BookType::L1_MBP => self.get_mbp().asks().collect(),
        }
    }

    #[must_use]
    pub fn has_bid(&self) -> bool {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().has_bid(),
            BookType::L2_MBP => self.get_mbp().has_bid(),
            BookType::L1_MBP => self.get_mbp().has_bid(),
        }
    }

    #[must_use]
    pub fn has_ask(&self) -> bool {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().has_ask(),
            BookType::L2_MBP => self.get_mbp().has_ask(),
            BookType::L1_MBP => self.get_mbp().has_ask(),
        }
    }

    #[must_use]
    pub fn best_bid_price(&self) -> Option<Price> {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().best_bid_price(),
            BookType::L2_MBP => self.get_mbp().best_bid_price(),
            BookType::L1_MBP => self.get_mbp().best_bid_price(),
        }
    }

    #[must_use]
    pub fn best_ask_price(&self) -> Option<Price> {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().best_ask_price(),
            BookType::L2_MBP => self.get_mbp().best_ask_price(),
            BookType::L1_MBP => self.get_mbp().best_ask_price(),
        }
    }

    #[must_use]
    pub fn best_bid_size(&self) -> Option<Quantity> {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().best_bid_size(),
            BookType::L2_MBP => self.get_mbp().best_bid_size(),
            BookType::L1_MBP => self.get_mbp().best_bid_size(),
        }
    }

    #[must_use]
    pub fn best_ask_size(&self) -> Option<Quantity> {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().best_ask_size(),
            BookType::L2_MBP => self.get_mbp().best_ask_size(),
            BookType::L1_MBP => self.get_mbp().best_ask_size(),
        }
    }

    #[must_use]
    pub fn spread(&self) -> Option<f64> {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().spread(),
            BookType::L2_MBP => self.get_mbp().spread(),
            BookType::L1_MBP => self.get_mbp().spread(),
        }
    }

    #[must_use]
    pub fn midpoint(&self) -> Option<f64> {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().midpoint(),
            BookType::L2_MBP => self.get_mbp().midpoint(),
            BookType::L1_MBP => self.get_mbp().midpoint(),
        }
    }

    #[must_use]
    pub fn get_avg_px_for_quantity(&self, qty: Quantity, order_side: OrderSide) -> f64 {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().get_avg_px_for_quantity(qty, order_side),
            BookType::L2_MBP => self.get_mbp().get_avg_px_for_quantity(qty, order_side),
            BookType::L1_MBP => self.get_mbp().get_avg_px_for_quantity(qty, order_side),
        }
    }

    #[must_use]
    pub fn get_quantity_for_price(&self, price: Price, order_side: OrderSide) -> f64 {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().get_quantity_for_price(price, order_side),
            BookType::L2_MBP => self.get_mbp().get_quantity_for_price(price, order_side),
            BookType::L1_MBP => self.get_mbp().get_quantity_for_price(price, order_side),
        }
    }

    #[must_use]
    pub fn simulate_fills(&self, order: &BookOrder) -> Vec<(Price, Quantity)> {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().simulate_fills(order),
            BookType::L2_MBP => self.get_mbp().simulate_fills(order),
            BookType::L1_MBP => self.get_mbp().simulate_fills(order),
        }
    }

    pub fn check_integrity(&self) -> Result<(), BookIntegrityError> {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().check_integrity(),
            BookType::L2_MBP => self.get_mbp().check_integrity(),
            BookType::L1_MBP => self.get_mbp().check_integrity(),
        }
    }

    #[must_use]
    pub fn pprint(&self, num_levels: usize) -> String {
        match self.book_type {
            BookType::L3_MBO => self.get_mbo().pprint(num_levels),
            BookType::L2_MBP => self.get_mbp().pprint(num_levels),
            BookType::L1_MBP => self.get_mbp().pprint(num_levels),
        }
    }

    fn get_mbo(&self) -> &OrderBookMbo {
        self.mbo.as_ref().expect(L3_MBO_NOT_INITILIZED)
    }

    fn get_mbp(&self) -> &OrderBookMbp {
        self.mbp.as_ref().expect(L2_MBP_NOT_INITILIZED)
    }

    fn get_mbo_mut(&mut self) -> &mut OrderBookMbo {
        self.mbo.as_mut().expect(L3_MBO_NOT_INITILIZED)
    }

    fn get_mbp_mut(&mut self) -> &mut OrderBookMbp {
        self.mbp.as_mut().expect(L2_MBP_NOT_INITILIZED)
    }
}
