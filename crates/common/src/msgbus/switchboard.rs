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

use std::{num::NonZeroUsize, sync::OnceLock};

use ahash::AHashMap;
use nautilus_model::{
    data::{BarType, DataType},
    identifiers::{ClientOrderId, InstrumentId, PositionId, StrategyId, Venue},
};

use super::core::{Endpoint, MStr, Topic};
use crate::msgbus::get_message_bus;

pub const CLOSE_TOPIC: &str = "CLOSE";

////////////////////////////////////////////////////////////////////////////////
// Built-in endpoint constants
////////////////////////////////////////////////////////////////////////////////
// These are static endpoint addresses.
// They use OnceLock for thread-safe lazy initialization without instance state.

static DATA_QUEUE_EXECUTE_ENDPOINT: OnceLock<MStr<Endpoint>> = OnceLock::new();
static DATA_EXECUTE_ENDPOINT: OnceLock<MStr<Endpoint>> = OnceLock::new();
static DATA_PROCESS_ENDPOINT: OnceLock<MStr<Endpoint>> = OnceLock::new();
static DATA_RESPONSE_ENDPOINT: OnceLock<MStr<Endpoint>> = OnceLock::new();
static EXEC_EXECUTE_ENDPOINT: OnceLock<MStr<Endpoint>> = OnceLock::new();
static EXEC_PROCESS_ENDPOINT: OnceLock<MStr<Endpoint>> = OnceLock::new();
static EXEC_RECONCILE_REPORT_ENDPOINT: OnceLock<MStr<Endpoint>> = OnceLock::new();
static EXEC_RECONCILE_MASS_STATUS_ENDPOINT: OnceLock<MStr<Endpoint>> = OnceLock::new();
static RISK_EXECUTE_ENDPOINT: OnceLock<MStr<Endpoint>> = OnceLock::new();
static RISK_PROCESS_ENDPOINT: OnceLock<MStr<Endpoint>> = OnceLock::new();
static PORTFOLIO_UPDATE_ACCOUNT_ENDPOINT: OnceLock<MStr<Endpoint>> = OnceLock::new();

macro_rules! define_switchboard {
    ($(
        $field:ident: $key_ty:ty,
        $method:ident($($arg_name:ident: $arg_ty:ty),*) -> $key_expr:expr,
        $val_fmt:expr,
        $($val_args:expr),*
    );* $(;)?) => {
        /// Represents a switchboard of built-in messaging endpoint names.
        #[derive(Clone, Debug)]
        pub struct MessagingSwitchboard {
            $(
                $field: AHashMap<$key_ty, MStr<Topic>>,
            )*
            #[cfg(feature = "defi")]
            pub(crate) defi: crate::defi::switchboard::DefiSwitchboard,
        }

        impl Default for MessagingSwitchboard {
            /// Creates a new default [`MessagingSwitchboard`] instance.
            fn default() -> Self {
                Self {
                    $(
                        $field: AHashMap::new(),
                    )*
                    #[cfg(feature = "defi")]
                    defi: crate::defi::switchboard::DefiSwitchboard::default(),
                }
            }
        }

        impl MessagingSwitchboard {
            // Static endpoints
            #[inline]
            #[must_use]
            pub fn data_engine_queue_execute() -> MStr<Endpoint> {
                *DATA_QUEUE_EXECUTE_ENDPOINT.get_or_init(|| "DataEngine.queue_execute".into())
            }

            #[inline]
            #[must_use]
            pub fn data_engine_execute() -> MStr<Endpoint> {
                *DATA_EXECUTE_ENDPOINT.get_or_init(|| "DataEngine.execute".into())
            }

            #[inline]
            #[must_use]
            pub fn data_engine_process() -> MStr<Endpoint> {
                *DATA_PROCESS_ENDPOINT.get_or_init(|| "DataEngine.process".into())
            }

            #[inline]
            #[must_use]
            pub fn data_engine_response() -> MStr<Endpoint> {
                *DATA_RESPONSE_ENDPOINT.get_or_init(|| "DataEngine.response".into())
            }

            #[inline]
            #[must_use]
            pub fn exec_engine_execute() -> MStr<Endpoint> {
                *EXEC_EXECUTE_ENDPOINT.get_or_init(|| "ExecEngine.execute".into())
            }

            #[inline]
            #[must_use]
            pub fn exec_engine_process() -> MStr<Endpoint> {
                *EXEC_PROCESS_ENDPOINT.get_or_init(|| "ExecEngine.process".into())
            }

            #[inline]
            #[must_use]
            pub fn exec_engine_reconcile_execution_report() -> MStr<Endpoint> {
                *EXEC_RECONCILE_REPORT_ENDPOINT.get_or_init(|| "ExecEngine.reconcile_execution_report".into())
            }

            #[inline]
            #[must_use]
            pub fn exec_engine_reconcile_execution_mass_status() -> MStr<Endpoint> {
                *EXEC_RECONCILE_MASS_STATUS_ENDPOINT
                    .get_or_init(|| "ExecEngine.reconcile_execution_mass_status".into())
            }

            #[inline]
            #[must_use]
            pub fn risk_engine_execute() -> MStr<Endpoint> {
                *RISK_EXECUTE_ENDPOINT.get_or_init(|| "RiskEngine.execute".into())
            }

            #[inline]
            #[must_use]
            pub fn risk_engine_process() -> MStr<Endpoint> {
                *RISK_PROCESS_ENDPOINT.get_or_init(|| "RiskEngine.process".into())
            }

            #[inline]
            #[must_use]
            pub fn portfolio_update_account() -> MStr<Endpoint> {
                *PORTFOLIO_UPDATE_ACCOUNT_ENDPOINT.get_or_init(|| "Portfolio.update_account".into())
            }

            // Dynamic topics
            $(
                #[must_use]
                pub fn $method(&mut self, $($arg_name: $arg_ty),*) -> MStr<Topic> {
                    let key = $key_expr;
                    *self.$field
                        .entry(key)
                        .or_insert_with(|| format!($val_fmt, $($val_args),*).into())
                }
            )*
        }
    };
}

define_switchboard! {
    custom_topics: DataType,
    get_custom_topic(data_type: &DataType) -> data_type.clone(),
    "data.{}", data_type.topic();

    instruments_topics: Venue,
    get_instruments_topic(venue: Venue) -> venue,
    "data.instrument.{}", venue;

    instrument_topics: InstrumentId,
    get_instrument_topic(instrument_id: InstrumentId) -> instrument_id,
    "data.instrument.{}.{}", instrument_id.venue, instrument_id.symbol;

    book_deltas_topics: InstrumentId,
    get_book_deltas_topic(instrument_id: InstrumentId) -> instrument_id,
    "data.book.deltas.{}.{}", instrument_id.venue, instrument_id.symbol;

    book_depth10_topics: InstrumentId,
    get_book_depth10_topic(instrument_id: InstrumentId) -> instrument_id,
    "data.book.depth10.{}.{}", instrument_id.venue, instrument_id.symbol;

    book_snapshots_topics: (InstrumentId, NonZeroUsize),
    get_book_snapshots_topic(instrument_id: InstrumentId, interval_ms: NonZeroUsize) -> (instrument_id, interval_ms),
    "data.book.snapshots.{}.{}.{}", instrument_id.venue, instrument_id.symbol, interval_ms;

    quote_topics: InstrumentId,
    get_quotes_topic(instrument_id: InstrumentId) -> instrument_id,
    "data.quotes.{}.{}", instrument_id.venue, instrument_id.symbol;

    trade_topics: InstrumentId,
    get_trades_topic(instrument_id: InstrumentId) -> instrument_id,
    "data.trades.{}.{}", instrument_id.venue, instrument_id.symbol;

    bar_topics: BarType,
    get_bars_topic(bar_type: BarType) -> bar_type,
    "data.bars.{}", bar_type;

    mark_price_topics: InstrumentId,
    get_mark_price_topic(instrument_id: InstrumentId) -> instrument_id,
    "data.mark_prices.{}.{}", instrument_id.venue, instrument_id.symbol;

    index_price_topics: InstrumentId,
    get_index_price_topic(instrument_id: InstrumentId) -> instrument_id,
    "data.index_prices.{}.{}", instrument_id.venue, instrument_id.symbol;

    funding_rate_topics: InstrumentId,
    get_funding_rate_topic(instrument_id: InstrumentId) -> instrument_id,
    "data.funding_rates.{}.{}", instrument_id.venue, instrument_id.symbol;

    instrument_status_topics: InstrumentId,
    get_instrument_status_topic(instrument_id: InstrumentId) -> instrument_id,
    "data.status.{}.{}", instrument_id.venue, instrument_id.symbol;

    instrument_close_topics: InstrumentId,
    get_instrument_close_topic(instrument_id: InstrumentId) -> instrument_id,
    "data.close.{}.{}", instrument_id.venue, instrument_id.symbol;

    order_fills_topics: InstrumentId,
    get_order_fills_topic(instrument_id: InstrumentId) -> instrument_id,
    "events.fills.{}", instrument_id;

    order_snapshots_topics: ClientOrderId,
    get_order_snapshots_topic(client_order_id: ClientOrderId) -> client_order_id,
    "order.snapshots.{}", client_order_id;

    positions_snapshots_topics: PositionId,
    get_positions_snapshots_topic(position_id: PositionId) -> position_id,
    "positions.snapshots.{}", position_id;

    event_orders_topics: StrategyId,
    get_event_orders_topic(strategy_id: StrategyId) -> strategy_id,
    "events.order.{}", strategy_id;

    event_positions_topics: StrategyId,
    get_event_positions_topic(strategy_id: StrategyId) -> strategy_id,
    "events.position.{}", strategy_id;
}

////////////////////////////////////////////////////////////////////////////////
// Topic wrapper functions
////////////////////////////////////////////////////////////////////////////////
// These wrapper functions provide convenient access to switchboard topic methods
// by accessing the thread-local message bus instance.

macro_rules! define_wrappers {
    ($($method:ident($($arg_name:ident: $arg_ty:ty),*) -> $ret:ty),* $(,)?) => {
        $(
            #[must_use]
            pub fn $method($($arg_name: $arg_ty),*) -> $ret {
                get_message_bus()
                    .borrow_mut()
                    .switchboard
                    .$method($($arg_name),*)
            }
        )*
    }
}

define_wrappers! {
    get_custom_topic(data_type: &DataType) -> MStr<Topic>,
    get_instruments_topic(venue: Venue) -> MStr<Topic>,
    get_instrument_topic(instrument_id: InstrumentId) -> MStr<Topic>,
    get_book_deltas_topic(instrument_id: InstrumentId) -> MStr<Topic>,
    get_book_depth10_topic(instrument_id: InstrumentId) -> MStr<Topic>,
    get_book_snapshots_topic(instrument_id: InstrumentId, interval_ms: NonZeroUsize) -> MStr<Topic>,
    get_quotes_topic(instrument_id: InstrumentId) -> MStr<Topic>,
    get_trades_topic(instrument_id: InstrumentId) -> MStr<Topic>,
    get_bars_topic(bar_type: BarType) -> MStr<Topic>,
    get_mark_price_topic(instrument_id: InstrumentId) -> MStr<Topic>,
    get_index_price_topic(instrument_id: InstrumentId) -> MStr<Topic>,
    get_funding_rate_topic(instrument_id: InstrumentId) -> MStr<Topic>,
    get_instrument_status_topic(instrument_id: InstrumentId) -> MStr<Topic>,
    get_instrument_close_topic(instrument_id: InstrumentId) -> MStr<Topic>,
    get_order_fills_topic(instrument_id: InstrumentId) -> MStr<Topic>,
    get_order_snapshots_topic(client_order_id: ClientOrderId) -> MStr<Topic>,
    get_positions_snapshots_topic(position_id: PositionId) -> MStr<Topic>,
    get_event_orders_topic(strategy_id: StrategyId) -> MStr<Topic>,
    get_event_positions_topic(strategy_id: StrategyId) -> MStr<Topic>,
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
                .contains_key(&(instrument_id, interval_ms))
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
