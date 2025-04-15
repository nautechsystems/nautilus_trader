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

use std::collections::HashMap;

use nautilus_model::{
    data::{BarType, DataType},
    identifiers::{ClientOrderId, InstrumentId, PositionId, StrategyId, Venue},
};
use ustr::Ustr;

use crate::msgbus::get_message_bus;

#[must_use]
pub fn get_custom_topic(data_type: &DataType) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_custom_topic(data_type)
}

#[must_use]
pub fn get_instruments_topic(venue: Venue) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_instruments_topic(venue)
}

#[must_use]
pub fn get_instrument_topic(instrument_id: InstrumentId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_instrument_topic(instrument_id)
}

#[must_use]
pub fn get_book_deltas_topic(instrument_id: InstrumentId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_book_deltas_topic(instrument_id)
}

#[must_use]
pub fn get_book_depth10_topic(instrument_id: InstrumentId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_book_depth10_topic(instrument_id)
}

#[must_use]
pub fn get_book_snapshots_topic(instrument_id: InstrumentId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_book_snapshots_topic(instrument_id)
}

#[must_use]
pub fn get_quotes_topic(instrument_id: InstrumentId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_quotes_topic(instrument_id)
}

#[must_use]
pub fn get_trades_topic(instrument_id: InstrumentId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_trades_topic(instrument_id)
}

#[must_use]
pub fn get_bars_topic(bar_type: BarType) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_bars_topic(bar_type)
}

#[must_use]
pub fn get_mark_price_topic(instrument_id: InstrumentId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_mark_price_topic(instrument_id)
}

#[must_use]
pub fn get_index_price_topic(instrument_id: InstrumentId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_index_price_topic(instrument_id)
}

#[must_use]
pub fn get_instrument_status_topic(instrument_id: InstrumentId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_instrument_status_topic(instrument_id)
}

#[must_use]
pub fn get_instrument_close_topic(instrument_id: InstrumentId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_instrument_close_topic(instrument_id)
}

#[must_use]
pub fn get_order_snapshots_topic(client_order_id: ClientOrderId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_order_snapshots_topic(client_order_id)
}

#[must_use]
pub fn get_positions_snapshots_topic(position_id: PositionId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_positions_snapshots_topic(position_id)
}

#[must_use]
pub fn get_event_orders_topic(strategy_id: StrategyId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_event_orders_topic(strategy_id)
}

#[must_use]
pub fn get_event_positions_topic(strategy_id: StrategyId) -> Ustr {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_event_positions_topic(strategy_id)
}

/// Represents a switchboard of built-in messaging endpoint names.
#[derive(Clone, Debug)]
pub struct MessagingSwitchboard {
    custom_topics: HashMap<DataType, Ustr>,
    instruments_topics: HashMap<Venue, Ustr>,
    instrument_topics: HashMap<InstrumentId, Ustr>,
    book_deltas_topics: HashMap<InstrumentId, Ustr>,
    book_depth10_topics: HashMap<InstrumentId, Ustr>,
    book_snapshots_topics: HashMap<InstrumentId, Ustr>,
    quote_topics: HashMap<InstrumentId, Ustr>,
    trade_topics: HashMap<InstrumentId, Ustr>,
    bar_topics: HashMap<BarType, Ustr>,
    mark_price_topics: HashMap<InstrumentId, Ustr>,
    index_price_topics: HashMap<InstrumentId, Ustr>,
    instrument_status_topics: HashMap<InstrumentId, Ustr>,
    instrument_close_topics: HashMap<InstrumentId, Ustr>,
    event_orders_topics: HashMap<StrategyId, Ustr>,
    event_positions_topics: HashMap<StrategyId, Ustr>,
    order_snapshots_topics: HashMap<ClientOrderId, Ustr>,
    positions_snapshots_topics: HashMap<PositionId, Ustr>,
}

impl Default for MessagingSwitchboard {
    /// Creates a new default [`MessagingSwitchboard`] instance.
    fn default() -> Self {
        Self {
            custom_topics: HashMap::new(),
            instruments_topics: HashMap::new(),
            instrument_topics: HashMap::new(),
            book_deltas_topics: HashMap::new(),
            book_snapshots_topics: HashMap::new(),
            book_depth10_topics: HashMap::new(),
            quote_topics: HashMap::new(),
            trade_topics: HashMap::new(),
            mark_price_topics: HashMap::new(),
            index_price_topics: HashMap::new(),
            bar_topics: HashMap::new(),
            instrument_status_topics: HashMap::new(),
            instrument_close_topics: HashMap::new(),
            order_snapshots_topics: HashMap::new(),
            event_orders_topics: HashMap::new(),
            event_positions_topics: HashMap::new(),
            positions_snapshots_topics: HashMap::new(),
        }
    }
}

impl MessagingSwitchboard {
    #[must_use]
    pub fn data_engine_execute() -> Ustr {
        Ustr::from("DataEngine.execute")
    }

    #[must_use]
    pub fn data_engine_process() -> Ustr {
        Ustr::from("DataEngine.process")
    }

    #[must_use]
    pub fn exec_engine_execute() -> Ustr {
        Ustr::from("ExecEngine.execute")
    }

    #[must_use]
    pub fn exec_engine_process() -> Ustr {
        Ustr::from("ExecEngine.process")
    }

    #[must_use]
    pub fn get_custom_topic(&mut self, data_type: &DataType) -> Ustr {
        *self
            .custom_topics
            .entry(data_type.clone())
            .or_insert_with(|| Ustr::from(&format!("data.{}", data_type.topic())))
    }

    #[must_use]
    pub fn get_instruments_topic(&mut self, venue: Venue) -> Ustr {
        *self
            .instruments_topics
            .entry(venue)
            .or_insert_with(|| Ustr::from(&format!("data.instrument.{}", venue)))
    }

    #[must_use]
    pub fn get_instrument_topic(&mut self, instrument_id: InstrumentId) -> Ustr {
        *self
            .instrument_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                Ustr::from(&format!(
                    "data.instrument.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                ))
            })
    }

    #[must_use]
    pub fn get_book_deltas_topic(&mut self, instrument_id: InstrumentId) -> Ustr {
        *self
            .book_deltas_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                Ustr::from(&format!(
                    "data.book.deltas.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                ))
            })
    }

    #[must_use]
    pub fn get_book_depth10_topic(&mut self, instrument_id: InstrumentId) -> Ustr {
        *self
            .book_depth10_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                Ustr::from(&format!(
                    "data.book.depth10.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                ))
            })
    }

    #[must_use]
    pub fn get_book_snapshots_topic(&mut self, instrument_id: InstrumentId) -> Ustr {
        *self
            .book_snapshots_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                Ustr::from(&format!(
                    "data.book.snapshots.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                ))
            })
    }

    #[must_use]
    pub fn get_quotes_topic(&mut self, instrument_id: InstrumentId) -> Ustr {
        *self.quote_topics.entry(instrument_id).or_insert_with(|| {
            Ustr::from(&format!(
                "data.quotes.{}.{}",
                instrument_id.venue, instrument_id.symbol
            ))
        })
    }

    #[must_use]
    pub fn get_trades_topic(&mut self, instrument_id: InstrumentId) -> Ustr {
        *self.trade_topics.entry(instrument_id).or_insert_with(|| {
            Ustr::from(&format!(
                "data.trades.{}.{}",
                instrument_id.venue, instrument_id.symbol
            ))
        })
    }

    #[must_use]
    pub fn get_bars_topic(&mut self, bar_type: BarType) -> Ustr {
        *self
            .bar_topics
            .entry(bar_type)
            .or_insert_with(|| Ustr::from(&format!("data.bars.{bar_type}")))
    }

    #[must_use]
    pub fn get_mark_price_topic(&mut self, instrument_id: InstrumentId) -> Ustr {
        *self
            .mark_price_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                Ustr::from(&format!(
                    "data.mark_prices.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                ))
            })
    }

    #[must_use]
    pub fn get_index_price_topic(&mut self, instrument_id: InstrumentId) -> Ustr {
        *self
            .index_price_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                Ustr::from(&format!(
                    "data.index_prices.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                ))
            })
    }

    #[must_use]
    pub fn get_instrument_status_topic(&mut self, instrument_id: InstrumentId) -> Ustr {
        *self
            .instrument_status_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                Ustr::from(&format!(
                    "data.status.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                ))
            })
    }

    #[must_use]
    pub fn get_instrument_close_topic(&mut self, instrument_id: InstrumentId) -> Ustr {
        *self
            .instrument_close_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                Ustr::from(&format!(
                    "data.close.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                ))
            })
    }

    #[must_use]
    pub fn get_order_snapshots_topic(&mut self, client_order_id: ClientOrderId) -> Ustr {
        *self
            .order_snapshots_topics
            .entry(client_order_id)
            .or_insert_with(|| Ustr::from(&format!("order.snapshots.{client_order_id}")))
    }

    #[must_use]
    pub fn get_positions_snapshots_topic(&mut self, position_id: PositionId) -> Ustr {
        *self
            .positions_snapshots_topics
            .entry(position_id)
            .or_insert_with(|| Ustr::from(&format!("positions.snapshots.{position_id}")))
    }

    #[must_use]
    pub fn get_event_orders_topic(&mut self, strategy_id: StrategyId) -> Ustr {
        *self
            .event_orders_topics
            .entry(strategy_id)
            .or_insert_with(|| Ustr::from(&format!("events.order.{strategy_id}")))
    }

    #[must_use]
    pub fn get_event_positions_topic(&mut self, strategy_id: StrategyId) -> Ustr {
        *self
            .event_positions_topics
            .entry(strategy_id)
            .or_insert_with(|| Ustr::from(&format!("events.position.{strategy_id}")))
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::{BarType, DataType},
        identifiers::InstrumentId,
    };
    use rstest::*;

    use super::*;

    #[fixture]
    fn switchboard() -> MessagingSwitchboard {
        MessagingSwitchboard::default()
    }

    #[fixture]
    fn instrument_id() -> InstrumentId {
        InstrumentId::from("ESZ24.XCME")
    }

    #[rstest]
    fn test_get_custom_topic(mut switchboard: MessagingSwitchboard) {
        let data_type = DataType::new("ExampleDataType", None);
        let expected_topic = Ustr::from("data.ExampleDataType");
        let result = switchboard.get_custom_topic(&data_type);
        assert_eq!(result, expected_topic);
        assert!(switchboard.custom_topics.contains_key(&data_type));
    }

    #[rstest]
    fn test_get_instrument_topic(
        mut switchboard: MessagingSwitchboard,
        instrument_id: InstrumentId,
    ) {
        let expected_topic = Ustr::from("data.instrument.XCME.ESZ24");
        let result = switchboard.get_instrument_topic(instrument_id);
        assert_eq!(result, expected_topic);
        assert!(switchboard.instrument_topics.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_get_book_deltas_topic(
        mut switchboard: MessagingSwitchboard,
        instrument_id: InstrumentId,
    ) {
        let expected_topic = Ustr::from("data.book.deltas.XCME.ESZ24");
        let result = switchboard.get_book_deltas_topic(instrument_id);
        assert_eq!(result, expected_topic);
        assert!(switchboard.book_deltas_topics.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_get_book_depth10_topic(
        mut switchboard: MessagingSwitchboard,
        instrument_id: InstrumentId,
    ) {
        let expected_topic = Ustr::from("data.book.depth10.XCME.ESZ24");
        let result = switchboard.get_book_depth10_topic(instrument_id);
        assert_eq!(result, expected_topic);
        assert!(switchboard.book_depth10_topics.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_get_book_snapshots_topic(
        mut switchboard: MessagingSwitchboard,
        instrument_id: InstrumentId,
    ) {
        let expected_topic = Ustr::from("data.book.snapshots.XCME.ESZ24");
        let result = switchboard.get_book_snapshots_topic(instrument_id);
        assert_eq!(result, expected_topic);
        assert!(
            switchboard
                .book_snapshots_topics
                .contains_key(&instrument_id)
        );
    }

    #[rstest]
    fn test_get_quotes_topic(mut switchboard: MessagingSwitchboard, instrument_id: InstrumentId) {
        let expected_topic = Ustr::from("data.quotes.XCME.ESZ24");
        let result = switchboard.get_quotes_topic(instrument_id);
        assert_eq!(result, expected_topic);
        assert!(switchboard.quote_topics.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_get_trades_topic(mut switchboard: MessagingSwitchboard, instrument_id: InstrumentId) {
        let expected_topic = Ustr::from("data.trades.XCME.ESZ24");
        let result = switchboard.get_trades_topic(instrument_id);
        assert_eq!(result, expected_topic);
        assert!(switchboard.trade_topics.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_get_bars_topic(mut switchboard: MessagingSwitchboard) {
        let bar_type = BarType::from("ESZ24.XCME-1-MINUTE-LAST-INTERNAL");
        let expected_topic = Ustr::from(&format!("data.bars.{bar_type}"));
        let result = switchboard.get_bars_topic(bar_type);
        assert_eq!(result, expected_topic);
        assert!(switchboard.bar_topics.contains_key(&bar_type));
    }

    #[rstest]
    fn test_get_order_snapshots_topic(mut switchboard: MessagingSwitchboard) {
        let client_order_id = ClientOrderId::from("O-123456789");
        let expected_topic = Ustr::from(&format!("order.snapshots.{client_order_id}"));
        let result = switchboard.get_order_snapshots_topic(client_order_id);
        assert_eq!(result, expected_topic);
        assert!(
            switchboard
                .order_snapshots_topics
                .contains_key(&client_order_id)
        );
    }
}
