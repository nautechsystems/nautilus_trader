# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

import pytest

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.c_enums.contingency_type import ContingencyType
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events.order import OrderDenied
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderInitialized
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.market import MarketOrder
from nautilus_trader.model.orders.stop_limit import StopLimitOrder
from nautilus_trader.model.orders.stop_market import StopMarketOrder
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import UNIX_EPOCH
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestOrders:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestStubs.trader_id()
        self.strategy_id = TestStubs.strategy_id()
        self.account_id = TestStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            clock=TestClock(),
        )

    def test_opposite_side_given_invalid_value_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            Order.opposite_side(0)  # <-- invalid value

    def test_flatten_side_given_invalid_value_or_flat_raises_value_error(self):
        # Arrange, Act
        with pytest.raises(ValueError):
            Order.flatten_side(0)  # <-- invalid value

        with pytest.raises(ValueError):
            Order.flatten_side(PositionSide.FLAT)

    @pytest.mark.parametrize(
        "side, expected",
        [
            [OrderSide.BUY, OrderSide.SELL],
            [OrderSide.SELL, OrderSide.BUY],
        ],
    )
    def test_opposite_side_returns_expected_sides(self, side, expected):
        # Arrange, Act
        result = Order.opposite_side(side)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "side, expected",
        [
            [PositionSide.LONG, OrderSide.SELL],
            [PositionSide.SHORT, OrderSide.BUY],
        ],
    )
    def test_flatten_side_returns_expected_sides(self, side, expected):
        # Arrange, Act
        result = Order.flatten_side(side)

        # Assert
        assert result == expected

    def test_market_order_with_quantity_zero_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            MarketOrder(
                self.trader_id,
                self.strategy_id,
                AUDUSD_SIM.id,
                ClientOrderId("O-123456"),
                OrderSide.BUY,
                Quantity.zero(),
                TimeInForce.DAY,
                UUID4(),
                0,
            )

    def test_market_order_with_invalid_tif_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            MarketOrder(
                self.trader_id,
                self.strategy_id,
                AUDUSD_SIM.id,
                ClientOrderId("O-123456"),
                OrderSide.BUY,
                Quantity.zero(),
                TimeInForce.GTD,  # <-- invalid
                UUID4(),
                0,
            )

    def test_stop_market_order_with_gtd_and_expire_time_none_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            StopMarketOrder(
                self.trader_id,
                self.strategy_id,
                AUDUSD_SIM.id,
                ClientOrderId("O-123456"),
                OrderSide.BUY,
                Quantity.from_int(100000),
                price=Price.from_str("1.00000"),
                init_id=UUID4(),
                ts_init=0,
                time_in_force=TimeInForce.GTD,
                expire_time=None,
            )

    def test_stop_limit_buy_order_with_gtd_and_expire_time_none_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            StopLimitOrder(
                self.trader_id,
                self.strategy_id,
                AUDUSD_SIM.id,
                ClientOrderId("O-123456"),
                OrderSide.BUY,
                Quantity.from_int(100000),
                price=Price.from_str("1.00001"),
                trigger=Price.from_str("1.00000"),
                init_id=UUID4(),
                ts_init=0,
                time_in_force=TimeInForce.GTD,
                expire_time=None,
            )

    def test_overfill_limit_buy_order_raises_value_error(self):
        # Arrange, Act, Assert
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))
        over_fill = TestStubs.event_order_filled(
            order, instrument=AUDUSD_SIM, last_qty=Quantity.from_int(110000)  # <-- overfill
        )

        # Assert
        with pytest.raises(ValueError):
            order.apply(over_fill)

    def test_reset_order_factory(self):
        # Arrange
        self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        # Act
        self.order_factory.reset()

        order2 = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        assert order2.client_order_id.value == "O-19700101-000000-000-001-1"

    def test_initialize_buy_market_order(self):
        # Arrange, Act
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        # Assert
        assert order.symbol == AUDUSD_SIM.id.symbol
        assert order.venue == AUDUSD_SIM.id.venue
        assert order.type == OrderType.MARKET
        assert order.status == OrderStatus.INITIALIZED
        assert order.event_count == 1
        assert isinstance(order.last_event, OrderInitialized)
        assert order.is_active
        assert not order.is_inflight
        assert not order.is_working
        assert not order.is_completed
        assert order.is_buy
        assert order.is_aggressive
        assert not order.is_sell
        assert not order.is_contingency
        assert not order.is_passive
        assert not order.is_parent_order
        assert not order.is_child_order
        assert order.ts_last == 0
        assert order.last_event.ts_init == 0
        assert isinstance(order.init_event, OrderInitialized)

    def test_initialize_sell_market_order(self):
        # Arrange, Act
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        # Assert
        assert order.type == OrderType.MARKET
        assert order.status == OrderStatus.INITIALIZED
        assert order.event_count == 1
        assert isinstance(order.last_event, OrderInitialized)
        assert len(order.events) == 1
        assert order.is_active
        assert not order.is_inflight
        assert not order.is_working
        assert not order.is_completed
        assert not order.is_buy
        assert order.is_sell
        assert order.ts_last == 0
        assert isinstance(order.init_event, OrderInitialized)

    def test_order_equality(self):
        # Arrange, Act
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        # Assert
        assert order == order

    def test_order_hash_str_and_repr(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            tags="ENTRY",
        )

        # Act, Assert
        assert isinstance(hash(order), int)
        assert (
            str(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, tags=ENTRY)"  # noqa
        )
        assert (
            repr(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, tags=ENTRY)"  # noqa
        )

    def test_market_order_to_dict(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        # Act
        result = order.to_dict()

        # Assert
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "execution_id": None,
            "type": "MARKET",
            "side": "BUY",
            "quantity": "100000",
            "time_in_force": "GTC",
            "reduce_only": False,
            "filled_qty": "0",
            "avg_px": None,
            "slippage": "0",
            "status": "INITIALIZED",
            "order_list_id": None,
            "parent_order_id": None,
            "child_order_ids": None,
            "contingency": "NONE",
            "contingency_ids": None,
            "tags": None,
            "ts_last": 0,
            "ts_init": 0,
        }

    def test_initialize_limit_order(self):
        # Arrange, Act
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        # Assert
        assert order.type == OrderType.LIMIT
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.is_passive
        assert order.is_active
        assert not order.is_aggressive
        assert not order.is_completed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, tags=None)"  # noqa
        )
        assert (
            repr(order)
            == "LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, tags=None)"  # noqa
        )

    def test_limit_order_to_dict(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        # Act
        result = order.to_dict()

        # Assert
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "execution_id": None,
            "type": "LIMIT",
            "side": "BUY",
            "quantity": "100000",
            "price": "1.00000",
            "liquidity_side": "NONE",
            "expire_time_ns": 0,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "avg_px": None,
            "slippage": "0",
            "status": "INITIALIZED",
            "is_post_only": False,
            "is_reduce_only": False,
            "is_hidden": False,
            "order_list_id": None,
            "parent_order_id": None,
            "child_order_ids": None,
            "contingency": "NONE",
            "contingency_ids": None,
            "tags": None,
            "ts_last": 0,
            "ts_init": 0,
        }

    def test_initialize_limit_order_with_expire_time(self):
        # Arrange, Act
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
            TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
        )

        # Assert
        assert order.instrument_id == AUDUSD_SIM.id
        assert order.type == OrderType.LIMIT
        assert order.price == Price.from_str("1.00000")
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTD
        assert order.expire_time == UNIX_EPOCH
        assert not order.is_completed
        assert isinstance(order.init_event, OrderInitialized)

    def test_initialize_stop_market_order(self):
        # Arrange, Act
        order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        # Assert
        assert order.type == OrderType.STOP_MARKET
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.is_passive
        assert not order.is_aggressive
        assert order.is_active
        assert not order.is_completed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "StopMarketOrder(BUY 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, tags=None)"  # noqa
        )
        assert (
            repr(order)
            == "StopMarketOrder(BUY 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, tags=None)"  # noqa
        )

    def test_stop_market_order_to_dict(self):
        # Arrange
        order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        # Act
        result = order.to_dict()

        # Assert
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "execution_id": None,
            "type": "STOP_MARKET",
            "side": "BUY",
            "quantity": "100000",
            "price": "1.00000",
            "liquidity_side": "NONE",
            "expire_time_ns": 0,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "avg_px": None,
            "slippage": "0",
            "status": "INITIALIZED",
            "is_reduce_only": False,
            "order_list_id": None,
            "parent_order_id": None,
            "child_order_ids": None,
            "contingency": "NONE",
            "contingency_ids": None,
            "tags": None,
            "ts_last": 0,
            "ts_init": 0,
        }

    def test_initialize_stop_limit_order(self):
        # Arrange, Act
        order = self.order_factory.stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
            Price.from_str("1.10010"),
            tags="ENTRY",
        )

        # Assert
        assert order.type == OrderType.STOP_LIMIT
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_completed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "StopLimitOrder(BUY 100_000 AUD/USD.SIM STOP_LIMIT @ 1.00000 GTC, trigger=1.10010, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1)"  # noqa
        )
        assert (
            repr(order)
            == "StopLimitOrder(BUY 100_000 AUD/USD.SIM STOP_LIMIT @ 1.00000 GTC, trigger=1.10010, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1)"  # noqa
        )

    def test_stop_limit_order_to_dict(self):
        # Arrange
        order = self.order_factory.stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
            Price.from_str("1.10010"),
            tags="STOP_LOSS",
        )

        # Act
        result = order.to_dict()

        # Assert
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "execution_id": None,
            "type": "STOP_LIMIT",
            "side": "BUY",
            "quantity": "100000",
            "trigger": "1.10010",
            "price": "1.00000",
            "liquidity_side": "NONE",
            "expire_time_ns": 0,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "avg_px": None,
            "slippage": "0",
            "status": "INITIALIZED",
            "is_post_only": False,
            "is_reduce_only": False,
            "is_hidden": False,
            "order_list_id": None,
            "parent_order_id": None,
            "child_order_ids": None,
            "contingency": "NONE",
            "contingency_ids": None,
            "tags": "STOP_LOSS",
            "ts_last": 0,
            "ts_init": 0,
        }

    def test_order_list_equality(self):
        # Arrange
        bracket1 = self.order_factory.bracket_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
            Price.from_str("1.00010"),
        )
        bracket2 = self.order_factory.bracket_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
            Price.from_str("1.00010"),
        )

        # Act, Assert
        assert bracket1 == bracket1
        assert bracket1 != bracket2

    def test_bracket_market_order_list(self):
        # Arrange, Act
        bracket = self.order_factory.bracket_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("0.99990"),
            Price.from_str("1.00010"),
            TimeInForce.GTC,
        )

        # Assert
        assert bracket.id == OrderListId("1")
        assert bracket.instrument_id == AUDUSD_SIM.id
        assert len(bracket.orders) == 3
        assert bracket.orders[0].type == OrderType.MARKET
        assert bracket.orders[1].type == OrderType.STOP_MARKET
        assert bracket.orders[2].type == OrderType.LIMIT
        assert bracket.orders[0].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[1].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[2].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[0].client_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[1].client_order_id == ClientOrderId("O-19700101-000000-000-001-2")
        assert bracket.orders[2].client_order_id == ClientOrderId("O-19700101-000000-000-001-3")
        assert bracket.orders[0].side == OrderSide.BUY
        assert bracket.orders[1].side == OrderSide.SELL
        assert bracket.orders[2].side == OrderSide.SELL
        assert bracket.orders[0].quantity == Quantity.from_int(100000)
        assert bracket.orders[1].quantity == Quantity.from_int(100000)
        assert bracket.orders[2].quantity == Quantity.from_int(100000)
        assert bracket.orders[1].price == Price.from_str("0.99990")
        assert bracket.orders[2].price == Price.from_str("1.00010")
        assert bracket.orders[1].time_in_force == TimeInForce.GTC
        assert bracket.orders[2].time_in_force == TimeInForce.GTC
        assert bracket.orders[1].expire_time is None
        assert bracket.orders[2].expire_time is None
        assert bracket.orders[0].contingency == ContingencyType.OTO
        assert bracket.orders[1].contingency == ContingencyType.OCO
        assert bracket.orders[2].contingency == ContingencyType.OCO
        assert bracket.orders[0].contingency_ids == [
            ClientOrderId("O-19700101-000000-000-001-2"),
            ClientOrderId("O-19700101-000000-000-001-3"),
        ]
        assert bracket.orders[1].contingency_ids == [ClientOrderId("O-19700101-000000-000-001-3")]
        assert bracket.orders[2].contingency_ids == [ClientOrderId("O-19700101-000000-000-001-2")]
        assert bracket.orders[0].child_order_ids == [
            ClientOrderId("O-19700101-000000-000-001-2"),
            ClientOrderId("O-19700101-000000-000-001-3"),
        ]
        assert bracket.orders[1].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[2].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.ts_init == 0

    def test_bracket_limit_order_list(self):
        # Arrange, Act
        bracket = self.order_factory.bracket_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
            Price.from_str("0.99990"),
            Price.from_str("1.00010"),
            TimeInForce.GTC,
        )

        # Assert
        assert bracket.id == OrderListId("1")
        assert bracket.instrument_id == AUDUSD_SIM.id
        assert len(bracket.orders) == 3
        assert bracket.orders[0].type == OrderType.LIMIT
        assert bracket.orders[1].type == OrderType.STOP_MARKET
        assert bracket.orders[2].type == OrderType.LIMIT
        assert bracket.orders[0].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[1].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[2].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[0].client_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[1].client_order_id == ClientOrderId("O-19700101-000000-000-001-2")
        assert bracket.orders[2].client_order_id == ClientOrderId("O-19700101-000000-000-001-3")
        assert bracket.orders[0].side == OrderSide.BUY
        assert bracket.orders[1].side == OrderSide.SELL
        assert bracket.orders[2].side == OrderSide.SELL
        assert bracket.orders[0].quantity == Quantity.from_int(100000)
        assert bracket.orders[1].quantity == Quantity.from_int(100000)
        assert bracket.orders[2].quantity == Quantity.from_int(100000)
        assert bracket.orders[1].price == Price.from_str("0.99990")
        assert bracket.orders[2].price == Price.from_str("1.00010")
        assert bracket.orders[1].time_in_force == TimeInForce.GTC
        assert bracket.orders[2].time_in_force == TimeInForce.GTC
        assert bracket.orders[1].expire_time is None
        assert bracket.orders[2].expire_time is None
        assert bracket.orders[0].contingency == ContingencyType.OTO
        assert bracket.orders[1].contingency == ContingencyType.OCO
        assert bracket.orders[2].contingency == ContingencyType.OCO
        assert bracket.orders[0].contingency_ids == [
            ClientOrderId("O-19700101-000000-000-001-2"),
            ClientOrderId("O-19700101-000000-000-001-3"),
        ]
        assert bracket.orders[1].contingency_ids == [ClientOrderId("O-19700101-000000-000-001-3")]
        assert bracket.orders[2].contingency_ids == [ClientOrderId("O-19700101-000000-000-001-2")]
        assert bracket.orders[0].child_order_ids == [
            ClientOrderId("O-19700101-000000-000-001-2"),
            ClientOrderId("O-19700101-000000-000-001-3"),
        ]
        assert bracket.orders[1].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[2].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.ts_init == 0

    def test_order_list_str_and_repr(self):
        # Arrange, Act
        bracket = self.order_factory.bracket_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("0.99990"),
            Price.from_str("1.00010"),
        )

        # Assert
        assert str(bracket) == (
            "OrderList(id=1, instrument_id=AUD/USD.SIM, orders=[MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, tags=ENTRY), StopMarketOrder(SELL 100_000 AUD/USD.SIM STOP_MARKET @ 0.99990 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-2, venue_order_id=None, tags=STOP_LOSS), LimitOrder(SELL 100_000 AUD/USD.SIM LIMIT @ 1.00010 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-3, venue_order_id=None, tags=TAKE_PROFIT)])"  # noqa
        )
        assert repr(bracket) == (
            "OrderList(id=1, instrument_id=AUD/USD.SIM, orders=[MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, tags=ENTRY), StopMarketOrder(SELL 100_000 AUD/USD.SIM STOP_MARKET @ 0.99990 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-2, venue_order_id=None, tags=STOP_LOSS), LimitOrder(SELL 100_000 AUD/USD.SIM LIMIT @ 1.00010 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-3, venue_order_id=None, tags=TAKE_PROFIT)])"  # noqa
        )

    def test_apply_order_denied_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        denied = OrderDenied(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            order.client_order_id,
            "SOME_REASON",
            UUID4(),
            0,
        )

        # Act
        order.apply(denied)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert order.event_count == 2
        assert order.last_event == denied
        assert not order.is_active
        assert order.is_completed

    def test_apply_order_submitted_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submitted = TestStubs.event_order_submitted(order)

        # Act
        order.apply(submitted)

        # Assert
        assert order.status == OrderStatus.SUBMITTED
        assert order.event_count == 2
        assert order.last_event == submitted
        assert order.is_active
        assert order.is_inflight
        assert not order.is_working
        assert not order.is_completed
        assert not order.is_pending_update
        assert not order.is_pending_cancel

    def test_apply_order_accepted_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))

        # Act
        order.apply(TestStubs.event_order_accepted(order))

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert order.is_active
        assert not order.is_inflight
        assert order.is_working
        assert not order.is_completed
        assert (
            str(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=ACCEPTED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=1, tags=None)"  # noqa
        )
        assert (
            repr(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=ACCEPTED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=1, tags=None)"  # noqa
        )

    def test_apply_order_rejected_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))

        # Act
        order.apply(TestStubs.event_order_rejected(order))

        # Assert
        assert order.status == OrderStatus.REJECTED
        assert not order.is_active
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed

    def test_apply_order_expired_event(self):
        # Arrange
        order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("0.99990"),
            TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        # Act
        order.apply(TestStubs.event_order_expired(order))

        # Assert
        assert order.status == OrderStatus.EXPIRED
        assert not order.is_active
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed

    def test_apply_order_triggered_event(self):
        # Arrange
        order = self.order_factory.stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
            Price.from_str("0.99990"),
            TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        # Act
        order.apply(TestStubs.event_order_triggered(order))

        # Assert
        assert order.status == OrderStatus.TRIGGERED
        assert order.is_active
        assert not order.is_inflight
        assert order.is_working
        assert not order.is_completed

    def test_order_status_pending_cancel(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        # Act
        order.apply(TestStubs.event_order_pending_cancel(order))

        # Assert
        assert order.status == OrderStatus.PENDING_CANCEL
        assert order.is_active
        assert order.is_inflight
        assert order.is_working
        assert not order.is_completed
        assert not order.is_pending_update
        assert order.is_pending_cancel
        assert order.event_count == 4

    def test_apply_order_canceled_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))
        order.apply(TestStubs.event_order_pending_cancel(order))

        # Act
        order.apply(TestStubs.event_order_canceled(order))

        # Assert
        assert order.status == OrderStatus.CANCELED
        assert not order.is_active
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed
        assert not order.is_pending_update
        assert not order.is_pending_cancel
        assert order.event_count == 5

    def test_order_status_pending_replace(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        # Act
        order.apply(TestStubs.event_order_pending_update(order))

        # Assert
        assert order.status == OrderStatus.PENDING_UPDATE
        assert order.is_active
        assert order.is_inflight
        assert order.is_working
        assert not order.is_completed
        assert order.is_pending_update
        assert not order.is_pending_cancel
        assert order.event_count == 4

    def test_apply_order_updated_event_to_stop_order(self):
        # Arrange
        order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))
        order.apply(TestStubs.event_order_pending_update(order))

        updated = OrderUpdated(
            order.trader_id,
            order.strategy_id,
            order.account_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            Quantity.from_int(120000),
            Price.from_str("1.00001"),
            None,
            UUID4(),
            0,
            0,
        )

        # Act
        order.apply(updated)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert order.venue_order_id == VenueOrderId("1")
        assert order.quantity == Quantity.from_int(120000)
        assert order.price == Price.from_str("1.00001")
        assert order.is_active
        assert not order.is_inflight
        assert order.is_working
        assert not order.is_completed
        assert order.event_count == 5

    def test_apply_order_updated_venue_id_change(self):
        # Arrange
        order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))
        order.apply(TestStubs.event_order_pending_update(order))

        updated = OrderUpdated(
            order.trader_id,
            order.strategy_id,
            order.account_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("2"),
            Quantity.from_int(120000),
            Price.from_str("1.00001"),
            None,
            UUID4(),
            0,
            0,
        )

        # Act
        order.apply(updated)

        # Assert
        assert order.venue_order_id == VenueOrderId("2")
        assert order.venue_order_ids == [VenueOrderId("1")]

    def test_apply_order_filled_event_to_order_without_accepted(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        filled = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        # Act
        order.apply(filled)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.filled_qty == Quantity.from_int(100000)
        assert order.leaves_qty == Quantity.zero()
        assert order.avg_px == Decimal("1.00001")
        assert len(order.execution_ids) == 1
        assert not order.is_active
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed
        assert order.ts_last == 0

    def test_apply_order_filled_event_to_market_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        filled = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        # Act
        order.apply(filled)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.filled_qty == Quantity.from_int(100000)
        assert order.avg_px == Decimal("1.00001")
        assert len(order.execution_ids) == 1
        assert not order.is_active
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed
        assert order.ts_last == 0

    def test_apply_partial_fill_events_to_market_order_results_in_partially_filled(
        self,
    ):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        fill1 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            execution_id=ExecutionId("1"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
            last_qty=Quantity.from_int(20000),
        )

        fill2 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            execution_id=ExecutionId("2"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00002"),
            last_qty=Quantity.from_int(40000),
        )

        # Act
        order.apply(fill1)
        order.apply(fill2)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_int(60000)
        assert order.leaves_qty == Quantity.from_int(40000)
        assert order.avg_px == Decimal("1.000014")
        assert len(order.execution_ids) == 2
        assert order.is_active
        assert not order.is_inflight
        assert order.is_working
        assert not order.is_completed
        assert order.ts_last == 0

    def test_apply_filled_events_to_market_order_results_in_filled(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        fill1 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            execution_id=ExecutionId("1"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
            last_qty=Quantity.from_int(20000),
        )

        fill2 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            execution_id=ExecutionId("2"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00002"),
            last_qty=Quantity.from_int(40000),
        )

        fill3 = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            execution_id=ExecutionId("3"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00003"),
            last_qty=Quantity.from_int(40000),
        )

        # Act
        order.apply(fill1)
        order.apply(fill2)
        order.apply(fill3)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.filled_qty == Quantity.from_int(100000)
        assert order.avg_px == Decimal("1.000018571428571428571428571")
        assert len(order.execution_ids) == 3
        assert not order.is_active
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed
        assert order.ts_last == 0

    def test_apply_order_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        filled = OrderFilled(
            order.trader_id,
            order.strategy_id,
            order.account_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            ExecutionId("E-1"),
            PositionId("P-1"),
            order.side,
            order.type,
            order.quantity,
            Price.from_str("1.00001"),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.MAKER,
            UUID4(),
            0,
            0,
        )

        # Act
        order.apply(filled)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.filled_qty == Quantity.from_int(100000)
        assert order.price == Price.from_str("1.00000")
        assert order.avg_px == Decimal("1.00001")
        assert order.slippage == Decimal("0.00001")
        assert not order.is_active
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed
        assert order.ts_last == 0

    def test_apply_order_partially_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        order.apply(TestStubs.event_order_submitted(order))
        order.apply(TestStubs.event_order_accepted(order))

        partially = OrderFilled(
            order.trader_id,
            order.strategy_id,
            order.account_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            ExecutionId("E-1"),
            PositionId("P-1"),
            order.side,
            order.type,
            Quantity.from_int(50000),
            Price.from_str("0.999999"),
            AUDUSD_SIM.quote_currency,
            Money(0, USD),
            LiquiditySide.MAKER,
            UUID4(),
            1_000_000_000,
            1_000_000_000,
        )

        # Act
        order.apply(partially)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_int(50000)
        assert order.price == Price.from_str("1.00000")
        assert order.avg_px == Decimal("0.999999")
        assert order.slippage == Decimal("-0.000001")
        assert order.is_active
        assert not order.is_inflight
        assert order.is_working
        assert not order.is_completed
        assert order.ts_last == 1_000_000_000, order.ts_last
