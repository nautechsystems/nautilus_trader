// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Receiver-side identifier normalization for plug-in boundary payloads.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use nautilus_common::{signal::Signal, timer::TimeEvent};
use nautilus_model::{
    data::{
        Bar, BarType, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, OptionChainSlice, OptionGreeks, OptionStrikeData, OrderBookDelta,
        OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{OrderStatus, OrderType},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected, OrderPendingCancel,
        OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted, OrderTriggered,
        OrderUpdated, PositionChanged, PositionClosed, PositionOpened,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, OptionSeriesId,
        OrderListId, PositionId, StrategyId, Symbol, TraderId, Venue, VenueOrderId,
    },
    instruments::InstrumentAny,
    orderbook::OrderBook,
    orders::{
        LimitIfTouchedOrder, LimitOrder, MarketIfTouchedOrder, MarketOrder, MarketToLimitOrder,
        Order, OrderAny, StopLimitOrder, StopMarketOrder, TrailingStopLimitOrder,
        TrailingStopMarketOrder,
    },
    types::{Currency, Money},
};
use ustr::Ustr;

use crate::surfaces::commands::{
    CancelAllOrdersCommand, CancelOrderCommand, CancelOrdersCommand, CloseAllPositionsCommand,
    ClosePositionCommand, ModifyOrderCommand, QueryAccountCommand, QueryOrderCommand,
    SubmitOrderCommand, SubmitOrderListCommand,
};
#[cfg(feature = "host")]
use crate::surfaces::commands::{
    CancelAllOrdersHandle, CancelOrderHandle, CancelOrdersHandle, CloseAllPositionsHandle,
    ClosePositionHandle, ModifyOrderHandle, QueryAccountHandle, QueryOrderHandle,
    SubmitOrderHandle, SubmitOrderListHandle,
};

pub(crate) trait BoundaryNormalize {
    fn boundary_normalized(&self) -> Self;
}

#[cfg(feature = "host")]
pub(crate) trait BoundaryCommandHandle {
    type Command;

    fn boundary_normalized_command(&self) -> Self::Command;
}

impl BoundaryNormalize for Ustr {
    fn boundary_normalized(&self) -> Self {
        Self::from(self.as_str())
    }
}

impl<T> BoundaryNormalize for Option<T>
where
    T: BoundaryNormalize,
{
    fn boundary_normalized(&self) -> Self {
        self.as_ref().map(BoundaryNormalize::boundary_normalized)
    }
}

impl<T> BoundaryNormalize for Vec<T>
where
    T: BoundaryNormalize,
{
    fn boundary_normalized(&self) -> Self {
        self.iter()
            .map(BoundaryNormalize::boundary_normalized)
            .collect()
    }
}

impl<K, V> BoundaryNormalize for IndexMap<K, V>
where
    K: BoundaryNormalize + Eq + std::hash::Hash,
    V: BoundaryNormalize,
{
    fn boundary_normalized(&self) -> Self {
        self.iter()
            .map(|(key, value)| (key.boundary_normalized(), value.boundary_normalized()))
            .collect()
    }
}

impl<K, V> BoundaryNormalize for BTreeMap<K, V>
where
    K: Clone + Ord,
    V: BoundaryNormalize,
{
    fn boundary_normalized(&self) -> Self {
        self.iter()
            .map(|(key, value)| (key.clone(), value.boundary_normalized()))
            .collect()
    }
}

macro_rules! impl_ustr_identifier_normalize {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl BoundaryNormalize for $ty {
                fn boundary_normalized(&self) -> Self {
                    Self::from(self.as_str())
                }
            }
        )+
    };
}

impl_ustr_identifier_normalize!(
    AccountId,
    ClientId,
    ClientOrderId,
    ExecAlgorithmId,
    OrderListId,
    PositionId,
    StrategyId,
    Symbol,
    TraderId,
    Venue,
    VenueOrderId,
);

impl BoundaryNormalize for InstrumentId {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.symbol.boundary_normalized(),
            self.venue.boundary_normalized(),
        )
    }
}

impl BoundaryNormalize for OptionSeriesId {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.venue.boundary_normalized(),
            self.underlying.boundary_normalized(),
            self.settlement_currency.boundary_normalized(),
            self.expiration_ns,
        )
    }
}

impl BoundaryNormalize for Currency {
    fn boundary_normalized(&self) -> Self {
        Self {
            code: self.code.boundary_normalized(),
            precision: self.precision,
            iso4217: self.iso4217,
            name: self.name.boundary_normalized(),
            currency_type: self.currency_type,
        }
    }
}

impl BoundaryNormalize for Money {
    fn boundary_normalized(&self) -> Self {
        Self {
            raw: self.raw,
            currency: self.currency.boundary_normalized(),
        }
    }
}

impl BoundaryNormalize for TimeEvent {
    fn boundary_normalized(&self) -> Self {
        let mut value = self.clone();
        value.name = value.name.boundary_normalized();
        value
    }
}

impl BoundaryNormalize for Signal {
    fn boundary_normalized(&self) -> Self {
        let mut value = self.clone();
        value.name = value.name.boundary_normalized();
        value
    }
}

impl BoundaryNormalize for BarType {
    fn boundary_normalized(&self) -> Self {
        match *self {
            Self::Standard {
                instrument_id,
                spec,
                aggregation_source,
            } => Self::new(
                instrument_id.boundary_normalized(),
                spec,
                aggregation_source,
            ),
            Self::Composite {
                instrument_id,
                spec,
                aggregation_source,
                composite_step,
                composite_aggregation,
                composite_aggregation_source,
            } => Self::new_composite(
                instrument_id.boundary_normalized(),
                spec,
                aggregation_source,
                composite_step,
                composite_aggregation,
                composite_aggregation_source,
            ),
        }
    }
}

macro_rules! impl_normalize_fields {
    ($ty:ty, [$($field:ident),+ $(,)?]) => {
        impl BoundaryNormalize for $ty {
            fn boundary_normalized(&self) -> Self {
                let mut value = self.clone();
                $(
                    value.$field = value.$field.boundary_normalized();
                )+
                value
            }
        }
    };
}

impl_normalize_fields!(QuoteTick, [instrument_id]);
impl_normalize_fields!(TradeTick, [instrument_id]);
impl_normalize_fields!(Bar, [bar_type]);
impl_normalize_fields!(OrderBookDelta, [instrument_id]);
impl_normalize_fields!(OrderBookDeltas, [instrument_id, deltas]);
impl_normalize_fields!(OrderBookDepth10, [instrument_id]);
impl_normalize_fields!(OrderBook, [instrument_id]);
impl_normalize_fields!(MarkPriceUpdate, [instrument_id]);
impl_normalize_fields!(IndexPriceUpdate, [instrument_id]);
impl_normalize_fields!(FundingRateUpdate, [instrument_id]);
impl_normalize_fields!(OptionGreeks, [instrument_id]);
impl_normalize_fields!(InstrumentStatus, [instrument_id, reason, trading_event]);
impl_normalize_fields!(InstrumentClose, [instrument_id]);
impl_normalize_fields!(OptionStrikeData, [quote, greeks]);
impl_normalize_fields!(OptionChainSlice, [series_id, calls, puts]);

impl BoundaryNormalize for InstrumentAny {
    fn boundary_normalized(&self) -> Self {
        let mut value = self.clone();
        match &mut value {
            Self::Betting(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.event_type_name = instrument.event_type_name.boundary_normalized();
                instrument.competition_name = instrument.competition_name.boundary_normalized();
                instrument.event_name = instrument.event_name.boundary_normalized();
                instrument.event_country_code = instrument.event_country_code.boundary_normalized();
                instrument.betting_type = instrument.betting_type.boundary_normalized();
                instrument.market_id = instrument.market_id.boundary_normalized();
                instrument.market_name = instrument.market_name.boundary_normalized();
                instrument.market_type = instrument.market_type.boundary_normalized();
                instrument.selection_name = instrument.selection_name.boundary_normalized();
                instrument.currency = instrument.currency.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
            Self::BinaryOption(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.currency = instrument.currency.boundary_normalized();
                instrument.outcome = instrument.outcome.boundary_normalized();
                instrument.description = instrument.description.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
            Self::Cfd(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.base_currency = instrument.base_currency.boundary_normalized();
                instrument.quote_currency = instrument.quote_currency.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
            Self::Commodity(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.quote_currency = instrument.quote_currency.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
            Self::CryptoFuture(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.underlying = instrument.underlying.boundary_normalized();
                instrument.quote_currency = instrument.quote_currency.boundary_normalized();
                instrument.settlement_currency =
                    instrument.settlement_currency.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
            Self::CryptoFuturesSpread(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.underlying = instrument.underlying.boundary_normalized();
                instrument.quote_currency = instrument.quote_currency.boundary_normalized();
                instrument.settlement_currency =
                    instrument.settlement_currency.boundary_normalized();
                instrument.strategy_type = instrument.strategy_type.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
            Self::CryptoOption(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.underlying = instrument.underlying.boundary_normalized();
                instrument.quote_currency = instrument.quote_currency.boundary_normalized();
                instrument.settlement_currency =
                    instrument.settlement_currency.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
            Self::CryptoOptionSpread(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.underlying = instrument.underlying.boundary_normalized();
                instrument.quote_currency = instrument.quote_currency.boundary_normalized();
                instrument.settlement_currency =
                    instrument.settlement_currency.boundary_normalized();
                instrument.strategy_type = instrument.strategy_type.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
            Self::CryptoPerpetual(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.base_currency = instrument.base_currency.boundary_normalized();
                instrument.quote_currency = instrument.quote_currency.boundary_normalized();
                instrument.settlement_currency =
                    instrument.settlement_currency.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
            Self::CurrencyPair(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.base_currency = instrument.base_currency.boundary_normalized();
                instrument.quote_currency = instrument.quote_currency.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
            Self::Equity(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.isin = instrument.isin.boundary_normalized();
                instrument.currency = instrument.currency.boundary_normalized();
            }
            Self::FuturesContract(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.exchange = instrument.exchange.boundary_normalized();
                instrument.underlying = instrument.underlying.boundary_normalized();
                instrument.currency = instrument.currency.boundary_normalized();
            }
            Self::FuturesSpread(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.exchange = instrument.exchange.boundary_normalized();
                instrument.underlying = instrument.underlying.boundary_normalized();
                instrument.strategy_type = instrument.strategy_type.boundary_normalized();
                instrument.currency = instrument.currency.boundary_normalized();
            }
            Self::IndexInstrument(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.currency = instrument.currency.boundary_normalized();
            }
            Self::OptionContract(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.exchange = instrument.exchange.boundary_normalized();
                instrument.underlying = instrument.underlying.boundary_normalized();
                instrument.currency = instrument.currency.boundary_normalized();
            }
            Self::OptionSpread(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.exchange = instrument.exchange.boundary_normalized();
                instrument.underlying = instrument.underlying.boundary_normalized();
                instrument.strategy_type = instrument.strategy_type.boundary_normalized();
                instrument.currency = instrument.currency.boundary_normalized();
            }
            Self::PerpetualContract(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.underlying = instrument.underlying.boundary_normalized();
                instrument.base_currency = instrument.base_currency.boundary_normalized();
                instrument.quote_currency = instrument.quote_currency.boundary_normalized();
                instrument.settlement_currency =
                    instrument.settlement_currency.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
            Self::TokenizedAsset(instrument) => {
                instrument.id = instrument.id.boundary_normalized();
                instrument.raw_symbol = instrument.raw_symbol.boundary_normalized();
                instrument.base_currency = instrument.base_currency.boundary_normalized();
                instrument.quote_currency = instrument.quote_currency.boundary_normalized();
                instrument.isin = instrument.isin.boundary_normalized();
                instrument.max_notional = instrument.max_notional.boundary_normalized();
                instrument.min_notional = instrument.min_notional.boundary_normalized();
            }
        }
        value
    }
}

impl BoundaryNormalize for OrderInitialized {
    fn boundary_normalized(&self) -> Self {
        let mut value = self.clone();
        value.trader_id = value.trader_id.boundary_normalized();
        value.strategy_id = value.strategy_id.boundary_normalized();
        value.instrument_id = value.instrument_id.boundary_normalized();
        value.client_order_id = value.client_order_id.boundary_normalized();
        value.trigger_instrument_id = value.trigger_instrument_id.boundary_normalized();
        value.order_list_id = value.order_list_id.boundary_normalized();
        value.linked_order_ids = value.linked_order_ids.boundary_normalized();
        value.parent_order_id = value.parent_order_id.boundary_normalized();
        value.exec_algorithm_id = value.exec_algorithm_id.boundary_normalized();
        value.exec_algorithm_params = value.exec_algorithm_params.boundary_normalized();
        value.exec_spawn_id = value.exec_spawn_id.boundary_normalized();
        value.tags = value.tags.boundary_normalized();
        value
    }
}

impl BoundaryNormalize for OrderAny {
    fn boundary_normalized(&self) -> Self {
        if self.status() != OrderStatus::Initialized {
            return self.clone();
        }

        let init = OrderInitialized::from(self).boundary_normalized();
        match init.order_type {
            OrderType::Limit => Self::Limit(
                LimitOrder::try_from(init).expect("normalized Limit order remains valid"),
            ),
            OrderType::LimitIfTouched => Self::LimitIfTouched(
                LimitIfTouchedOrder::try_from(init)
                    .expect("normalized LimitIfTouched order remains valid"),
            ),
            OrderType::Market => Self::Market(
                MarketOrder::try_from(init).expect("normalized Market order remains valid"),
            ),
            OrderType::MarketIfTouched => Self::MarketIfTouched(
                MarketIfTouchedOrder::try_from(init)
                    .expect("normalized MarketIfTouched order remains valid"),
            ),
            OrderType::MarketToLimit => Self::MarketToLimit(
                MarketToLimitOrder::try_from(init)
                    .expect("normalized MarketToLimit order remains valid"),
            ),
            OrderType::StopLimit => Self::StopLimit(
                StopLimitOrder::try_from(init).expect("normalized StopLimit order remains valid"),
            ),
            OrderType::StopMarket => Self::StopMarket(
                StopMarketOrder::try_from(init).expect("normalized StopMarket order remains valid"),
            ),
            OrderType::TrailingStopLimit => Self::TrailingStopLimit(
                TrailingStopLimitOrder::try_from(init)
                    .expect("normalized TrailingStopLimit order remains valid"),
            ),
            OrderType::TrailingStopMarket => Self::TrailingStopMarket(
                TrailingStopMarketOrder::try_from(init)
                    .expect("normalized TrailingStopMarket order remains valid"),
            ),
        }
    }
}

impl_normalize_fields!(
    OrderSubmitted,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        account_id
    ]
);
impl_normalize_fields!(
    OrderAccepted,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id
    ]
);
impl_normalize_fields!(
    OrderRejected,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        account_id,
        reason
    ]
);
impl_normalize_fields!(
    OrderFilled,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        position_id,
        currency,
        commission
    ]
);
impl_normalize_fields!(
    OrderCanceled,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id
    ]
);
impl_normalize_fields!(
    OrderExpired,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id
    ]
);
impl_normalize_fields!(
    OrderTriggered,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id
    ]
);
impl_normalize_fields!(
    OrderDenied,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        reason
    ]
);
impl_normalize_fields!(
    OrderEmulated,
    [trader_id, strategy_id, instrument_id, client_order_id]
);
impl_normalize_fields!(
    OrderReleased,
    [trader_id, strategy_id, instrument_id, client_order_id]
);
impl_normalize_fields!(
    OrderPendingUpdate,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        account_id,
        venue_order_id
    ]
);
impl_normalize_fields!(
    OrderPendingCancel,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        account_id,
        venue_order_id
    ]
);
impl_normalize_fields!(
    OrderModifyRejected,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        reason
    ]
);
impl_normalize_fields!(
    OrderCancelRejected,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        reason
    ]
);
impl_normalize_fields!(
    OrderUpdated,
    [
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id
    ]
);
impl_normalize_fields!(
    PositionOpened,
    [
        trader_id,
        strategy_id,
        instrument_id,
        position_id,
        account_id,
        opening_order_id,
        currency
    ]
);
impl_normalize_fields!(
    PositionChanged,
    [
        trader_id,
        strategy_id,
        instrument_id,
        position_id,
        account_id,
        opening_order_id,
        currency,
        realized_pnl,
        unrealized_pnl
    ]
);
impl_normalize_fields!(
    PositionClosed,
    [
        trader_id,
        strategy_id,
        instrument_id,
        position_id,
        account_id,
        opening_order_id,
        closing_order_id,
        currency,
        realized_pnl,
        unrealized_pnl
    ]
);

impl BoundaryNormalize for SubmitOrderCommand {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.order.boundary_normalized(),
            self.position_id.boundary_normalized(),
            self.client_id.boundary_normalized(),
            self.params.clone(),
        )
    }
}

impl BoundaryNormalize for SubmitOrderListCommand {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.orders.boundary_normalized(),
            self.position_id.boundary_normalized(),
            self.client_id.boundary_normalized(),
            self.params.clone(),
        )
    }
}

impl BoundaryNormalize for CancelOrderCommand {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.client_order_id.boundary_normalized(),
            self.client_id.boundary_normalized(),
            self.params.clone(),
        )
    }
}

impl BoundaryNormalize for CancelOrdersCommand {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.client_order_ids.boundary_normalized(),
            self.client_id.boundary_normalized(),
            self.params.clone(),
        )
    }
}

impl BoundaryNormalize for CancelAllOrdersCommand {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.instrument_id.boundary_normalized(),
            self.order_side,
            self.client_id.boundary_normalized(),
            self.params.clone(),
        )
    }
}

impl BoundaryNormalize for ModifyOrderCommand {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.client_order_id.boundary_normalized(),
            self.quantity,
            self.price,
            self.trigger_price,
            self.client_id.boundary_normalized(),
            self.params.clone(),
        )
    }
}

impl BoundaryNormalize for ClosePositionCommand {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.position_id.boundary_normalized(),
            self.client_id.boundary_normalized(),
            self.tags.boundary_normalized(),
            self.time_in_force,
            self.reduce_only,
            self.quote_quantity,
        )
    }
}

impl BoundaryNormalize for CloseAllPositionsCommand {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.instrument_id.boundary_normalized(),
            self.position_side,
            self.client_id.boundary_normalized(),
            self.tags.boundary_normalized(),
            self.time_in_force,
            self.reduce_only,
            self.quote_quantity,
        )
    }
}

impl BoundaryNormalize for QueryAccountCommand {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.account_id.boundary_normalized(),
            self.client_id.boundary_normalized(),
            self.params.clone(),
        )
    }
}

impl BoundaryNormalize for QueryOrderCommand {
    fn boundary_normalized(&self) -> Self {
        Self::new(
            self.client_order_id.boundary_normalized(),
            self.client_id.boundary_normalized(),
            self.params.clone(),
        )
    }
}

macro_rules! impl_command_handle_normalize {
    ($handle:ty, $command:ty) => {
        #[cfg(feature = "host")]
        impl BoundaryCommandHandle for $handle {
            type Command = $command;

            fn boundary_normalized_command(&self) -> Self::Command {
                self.command().boundary_normalized()
            }
        }
    };
}

impl_command_handle_normalize!(SubmitOrderHandle, SubmitOrderCommand);
impl_command_handle_normalize!(SubmitOrderListHandle, SubmitOrderListCommand);
impl_command_handle_normalize!(CancelOrderHandle, CancelOrderCommand);
impl_command_handle_normalize!(CancelOrdersHandle, CancelOrdersCommand);
impl_command_handle_normalize!(CancelAllOrdersHandle, CancelAllOrdersCommand);
impl_command_handle_normalize!(ModifyOrderHandle, ModifyOrderCommand);
impl_command_handle_normalize!(ClosePositionHandle, ClosePositionCommand);
impl_command_handle_normalize!(CloseAllPositionsHandle, CloseAllPositionsCommand);
impl_command_handle_normalize!(QueryAccountHandle, QueryAccountCommand);
impl_command_handle_normalize!(QueryOrderHandle, QueryOrderCommand);

#[cfg(test)]
mod tests {
    use std::{
        alloc::{Layout, alloc},
        mem::{align_of, size_of},
        ptr::NonNull,
    };

    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce},
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol,
            TradeId, TraderId, Venue, VenueOrderId,
        },
        instruments::stubs,
        orders::{MarketOrder, Order, stubs::TestOrderEventStubs},
        stubs::TestDefault,
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[repr(C)]
    struct ForeignUstrHeader {
        hash: u64,
        len: usize,
    }

    fn foreign_ustr(value: &str) -> Ustr {
        let header_size = size_of::<ForeignUstrHeader>();
        let size = header_size + value.len() + 1;
        let layout = Layout::from_size_align(size, align_of::<ForeignUstrHeader>())
            .expect("foreign ustr layout is valid");

        // SAFETY: layout is non-zero and valid. The allocation is intentionally
        // leaked for the test process, matching ustr's never-free interner.
        let base = unsafe { alloc(layout) };
        assert!(!base.is_null(), "foreign ustr allocation failed");

        let header = base.cast::<ForeignUstrHeader>();
        // SAFETY: `base` points to `size` writable bytes with the requested
        // alignment. The layout matches the header shape that `ustr` reads.
        unsafe {
            header.write(ForeignUstrHeader {
                hash: 0,
                len: value.len(),
            });
        }

        // SAFETY: `base` points to `size` bytes and `header_size` is in bounds.
        let value_ptr = unsafe { base.add(header_size) };
        // SAFETY: `value_ptr` points to `value.len()` writable bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(value.as_ptr(), value_ptr, value.len());
        }

        // SAFETY: the allocation includes one trailing byte after `value`.
        let terminator = unsafe { base.add(header_size + value.len()) };
        // SAFETY: `terminator` points to the trailing writable byte.
        unsafe {
            terminator.write(0);
        }

        // SAFETY: `value_ptr` is non-null because it is derived from `base`.
        let ptr = unsafe { NonNull::new_unchecked(value_ptr) };
        // SAFETY: `Ustr` is represented by the same pointer value as `NonNull<u8>`.
        unsafe { std::mem::transmute::<NonNull<u8>, Ustr>(ptr) }
    }

    fn foreign_ustr_backed<T>(value: &str) -> T {
        let value = foreign_ustr(value);
        assert_eq!(size_of::<T>(), size_of::<Ustr>());
        // SAFETY: the Nautilus identifier newtypes covered by these tests are
        // single-`Ustr` `#[repr(C)]` wrappers. The fake Ustr storage stays live.
        unsafe { std::ptr::read((&raw const value).cast::<T>()) }
    }

    fn foreign_currency_from(currency: Currency) -> Currency {
        Currency {
            code: foreign_ustr(currency.code.as_str()),
            name: foreign_ustr(currency.name.as_str()),
            ..currency
        }
    }

    fn foreign_currency(code: &str) -> Currency {
        foreign_currency_from(Currency::from(code))
    }

    fn foreign_money(code: &str) -> Money {
        foreign_money_from(Money::new(1.0, Currency::from(code)))
    }

    fn foreign_money_from(money: Money) -> Money {
        Money {
            raw: money.raw,
            currency: foreign_currency_from(money.currency),
        }
    }

    fn foreign_instrument_id(value: &str) -> InstrumentId {
        let (symbol, venue) = value.rsplit_once('.').expect("instrument id has venue");
        InstrumentId::new(
            foreign_ustr_backed::<Symbol>(symbol),
            foreign_ustr_backed::<Venue>(venue),
        )
    }

    fn foreign_market_order(client_order_id: &str) -> OrderAny {
        OrderAny::Market(MarketOrder::new(
            foreign_ustr_backed::<TraderId>("TRADER-001"),
            foreign_ustr_backed::<StrategyId>("S-001"),
            foreign_instrument_id("ETH-USDT.BINANCE"),
            foreign_ustr_backed::<ClientOrderId>(client_order_id),
            OrderSide::Buy,
            Quantity::from("1.0"),
            TimeInForce::Gtc,
            UUID4::new(),
            UnixNanos::default(),
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ))
    }

    fn test_order(order_type: OrderType) -> OrderAny {
        match order_type {
            OrderType::Limit => OrderAny::Limit(LimitOrder::test_default()),
            OrderType::LimitIfTouched => {
                OrderAny::LimitIfTouched(LimitIfTouchedOrder::test_default())
            }
            OrderType::Market => OrderAny::Market(MarketOrder::test_default()),
            OrderType::MarketIfTouched => {
                OrderAny::MarketIfTouched(MarketIfTouchedOrder::test_default())
            }
            OrderType::MarketToLimit => OrderAny::MarketToLimit(MarketToLimitOrder::test_default()),
            OrderType::StopLimit => OrderAny::StopLimit(StopLimitOrder::test_default()),
            OrderType::StopMarket => OrderAny::StopMarket(StopMarketOrder::test_default()),
            OrderType::TrailingStopLimit => {
                OrderAny::TrailingStopLimit(TrailingStopLimitOrder::test_default())
            }
            OrderType::TrailingStopMarket => {
                OrderAny::TrailingStopMarket(TrailingStopMarketOrder::test_default())
            }
        }
    }

    fn assert_local_ustr(value: Ustr) {
        assert_eq!(value, Ustr::from(value.as_str()));
    }

    fn assert_local_currency(currency: Currency) {
        assert_local_ustr(currency.code);
        assert_local_ustr(currency.name);
    }

    fn assert_local_money(money: Money) {
        assert_local_currency(money.currency);
    }

    fn foreign_symbol_from(symbol: Symbol) -> Symbol {
        foreign_ustr_backed::<Symbol>(symbol.as_str())
    }

    fn foreign_instrument_id_from(instrument_id: InstrumentId) -> InstrumentId {
        foreign_instrument_id(&instrument_id.to_string())
    }

    fn foreignize_instrument_key(instrument_id: &mut InstrumentId, raw_symbol: &mut Symbol) {
        *instrument_id = foreign_instrument_id_from(*instrument_id);
        *raw_symbol = foreign_symbol_from(*raw_symbol);
    }

    fn assert_local_symbol(symbol: Symbol) {
        assert_eq!(symbol, Symbol::from(symbol.as_str()));
    }

    fn assert_local_instrument_id(instrument_id: InstrumentId) {
        assert_eq!(
            instrument_id,
            InstrumentId::from(instrument_id.to_string().as_str())
        );
    }

    fn assert_local_instrument_key(instrument_id: InstrumentId, raw_symbol: Symbol) {
        assert_local_instrument_id(instrument_id);
        assert_local_symbol(raw_symbol);
    }

    fn foreign_instrument_any(mut instrument: InstrumentAny) -> InstrumentAny {
        match &mut instrument {
            InstrumentAny::Betting(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.event_type_name = foreign_ustr(instrument.event_type_name.as_str());
                instrument.competition_name = foreign_ustr(instrument.competition_name.as_str());
                instrument.event_name = foreign_ustr(instrument.event_name.as_str());
                instrument.event_country_code =
                    foreign_ustr(instrument.event_country_code.as_str());
                instrument.betting_type = foreign_ustr(instrument.betting_type.as_str());
                instrument.market_id = foreign_ustr(instrument.market_id.as_str());
                instrument.market_name = foreign_ustr(instrument.market_name.as_str());
                instrument.market_type = foreign_ustr(instrument.market_type.as_str());
                instrument.selection_name = foreign_ustr(instrument.selection_name.as_str());
                instrument.currency = foreign_currency_from(instrument.currency);
                instrument.max_notional = Some(foreign_money("USD"));
                instrument.min_notional = Some(foreign_money("USD"));
            }
            InstrumentAny::BinaryOption(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.currency = foreign_currency_from(instrument.currency);
                instrument.outcome = Some(foreign_ustr("YES"));
                instrument.description = Some(foreign_ustr("Market settles yes"));
                instrument.max_notional = Some(foreign_money("USDC"));
                instrument.min_notional = Some(foreign_money("USDC"));
            }
            InstrumentAny::Cfd(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.base_currency = Some(foreign_currency("XAU"));
                instrument.quote_currency = foreign_currency_from(instrument.quote_currency);
                instrument.max_notional = Some(foreign_money("USD"));
                instrument.min_notional = Some(foreign_money("USD"));
            }
            InstrumentAny::Commodity(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.quote_currency = foreign_currency_from(instrument.quote_currency);
                instrument.max_notional = Some(foreign_money("USD"));
                instrument.min_notional = Some(foreign_money("USD"));
            }
            InstrumentAny::CryptoFuture(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.underlying = foreign_currency_from(instrument.underlying);
                instrument.quote_currency = foreign_currency_from(instrument.quote_currency);
                instrument.settlement_currency =
                    foreign_currency_from(instrument.settlement_currency);
                instrument.max_notional = Some(foreign_money("USDT"));
                instrument.min_notional = Some(foreign_money("USDT"));
            }
            InstrumentAny::CryptoFuturesSpread(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.underlying = foreign_currency_from(instrument.underlying);
                instrument.quote_currency = foreign_currency_from(instrument.quote_currency);
                instrument.settlement_currency =
                    foreign_currency_from(instrument.settlement_currency);
                instrument.strategy_type = foreign_ustr(instrument.strategy_type.as_str());
                instrument.max_notional = Some(foreign_money("USDT"));
                instrument.min_notional = Some(foreign_money("USDT"));
            }
            InstrumentAny::CryptoOption(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.underlying = foreign_currency_from(instrument.underlying);
                instrument.quote_currency = foreign_currency_from(instrument.quote_currency);
                instrument.settlement_currency =
                    foreign_currency_from(instrument.settlement_currency);
                instrument.max_notional = Some(foreign_money("USD"));
                instrument.min_notional = Some(foreign_money("USD"));
            }
            InstrumentAny::CryptoOptionSpread(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.underlying = foreign_currency_from(instrument.underlying);
                instrument.quote_currency = foreign_currency_from(instrument.quote_currency);
                instrument.settlement_currency =
                    foreign_currency_from(instrument.settlement_currency);
                instrument.strategy_type = foreign_ustr(instrument.strategy_type.as_str());
                instrument.max_notional = Some(foreign_money("USD"));
                instrument.min_notional = Some(foreign_money("USD"));
            }
            InstrumentAny::CryptoPerpetual(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.base_currency = foreign_currency_from(instrument.base_currency);
                instrument.quote_currency = foreign_currency_from(instrument.quote_currency);
                instrument.settlement_currency =
                    foreign_currency_from(instrument.settlement_currency);
                instrument.max_notional = Some(foreign_money("USDT"));
                instrument.min_notional = Some(foreign_money("USDT"));
            }
            InstrumentAny::CurrencyPair(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.base_currency = foreign_currency_from(instrument.base_currency);
                instrument.quote_currency = foreign_currency_from(instrument.quote_currency);
                instrument.max_notional = Some(foreign_money("USDT"));
                instrument.min_notional = Some(foreign_money("USDT"));
            }
            InstrumentAny::Equity(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.isin = Some(foreign_ustr("US0378331005"));
                instrument.currency = foreign_currency_from(instrument.currency);
            }
            InstrumentAny::FuturesContract(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.exchange = Some(foreign_ustr("XCME"));
                instrument.underlying = foreign_ustr(instrument.underlying.as_str());
                instrument.currency = foreign_currency_from(instrument.currency);
            }
            InstrumentAny::FuturesSpread(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.exchange = Some(foreign_ustr("XCME"));
                instrument.underlying = foreign_ustr(instrument.underlying.as_str());
                instrument.strategy_type = foreign_ustr(instrument.strategy_type.as_str());
                instrument.currency = foreign_currency_from(instrument.currency);
            }
            InstrumentAny::IndexInstrument(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.currency = foreign_currency_from(instrument.currency);
            }
            InstrumentAny::OptionContract(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.exchange = Some(foreign_ustr("XCME"));
                instrument.underlying = foreign_ustr(instrument.underlying.as_str());
                instrument.currency = foreign_currency_from(instrument.currency);
            }
            InstrumentAny::OptionSpread(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.exchange = Some(foreign_ustr("XCME"));
                instrument.underlying = foreign_ustr(instrument.underlying.as_str());
                instrument.strategy_type = foreign_ustr(instrument.strategy_type.as_str());
                instrument.currency = foreign_currency_from(instrument.currency);
            }
            InstrumentAny::PerpetualContract(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.underlying = foreign_ustr(instrument.underlying.as_str());
                instrument.base_currency = Some(foreign_currency("EUR"));
                instrument.quote_currency = foreign_currency_from(instrument.quote_currency);
                instrument.settlement_currency =
                    foreign_currency_from(instrument.settlement_currency);
                instrument.max_notional = Some(foreign_money("USD"));
                instrument.min_notional = Some(foreign_money("USD"));
            }
            InstrumentAny::TokenizedAsset(instrument) => {
                foreignize_instrument_key(&mut instrument.id, &mut instrument.raw_symbol);
                instrument.base_currency = foreign_currency_from(instrument.base_currency);
                instrument.quote_currency = foreign_currency_from(instrument.quote_currency);
                instrument.isin = Some(foreign_ustr("US0378331005"));
                instrument.max_notional = Some(foreign_money("USD"));
                instrument.min_notional = Some(foreign_money("USD"));
            }
        }
        instrument
    }

    fn assert_instrument_any_normalized(instrument: InstrumentAny) {
        match instrument {
            InstrumentAny::Betting(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_ustr(instrument.event_type_name);
                assert_local_ustr(instrument.competition_name);
                assert_local_ustr(instrument.event_name);
                assert_local_ustr(instrument.event_country_code);
                assert_local_ustr(instrument.betting_type);
                assert_local_ustr(instrument.market_id);
                assert_local_ustr(instrument.market_name);
                assert_local_ustr(instrument.market_type);
                assert_local_ustr(instrument.selection_name);
                assert_local_currency(instrument.currency);
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
            InstrumentAny::BinaryOption(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_currency(instrument.currency);
                assert_local_ustr(instrument.outcome.expect("outcome is set"));
                assert_local_ustr(instrument.description.expect("description is set"));
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
            InstrumentAny::Cfd(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_currency(instrument.base_currency.expect("base currency is set"));
                assert_local_currency(instrument.quote_currency);
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
            InstrumentAny::Commodity(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_currency(instrument.quote_currency);
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
            InstrumentAny::CryptoFuture(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_currency(instrument.underlying);
                assert_local_currency(instrument.quote_currency);
                assert_local_currency(instrument.settlement_currency);
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
            InstrumentAny::CryptoFuturesSpread(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_currency(instrument.underlying);
                assert_local_currency(instrument.quote_currency);
                assert_local_currency(instrument.settlement_currency);
                assert_local_ustr(instrument.strategy_type);
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
            InstrumentAny::CryptoOption(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_currency(instrument.underlying);
                assert_local_currency(instrument.quote_currency);
                assert_local_currency(instrument.settlement_currency);
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
            InstrumentAny::CryptoOptionSpread(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_currency(instrument.underlying);
                assert_local_currency(instrument.quote_currency);
                assert_local_currency(instrument.settlement_currency);
                assert_local_ustr(instrument.strategy_type);
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
            InstrumentAny::CryptoPerpetual(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_currency(instrument.base_currency);
                assert_local_currency(instrument.quote_currency);
                assert_local_currency(instrument.settlement_currency);
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
            InstrumentAny::CurrencyPair(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_currency(instrument.base_currency);
                assert_local_currency(instrument.quote_currency);
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
            InstrumentAny::Equity(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_ustr(instrument.isin.expect("isin is set"));
                assert_local_currency(instrument.currency);
            }
            InstrumentAny::FuturesContract(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_ustr(instrument.exchange.expect("exchange is set"));
                assert_local_ustr(instrument.underlying);
                assert_local_currency(instrument.currency);
            }
            InstrumentAny::FuturesSpread(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_ustr(instrument.exchange.expect("exchange is set"));
                assert_local_ustr(instrument.underlying);
                assert_local_ustr(instrument.strategy_type);
                assert_local_currency(instrument.currency);
            }
            InstrumentAny::IndexInstrument(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_currency(instrument.currency);
            }
            InstrumentAny::OptionContract(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_ustr(instrument.exchange.expect("exchange is set"));
                assert_local_ustr(instrument.underlying);
                assert_local_currency(instrument.currency);
            }
            InstrumentAny::OptionSpread(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_ustr(instrument.exchange.expect("exchange is set"));
                assert_local_ustr(instrument.underlying);
                assert_local_ustr(instrument.strategy_type);
                assert_local_currency(instrument.currency);
            }
            InstrumentAny::PerpetualContract(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_ustr(instrument.underlying);
                assert_local_currency(instrument.base_currency.expect("base currency is set"));
                assert_local_currency(instrument.quote_currency);
                assert_local_currency(instrument.settlement_currency);
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
            InstrumentAny::TokenizedAsset(instrument) => {
                assert_local_instrument_key(instrument.id, instrument.raw_symbol);
                assert_local_currency(instrument.base_currency);
                assert_local_currency(instrument.quote_currency);
                assert_local_ustr(instrument.isin.expect("isin is set"));
                assert_local_money(instrument.max_notional.expect("max notional is set"));
                assert_local_money(instrument.min_notional.expect("min notional is set"));
            }
        }
    }

    #[rstest]
    fn submit_order_command_normalizes_foreign_identifiers() {
        let order = foreign_market_order("O-1");
        assert_ne!(order.client_order_id(), ClientOrderId::from("O-1"));
        let command = SubmitOrderCommand::new(
            order,
            Some(foreign_ustr_backed::<PositionId>("P-001")),
            Some(foreign_ustr_backed::<ClientId>("SIM")),
            None,
        );

        let normalized = command.boundary_normalized();

        assert_eq!(
            normalized.order.client_order_id(),
            ClientOrderId::from("O-1")
        );
        assert_eq!(normalized.order.strategy_id(), StrategyId::from("S-001"));
        assert_eq!(
            normalized.order.instrument_id(),
            InstrumentId::from("ETH-USDT.BINANCE")
        );
        assert_eq!(normalized.position_id, Some(PositionId::from("P-001")));
        assert_eq!(normalized.client_id, Some(ClientId::from("SIM")));
    }

    #[rstest]
    fn submit_order_list_command_normalizes_foreign_identifiers() {
        let command = SubmitOrderListCommand::new(
            vec![foreign_market_order("O-1"), foreign_market_order("O-2")],
            Some(foreign_ustr_backed::<PositionId>("P-001")),
            Some(foreign_ustr_backed::<ClientId>("SIM")),
            None,
        );

        let normalized = command.boundary_normalized();

        assert_eq!(
            normalized.orders[0].client_order_id(),
            ClientOrderId::from("O-1")
        );
        assert_eq!(
            normalized.orders[1].client_order_id(),
            ClientOrderId::from("O-2")
        );
        assert_eq!(normalized.position_id, Some(PositionId::from("P-001")));
        assert_eq!(normalized.client_id, Some(ClientId::from("SIM")));
    }

    #[rstest]
    #[case(OrderType::Limit)]
    #[case(OrderType::LimitIfTouched)]
    #[case(OrderType::Market)]
    #[case(OrderType::MarketIfTouched)]
    #[case(OrderType::MarketToLimit)]
    #[case(OrderType::StopLimit)]
    #[case(OrderType::StopMarket)]
    #[case(OrderType::TrailingStopLimit)]
    #[case(OrderType::TrailingStopMarket)]
    fn order_any_normalizes_initialized_order_type(#[case] order_type: OrderType) {
        let order = test_order(order_type);

        let normalized = order.boundary_normalized();

        assert_eq!(normalized.status(), OrderStatus::Initialized);
        assert_eq!(normalized.order_type(), order_type);
        assert_eq!(normalized.client_order_id(), order.client_order_id());
        assert_eq!(normalized.instrument_id(), order.instrument_id());
    }

    #[rstest]
    fn order_any_does_not_rebuild_non_initialized_order() {
        let mut order = foreign_market_order("O-SUBMITTED");
        order
            .apply(TestOrderEventStubs::submitted(
                &order,
                AccountId::from("SIM-001"),
            ))
            .expect("submitted event applies");

        let normalized = order.boundary_normalized();

        assert_eq!(normalized.status(), OrderStatus::Submitted);
        assert_ne!(
            normalized.client_order_id(),
            ClientOrderId::from("O-SUBMITTED")
        );
    }

    #[rstest]
    fn cancel_order_command_normalizes_foreign_identifiers() {
        let command = CancelOrderCommand::new(
            foreign_ustr_backed::<ClientOrderId>("O-1"),
            Some(foreign_ustr_backed::<ClientId>("SIM")),
            None,
        );
        assert_ne!(command.client_order_id, ClientOrderId::from("O-1"));

        let normalized = command.boundary_normalized();

        assert_eq!(normalized.client_order_id, ClientOrderId::from("O-1"));
        assert_eq!(normalized.client_id, Some(ClientId::from("SIM")));
    }

    #[rstest]
    fn cancel_orders_command_normalizes_foreign_identifiers() {
        let command = CancelOrdersCommand::new(
            vec![
                foreign_ustr_backed::<ClientOrderId>("O-1"),
                foreign_ustr_backed::<ClientOrderId>("O-2"),
            ],
            Some(foreign_ustr_backed::<ClientId>("SIM")),
            None,
        );

        let normalized = command.boundary_normalized();

        assert_eq!(
            normalized.client_order_ids,
            vec![ClientOrderId::from("O-1"), ClientOrderId::from("O-2")]
        );
        assert_eq!(normalized.client_id, Some(ClientId::from("SIM")));
    }

    #[rstest]
    fn cancel_all_orders_command_normalizes_foreign_identifiers() {
        let command = CancelAllOrdersCommand::new(
            foreign_instrument_id("ETH-USDT.BINANCE"),
            Some(OrderSide::Buy),
            Some(foreign_ustr_backed::<ClientId>("SIM")),
            None,
        );

        let normalized = command.boundary_normalized();

        assert_eq!(
            normalized.instrument_id,
            InstrumentId::from("ETH-USDT.BINANCE")
        );
        assert_eq!(normalized.order_side, Some(OrderSide::Buy));
        assert_eq!(normalized.client_id, Some(ClientId::from("SIM")));
    }

    #[rstest]
    fn modify_order_command_normalizes_foreign_identifiers() {
        let command = ModifyOrderCommand::new(
            foreign_ustr_backed::<ClientOrderId>("O-1"),
            Some(Quantity::from("2.0")),
            Some(Price::from("100.00")),
            None,
            Some(foreign_ustr_backed::<ClientId>("SIM")),
            None,
        );

        let normalized = command.boundary_normalized();

        assert_eq!(normalized.client_order_id, ClientOrderId::from("O-1"));
        assert_eq!(normalized.client_id, Some(ClientId::from("SIM")));
    }

    #[rstest]
    fn close_position_command_normalizes_foreign_identifiers() {
        let foreign_tag = foreign_ustr("exit");
        assert_ne!(foreign_tag, Ustr::from("exit"));
        let command = ClosePositionCommand::new(
            foreign_ustr_backed::<PositionId>("P-001"),
            Some(foreign_ustr_backed::<ClientId>("SIM")),
            Some(vec![foreign_tag]),
            Some(TimeInForce::Ioc),
            Some(true),
            Some(false),
        );

        let normalized = command.boundary_normalized();

        assert_eq!(normalized.position_id, PositionId::from("P-001"));
        assert_eq!(normalized.client_id, Some(ClientId::from("SIM")));
        assert_eq!(normalized.tags, Some(vec![Ustr::from("exit")]));
    }

    #[rstest]
    fn close_all_positions_command_normalizes_foreign_identifiers() {
        let command = CloseAllPositionsCommand::new(
            foreign_instrument_id("ETH-USDT.BINANCE"),
            Some(PositionSide::Long),
            Some(foreign_ustr_backed::<ClientId>("SIM")),
            Some(vec![foreign_ustr("exit-all")]),
            Some(TimeInForce::Ioc),
            Some(true),
            Some(false),
        );

        let normalized = command.boundary_normalized();

        assert_eq!(
            normalized.instrument_id,
            InstrumentId::from("ETH-USDT.BINANCE")
        );
        assert_eq!(normalized.position_side, Some(PositionSide::Long));
        assert_eq!(normalized.client_id, Some(ClientId::from("SIM")));
        assert_eq!(normalized.tags, Some(vec![Ustr::from("exit-all")]));
    }

    #[rstest]
    fn query_account_command_normalizes_foreign_identifiers() {
        let command = QueryAccountCommand::new(
            foreign_ustr_backed::<AccountId>("SIM-001"),
            Some(foreign_ustr_backed::<ClientId>("SIM")),
            None,
        );

        let normalized = command.boundary_normalized();

        assert_eq!(normalized.account_id, AccountId::from("SIM-001"));
        assert_eq!(normalized.client_id, Some(ClientId::from("SIM")));
    }

    #[rstest]
    fn query_order_command_normalizes_foreign_identifiers() {
        let command = QueryOrderCommand::new(
            foreign_ustr_backed::<ClientOrderId>("O-1"),
            Some(foreign_ustr_backed::<ClientId>("SIM")),
            None,
        );

        let normalized = command.boundary_normalized();

        assert_eq!(normalized.client_order_id, ClientOrderId::from("O-1"));
        assert_eq!(normalized.client_id, Some(ClientId::from("SIM")));
    }

    #[rstest]
    fn money_normalizes_foreign_currency_identifiers() {
        let money = foreign_money("USD");
        assert_ne!(money.currency.code, Ustr::from("USD"));

        let normalized = money.boundary_normalized();

        assert_local_money(normalized);
    }

    #[rstest]
    fn order_filled_normalizes_currency_and_commission() {
        let event = OrderFilled::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("ETH-USDT.BINANCE"),
            ClientOrderId::from("O-1"),
            VenueOrderId::from("V-1"),
            AccountId::from("SIM-001"),
            TradeId::from("T-1"),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("1.0"),
            Price::from("100.00"),
            foreign_currency("USD"),
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(PositionId::from("P-001")),
            Some(foreign_money("USD")),
        );

        let normalized = event.boundary_normalized();

        assert_local_currency(normalized.currency);
        assert_local_money(normalized.commission.expect("commission is set"));
    }

    #[rstest]
    fn rejection_events_normalize_reason() {
        let rejected = OrderRejected::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("ETH-USDT.BINANCE"),
            ClientOrderId::from("O-1"),
            AccountId::from("SIM-001"),
            foreign_ustr("VENUE_REJECTED"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            false,
        );
        let denied = OrderDenied::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("ETH-USDT.BINANCE"),
            ClientOrderId::from("O-1"),
            foreign_ustr("RISK_DENIED"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let modify_rejected = OrderModifyRejected::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("ETH-USDT.BINANCE"),
            ClientOrderId::from("O-1"),
            foreign_ustr("MODIFY_REJECTED"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(VenueOrderId::from("V-1")),
            Some(AccountId::from("SIM-001")),
        );
        let cancel_rejected = OrderCancelRejected::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("ETH-USDT.BINANCE"),
            ClientOrderId::from("O-1"),
            foreign_ustr("CANCEL_REJECTED"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(VenueOrderId::from("V-1")),
            Some(AccountId::from("SIM-001")),
        );

        assert_local_ustr(rejected.boundary_normalized().reason);
        assert_local_ustr(denied.boundary_normalized().reason);
        assert_local_ustr(modify_rejected.boundary_normalized().reason);
        assert_local_ustr(cancel_rejected.boundary_normalized().reason);
    }

    #[rstest]
    fn position_events_normalize_currency_and_money() {
        let opened = PositionOpened {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("ETH-USDT.BINANCE"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 1.0,
            quantity: Quantity::from("1.0"),
            last_qty: Quantity::from("1.0"),
            last_px: Price::from("100.00"),
            currency: foreign_currency("USD"),
            avg_px_open: 100.0,
            event_id: UUID4::new(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        };
        let changed = PositionChanged {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("ETH-USDT.BINANCE"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 1.0,
            quantity: Quantity::from("1.0"),
            peak_quantity: Quantity::from("1.0"),
            last_qty: Quantity::from("1.0"),
            last_px: Price::from("100.00"),
            currency: foreign_currency("USD"),
            avg_px_open: 100.0,
            avg_px_close: Some(101.0),
            realized_return: 0.01,
            realized_pnl: Some(foreign_money("USD")),
            unrealized_pnl: foreign_money("USD"),
            event_id: UUID4::new(),
            ts_opened: UnixNanos::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        };
        let closed = PositionClosed {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("ETH-USDT.BINANCE"),
            position_id: PositionId::from("P-001"),
            account_id: AccountId::from("SIM-001"),
            opening_order_id: ClientOrderId::from("O-1"),
            closing_order_id: Some(ClientOrderId::from("O-2")),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 0.0,
            quantity: Quantity::from("0.0"),
            peak_quantity: Quantity::from("1.0"),
            last_qty: Quantity::from("1.0"),
            last_px: Price::from("101.00"),
            currency: foreign_currency("USD"),
            avg_px_open: 100.0,
            avg_px_close: Some(101.0),
            realized_return: 0.01,
            realized_pnl: Some(foreign_money("USD")),
            unrealized_pnl: foreign_money("USD"),
            duration: 0,
            event_id: UUID4::new(),
            ts_opened: UnixNanos::default(),
            ts_closed: Some(UnixNanos::default()),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        };

        let opened = opened.boundary_normalized();
        let changed = changed.boundary_normalized();
        let closed = closed.boundary_normalized();

        assert_local_currency(opened.currency);
        assert_local_currency(changed.currency);
        assert_local_money(changed.realized_pnl.expect("realized pnl is set"));
        assert_local_money(changed.unrealized_pnl);
        assert_local_currency(closed.currency);
        assert_local_money(closed.realized_pnl.expect("realized pnl is set"));
        assert_local_money(closed.unrealized_pnl);
    }

    #[rstest]
    fn instrument_variants_normalize_nested_ustr_currency_and_money_fields() {
        let instruments = [
            InstrumentAny::Betting(stubs::betting()),
            InstrumentAny::BinaryOption(stubs::binary_option()),
            InstrumentAny::Cfd(stubs::cfd_gold()),
            InstrumentAny::Commodity(stubs::commodity_gold()),
            InstrumentAny::CryptoFuture(stubs::crypto_future_btcusdt(
                2,
                6,
                Price::from("0.01"),
                Quantity::from("0.000001"),
            )),
            InstrumentAny::CryptoOption(stubs::crypto_option_btc_deribit(
                3,
                1,
                Price::from("0.001"),
                Quantity::from("0.1"),
            )),
            InstrumentAny::CryptoPerpetual(stubs::crypto_perpetual_ethusdt()),
            InstrumentAny::CurrencyPair(stubs::currency_pair_ethusdt()),
            InstrumentAny::Equity(stubs::equity_aapl()),
            InstrumentAny::FuturesContract(stubs::futures_contract_es(None, None)),
            InstrumentAny::FuturesSpread(stubs::futures_spread_es()),
            InstrumentAny::IndexInstrument(stubs::index_instrument_spx()),
            InstrumentAny::OptionContract(stubs::option_contract_appl()),
            InstrumentAny::OptionSpread(stubs::option_spread()),
            InstrumentAny::PerpetualContract(stubs::perpetual_contract_eurusd()),
            InstrumentAny::TokenizedAsset(stubs::tokenized_asset_aaplx()),
        ];

        for instrument in instruments {
            let normalized = foreign_instrument_any(instrument).boundary_normalized();
            assert_instrument_any_normalized(normalized);
        }
    }
}
