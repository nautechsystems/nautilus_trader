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

use std::fmt::Display;

use nautilus_model::{orderbook::OrderBook, types::Quantity};

use crate::indicator::Indicator;

#[repr(C)]
#[derive(Debug, Default)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct BookImbalanceRatio {
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
}

impl Display for BookImbalanceRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}()", self.name())
    }
}

impl Indicator for BookImbalanceRatio {
    fn name(&self) -> String {
        stringify!(BookImbalanceRatio).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_book(&mut self, book: &OrderBook) {
        self.update(book.best_bid_size(), book.best_ask_size());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl BookImbalanceRatio {
    /// Creates a new [`BookImbalanceRatio`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            value: 0.0,
            count: 0,
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update(&mut self, best_bid: Option<Quantity>, best_ask: Option<Quantity>) {
        self.has_inputs = true;
        self.count += 1;

        if let (Some(best_bid), Some(best_ask)) = (best_bid, best_ask) {
            let smaller = std::cmp::min(best_bid, best_ask);
            let larger = std::cmp::max(best_bid, best_ask);

            let ratio = smaller.as_f64() / larger.as_f64();
            self.value = ratio;

            self.initialized = true;
        }
        // No market yet
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::{
        identifiers::InstrumentId,
        stubs::{stub_order_book_mbp, stub_order_book_mbp_appl_xnas},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_initialized() {
        let imbalance = BookImbalanceRatio::new();
        let display_str = format!("{imbalance}");
        assert_eq!(display_str, "BookImbalanceRatio()");
        assert_eq!(imbalance.value, 0.0);
        assert_eq!(imbalance.count, 0);
        assert!(!imbalance.has_inputs);
        assert!(!imbalance.initialized);
    }

    #[rstest]
    fn test_one_value_input_balanced() {
        let mut imbalance = BookImbalanceRatio::new();
        let book = stub_order_book_mbp_appl_xnas();
        imbalance.handle_book(&book);

        assert_eq!(imbalance.count, 1);
        assert_eq!(imbalance.value, 1.0);
        assert!(imbalance.initialized);
        assert!(imbalance.has_inputs);
    }

    #[rstest]
    fn test_reset() {
        let mut imbalance = BookImbalanceRatio::new();
        let book = stub_order_book_mbp_appl_xnas();
        imbalance.handle_book(&book);
        imbalance.reset();

        assert_eq!(imbalance.count, 0);
        assert_eq!(imbalance.value, 0.0);
        assert!(!imbalance.initialized);
        assert!(!imbalance.has_inputs);
    }

    #[rstest]
    fn test_one_value_input_with_bid_imbalance() {
        let mut imbalance = BookImbalanceRatio::new();
        let book = stub_order_book_mbp(
            InstrumentId::from("AAPL.XNAS"),
            101.0,
            100.0,
            200.0, // <-- Larger bid side
            100.0,
            2,
            0.01,
            0,
            100.0,
            10,
        );
        imbalance.handle_book(&book);

        assert_eq!(imbalance.count, 1);
        assert_eq!(imbalance.value, 0.5);
        assert!(imbalance.initialized);
        assert!(imbalance.has_inputs);
    }

    #[rstest]
    fn test_one_value_input_with_ask_imbalance() {
        let mut imbalance = BookImbalanceRatio::new();
        let book = stub_order_book_mbp(
            InstrumentId::from("AAPL.XNAS"),
            101.0,
            100.0,
            100.0,
            200.0, // <-- Larger ask side
            2,
            0.01,
            0,
            100.0,
            10,
        );
        imbalance.handle_book(&book);

        assert_eq!(imbalance.count, 1);
        assert_eq!(imbalance.value, 0.5);
        assert!(imbalance.initialized);
        assert!(imbalance.has_inputs);
    }

    #[rstest]
    fn test_one_value_input_with_bid_imbalance_multiple_inputs() {
        let mut imbalance = BookImbalanceRatio::new();
        let book = stub_order_book_mbp(
            InstrumentId::from("AAPL.XNAS"),
            101.0,
            100.0,
            200.0, // <-- Larger bid side
            100.0,
            2,
            0.01,
            0,
            100.0,
            10,
        );
        imbalance.handle_book(&book);
        imbalance.handle_book(&book);
        imbalance.handle_book(&book);

        assert_eq!(imbalance.count, 3);
        assert_eq!(imbalance.value, 0.5);
        assert!(imbalance.initialized);
        assert!(imbalance.has_inputs);
    }
}
