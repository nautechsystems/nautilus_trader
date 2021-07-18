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
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events.order import OrderDenied
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderInitialized
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ClientOrderLinkId
from nautilus_trader.model.identifiers import ExecutionId
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
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


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
                uuid4(),
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
                uuid4(),
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
                init_id=uuid4(),
                timestamp_ns=0,
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
                init_id=uuid4(),
                timestamp_ns=0,
                time_in_force=TimeInForce.GTD,
                expire_time=None,
            )

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
        assert order.state == OrderState.INITIALIZED
        assert order.event_count == 1
        assert isinstance(order.last_event, OrderInitialized)
        assert not order.is_inflight
        assert not order.is_working
        assert not order.is_completed
        assert order.is_buy
        assert not order.is_sell
        assert not order.is_passive
        assert order.is_aggressive
        assert order.ts_filled_ns == 0
        assert order.last_event.timestamp_ns == 0
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
        assert order.state == OrderState.INITIALIZED
        assert order.event_count == 1
        assert isinstance(order.last_event, OrderInitialized)
        assert len(order.events) == 1
        assert not order.is_inflight
        assert not order.is_working
        assert not order.is_completed
        assert not order.is_buy
        assert order.is_sell
        assert order.ts_filled_ns == 0
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
        )

        # Act, Assert
        assert isinstance(hash(order), int)
        assert (
            str(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, state=INITIALIZED, client_order_id=O-19700101-000000-000-001-1)"  # noqa
        )
        assert (
            repr(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, state=INITIALIZED, client_order_id=O-19700101-000000-000-001-1)"  # noqa
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
            "venue_order_id": "NULL",
            "position_id": "NULL",
            "account_id": None,
            "execution_id": None,
            "type": "MARKET",
            "side": "BUY",
            "quantity": "100000",
            "timestamp_ns": 0,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "ts_filled_ns": 0,
            "avg_px": None,
            "slippage": "0",
            "state": "INITIALIZED",
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
        assert order.state == OrderState.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_completed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, state=INITIALIZED, client_order_id=O-19700101-000000-000-001-1)"  # noqa
        )
        assert (
            repr(order)
            == "LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, state=INITIALIZED, client_order_id=O-19700101-000000-000-001-1)"  # noqa
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
            "venue_order_id": "NULL",
            "position_id": "NULL",
            "account_id": None,
            "execution_id": None,
            "type": "LIMIT",
            "side": "BUY",
            "quantity": "100000",
            "price": "1.00000",
            "liquidity_side": "NONE",
            "expire_time_ns": 0,
            "timestamp_ns": 0,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "ts_filled_ns": 0,
            "avg_px": None,
            "slippage": "0",
            "state": "INITIALIZED",
            "is_post_only": False,
            "is_reduce_only": False,
            "is_hidden": False,
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
        assert order.state == OrderState.INITIALIZED
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
        assert order.state == OrderState.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_completed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "StopMarketOrder(BUY 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000 GTC, state=INITIALIZED, client_order_id=O-19700101-000000-000-001-1)"  # noqa
        )
        assert (
            repr(order)
            == "StopMarketOrder(BUY 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000 GTC, state=INITIALIZED, client_order_id=O-19700101-000000-000-001-1)"  # noqa
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
            "venue_order_id": "NULL",
            "position_id": "NULL",
            "account_id": None,
            "execution_id": None,
            "type": "STOP_MARKET",
            "side": "BUY",
            "quantity": "100000",
            "price": "1.00000",
            "liquidity_side": "NONE",
            "expire_time_ns": 0,
            "timestamp_ns": 0,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "ts_filled_ns": 0,
            "avg_px": None,
            "slippage": "0",
            "state": "INITIALIZED",
            "is_reduce_only": False,
        }

    def test_initialize_stop_limit_order(self):
        # Arrange, Act
        order = self.order_factory.stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
            Price.from_str("1.10010"),
        )

        # Assert
        assert order.type == OrderType.STOP_LIMIT
        assert order.state == OrderState.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_completed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "StopLimitOrder(BUY 100_000 AUD/USD.SIM STOP_LIMIT @ 1.00000 GTC, trigger=1.10010, state=INITIALIZED, client_order_id=O-19700101-000000-000-001-1)"  # noqa
        )
        assert (
            repr(order)
            == "StopLimitOrder(BUY 100_000 AUD/USD.SIM STOP_LIMIT @ 1.00000 GTC, trigger=1.10010, state=INITIALIZED, client_order_id=O-19700101-000000-000-001-1)"  # noqa
        )

    def test_stop_limit_order_to_dict(self):
        # Arrange
        order = self.order_factory.stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
            Price.from_str("1.10010"),
        )

        # Act
        result = order.to_dict()

        # Assert
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": "NULL",
            "position_id": "NULL",
            "account_id": None,
            "execution_id": None,
            "type": "STOP_LIMIT",
            "side": "BUY",
            "quantity": "100000",
            "trigger": "1.10010",
            "price": "1.00000",
            "liquidity_side": "NONE",
            "expire_time_ns": 0,
            "timestamp_ns": 0,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "ts_filled_ns": 0,
            "avg_px": None,
            "slippage": "0",
            "state": "INITIALIZED",
            "is_post_only": False,
            "is_reduce_only": False,
            "is_hidden": False,
        }

    def test_bracket_order_equality(self):
        # Arrange
        entry1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        entry2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        bracket_order1 = self.order_factory.bracket(
            entry1, Price.from_str("1.00000"), Price.from_str("1.00010")
        )
        bracket_order2 = self.order_factory.bracket(
            entry2, Price.from_str("1.00000"), Price.from_str("1.00010")
        )

        # Act, Assert
        assert bracket_order1 == bracket_order1
        assert bracket_order1 != bracket_order2

    def test_initialize_bracket_order(self):
        # Arrange
        entry_order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("0.99995"),
        )

        # Act
        bracket_order = self.order_factory.bracket(
            entry_order,
            Price.from_str("0.99990"),
            Price.from_str("1.00010"),
            TimeInForce.GTC,
            TimeInForce.GTC,
        )

        # Assert
        assert bracket_order.stop_loss.instrument_id == AUDUSD_SIM.id
        assert bracket_order.take_profit is not None
        assert bracket_order.take_profit.instrument_id == AUDUSD_SIM.id
        assert bracket_order.entry.client_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket_order.stop_loss.client_order_id == ClientOrderId(
            "O-19700101-000000-000-001-2"
        )
        assert bracket_order.take_profit.client_order_id == ClientOrderId(
            "O-19700101-000000-000-001-3"
        )
        assert bracket_order.stop_loss.side == OrderSide.SELL
        assert bracket_order.take_profit.side == OrderSide.SELL
        assert bracket_order.stop_loss.quantity == Quantity.from_int(100000)
        assert bracket_order.take_profit.quantity == Quantity.from_int(100000)
        assert bracket_order.stop_loss.price == Price.from_str("0.99990")
        assert bracket_order.take_profit.price == Price.from_str("1.00010")
        assert bracket_order.stop_loss.time_in_force == TimeInForce.GTC
        assert bracket_order.take_profit.time_in_force == TimeInForce.GTC
        assert bracket_order.entry.expire_time is None
        assert bracket_order.stop_loss.expire_time is None
        assert bracket_order.take_profit.expire_time is None
        assert bracket_order.id == ClientOrderLinkId("BO-19700101-000000-000-001-1")
        assert bracket_order.timestamp_ns == 0

    def test_bracket_order_str_and_repr(self):
        # Arrange, Act
        entry_order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        bracket_order = self.order_factory.bracket(
            entry_order,
            Price.from_str("0.99990"),
            Price.from_str("1.00010"),
        )

        # Assert
        assert str(bracket_order) == (
            "BracketOrder(id=BO-19700101-000000-000-001-1, "
            "EntryMarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, "
            "state=INITIALIZED, client_order_id=O-19700101-000000-000-001-1), "
            "SL=0.99990, TP=1.00010)"
        )
        assert repr(bracket_order) == (
            "BracketOrder(id=BO-19700101-000000-000-001-1, "
            "EntryMarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, "
            "state=INITIALIZED, client_order_id=O-19700101-000000-000-001-1), "
            "SL=0.99990, TP=1.00010)"
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
            uuid4(),
            0,
        )

        # Act
        order.apply(denied)

        # Assert
        assert order.state == OrderState.DENIED
        assert order.event_count == 2
        assert order.last_event == denied
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
        assert order.state == OrderState.SUBMITTED
        assert order.event_count == 2
        assert order.last_event == submitted
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
        assert order.state == OrderState.ACCEPTED
        assert not order.is_inflight
        assert order.is_working
        assert not order.is_completed
        assert (
            str(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, state=ACCEPTED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=1)"  # noqa
        )
        assert (
            repr(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, state=ACCEPTED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=1)"  # noqa
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
        assert order.state == OrderState.REJECTED
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
        assert order.state == OrderState.EXPIRED
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
        assert order.state == OrderState.TRIGGERED
        assert not order.is_inflight
        assert order.is_working
        assert not order.is_completed

    def test_order_state_pending_cancel(self):
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
        assert order.state == OrderState.PENDING_CANCEL
        assert not order.is_inflight
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
        assert order.state == OrderState.CANCELED
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed
        assert not order.is_pending_update
        assert not order.is_pending_cancel
        assert order.event_count == 5

    def test_order_state_pending_replace(self):
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
        assert order.state == OrderState.PENDING_UPDATE
        assert not order.is_inflight
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
            order.instrument_id,
            order.account_id,
            order.client_order_id,
            VenueOrderId("1"),
            Quantity.from_int(120000),
            Price.from_str("1.00001"),
            None,
            0,
            uuid4(),
            0,
        )

        # Act
        order.apply(updated)

        # Assert
        assert order.state == OrderState.ACCEPTED
        assert order.venue_order_id == VenueOrderId("1")
        assert order.quantity == Quantity.from_int(120000)
        assert order.price == Price.from_str("1.00001")
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
            order.instrument_id,
            order.account_id,
            order.client_order_id,
            VenueOrderId("2"),
            Quantity.from_int(120000),
            Price.from_str("1.00001"),
            None,
            0,
            uuid4(),
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
        assert order.state == OrderState.FILLED
        assert order.filled_qty == Quantity.from_int(100000)
        assert order.avg_px == Decimal("1.00001")
        assert len(order.execution_ids) == 1
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed
        assert order.ts_filled_ns == 0

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
        assert order.state == OrderState.FILLED
        assert order.filled_qty == Quantity.from_int(100000)
        assert order.avg_px == Decimal("1.00001")
        assert len(order.execution_ids) == 1
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed
        assert order.ts_filled_ns == 0

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
        assert order.state == OrderState.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_int(60000)
        assert order.avg_px == Decimal("1.000014")
        assert len(order.execution_ids) == 2
        assert not order.is_inflight
        assert order.is_working
        assert not order.is_completed
        assert order.ts_filled_ns == 0

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
        assert order.state == OrderState.FILLED
        assert order.filled_qty == Quantity.from_int(100000)
        assert order.avg_px == Decimal("1.000018571428571428571428571")
        assert len(order.execution_ids) == 3
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed
        assert order.ts_filled_ns == 0

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
            order.instrument_id,
            order.account_id,
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
            0,
            uuid4(),
            0,
        )

        # Act
        order.apply(filled)

        # Assert
        assert order.state == OrderState.FILLED
        assert order.filled_qty == Quantity.from_int(100000)
        assert order.price == Price.from_str("1.00000")
        assert order.avg_px == Decimal("1.00001")
        assert order.slippage == Decimal("0.00001")
        assert not order.is_inflight
        assert not order.is_working
        assert order.is_completed
        assert order.ts_filled_ns == 0

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
            order.instrument_id,
            order.account_id,
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
            1_000_000_000,
            uuid4(),
            1_000_000_000,
        )

        # Act
        order.apply(partially)

        # Assert
        assert order.state == OrderState.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_int(50000)
        assert order.price == Price.from_str("1.00000")
        assert order.avg_px == Decimal("0.999999")
        assert order.slippage == Decimal("-0.000001")
        assert not order.is_inflight
        assert order.is_working
        assert not order.is_completed
        assert order.ts_filled_ns == 1_000_000_000, order.ts_filled_ns
