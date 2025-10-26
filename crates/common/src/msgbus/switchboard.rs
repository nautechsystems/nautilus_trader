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

use std::num::NonZeroUsize;

use ahash::AHashMap;
use nautilus_model::{
    data::{BarType, DataType},
    identifiers::{ClientOrderId, InstrumentId, PositionId, StrategyId, Venue},
};

use super::core::{Endpoint, MStr, Topic};
use crate::msgbus::get_message_bus;

pub const CLOSE_TOPIC: &str = "CLOSE";

#[must_use]
pub fn get_custom_topic(data_type: &DataType) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_custom_topic(data_type)
}

#[must_use]
pub fn get_instruments_topic(venue: Venue) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_instruments_topic(venue)
}

#[must_use]
pub fn get_instrument_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_instrument_topic(instrument_id)
}

#[must_use]
pub fn get_book_deltas_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_book_deltas_topic(instrument_id)
}

#[must_use]
pub fn get_book_depth10_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_book_depth10_topic(instrument_id)
}

#[must_use]
pub fn get_book_snapshots_topic(
    instrument_id: InstrumentId,
    interval_ms: NonZeroUsize,
) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_book_snapshots_topic(instrument_id, interval_ms)
}

#[must_use]
pub fn get_quotes_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_quotes_topic(instrument_id)
}

#[must_use]
pub fn get_trades_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_trades_topic(instrument_id)
}

#[must_use]
pub fn get_bars_topic(bar_type: BarType) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_bars_topic(bar_type)
}

#[must_use]
pub fn get_mark_price_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_mark_price_topic(instrument_id)
}

#[must_use]
pub fn get_index_price_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_index_price_topic(instrument_id)
}

#[must_use]
pub fn get_funding_rate_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_funding_rate_topic(instrument_id)
}

#[must_use]
pub fn get_instrument_status_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_instrument_status_topic(instrument_id)
}

#[must_use]
pub fn get_instrument_close_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_instrument_close_topic(instrument_id)
}

#[must_use]
pub fn get_order_fills_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_order_fills_topic(instrument_id)
}

#[must_use]
pub fn get_order_snapshots_topic(client_order_id: ClientOrderId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_order_snapshots_topic(client_order_id)
}

#[must_use]
pub fn get_positions_snapshots_topic(position_id: PositionId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_positions_snapshots_topic(position_id)
}

#[must_use]
pub fn get_event_orders_topic(strategy_id: StrategyId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_event_orders_topic(strategy_id)
}

#[must_use]
pub fn get_event_positions_topic(strategy_id: StrategyId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_event_positions_topic(strategy_id)
}

/// Represents a switchboard of built-in messaging endpoint names.
#[derive(Clone, Debug)]
pub struct MessagingSwitchboard {
    custom_topics: AHashMap<DataType, MStr<Topic>>,
    instruments_topics: AHashMap<Venue, MStr<Topic>>,
    instrument_topics: AHashMap<InstrumentId, MStr<Topic>>,
    book_deltas_topics: AHashMap<InstrumentId, MStr<Topic>>,
    book_depth10_topics: AHashMap<InstrumentId, MStr<Topic>>,
    book_snapshots_topics: AHashMap<InstrumentId, MStr<Topic>>,
    quote_topics: AHashMap<InstrumentId, MStr<Topic>>,
    trade_topics: AHashMap<InstrumentId, MStr<Topic>>,
    bar_topics: AHashMap<BarType, MStr<Topic>>,
    mark_price_topics: AHashMap<InstrumentId, MStr<Topic>>,
    index_price_topics: AHashMap<InstrumentId, MStr<Topic>>,
    funding_rate_topics: AHashMap<InstrumentId, MStr<Topic>>,
    instrument_status_topics: AHashMap<InstrumentId, MStr<Topic>>,
    instrument_close_topics: AHashMap<InstrumentId, MStr<Topic>>,
    order_fills_topics: AHashMap<InstrumentId, MStr<Topic>>,
    event_orders_topics: AHashMap<StrategyId, MStr<Topic>>,
    event_positions_topics: AHashMap<StrategyId, MStr<Topic>>,
    order_snapshots_topics: AHashMap<ClientOrderId, MStr<Topic>>,
    positions_snapshots_topics: AHashMap<PositionId, MStr<Topic>>,
    #[cfg(feature = "defi")]
    pub(crate) defi: crate::defi::switchboard::DefiSwitchboard,
}

impl Default for MessagingSwitchboard {
    /// Creates a new default [`MessagingSwitchboard`] instance.
    fn default() -> Self {
        Self {
            custom_topics: AHashMap::new(),
            instruments_topics: AHashMap::new(),
            instrument_topics: AHashMap::new(),
            book_deltas_topics: AHashMap::new(),
            book_snapshots_topics: AHashMap::new(),
            book_depth10_topics: AHashMap::new(),
            quote_topics: AHashMap::new(),
            trade_topics: AHashMap::new(),
            mark_price_topics: AHashMap::new(),
            index_price_topics: AHashMap::new(),
            funding_rate_topics: AHashMap::new(),
            bar_topics: AHashMap::new(),
            instrument_status_topics: AHashMap::new(),
            instrument_close_topics: AHashMap::new(),
            order_fills_topics: AHashMap::new(),
            order_snapshots_topics: AHashMap::new(),
            event_orders_topics: AHashMap::new(),
            event_positions_topics: AHashMap::new(),
            positions_snapshots_topics: AHashMap::new(),
            #[cfg(feature = "defi")]
            defi: crate::defi::switchboard::DefiSwitchboard::default(),
        }
    }
}

impl MessagingSwitchboard {
    #[must_use]
    pub fn data_engine_queue_execute() -> MStr<Endpoint> {
        "DataEngine.queue_execute".into()
    }

    #[must_use]
    pub fn data_engine_execute() -> MStr<Endpoint> {
        "DataEngine.execute".into()
    }

    #[must_use]
    pub fn data_engine_process() -> MStr<Endpoint> {
        "DataEngine.process".into()
    }

    #[must_use]
    pub fn data_engine_response() -> MStr<Endpoint> {
        "DataEngine.response".into()
    }

    #[must_use]
    pub fn exec_engine_execute() -> MStr<Endpoint> {
        "ExecEngine.execute".into()
    }

    #[must_use]
    pub fn exec_engine_process() -> MStr<Endpoint> {
        "ExecEngine.process".into()
    }

    #[must_use]
    pub fn get_custom_topic(&mut self, data_type: &DataType) -> MStr<Topic> {
        *self
            .custom_topics
            .entry(data_type.clone())
            .or_insert_with(|| format!("data.{}", data_type.topic()).into())
    }

    #[must_use]
    pub fn get_instruments_topic(&mut self, venue: Venue) -> MStr<Topic> {
        *self
            .instruments_topics
            .entry(venue)
            .or_insert_with(|| format!("data.instrument.{venue}").into())
    }

    #[must_use]
    pub fn get_instrument_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .instrument_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                format!(
                    "data.instrument.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                )
                .into()
            })
    }

    #[must_use]
    pub fn get_book_deltas_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .book_deltas_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                format!(
                    "data.book.deltas.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                )
                .into()
            })
    }

    #[must_use]
    pub fn get_book_depth10_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .book_depth10_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                format!(
                    "data.book.depth10.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                )
                .into()
            })
    }

    #[must_use]
    pub fn get_book_snapshots_topic(
        &mut self,
        instrument_id: InstrumentId,
        interval_ms: NonZeroUsize,
    ) -> MStr<Topic> {
        *self
            .book_snapshots_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                format!(
                    "data.book.snapshots.{}.{}.{}",
                    instrument_id.venue, instrument_id.symbol, interval_ms
                )
                .into()
            })
    }

    #[must_use]
    pub fn get_quotes_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self.quote_topics.entry(instrument_id).or_insert_with(|| {
            format!(
                "data.quotes.{}.{}",
                instrument_id.venue, instrument_id.symbol
            )
            .into()
        })
    }

    #[must_use]
    pub fn get_trades_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self.trade_topics.entry(instrument_id).or_insert_with(|| {
            format!(
                "data.trades.{}.{}",
                instrument_id.venue, instrument_id.symbol
            )
            .into()
        })
    }

    #[must_use]
    pub fn get_bars_topic(&mut self, bar_type: BarType) -> MStr<Topic> {
        *self
            .bar_topics
            .entry(bar_type)
            .or_insert_with(|| format!("data.bars.{bar_type}").into())
    }

    #[must_use]
    pub fn get_mark_price_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .mark_price_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                format!(
                    "data.mark_prices.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                )
                .into()
            })
    }

    #[must_use]
    pub fn get_index_price_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .index_price_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                format!(
                    "data.index_prices.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                )
                .into()
            })
    }

    pub fn get_funding_rate_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .funding_rate_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                format!(
                    "data.funding_rates.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                )
                .into()
            })
    }

    #[must_use]
    pub fn get_instrument_status_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .instrument_status_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                format!(
                    "data.status.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                )
                .into()
            })
    }

    #[must_use]
    pub fn get_instrument_close_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .instrument_close_topics
            .entry(instrument_id)
            .or_insert_with(|| {
                format!(
                    "data.close.{}.{}",
                    instrument_id.venue, instrument_id.symbol
                )
                .into()
            })
    }

    #[must_use]
    pub fn get_order_fills_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .order_fills_topics
            .entry(instrument_id)
            .or_insert_with(|| format!("events.fills.{instrument_id}").into())
    }

    #[must_use]
    pub fn get_order_snapshots_topic(&mut self, client_order_id: ClientOrderId) -> MStr<Topic> {
        *self
            .order_snapshots_topics
            .entry(client_order_id)
            .or_insert_with(|| format!("order.snapshots.{client_order_id}").into())
    }

    #[must_use]
    pub fn get_positions_snapshots_topic(&mut self, position_id: PositionId) -> MStr<Topic> {
        *self
            .positions_snapshots_topics
            .entry(position_id)
            .or_insert_with(|| format!("positions.snapshots.{position_id}").into())
    }

    #[must_use]
    pub fn get_event_orders_topic(&mut self, strategy_id: StrategyId) -> MStr<Topic> {
        *self
            .event_orders_topics
            .entry(strategy_id)
            .or_insert_with(|| format!("events.order.{strategy_id}").into())
    }

    #[must_use]
    pub fn get_event_positions_topic(&mut self, strategy_id: StrategyId) -> MStr<Topic> {
        *self
            .event_positions_topics
            .entry(strategy_id)
            .or_insert_with(|| format!("events.position.{strategy_id}").into())
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
        let expected_topic = "data.ExampleDataType".into();
        let result = switchboard.get_custom_topic(&data_type);
        assert_eq!(result, expected_topic);
        assert!(switchboard.custom_topics.contains_key(&data_type));
    }

    #[rstest]
    fn test_get_instrument_topic(
        mut switchboard: MessagingSwitchboard,
        instrument_id: InstrumentId,
    ) {
        let expected_topic = "data.instrument.XCME.ESZ24".into();
        let result = switchboard.get_instrument_topic(instrument_id);
        assert_eq!(result, expected_topic);
        assert!(switchboard.instrument_topics.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_get_book_deltas_topic(
        mut switchboard: MessagingSwitchboard,
        instrument_id: InstrumentId,
    ) {
        let expected_topic = "data.book.deltas.XCME.ESZ24".into();
        let result = switchboard.get_book_deltas_topic(instrument_id);
        assert_eq!(result, expected_topic);
        assert!(switchboard.book_deltas_topics.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_get_book_depth10_topic(
        mut switchboard: MessagingSwitchboard,
        instrument_id: InstrumentId,
    ) {
        let expected_topic = "data.book.depth10.XCME.ESZ24".into();
        let result = switchboard.get_book_depth10_topic(instrument_id);
        assert_eq!(result, expected_topic);
        assert!(switchboard.book_depth10_topics.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_get_book_snapshots_topic(
        mut switchboard: MessagingSwitchboard,
        instrument_id: InstrumentId,
    ) {
        let expected_topic = "data.book.snapshots.XCME.ESZ24.1000".into();
        let interval_ms = NonZeroUsize::new(1000).unwrap();
        let result = switchboard.get_book_snapshots_topic(instrument_id, interval_ms);
        assert_eq!(result, expected_topic);
        assert!(
            switchboard
                .book_snapshots_topics
                .contains_key(&instrument_id)
        );
    }

    #[rstest]
    fn test_get_quotes_topic(mut switchboard: MessagingSwitchboard, instrument_id: InstrumentId) {
        let expected_topic = "data.quotes.XCME.ESZ24".into();
        let result = switchboard.get_quotes_topic(instrument_id);
        assert_eq!(result, expected_topic);
        assert!(switchboard.quote_topics.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_get_trades_topic(mut switchboard: MessagingSwitchboard, instrument_id: InstrumentId) {
        let expected_topic = "data.trades.XCME.ESZ24".into();
        let result = switchboard.get_trades_topic(instrument_id);
        assert_eq!(result, expected_topic);
        assert!(switchboard.trade_topics.contains_key(&instrument_id));
    }

    #[rstest]
    fn test_get_bars_topic(mut switchboard: MessagingSwitchboard) {
        let bar_type = BarType::from("ESZ24.XCME-1-MINUTE-LAST-INTERNAL");
        let expected_topic = format!("data.bars.{bar_type}").into();
        let result = switchboard.get_bars_topic(bar_type);
        assert_eq!(result, expected_topic);
        assert!(switchboard.bar_topics.contains_key(&bar_type));
    }

    #[rstest]
    fn test_get_order_snapshots_topic(mut switchboard: MessagingSwitchboard) {
        let client_order_id = ClientOrderId::from("O-123456789");
        let expected_topic = format!("order.snapshots.{client_order_id}").into();
        let result = switchboard.get_order_snapshots_topic(client_order_id);
        assert_eq!(result, expected_topic);
        assert!(
            switchboard
                .order_snapshots_topics
                .contains_key(&client_order_id)
        );
    }
}
