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

//! Cap'n Proto serialization for trading commands.

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::{ClientId, InstrumentId, StrategyId, TraderId};
use nautilus_serialization::{
    capnp::{ToCapnp, order_side_to_capnp},
    trading_capnp,
};

use crate::messages::execution::{
    BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, QueryOrder,
    SubmitOrder, SubmitOrderList, TradingCommand,
};

/// Helper function to populate a TradingCommandHeader builder
fn populate_trading_command_header<'a>(
    mut builder: trading_capnp::trading_command_header::Builder<'a>,
    trader_id: &TraderId,
    client_id: &ClientId,
    strategy_id: &StrategyId,
    instrument_id: &InstrumentId,
    command_id: &UUID4,
    ts_init: UnixNanos,
) {
    let trader_id_builder = builder.reborrow().init_trader_id();
    trader_id.to_capnp(trader_id_builder);

    let client_id_builder = builder.reborrow().init_client_id();
    client_id.to_capnp(client_id_builder);

    let strategy_id_builder = builder.reborrow().init_strategy_id();
    strategy_id.to_capnp(strategy_id_builder);

    let instrument_id_builder = builder.reborrow().init_instrument_id();
    instrument_id.to_capnp(instrument_id_builder);

    let command_id_builder = builder.reborrow().init_command_id();
    command_id.to_capnp(command_id_builder);

    let mut ts_init_builder = builder.reborrow().init_ts_init();
    ts_init_builder.set_value(*ts_init);
}

impl<'a> ToCapnp<'a> for CancelOrder {
    type Builder = trading_capnp::cancel_order::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let header_builder = builder.reborrow().init_header();
        populate_trading_command_header(
            header_builder,
            &self.trader_id,
            &self.client_id,
            &self.strategy_id,
            &self.instrument_id,
            &self.command_id,
            self.ts_init,
        );

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        let venue_order_id_builder = builder.reborrow().init_venue_order_id();
        self.venue_order_id.to_capnp(venue_order_id_builder);
    }
}

impl<'a> ToCapnp<'a> for CancelAllOrders {
    type Builder = trading_capnp::cancel_all_orders::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let header_builder = builder.reborrow().init_header();
        populate_trading_command_header(
            header_builder,
            &self.trader_id,
            &self.client_id,
            &self.strategy_id,
            &self.instrument_id,
            &self.command_id,
            self.ts_init,
        );

        builder.set_order_side(order_side_to_capnp(self.order_side));
    }
}

impl<'a> ToCapnp<'a> for BatchCancelOrders {
    type Builder = trading_capnp::batch_cancel_orders::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let header_builder = builder.reborrow().init_header();
        populate_trading_command_header(
            header_builder,
            &self.trader_id,
            &self.client_id,
            &self.strategy_id,
            &self.instrument_id,
            &self.command_id,
            self.ts_init,
        );

        let mut cancellations_builder = builder
            .reborrow()
            .init_cancellations(self.cancels.len() as u32);
        for (i, cancel) in self.cancels.iter().enumerate() {
            let cancel_builder = cancellations_builder.reborrow().get(i as u32);
            cancel.to_capnp(cancel_builder);
        }
    }
}

impl<'a> ToCapnp<'a> for ModifyOrder {
    type Builder = trading_capnp::modify_order::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let header_builder = builder.reborrow().init_header();
        populate_trading_command_header(
            header_builder,
            &self.trader_id,
            &self.client_id,
            &self.strategy_id,
            &self.instrument_id,
            &self.command_id,
            self.ts_init,
        );

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        let venue_order_id_builder = builder.reborrow().init_venue_order_id();
        self.venue_order_id.to_capnp(venue_order_id_builder);

        if let Some(ref quantity) = self.quantity {
            let quantity_builder = builder.reborrow().init_quantity();
            quantity.to_capnp(quantity_builder);
        }

        if let Some(ref price) = self.price {
            let price_builder = builder.reborrow().init_price();
            price.to_capnp(price_builder);
        }

        if let Some(ref trigger_price) = self.trigger_price {
            let trigger_price_builder = builder.reborrow().init_trigger_price();
            trigger_price.to_capnp(trigger_price_builder);
        }
    }
}

impl<'a> ToCapnp<'a> for QueryOrder {
    type Builder = trading_capnp::query_order::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let header_builder = builder.reborrow().init_header();
        populate_trading_command_header(
            header_builder,
            &self.trader_id,
            &self.client_id,
            &self.strategy_id,
            &self.instrument_id,
            &self.command_id,
            self.ts_init,
        );

        let client_order_id_builder = builder.reborrow().init_client_order_id();
        self.client_order_id.to_capnp(client_order_id_builder);

        let venue_order_id_builder = builder.reborrow().init_venue_order_id();
        self.venue_order_id.to_capnp(venue_order_id_builder);
    }
}

impl<'a> ToCapnp<'a> for QueryAccount {
    type Builder = trading_capnp::query_account::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let trader_id_builder = builder.reborrow().init_trader_id();
        self.trader_id.to_capnp(trader_id_builder);

        let account_id_builder = builder.reborrow().init_account_id();
        self.account_id.to_capnp(account_id_builder);

        let command_id_builder = builder.reborrow().init_command_id();
        self.command_id.to_capnp(command_id_builder);

        let mut ts_init_builder = builder.reborrow().init_ts_init();
        ts_init_builder.set_value(*self.ts_init);
    }
}

impl<'a> ToCapnp<'a> for SubmitOrder {
    type Builder = trading_capnp::submit_order::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let header_builder = builder.reborrow().init_header();
        populate_trading_command_header(
            header_builder,
            &self.trader_id,
            &self.client_id,
            &self.strategy_id,
            &self.instrument_id,
            &self.command_id,
            self.ts_init,
        );

        let order_init = self.order.init_event();
        let order_init_builder = builder.reborrow().init_order_init();
        order_init.to_capnp(order_init_builder);

        if let Some(ref position_id) = self.position_id {
            let position_id_builder = builder.reborrow().init_position_id();
            position_id.to_capnp(position_id_builder);
        }
    }
}

impl<'a> ToCapnp<'a> for SubmitOrderList {
    type Builder = trading_capnp::submit_order_list::Builder<'a>;

    fn to_capnp(&self, mut builder: Self::Builder) {
        let header_builder = builder.reborrow().init_header();
        populate_trading_command_header(
            header_builder,
            &self.trader_id,
            &self.client_id,
            &self.strategy_id,
            &self.instrument_id,
            &self.command_id,
            self.ts_init,
        );

        let mut order_inits_builder = builder
            .reborrow()
            .init_order_inits(self.order_list.orders.len() as u32);
        for (i, order) in self.order_list.orders.iter().enumerate() {
            let order_init = order.init_event();
            let order_init_builder = order_inits_builder.reborrow().get(i as u32);
            order_init.to_capnp(order_init_builder);
        }

        if let Some(ref position_id) = self.position_id {
            let position_id_builder = builder.reborrow().init_position_id();
            position_id.to_capnp(position_id_builder);
        }
    }
}

impl<'a> ToCapnp<'a> for TradingCommand {
    type Builder = trading_capnp::trading_command::Builder<'a>;

    fn to_capnp(&self, builder: Self::Builder) {
        match self {
            Self::SubmitOrder(command) => {
                let submit_builder = builder.init_submit_order();
                command.to_capnp(submit_builder);
            }
            Self::SubmitOrderList(command) => {
                let submit_list_builder = builder.init_submit_order_list();
                command.to_capnp(submit_list_builder);
            }
            Self::ModifyOrder(command) => {
                let modify_builder = builder.init_modify_order();
                command.to_capnp(modify_builder);
            }
            Self::CancelOrder(command) => {
                let cancel_builder = builder.init_cancel_order();
                command.to_capnp(cancel_builder);
            }
            Self::CancelAllOrders(command) => {
                let cancel_all_builder = builder.init_cancel_all_orders();
                command.to_capnp(cancel_all_builder);
            }
            Self::BatchCancelOrders(command) => {
                let batch_cancel_builder = builder.init_batch_cancel_orders();
                command.to_capnp(batch_cancel_builder);
            }
            Self::QueryOrder(command) => {
                let query_builder = builder.init_query_order();
                command.to_capnp(query_builder);
            }
            Self::QueryAccount(command) => {
                let query_builder = builder.init_query_account();
                command.to_capnp(query_builder);
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use capnp::message::Builder;
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId},
        orders::{Order, OrderList, OrderTestBuilder},
        types::{Price, Quantity},
    };
    use rstest::*;

    use super::*;
    use crate::messages::execution::{
        cancel::{BatchCancelOrdersBuilder, CancelAllOrdersBuilder, CancelOrderBuilder},
        modify::ModifyOrderBuilder,
        query::{QueryAccountBuilder, QueryOrderBuilder},
    };

    #[fixture]
    fn command_id() -> UUID4 {
        UUID4::new()
    }

    #[fixture]
    fn ts_init() -> UnixNanos {
        UnixNanos::default()
    }

    #[fixture]
    fn client_id() -> ClientId {
        ClientId::new("TEST")
    }

    #[rstest]
    fn test_cancel_order_serialization(command_id: UUID4, ts_init: UnixNanos) {
        let command = CancelOrderBuilder::default()
            .command_id(command_id)
            .ts_init(ts_init)
            .build()
            .unwrap();

        let mut message = Builder::new_default();
        {
            let builder = message.init_root::<trading_capnp::cancel_order::Builder>();
            command.to_capnp(builder);
        }

        let reader = message
            .get_root_as_reader::<trading_capnp::cancel_order::Reader>()
            .expect("Valid capnp message");

        // Verify header is populated
        assert!(reader.has_header());
        let header = reader.get_header().unwrap();
        assert!(header.has_trader_id());
        assert!(header.has_client_id());
        assert!(header.has_strategy_id());
        assert!(header.has_instrument_id());
        assert!(header.has_command_id());
        assert!(header.has_ts_init());
    }

    #[rstest]
    fn test_cancel_all_orders_serialization(command_id: UUID4, ts_init: UnixNanos) {
        let command = CancelAllOrdersBuilder::default()
            .order_side(OrderSide::Buy)
            .command_id(command_id)
            .ts_init(ts_init)
            .build()
            .unwrap();

        let mut message = Builder::new_default();
        {
            let builder = message.init_root::<trading_capnp::cancel_all_orders::Builder>();
            command.to_capnp(builder);
        }

        let reader = message
            .get_root_as_reader::<trading_capnp::cancel_all_orders::Reader>()
            .expect("Valid capnp message");

        assert!(reader.has_header());
    }

    #[rstest]
    fn test_batch_cancel_orders_serialization(command_id: UUID4, ts_init: UnixNanos) {
        let cancel1 = CancelOrderBuilder::default()
            .client_order_id(ClientOrderId::new("O-001"))
            .command_id(UUID4::new())
            .ts_init(ts_init)
            .build()
            .unwrap();

        let cancel2 = CancelOrderBuilder::default()
            .client_order_id(ClientOrderId::new("O-002"))
            .command_id(UUID4::new())
            .ts_init(ts_init)
            .build()
            .unwrap();

        let command = BatchCancelOrdersBuilder::default()
            .cancels(vec![cancel1, cancel2])
            .command_id(command_id)
            .ts_init(ts_init)
            .build()
            .unwrap();

        let mut message = Builder::new_default();
        {
            let builder = message.init_root::<trading_capnp::batch_cancel_orders::Builder>();
            command.to_capnp(builder);
        }

        let reader = message
            .get_root_as_reader::<trading_capnp::batch_cancel_orders::Reader>()
            .expect("Valid capnp message");

        assert!(reader.has_header());
        assert!(reader.has_cancellations());
        assert_eq!(reader.get_cancellations().unwrap().len(), 2);
    }

    #[rstest]
    fn test_modify_order_serialization(command_id: UUID4, ts_init: UnixNanos) {
        let command = ModifyOrderBuilder::default()
            .quantity(Some(Quantity::new(100.0, 0)))
            .price(Some(Price::new(50_000.0, 2)))
            .trigger_price(Some(Price::new(49_000.0, 2)))
            .command_id(command_id)
            .ts_init(ts_init)
            .build()
            .unwrap();

        let mut message = Builder::new_default();
        {
            let builder = message.init_root::<trading_capnp::modify_order::Builder>();
            command.to_capnp(builder);
        }

        let reader = message
            .get_root_as_reader::<trading_capnp::modify_order::Reader>()
            .expect("Valid capnp message");

        assert!(reader.has_header());
        assert!(reader.has_quantity());
        assert!(reader.has_price());
        assert!(reader.has_trigger_price());
    }

    #[rstest]
    fn test_query_order_serialization(command_id: UUID4, ts_init: UnixNanos) {
        let command = QueryOrderBuilder::default()
            .command_id(command_id)
            .ts_init(ts_init)
            .build()
            .unwrap();

        let mut message = Builder::new_default();
        {
            let builder = message.init_root::<trading_capnp::query_order::Builder>();
            command.to_capnp(builder);
        }

        let reader = message
            .get_root_as_reader::<trading_capnp::query_order::Reader>()
            .expect("Valid capnp message");

        assert!(reader.has_header());
    }

    #[rstest]
    fn test_query_account_serialization(command_id: UUID4, ts_init: UnixNanos) {
        let command = QueryAccountBuilder::default()
            .account_id(AccountId::new("ACC-001"))
            .command_id(command_id)
            .ts_init(ts_init)
            .build()
            .unwrap();

        let mut message = Builder::new_default();
        {
            let builder = message.init_root::<trading_capnp::query_account::Builder>();
            command.to_capnp(builder);
        }

        let reader = message
            .get_root_as_reader::<trading_capnp::query_account::Reader>()
            .expect("Valid capnp message");

        assert!(reader.has_trader_id());
        assert!(reader.has_account_id());
    }

    #[rstest]
    fn test_submit_order_serialization(command_id: UUID4, ts_init: UnixNanos, client_id: ClientId) {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .side(OrderSide::Buy)
            .quantity(Quantity::new(1.0, 8))
            .price(Price::new(50_000.0, 2))
            .build();

        let command = SubmitOrder::new(
            order.trader_id(),
            client_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id().unwrap_or_default(),
            order,
            None,
            None,
            None,
            command_id,
            ts_init,
        )
        .unwrap();

        let mut message = Builder::new_default();
        {
            let builder = message.init_root::<trading_capnp::submit_order::Builder>();
            command.to_capnp(builder);
        }

        let reader = message
            .get_root_as_reader::<trading_capnp::submit_order::Reader>()
            .expect("Valid capnp message");

        assert!(reader.has_header());
        assert!(reader.has_order_init());
    }

    #[rstest]
    fn test_submit_order_list_serialization(
        command_id: UUID4,
        ts_init: UnixNanos,
        client_id: ClientId,
    ) {
        let order1 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .side(OrderSide::Buy)
            .quantity(Quantity::new(1.0, 8))
            .price(Price::new(50_000.0, 2))
            .build();

        let order2 = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .side(OrderSide::Sell)
            .quantity(Quantity::new(1.0, 8))
            .price(Price::new(51_000.0, 2))
            .build();

        let order_list = OrderList::new(
            OrderListId::new("OL-001"),
            InstrumentId::from("BTCUSDT.BINANCE"),
            order1.strategy_id(),
            vec![order1.clone(), order2],
            ts_init,
        );

        let command = SubmitOrderList::new(
            order1.trader_id(),
            client_id,
            order1.strategy_id(),
            order1.instrument_id(),
            order1.client_order_id(),
            order1.venue_order_id().unwrap_or_default(),
            order_list,
            None,
            None,
            command_id,
            ts_init,
        )
        .unwrap();

        let mut message = Builder::new_default();
        {
            let builder = message.init_root::<trading_capnp::submit_order_list::Builder>();
            command.to_capnp(builder);
        }

        let reader = message
            .get_root_as_reader::<trading_capnp::submit_order_list::Reader>()
            .expect("Valid capnp message");

        assert!(reader.has_header());
        assert!(reader.has_order_inits());
        assert_eq!(reader.get_order_inits().unwrap().len(), 2);
    }

    #[rstest]
    fn test_trading_command_enum_serialization(command_id: UUID4, ts_init: UnixNanos) {
        let cancel = CancelOrderBuilder::default()
            .command_id(command_id)
            .ts_init(ts_init)
            .build()
            .unwrap();

        let command = TradingCommand::CancelOrder(cancel);

        let mut message = Builder::new_default();
        {
            let builder = message.init_root::<trading_capnp::trading_command::Builder>();
            command.to_capnp(builder);
        }

        let reader = message
            .get_root_as_reader::<trading_capnp::trading_command::Reader>()
            .expect("Valid capnp message");

        // Verify it's a cancel order variant
        assert!(reader.has_cancel_order());
    }
}
