# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import uuid
from datetime import timedelta
from decimal import Decimal

import pytest

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import MarketToLimitOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import StopLimitOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import UNIX_EPOCH
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestOrders:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestIdStubs.trader_id()
        self.strategy_id = TestIdStubs.strategy_id()
        self.account_id = TestIdStubs.account_id()

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
            Order.closing_side(0)  # <-- invalid value

        with pytest.raises(ValueError):
            Order.closing_side(PositionSide.FLAT)

    @pytest.mark.parametrize(
        ("side", "expected"),
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
        ("side", "expected"),
        [
            [PositionSide.LONG, OrderSide.SELL],
            [PositionSide.SHORT, OrderSide.BUY],
        ],
    )
    def test_closing_side_returns_expected_sides(self, side, expected):
        # Arrange, Act
        result = Order.closing_side(side)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("order_side", "position_side", "position_qty", "expected"),
        [
            [OrderSide.BUY, PositionSide.FLAT, Quantity.from_int(0), False],
            [OrderSide.BUY, PositionSide.SHORT, Quantity.from_str("0.5"), False],
            [OrderSide.BUY, PositionSide.SHORT, Quantity.from_int(1), True],
            [OrderSide.BUY, PositionSide.SHORT, Quantity.from_int(2), True],
            [OrderSide.BUY, PositionSide.LONG, Quantity.from_int(2), False],
            [OrderSide.SELL, PositionSide.SHORT, Quantity.from_int(2), False],
            [OrderSide.SELL, PositionSide.LONG, Quantity.from_int(2), True],
            [OrderSide.SELL, PositionSide.LONG, Quantity.from_int(1), True],
            [OrderSide.SELL, PositionSide.LONG, Quantity.from_str("0.5"), False],
            [OrderSide.SELL, PositionSide.FLAT, Quantity.from_int(0), False],
        ],
    )
    def test_would_reduce_only_with_various_values_returns_expected(
        self,
        order_side,
        position_side,
        position_qty,
        expected,
    ):
        # Arrange
        order = MarketOrder(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            ClientOrderId("O-123456"),
            order_side,
            Quantity.from_int(1),
            UUID4(),
            0,
        )

        # Act, Assert
        assert (
            order.would_reduce_only(position_side=position_side, position_qty=position_qty)
            == expected
        )

    def test_market_order_with_quantity_zero_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            MarketOrder(
                self.trader_id,
                self.strategy_id,
                AUDUSD_SIM.id,
                ClientOrderId("O-123456"),
                OrderSide.BUY,
                Quantity.zero(),  # <- invalid
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
                Quantity.from_int(100_000),
                UUID4(),
                0,
                TimeInForce.GTD,  # <-- invalid
            )

    def test_stop_market_order_with_gtd_and_expiration_none_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            StopMarketOrder(
                self.trader_id,
                self.strategy_id,
                AUDUSD_SIM.id,
                ClientOrderId("O-123456"),
                OrderSide.BUY,
                Quantity.from_int(100_000),
                trigger_price=Price.from_str("1.00000"),
                init_id=UUID4(),
                ts_init=0,
                time_in_force=TimeInForce.GTD,
                expire_time=None,
            )

    def test_stop_limit_buy_order_with_gtd_and_expiration_none_raises_type_error(self):
        # Arrange, Act, Assert
        with pytest.raises(TypeError):
            StopLimitOrder(
                self.trader_id,
                self.strategy_id,
                AUDUSD_SIM.id,
                ClientOrderId("O-123456"),
                OrderSide.BUY,
                Quantity.from_int(100_000),
                price=Price.from_str("1.00001"),
                trigger_price=Price.from_str("1.00000"),
                init_id=UUID4(),
                ts_init=0,
                time_in_force=TimeInForce.GTD,
                expire_time=None,
            )

    def test_market_to_limit_order_with_invalid_tif_raises_value_error(self):
        # Arrange, Act, Assert
        with pytest.raises(ValueError):
            MarketToLimitOrder(
                self.trader_id,
                self.strategy_id,
                AUDUSD_SIM.id,
                ClientOrderId("O-123456"),
                OrderSide.BUY,
                Quantity.from_int(100_000),
                UUID4(),
                0,
                TimeInForce.AT_THE_CLOSE,  # <-- invalid
            )

    def test_overfill_limit_buy_order_raises_value_error(self):
        # Arrange, Act, Assert
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        over_fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            last_qty=Quantity.from_int(110_000),  # <-- overfill
        )

        # Assert
        with pytest.raises(ValueError):
            order.apply(over_fill)

    def test_reset_order_factory(self):
        # Arrange
        self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        # Act
        self.order_factory.reset()

        order2 = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        assert order2.client_order_id.value == "O-19700101-000000-000-001-1"

    def test_initialize_buy_market_order(self):
        # Arrange, Act
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Assert
        assert order.symbol == AUDUSD_SIM.id.symbol
        assert order.venue == AUDUSD_SIM.id.venue
        assert order.order_type == OrderType.MARKET
        assert order.status == OrderStatus.INITIALIZED
        assert order.side_string() == "BUY"
        assert order.type_string() == "MARKET"
        assert order.signed_decimal_qty() == Decimal(100_000)
        assert order.event_count == 1
        assert isinstance(order.last_event, OrderInitialized)
        assert not order.has_price
        assert not order.has_trigger_price
        assert not order.is_open
        assert not order.is_closed
        assert not order.is_inflight
        assert not order.is_emulated
        assert order.is_active_local
        assert order.is_buy
        assert order.is_aggressive
        assert not order.is_sell
        assert not order.is_contingency
        assert not order.is_primary
        assert not order.is_spawned
        assert not order.is_passive
        assert not order.is_parent_order
        assert not order.is_child_order
        assert order.ts_last == 0
        assert order.ts_accepted == 0
        assert order.ts_submitted == 0
        assert order.ts_closed == 0
        assert order.last_event.ts_init == 0
        assert isinstance(order.init_event, OrderInitialized)

    def test_initialize_sell_market_order(self):
        # Arrange, Act
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        # Assert
        assert order.order_type == OrderType.MARKET
        assert order.status == OrderStatus.INITIALIZED
        assert order.status_string() == "INITIALIZED"
        assert order.side_string() == "SELL"
        assert order.type_string() == "MARKET"
        assert order.signed_decimal_qty() == -Decimal(100_000)
        assert order.event_count == 1
        assert isinstance(order.last_event, OrderInitialized)
        assert len(order.events) == 1
        assert not order.has_price
        assert not order.has_trigger_price
        assert not order.is_open
        assert not order.is_closed
        assert not order.is_inflight
        assert not order.is_buy
        assert not order.is_emulated
        assert order.is_active_local
        assert order.is_sell
        assert order.ts_last == 0
        assert isinstance(order.init_event, OrderInitialized)

    def test_order_equality(self):
        # Arrange, Act
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        # Assert
        assert order == order

    def test_order_hash_str_and_repr(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            tags=["ENTRY"],
        )

        # Act, Assert
        assert isinstance(hash(order), int)
        assert (
            str(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=['ENTRY'])"
        )
        assert (
            repr(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=['ENTRY'])"
        )

    def test_market_order_to_dict(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            tags=["tag-01", "tag-02", "tag-03"],
        )

        # Act
        result = order.to_dict()
        # remove init_id as it non-deterministic with order-factory
        del result["init_id"]

        # Assert
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "last_trade_id": None,
            "type": "MARKET",
            "side": "BUY",
            "quantity": "100000",
            "time_in_force": "GTC",
            "is_reduce_only": False,
            "is_quote_quantity": False,
            "filled_qty": "0",
            "liquidity_side": "NO_LIQUIDITY_SIDE",
            "avg_px": None,
            "slippage": None,
            "commissions": None,
            "emulation_trigger": "NO_TRIGGER",
            "status": "INITIALIZED",
            "contingency_type": "NO_CONTINGENCY",
            "order_list_id": None,
            "linked_order_ids": None,
            "parent_order_id": None,
            "exec_algorithm_id": None,
            "exec_algorithm_params": None,
            "exec_spawn_id": None,
            "tags": ["tag-01", "tag-02", "tag-03"],
            "ts_init": 0,
            "ts_last": 0,
        }

    def test_initialize_limit_order(self):
        # Arrange, Act
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            exec_algorithm_id=ExecAlgorithmId("TWAP"),
        )

        # Assert
        assert order.order_type == OrderType.LIMIT
        assert order.expire_time is None
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.has_price
        assert not order.has_trigger_price
        assert order.is_passive
        assert not order.is_open
        assert not order.is_aggressive
        assert not order.is_closed
        assert not order.is_emulated
        assert order.is_active_local
        assert order.is_primary
        assert not order.is_spawned
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, exec_algorithm_id=TWAP, exec_spawn_id=O-19700101-000000-000-001-1, tags=None)"
        )
        assert (
            repr(order)
            == "LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, exec_algorithm_id=TWAP, exec_spawn_id=O-19700101-000000-000-001-1, tags=None)"
        )

    def test_limit_order_to_dict(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            display_qty=Quantity.from_int(20_000),
            exec_algorithm_id=ExecAlgorithmId("VWAP"),
            exec_algorithm_params={"period": 60},
        )

        # Act
        result = order.to_dict()
        # remove init_id as it non-deterministic with order-factory
        del result["init_id"]

        # Assert
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "last_trade_id": None,
            "type": "LIMIT",
            "side": "BUY",
            "quantity": "100000",
            "price": "1.00000",
            "expire_time_ns": None,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "liquidity_side": "NO_LIQUIDITY_SIDE",
            "avg_px": None,
            "slippage": None,
            "commissions": None,
            "status": "INITIALIZED",
            "is_post_only": False,
            "is_reduce_only": False,
            "is_quote_quantity": False,
            "display_qty": "20000",
            "emulation_trigger": "NO_TRIGGER",
            "trigger_instrument_id": None,
            "contingency_type": "NO_CONTINGENCY",
            "order_list_id": None,
            "linked_order_ids": None,
            "parent_order_id": None,
            "exec_algorithm_id": "VWAP",
            "exec_algorithm_params": {"period": 60},
            "exec_spawn_id": "O-19700101-000000-000-001-1",
            "tags": None,
            "ts_init": 0,
            "ts_last": 0,
        }

    def test_initialize_limit_order_with_expiration(self):
        # Arrange, Act
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=1),
        )

        # Assert
        assert order.instrument_id == AUDUSD_SIM.id
        assert order.order_type == OrderType.LIMIT
        assert order.price == Price.from_str("1.00000")
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTD
        assert order.expire_time == UNIX_EPOCH + timedelta(minutes=1)
        assert not order.is_closed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTD 1970-01-01T00:01:00.000Z, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "LimitOrder(BUY 100_000 AUD/USD.SIM LIMIT @ 1.00000 GTD 1970-01-01T00:01:00.000Z, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_initialize_stop_market_order(self):
        # Arrange, Act
        order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            TriggerType.BID_ASK,
        )

        # Assert
        assert order.order_type == OrderType.STOP_MARKET
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert not order.has_price
        assert order.has_trigger_price
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_open
        assert not order.is_closed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "StopMarketOrder(BUY 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000[BID_ASK] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "StopMarketOrder(BUY 100_000 AUD/USD.SIM STOP_MARKET @ 1.00000[BID_ASK] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_stop_market_order_to_dict(self):
        # Arrange
        order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            quote_quantity=True,
            emulation_trigger=TriggerType.BID_ASK,
            trigger_instrument_id=TestIdStubs.usdjpy_id(),
        )

        # Act
        result = order.to_dict()

        # Assert
        assert "init_id" in result and uuid.UUID(result["init_id"], version=4)
        del result["init_id"]  # Random UUID4
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "last_trade_id": None,
            "type": "STOP_MARKET",
            "side": "BUY",
            "quantity": "100000",
            "trigger_price": "1.00000",
            "trigger_type": "DEFAULT",
            "expire_time_ns": None,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "liquidity_side": "NO_LIQUIDITY_SIDE",
            "avg_px": None,
            "slippage": None,
            "commissions": None,
            "status": "INITIALIZED",
            "is_reduce_only": False,
            "is_quote_quantity": True,
            "emulation_trigger": "BID_ASK",
            "trigger_instrument_id": "USD/JPY.SIM",
            "contingency_type": "NO_CONTINGENCY",
            "order_list_id": None,
            "linked_order_ids": None,
            "parent_order_id": None,
            "exec_algorithm_id": None,
            "exec_algorithm_params": None,
            "exec_spawn_id": None,
            "tags": None,
            "ts_init": 0,
            "ts_last": 0,
        }

    def test_initialize_stop_limit_order(self):
        # Arrange, Act
        order = self.order_factory.stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            Price.from_str("1.10010"),
            tags=["ENTRY"],
        )

        # Assert
        assert order.order_type == OrderType.STOP_LIMIT
        assert order.expire_time is None
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.has_price
        assert order.has_trigger_price
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_closed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "StopLimitOrder(BUY 100_000 AUD/USD.SIM STOP_LIMIT @ 1.10010-STOP[DEFAULT] 1.00000-LIMIT GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=['ENTRY'])"
        )
        assert (
            repr(order)
            == "StopLimitOrder(BUY 100_000 AUD/USD.SIM STOP_LIMIT @ 1.10010-STOP[DEFAULT] 1.00000-LIMIT GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=['ENTRY'])"
        )

    def test_stop_limit_order_to_dict(self):
        # Arrange
        order = self.order_factory.stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            Price.from_str("1.10010"),
            trigger_type=TriggerType.MARK_PRICE,
            tags=["STOP_LOSS"],
        )

        # Act
        result = order.to_dict()
        # remove init_id as it non-deterministic with order-factory
        del result["init_id"]

        # Assert
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "exec_algorithm_id": None,
            "exec_algorithm_params": None,
            "exec_spawn_id": None,
            "last_trade_id": None,
            "type": "STOP_LIMIT",
            "side": "BUY",
            "quantity": "100000",
            "price": "1.00000",
            "trigger_price": "1.10010",
            "trigger_type": "MARK_PRICE",
            "expire_time_ns": None,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "liquidity_side": "NO_LIQUIDITY_SIDE",
            "avg_px": None,
            "slippage": None,
            "commissions": None,
            "status": "INITIALIZED",
            "is_post_only": False,
            "is_reduce_only": False,
            "is_quote_quantity": False,
            "display_qty": None,
            "emulation_trigger": "NO_TRIGGER",
            "trigger_instrument_id": None,
            "contingency_type": "NO_CONTINGENCY",
            "order_list_id": None,
            "linked_order_ids": None,
            "parent_order_id": None,
            "tags": ["STOP_LOSS"],
            "ts_init": 0,
            "ts_last": 0,
        }

    def test_initialize_market_to_limit_order(self):
        # Arrange, Act
        order = self.order_factory.market_to_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(hours=1),
        )

        # Assert
        assert order.order_type == OrderType.MARKET_TO_LIMIT
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTD
        assert order.expire_time == UNIX_EPOCH + timedelta(hours=1)
        assert order.expire_time_ns == 3600000000000
        assert not order.has_price
        assert not order.has_trigger_price
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_open
        assert not order.is_closed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "MarketToLimitOrder(BUY 100_000 AUD/USD.SIM MARKET_TO_LIMIT @ None GTD 1970-01-01T01:00:00.000Z, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "MarketToLimitOrder(BUY 100_000 AUD/USD.SIM MARKET_TO_LIMIT @ None GTD 1970-01-01T01:00:00.000Z, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_market_to_limit_order_to_dict(self):
        # Arrange
        order = self.order_factory.market_to_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(hours=1),
        )

        # Act
        result = order.to_dict()

        # Assert
        assert "init_id" in result and uuid.UUID(result["init_id"], version=4)
        del result["init_id"]  # Random UUID4
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "exec_algorithm_id": None,
            "exec_algorithm_params": None,
            "exec_spawn_id": None,
            "last_trade_id": None,
            "type": "MARKET_TO_LIMIT",
            "side": "BUY",
            "quantity": "100000",
            "price": "None",
            "time_in_force": "GTD",
            "expire_time_ns": 3600000000000,
            "is_reduce_only": False,
            "is_quote_quantity": False,
            "filled_qty": "0",
            "liquidity_side": "NO_LIQUIDITY_SIDE",
            "avg_px": None,
            "slippage": None,
            "commissions": None,
            "status": "INITIALIZED",
            "display_qty": None,
            "contingency_type": "NO_CONTINGENCY",
            "order_list_id": None,
            "linked_order_ids": None,
            "parent_order_id": None,
            "tags": None,
            "ts_init": 0,
            "ts_last": 0,
        }

    def test_initialize_market_if_touched_order(self):
        # Arrange, Act
        order = self.order_factory.market_if_touched(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            TriggerType.BID_ASK,
        )

        # Assert
        assert order.order_type == OrderType.MARKET_IF_TOUCHED
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.expire_time is None
        assert not order.has_price
        assert order.has_trigger_price
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_open
        assert not order.is_closed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "MarketIfTouchedOrder(BUY 100_000 AUD/USD.SIM MARKET_IF_TOUCHED @ 1.00000[BID_ASK] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "MarketIfTouchedOrder(BUY 100_000 AUD/USD.SIM MARKET_IF_TOUCHED @ 1.00000[BID_ASK] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_market_if_touched_order_to_dict(self):
        # Arrange
        order = self.order_factory.market_if_touched(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        # Act
        result = order.to_dict()

        # Assert
        assert "init_id" in result and uuid.UUID(result["init_id"], version=4)
        del result["init_id"]  # Random UUID4
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "exec_algorithm_id": None,
            "exec_algorithm_params": None,
            "exec_spawn_id": None,
            "last_trade_id": None,
            "type": "MARKET_IF_TOUCHED",
            "side": "BUY",
            "quantity": "100000",
            "trigger_price": "1.00000",
            "trigger_type": "DEFAULT",
            "expire_time_ns": None,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "liquidity_side": "NO_LIQUIDITY_SIDE",
            "avg_px": None,
            "slippage": None,
            "commissions": None,
            "status": "INITIALIZED",
            "is_reduce_only": False,
            "is_quote_quantity": False,
            "emulation_trigger": "NO_TRIGGER",
            "trigger_instrument_id": None,
            "contingency_type": "NO_CONTINGENCY",
            "order_list_id": None,
            "linked_order_ids": None,
            "parent_order_id": None,
            "tags": None,
            "ts_init": 0,
            "ts_last": 0,
        }

    def test_initialize_limit_if_touched_order(self):
        # Arrange, Act
        order = self.order_factory.limit_if_touched(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            Price.from_str("1.10010"),
            emulation_trigger=TriggerType.LAST_PRICE,
            tags=["ENTRY"],
        )

        # Assert
        assert order.order_type == OrderType.LIMIT_IF_TOUCHED
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.expire_time is None
        assert order.has_price
        assert order.has_trigger_price
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_open
        assert not order.is_closed
        assert not order.is_inflight
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "LimitIfTouchedOrder(BUY 100_000 AUD/USD.SIM LIMIT_IF_TOUCHED @ 1.10010-STOP[DEFAULT] 1.00000-LIMIT GTC EMULATED[LAST_PRICE], status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=['ENTRY'])"
        )
        assert (
            repr(order)
            == "LimitIfTouchedOrder(BUY 100_000 AUD/USD.SIM LIMIT_IF_TOUCHED @ 1.10010-STOP[DEFAULT] 1.00000-LIMIT GTC EMULATED[LAST_PRICE], status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=['ENTRY'])"
        )

    def test_limit_if_touched_order_to_dict(self):
        # Arrange
        order = self.order_factory.limit_if_touched(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            Price.from_str("1.10010"),
            trigger_type=TriggerType.MARK_PRICE,
            emulation_trigger=TriggerType.LAST_PRICE,
            trigger_instrument_id=TestIdStubs.usdjpy_id(),
            tags=["STOP_LOSS"],
        )

        # Act
        result = order.to_dict()

        # Assert
        assert "init_id" in result and uuid.UUID(result["init_id"], version=4)
        del result["init_id"]  # Random UUID4
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "exec_algorithm_id": None,
            "exec_algorithm_params": None,
            "exec_spawn_id": None,
            "last_trade_id": None,
            "type": "LIMIT_IF_TOUCHED",
            "side": "BUY",
            "quantity": "100000",
            "price": "1.00000",
            "trigger_price": "1.10010",
            "trigger_type": "MARK_PRICE",
            "expire_time_ns": None,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "liquidity_side": "NO_LIQUIDITY_SIDE",
            "avg_px": None,
            "slippage": None,
            "commissions": None,
            "status": "INITIALIZED",
            "is_post_only": False,
            "is_reduce_only": False,
            "is_quote_quantity": False,
            "display_qty": None,
            "emulation_trigger": "LAST_PRICE",
            "trigger_instrument_id": "USD/JPY.SIM",
            "contingency_type": "NO_CONTINGENCY",
            "order_list_id": None,
            "linked_order_ids": None,
            "parent_order_id": None,
            "tags": ["STOP_LOSS"],
            "ts_init": 0,
            "ts_last": 0,
        }

    def test_initialize_trailing_stop_market_order(self):
        # Arrange, Act
        order = self.order_factory.trailing_stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            trigger_price=Price.from_str("1.00000"),
            trailing_offset=Decimal("0.00050"),
            emulation_trigger=TriggerType.BID_ASK,
        )

        # Assert
        assert order.order_type == OrderType.TRAILING_STOP_MARKET
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.expire_time is None
        assert order.trailing_offset_type == TrailingOffsetType.PRICE
        assert not order.has_price
        assert order.has_trigger_price
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_open
        assert not order.is_closed
        assert not order.is_inflight
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "TrailingStopMarketOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_MARKET[DEFAULT] @ 1.00000-STOP 0.00050-TRAILING_OFFSET[PRICE] GTC EMULATED[BID_ASK], status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "TrailingStopMarketOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_MARKET[DEFAULT] @ 1.00000-STOP 0.00050-TRAILING_OFFSET[PRICE] GTC EMULATED[BID_ASK], status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_initialize_trailing_stop_market_order_with_no_initial_trigger(self):
        # Arrange, Act
        order = self.order_factory.trailing_stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            trailing_offset=Decimal("0.00050"),
        )

        # Assert
        assert order.order_type == OrderType.TRAILING_STOP_MARKET
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.expire_time is None
        assert order.trailing_offset_type == TrailingOffsetType.PRICE
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_open
        assert not order.is_closed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "TrailingStopMarketOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_MARKET[DEFAULT] 0.00050-TRAILING_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "TrailingStopMarketOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_MARKET[DEFAULT] 0.00050-TRAILING_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_initialize_trailing_stop_market_order_with_activation_price(self):
        # Arrange, Act
        order = self.order_factory.trailing_stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            activation_price=Price.from_str("1.00000"),
            trailing_offset=Decimal("0.00050"),
        )

        # Assert
        assert order.order_type == OrderType.TRAILING_STOP_MARKET
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.expire_time is None
        assert order.has_activation_price
        assert not order.has_trigger_price
        assert order.trailing_offset_type == TrailingOffsetType.PRICE
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_open
        assert not order.is_closed
        assert not order.is_activated
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "TrailingStopMarketOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_MARKET[DEFAULT] @ 1.00000-ACTIVATION 0.00050-TRAILING_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "TrailingStopMarketOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_MARKET[DEFAULT] @ 1.00000-ACTIVATION 0.00050-TRAILING_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_initialize_trailing_stop_market_order_with_activation_price_and_trigger(self):
        # Arrange, Act
        order = self.order_factory.trailing_stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            activation_price=Price.from_str("1.00000"),
            trigger_price=Price.from_str("1.01000"),
            trailing_offset=Decimal("0.00050"),
        )

        # Assert
        assert order.order_type == OrderType.TRAILING_STOP_MARKET
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.expire_time is None
        assert order.has_activation_price
        assert order.has_trigger_price
        assert order.trailing_offset_type == TrailingOffsetType.PRICE
        assert order.is_passive
        assert not order.is_activated
        assert not order.is_aggressive
        assert not order.is_open
        assert not order.is_closed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "TrailingStopMarketOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_MARKET[DEFAULT] @ 1.00000-ACTIVATION @ 1.01000-STOP 0.00050-TRAILING_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "TrailingStopMarketOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_MARKET[DEFAULT] @ 1.00000-ACTIVATION @ 1.01000-STOP 0.00050-TRAILING_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_trailing_stop_market_order_to_dict(self):
        # Arrange
        order = self.order_factory.trailing_stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            trigger_price=Price.from_str("1.00000"),
            trailing_offset=Decimal("0.00050"),
        )

        # Act
        result = order.to_dict()

        # Assert
        assert "init_id" in result and uuid.UUID(result["init_id"], version=4)
        del result["init_id"]  # Random UUID4
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "exec_algorithm_id": None,
            "exec_algorithm_params": None,
            "exec_spawn_id": None,
            "last_trade_id": None,
            "type": "TRAILING_STOP_MARKET",
            "side": "BUY",
            "quantity": "100000",
            "activation_price": None,
            "trigger_price": "1.00000",
            "trigger_type": "DEFAULT",
            "trailing_offset": "0.00050",
            "trailing_offset_type": "PRICE",
            "expire_time_ns": None,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "liquidity_side": "NO_LIQUIDITY_SIDE",
            "avg_px": None,
            "slippage": None,
            "commissions": None,
            "status": "INITIALIZED",
            "is_reduce_only": False,
            "is_quote_quantity": False,
            "emulation_trigger": "NO_TRIGGER",
            "trigger_instrument_id": None,
            "contingency_type": "NO_CONTINGENCY",
            "order_list_id": None,
            "linked_order_ids": None,
            "parent_order_id": None,
            "tags": None,
            "ts_init": 0,
            "ts_last": 0,
        }

    def test_trailing_stop_market_order_with_no_initial_trigger_to_dict(self):
        # Arrange
        order = self.order_factory.trailing_stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            trailing_offset=Decimal("0.00050"),
        )

        # Act
        result = order.to_dict()

        # Assert
        assert "init_id" in result and uuid.UUID(result["init_id"], version=4)
        del result["init_id"]  # Random UUID4
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "exec_algorithm_id": None,
            "exec_algorithm_params": None,
            "exec_spawn_id": None,
            "last_trade_id": None,
            "type": "TRAILING_STOP_MARKET",
            "side": "BUY",
            "quantity": "100000",
            "activation_price": None,
            "trigger_price": None,
            "trigger_type": "DEFAULT",
            "trailing_offset": "0.00050",
            "trailing_offset_type": "PRICE",
            "expire_time_ns": None,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "liquidity_side": "NO_LIQUIDITY_SIDE",
            "avg_px": None,
            "slippage": None,
            "commissions": None,
            "status": "INITIALIZED",
            "is_reduce_only": False,
            "is_quote_quantity": False,
            "emulation_trigger": "NO_TRIGGER",
            "trigger_instrument_id": None,
            "contingency_type": "NO_CONTINGENCY",
            "order_list_id": None,
            "linked_order_ids": None,
            "parent_order_id": None,
            "tags": None,
            "ts_init": 0,
            "ts_last": 0,
        }

    def test_initialize_trailing_stop_limit_order(self):
        # Arrange, Act
        order = self.order_factory.trailing_stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("1.00000"),
            trigger_price=Price.from_str("1.10010"),
            limit_offset=Decimal("5"),
            trailing_offset=Decimal("10"),
        )

        # Assert
        assert order.order_type == OrderType.TRAILING_STOP_LIMIT
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.has_price
        assert order.has_trigger_price
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_closed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "TrailingStopLimitOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_LIMIT[DEFAULT] @ 1.10010-STOP [DEFAULT] 1.00000-LIMIT 10-TRAILING_OFFSET[PRICE] 5-LIMIT_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "TrailingStopLimitOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_LIMIT[DEFAULT] @ 1.10010-STOP [DEFAULT] 1.00000-LIMIT 10-TRAILING_OFFSET[PRICE] 5-LIMIT_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_initialize_trailing_stop_limit_order_with_no_initial_prices(self):
        # Arrange, Act
        order = self.order_factory.trailing_stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            limit_offset=Decimal("5"),
            trailing_offset=Decimal("10"),
        )

        # Assert
        assert order.order_type == OrderType.TRAILING_STOP_LIMIT
        assert order.expire_time is None
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_closed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "TrailingStopLimitOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_LIMIT[DEFAULT] [DEFAULT] None-LIMIT 10-TRAILING_OFFSET[PRICE] 5-LIMIT_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "TrailingStopLimitOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_LIMIT[DEFAULT] [DEFAULT] None-LIMIT 10-TRAILING_OFFSET[PRICE] 5-LIMIT_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_initialize_trailing_stop_limit_order_with_activation_price(self):
        # Arrange, Act
        order = self.order_factory.trailing_stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            activation_price=Price.from_str("1.00000"),
            price=Price.from_str("1.00000"),
            limit_offset=Decimal("5"),
            trailing_offset=Decimal("10"),
        )

        # Assert
        assert order.order_type == OrderType.TRAILING_STOP_LIMIT
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.expire_time is None
        assert order.has_price
        assert order.has_activation_price
        assert not order.has_trigger_price
        assert order.trailing_offset_type == TrailingOffsetType.PRICE
        assert order.is_passive
        assert not order.is_aggressive
        assert not order.is_closed
        assert not order.is_activated
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "TrailingStopLimitOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_LIMIT[DEFAULT] @ 1.00000-ACTIVATION [DEFAULT] 1.00000-LIMIT 10-TRAILING_OFFSET[PRICE] 5-LIMIT_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "TrailingStopLimitOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_LIMIT[DEFAULT] @ 1.00000-ACTIVATION [DEFAULT] 1.00000-LIMIT 10-TRAILING_OFFSET[PRICE] 5-LIMIT_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_initialize_trailing_stop_market_order_with_activation_price_and_no_initial_price(self):
        # Arrange, Act
        order = self.order_factory.trailing_stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            activation_price=Price.from_str("1.00000"),
            limit_offset=Decimal("5"),
            trailing_offset=Decimal("10"),
        )

        # Assert
        assert order.order_type == OrderType.TRAILING_STOP_LIMIT
        assert order.status == OrderStatus.INITIALIZED
        assert order.time_in_force == TimeInForce.GTC
        assert order.expire_time is None
        assert not order.has_price
        assert order.has_activation_price
        assert not order.has_trigger_price
        assert order.trailing_offset_type == TrailingOffsetType.PRICE
        assert order.is_passive
        assert not order.is_activated
        assert not order.is_aggressive
        assert not order.is_closed
        assert isinstance(order.init_event, OrderInitialized)
        assert (
            str(order)
            == "TrailingStopLimitOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_LIMIT[DEFAULT] @ 1.00000-ACTIVATION [DEFAULT] None-LIMIT 10-TRAILING_OFFSET[PRICE] 5-LIMIT_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "TrailingStopLimitOrder(BUY 100_000 AUD/USD.SIM TRAILING_STOP_LIMIT[DEFAULT] @ 1.00000-ACTIVATION [DEFAULT] None-LIMIT 10-TRAILING_OFFSET[PRICE] 5-LIMIT_OFFSET[PRICE] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, tags=None)"
        )

    def test_trailing_stop_limit_order_to_dict(self):
        # Arrange
        order = self.order_factory.trailing_stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            price=Price.from_str("1.00000"),
            trigger_price=Price.from_str("1.10010"),
            limit_offset=Decimal("5"),
            trailing_offset=Decimal("10"),
            trigger_type=TriggerType.MARK_PRICE,
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
        )

        # Act
        result = order.to_dict()

        # Assert
        assert "init_id" in result and uuid.UUID(result["init_id"], version=4)
        del result["init_id"]  # Random UUID4
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "exec_algorithm_id": None,
            "exec_algorithm_params": None,
            "exec_spawn_id": None,
            "last_trade_id": None,
            "type": "TRAILING_STOP_LIMIT",
            "side": "BUY",
            "quantity": "100000",
            "price": "1.00000",
            "activation_price": None,
            "trigger_price": "1.10010",
            "trigger_type": "MARK_PRICE",
            "limit_offset": "5",
            "trailing_offset": "10",
            "trailing_offset_type": "BASIS_POINTS",
            "expire_time_ns": None,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "liquidity_side": "NO_LIQUIDITY_SIDE",
            "avg_px": None,
            "slippage": None,
            "commissions": None,
            "status": "INITIALIZED",
            "is_post_only": False,
            "is_reduce_only": False,
            "is_quote_quantity": False,
            "display_qty": None,
            "emulation_trigger": "NO_TRIGGER",
            "trigger_instrument_id": None,
            "contingency_type": "NO_CONTINGENCY",
            "order_list_id": None,
            "linked_order_ids": None,
            "parent_order_id": None,
            "tags": None,
            "ts_init": 0,
            "ts_last": 0,
        }

    def test_trailing_stop_limit_order_with_no_initial_prices_to_dict(self):
        # Arrange
        order = self.order_factory.trailing_stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            limit_offset=Decimal("5"),
            trailing_offset=Decimal("10"),
            trigger_type=TriggerType.MARK_PRICE,
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
        )

        # Act
        result = order.to_dict()

        # Assert
        assert "init_id" in result and uuid.UUID(result["init_id"], version=4)
        del result["init_id"]  # Random UUID4
        assert result == {
            "trader_id": "TESTER-000",
            "strategy_id": "S-001",
            "instrument_id": "AUD/USD.SIM",
            "client_order_id": "O-19700101-000000-000-001-1",
            "venue_order_id": None,
            "position_id": None,
            "account_id": None,
            "last_trade_id": None,
            "type": "TRAILING_STOP_LIMIT",
            "side": "BUY",
            "quantity": "100000",
            "price": None,
            "activation_price": None,
            "trigger_price": None,
            "trigger_type": "MARK_PRICE",
            "limit_offset": "5",
            "trailing_offset": "10",
            "trailing_offset_type": "BASIS_POINTS",
            "expire_time_ns": None,
            "time_in_force": "GTC",
            "filled_qty": "0",
            "liquidity_side": "NO_LIQUIDITY_SIDE",
            "avg_px": None,
            "slippage": None,
            "commissions": None,
            "status": "INITIALIZED",
            "is_post_only": False,
            "is_reduce_only": False,
            "is_quote_quantity": False,
            "display_qty": None,
            "emulation_trigger": "NO_TRIGGER",
            "trigger_instrument_id": None,
            "contingency_type": "NO_CONTINGENCY",
            "order_list_id": None,
            "linked_order_ids": None,
            "parent_order_id": None,
            "exec_algorithm_id": None,
            "exec_algorithm_params": None,
            "exec_spawn_id": None,
            "tags": None,
            "ts_init": 0,
            "ts_last": 0,
        }

    def test_order_list_equality(self):
        # Arrange
        bracket1 = self.order_factory.bracket(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("1.00000"),
            tp_price=Price.from_str("1.00010"),
        )
        bracket2 = self.order_factory.bracket(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("1.00000"),
            tp_price=Price.from_str("1.00010"),
        )

        # Act, Assert
        assert bracket1 == bracket1
        assert bracket1 != bracket2

    def test_bracket_market_entry_order_list(self):
        # Arrange, Act
        bracket = self.order_factory.bracket(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("0.99990"),
            tp_price=Price.from_str("1.00010"),
        )

        # Assert
        assert bracket.id == OrderListId("OL-19700101-000000-000-001-1")
        assert bracket.instrument_id == AUDUSD_SIM.id
        assert len(bracket) == 3
        assert len(bracket.orders) == 3
        assert bracket.orders[0].order_type == OrderType.MARKET
        assert bracket.orders[1].order_type == OrderType.STOP_MARKET
        assert bracket.orders[2].order_type == OrderType.LIMIT
        assert bracket.orders[0].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[1].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[2].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[0].client_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[1].client_order_id == ClientOrderId("O-19700101-000000-000-001-2")
        assert bracket.orders[2].client_order_id == ClientOrderId("O-19700101-000000-000-001-3")
        assert bracket.orders[0].side == OrderSide.BUY
        assert bracket.orders[1].side == OrderSide.SELL
        assert bracket.orders[2].side == OrderSide.SELL
        assert bracket.orders[0].quantity == Quantity.from_int(100_000)
        assert bracket.orders[1].quantity == Quantity.from_int(100_000)
        assert bracket.orders[2].quantity == Quantity.from_int(100_000)
        assert bracket.orders[1].trigger_price == Price.from_str("0.99990")
        assert bracket.orders[2].price == Price.from_str("1.00010")
        assert bracket.orders[1].time_in_force == TimeInForce.GTC
        assert bracket.orders[2].time_in_force == TimeInForce.GTC
        assert bracket.orders[1].expire_time is None
        assert bracket.orders[2].expire_time is None
        assert bracket.orders[0].contingency_type == ContingencyType.OTO
        assert bracket.orders[1].contingency_type == ContingencyType.OUO
        assert bracket.orders[2].contingency_type == ContingencyType.OUO
        assert bracket.orders[0].linked_order_ids == [
            ClientOrderId("O-19700101-000000-000-001-2"),
            ClientOrderId("O-19700101-000000-000-001-3"),
        ]
        assert bracket.orders[1].linked_order_ids == [ClientOrderId("O-19700101-000000-000-001-3")]
        assert bracket.orders[2].linked_order_ids == [ClientOrderId("O-19700101-000000-000-001-2")]
        assert bracket.orders[1].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[2].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.ts_init == 0

    def test_bracket_limit_entry_order_list(self):
        # Arrange, Act
        bracket = self.order_factory.bracket(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            entry_price=Price.from_str("1.00000"),
            sl_trigger_price=Price.from_str("0.99990"),
            tp_price=Price.from_str("1.00010"),
            tp_trigger_price=Price.from_str("1.00010"),
            time_in_force=TimeInForce.GTC,
            entry_order_type=OrderType.LIMIT,
            tp_order_type=OrderType.LIMIT_IF_TOUCHED,
            tp_post_only=False,
        )

        # Assert
        assert bracket.id == OrderListId("OL-19700101-000000-000-001-1")
        assert bracket.instrument_id == AUDUSD_SIM.id
        assert len(bracket) == 3
        assert len(bracket.orders) == 3
        assert bracket.orders[0].order_type == OrderType.LIMIT
        assert bracket.orders[1].order_type == OrderType.STOP_MARKET
        assert bracket.orders[2].order_type == OrderType.LIMIT_IF_TOUCHED
        assert bracket.orders[0].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[1].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[2].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[0].client_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[1].client_order_id == ClientOrderId("O-19700101-000000-000-001-2")
        assert bracket.orders[2].client_order_id == ClientOrderId("O-19700101-000000-000-001-3")
        assert bracket.orders[0].side == OrderSide.BUY
        assert bracket.orders[1].side == OrderSide.SELL
        assert bracket.orders[2].side == OrderSide.SELL
        assert bracket.orders[0].quantity == Quantity.from_int(100_000)
        assert bracket.orders[1].quantity == Quantity.from_int(100_000)
        assert bracket.orders[2].quantity == Quantity.from_int(100_000)
        assert bracket.orders[1].trigger_price == Price.from_str("0.99990")
        assert bracket.orders[2].price == Price.from_str("1.00010")
        assert bracket.orders[1].time_in_force == TimeInForce.GTC
        assert bracket.orders[2].time_in_force == TimeInForce.GTC
        assert bracket.orders[1].expire_time is None
        assert bracket.orders[2].expire_time is None
        assert bracket.orders[0].is_post_only is False
        assert bracket.orders[1].is_post_only is False
        assert bracket.orders[2].is_post_only is False
        assert bracket.orders[0].contingency_type == ContingencyType.OTO
        assert bracket.orders[1].contingency_type == ContingencyType.OUO
        assert bracket.orders[2].contingency_type == ContingencyType.OUO
        assert bracket.orders[0].linked_order_ids == [
            ClientOrderId("O-19700101-000000-000-001-2"),
            ClientOrderId("O-19700101-000000-000-001-3"),
        ]
        assert bracket.orders[1].linked_order_ids == [ClientOrderId("O-19700101-000000-000-001-3")]
        assert bracket.orders[2].linked_order_ids == [ClientOrderId("O-19700101-000000-000-001-2")]
        assert bracket.orders[1].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[2].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.ts_init == 0

    def test_bracket_limit_if_touched_entry_stop_limit_tp_order_list(self):
        # Arrange, Act
        bracket = self.order_factory.bracket(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            entry_trigger_price=Price.from_str("1.00000"),
            entry_price=Price.from_str("1.00000"),
            sl_trigger_price=Price.from_str("0.99990"),
            tp_trigger_price=Price.from_str("1.00010"),
            tp_price=Price.from_str("1.00010"),
            time_in_force=TimeInForce.GTC,
            entry_order_type=OrderType.LIMIT_IF_TOUCHED,
            tp_order_type=OrderType.LIMIT_IF_TOUCHED,
        )

        # Assert
        assert bracket.id == OrderListId("OL-19700101-000000-000-001-1")
        assert bracket.instrument_id == AUDUSD_SIM.id
        assert len(bracket) == 3
        assert len(bracket.orders) == 3
        assert bracket.orders[0].order_type == OrderType.LIMIT_IF_TOUCHED
        assert bracket.orders[1].order_type == OrderType.STOP_MARKET
        assert bracket.orders[2].order_type == OrderType.LIMIT_IF_TOUCHED
        assert bracket.orders[0].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[1].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[2].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[0].client_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[1].client_order_id == ClientOrderId("O-19700101-000000-000-001-2")
        assert bracket.orders[2].client_order_id == ClientOrderId("O-19700101-000000-000-001-3")
        assert bracket.orders[0].side == OrderSide.BUY
        assert bracket.orders[1].side == OrderSide.SELL
        assert bracket.orders[2].side == OrderSide.SELL
        assert bracket.orders[0].quantity == Quantity.from_int(100_000)
        assert bracket.orders[1].quantity == Quantity.from_int(100_000)
        assert bracket.orders[2].quantity == Quantity.from_int(100_000)
        assert bracket.orders[1].trigger_price == Price.from_str("0.99990")
        assert bracket.orders[2].price == Price.from_str("1.00010")
        assert bracket.orders[1].time_in_force == TimeInForce.GTC
        assert bracket.orders[2].time_in_force == TimeInForce.GTC
        assert bracket.orders[1].expire_time is None
        assert bracket.orders[2].expire_time is None
        assert bracket.orders[0].contingency_type == ContingencyType.OTO
        assert bracket.orders[1].contingency_type == ContingencyType.OUO
        assert bracket.orders[2].contingency_type == ContingencyType.OUO
        assert bracket.orders[0].linked_order_ids == [
            ClientOrderId("O-19700101-000000-000-001-2"),
            ClientOrderId("O-19700101-000000-000-001-3"),
        ]
        assert bracket.orders[1].linked_order_ids == [ClientOrderId("O-19700101-000000-000-001-3")]
        assert bracket.orders[2].linked_order_ids == [ClientOrderId("O-19700101-000000-000-001-2")]
        assert bracket.orders[1].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[2].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.ts_init == 0

    def test_bracket_stop_limit_entry_stop_limit_tp_order_list(self):
        # Arrange, Act
        bracket = self.order_factory.bracket(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            entry_trigger_price=Price.from_str("1.00000"),
            entry_price=Price.from_str("1.00001"),
            sl_trigger_price=Price.from_str("0.99990"),
            tp_trigger_price=Price.from_str("1.00010"),
            tp_price=Price.from_str("1.00010"),
            time_in_force=TimeInForce.GTC,
            entry_order_type=OrderType.STOP_LIMIT,
            tp_order_type=OrderType.LIMIT_IF_TOUCHED,
        )

        # Assert
        assert bracket.id == OrderListId("OL-19700101-000000-000-001-1")
        assert bracket.instrument_id == AUDUSD_SIM.id
        assert len(bracket) == 3
        assert len(bracket.orders) == 3
        assert bracket.orders[0].order_type == OrderType.STOP_LIMIT
        assert bracket.orders[1].order_type == OrderType.STOP_MARKET
        assert bracket.orders[2].order_type == OrderType.LIMIT_IF_TOUCHED
        assert bracket.orders[0].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[1].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[2].instrument_id == AUDUSD_SIM.id
        assert bracket.orders[0].client_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[1].client_order_id == ClientOrderId("O-19700101-000000-000-001-2")
        assert bracket.orders[2].client_order_id == ClientOrderId("O-19700101-000000-000-001-3")
        assert bracket.orders[0].side == OrderSide.BUY
        assert bracket.orders[1].side == OrderSide.SELL
        assert bracket.orders[2].side == OrderSide.SELL
        assert bracket.orders[0].quantity == Quantity.from_int(100_000)
        assert bracket.orders[1].quantity == Quantity.from_int(100_000)
        assert bracket.orders[2].quantity == Quantity.from_int(100_000)
        assert bracket.orders[0].trigger_price == Price.from_str("1.00000")
        assert bracket.orders[1].trigger_price == Price.from_str("0.99990")
        assert bracket.orders[0].price == Price.from_str("1.00001")
        assert bracket.orders[2].price == Price.from_str("1.00010")
        assert bracket.orders[1].time_in_force == TimeInForce.GTC
        assert bracket.orders[2].time_in_force == TimeInForce.GTC
        assert bracket.orders[1].expire_time is None
        assert bracket.orders[2].expire_time is None
        assert bracket.orders[0].contingency_type == ContingencyType.OTO
        assert bracket.orders[1].contingency_type == ContingencyType.OUO
        assert bracket.orders[2].contingency_type == ContingencyType.OUO
        assert bracket.orders[0].linked_order_ids == [
            ClientOrderId("O-19700101-000000-000-001-2"),
            ClientOrderId("O-19700101-000000-000-001-3"),
        ]
        assert bracket.orders[1].linked_order_ids == [ClientOrderId("O-19700101-000000-000-001-3")]
        assert bracket.orders[2].linked_order_ids == [ClientOrderId("O-19700101-000000-000-001-2")]
        assert bracket.orders[1].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.orders[2].parent_order_id == ClientOrderId("O-19700101-000000-000-001-1")
        assert bracket.ts_init == 0

    def test_order_list_str_and_repr(self):
        # Arrange, Act
        bracket = self.order_factory.bracket(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            sl_trigger_price=Price.from_str("0.99990"),
            tp_price=Price.from_str("1.00010"),
            entry_tags=["ENTRY"],
            sl_tags=["STOP_LOSS"],
            tp_tags=["TAKE_PROFIT"],
        )

        # Assert
        assert str(bracket) == (
            "OrderList(id=OL-19700101-000000-000-001-1, instrument_id=AUD/USD.SIM, strategy_id=S-001, orders=[MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, contingency_type=OTO, linked_order_ids=[O-19700101-000000-000-001-2, O-19700101-000000-000-001-3], tags=['ENTRY']), StopMarketOrder(SELL 100_000 AUD/USD.SIM STOP_MARKET @ 0.99990[DEFAULT] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-2, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-3], parent_order_id=O-19700101-000000-000-001-1, tags=['STOP_LOSS']), LimitOrder(SELL 100_000 AUD/USD.SIM LIMIT @ 1.00010 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-3, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-2], parent_order_id=O-19700101-000000-000-001-1, tags=['TAKE_PROFIT'])])"
        )
        assert repr(bracket) == (
            "OrderList(id=OL-19700101-000000-000-001-1, instrument_id=AUD/USD.SIM, strategy_id=S-001, orders=[MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=None, position_id=None, contingency_type=OTO, linked_order_ids=[O-19700101-000000-000-001-2, O-19700101-000000-000-001-3], tags=['ENTRY']), StopMarketOrder(SELL 100_000 AUD/USD.SIM STOP_MARKET @ 0.99990[DEFAULT] GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-2, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-3], parent_order_id=O-19700101-000000-000-001-1, tags=['STOP_LOSS']), LimitOrder(SELL 100_000 AUD/USD.SIM LIMIT @ 1.00010 GTC, status=INITIALIZED, client_order_id=O-19700101-000000-000-001-3, venue_order_id=None, position_id=None, contingency_type=OUO, linked_order_ids=[O-19700101-000000-000-001-2], parent_order_id=O-19700101-000000-000-001-1, tags=['TAKE_PROFIT'])])"
        )

    def test_apply_order_denied_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        denied = OrderDenied(
            self.trader_id,
            self.strategy_id,
            AUDUSD_SIM.id,
            order.client_order_id,
            "SOME_REASON",
            UUID4(),
            1,
        )

        # Act
        order.apply(denied)

        # Assert
        assert order.status == OrderStatus.DENIED
        assert order.event_count == 2
        assert order.last_event == denied
        assert not order.is_open
        assert order.is_closed
        assert order.ts_closed == 1

    def test_apply_order_submitted_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        submitted = TestEventStubs.order_submitted(order)

        # Act
        order.apply(submitted)

        # Assert
        assert order.status == OrderStatus.SUBMITTED
        assert order.event_count == 2
        assert order.last_event == submitted
        assert order.is_inflight
        assert not order.is_open
        assert not order.is_closed
        assert not order.is_pending_update
        assert not order.is_pending_cancel

    def test_apply_order_accepted_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order.apply(TestEventStubs.order_submitted(order))

        # Act
        order.apply(TestEventStubs.order_accepted(order))

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert not order.is_inflight
        assert order.is_open
        assert not order.is_closed
        assert (
            str(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=ACCEPTED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=1, position_id=None, tags=None)"
        )
        assert (
            repr(order)
            == "MarketOrder(BUY 100_000 AUD/USD.SIM MARKET GTC, status=ACCEPTED, client_order_id=O-19700101-000000-000-001-1, venue_order_id=1, position_id=None, tags=None)"
        )

    def test_apply_order_rejected_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order.apply(TestEventStubs.order_submitted(order, ts_event=1))

        # Act
        order.apply(TestEventStubs.order_rejected(order, ts_event=1))

        # Assert
        assert order.status == OrderStatus.REJECTED
        assert not order.is_inflight
        assert not order.is_open
        assert order.is_closed
        assert order.ts_closed == 1

    def test_apply_order_expired_event(self):
        # Arrange
        order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("0.99990"),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=1),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

        # Act
        order.apply(TestEventStubs.order_expired(order, ts_event=1))

        # Assert
        assert order.status == OrderStatus.EXPIRED
        assert not order.is_inflight
        assert not order.is_open
        assert order.is_closed
        assert order.ts_closed == 1

    def test_apply_order_triggered_event(self):
        # Arrange
        order = self.order_factory.stop_limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
            Price.from_str("0.99990"),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=1),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

        # Act
        order.apply(TestEventStubs.order_triggered(order))

        # Assert
        assert order.status == OrderStatus.TRIGGERED
        assert not order.is_inflight
        assert order.is_open
        assert not order.is_closed

    def test_order_status_pending_cancel(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

        # Act
        order.apply(TestEventStubs.order_pending_cancel(order))

        # Assert
        assert order.status == OrderStatus.PENDING_CANCEL
        assert order.is_inflight
        assert order.is_open
        assert not order.is_closed
        assert not order.is_pending_update
        assert order.is_pending_cancel
        assert order.event_count == 4

    def test_apply_order_canceled_event(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        order.apply(TestEventStubs.order_pending_cancel(order))

        # Act
        order.apply(TestEventStubs.order_canceled(order, ts_event=1))

        # Assert
        assert order.status == OrderStatus.CANCELED
        assert order.is_canceled
        assert not order.is_inflight
        assert not order.is_open
        assert order.is_closed
        assert not order.is_pending_update
        assert not order.is_pending_cancel
        assert order.event_count == 5
        assert order.ts_closed == 1

    def test_order_status_pending_update(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

        # Act
        order.apply(TestEventStubs.order_pending_update(order))

        # Assert
        assert order.status == OrderStatus.PENDING_UPDATE
        assert order.is_inflight
        assert order.is_open
        assert not order.is_closed
        assert order.is_pending_update
        assert not order.is_pending_cancel
        assert order.event_count == 4

    def test_apply_order_updated_event_to_stop_market_order(self):
        # Arrange
        order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        order.apply(TestEventStubs.order_pending_update(order))

        updated = OrderUpdated(
            order.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            order.account_id,
            Quantity.from_int(120000),
            None,
            Price.from_str("1.00001"),
            UUID4(),
            0,
            0,
        )

        # Act
        order.apply(updated)

        # Assert
        assert order.status == OrderStatus.ACCEPTED
        assert order.venue_order_id == VenueOrderId("1")
        assert order.quantity == Quantity.from_int(120_000)
        assert order.trigger_price == Price.from_str("1.00001")
        assert not order.is_inflight
        assert order.is_open
        assert not order.is_closed
        assert order.event_count == 5

    def test_apply_order_updated_event_when_buy_order_partially_filled(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        order.apply(
            TestEventStubs.order_filled(
                order,
                instrument=AUDUSD_SIM,
                last_qty=Quantity.from_int(50_000),
            ),
        )
        order.apply(TestEventStubs.order_pending_update(order))

        updated = OrderUpdated(
            order.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            order.account_id,
            Quantity.from_int(120_000),
            None,
            Price.from_str("1.00001"),
            UUID4(),
            0,
            0,
        )

        # Act
        order.apply(updated)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.venue_order_id == VenueOrderId("1")
        assert order.quantity == Quantity.from_int(120_000)
        assert order.filled_qty == Quantity.from_int(50_000)
        assert order.leaves_qty == Quantity.from_int(70_000)
        assert not order.is_inflight
        assert order.is_open
        assert not order.is_closed
        assert order.event_count == 6

    def test_apply_order_updated_event_when_sell_order_partially_filled(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        order.apply(
            TestEventStubs.order_filled(
                order,
                instrument=AUDUSD_SIM,
                last_qty=Quantity.from_int(50_000),
            ),
        )
        order.apply(TestEventStubs.order_pending_update(order))

        updated = OrderUpdated(
            order.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            order.account_id,
            Quantity.from_int(120_000),
            None,
            Price.from_str("1.00001"),
            UUID4(),
            0,
            0,
        )

        # Act
        order.apply(updated)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.venue_order_id == VenueOrderId("1")
        assert order.quantity == Quantity.from_int(120_000)
        assert order.filled_qty == Quantity.from_int(50_000)
        assert order.leaves_qty == Quantity.from_int(70_000)
        assert order.signed_decimal_qty() == -Decimal(70_000)
        assert not order.is_inflight
        assert order.is_open
        assert not order.is_closed
        assert order.event_count == 6

    def test_apply_order_updated_venue_id_change(self):
        # Arrange
        order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))
        order.apply(TestEventStubs.order_pending_update(order))

        updated = OrderUpdated(
            order.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("2"),
            order.account_id,
            Quantity.from_int(120_000),
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
            Quantity.from_int(100_000),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

        filled = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
            ts_event=1,
        )

        # Act
        order.apply(filled)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.filled_qty == Quantity.from_int(100_000)
        assert order.leaves_qty == Quantity.zero()
        assert order.signed_decimal_qty() == Decimal()
        assert order.avg_px == 1.00001
        assert len(order.trade_ids) == 1
        assert not order.is_inflight
        assert not order.is_open
        assert order.is_closed
        assert order.ts_last == 1
        assert order.ts_closed == 1

    def test_apply_order_filled_event_to_market_order(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

        filled = TestEventStubs.order_filled(
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
        assert order.filled_qty == Quantity.from_int(100_000)
        assert order.signed_decimal_qty() == Decimal()
        assert order.avg_px == 1.00001
        assert order.commissions() == [Money(2.0, USD)]
        assert len(order.trade_ids) == 1
        assert not order.is_inflight
        assert not order.is_open
        assert order.is_closed
        assert order.ts_last == 0

    def test_apply_partial_fill_events_to_market_order_results_in_partially_filled(
        self,
    ):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

        fill1 = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            trade_id=TradeId("1"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
            last_qty=Quantity.from_int(20_000),
        )

        fill2 = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            trade_id=TradeId("2"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00002"),
            last_qty=Quantity.from_int(40_000),
        )

        # Act
        order.apply(fill1)
        order.apply(fill2)

        # Assert
        assert order.status == OrderStatus.PARTIALLY_FILLED
        assert order.filled_qty == Quantity.from_int(60_000)
        assert order.leaves_qty == Quantity.from_int(40_000)
        assert order.signed_decimal_qty() == Decimal(40_000)
        # Correct weighted average: (20k * 1.00001 + 40k * 1.00002) / 60k = 1.0000166666...
        assert order.avg_px == pytest.approx(1.0000166666666668, rel=1e-9)
        assert order.commissions() == [Money(4.0, USD)]
        assert len(order.trade_ids) == 2
        assert not order.is_inflight
        assert order.is_open
        assert not order.is_closed
        assert order.ts_last == 0

    def test_apply_filled_events_to_market_order_results_in_filled(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

        fill1 = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            trade_id=TradeId("1"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
            last_qty=Quantity.from_int(20_000),
        )

        fill2 = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            trade_id=TradeId("2"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00002"),
            last_qty=Quantity.from_int(40_000),
        )

        fill3 = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            trade_id=TradeId("3"),
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00003"),
            last_qty=Quantity.from_int(40_000),
        )

        # Act
        order.apply(fill1)
        order.apply(fill2)
        order.apply(fill3)

        # Assert
        assert order.status == OrderStatus.FILLED
        assert order.filled_qty == Quantity.from_int(100_000)
        # Correct weighted average: (20k * 1.00001 + 40k * 1.00002 + 40k * 1.00003) / 100k = 1.000022
        assert order.avg_px == pytest.approx(1.000022, rel=1e-9)
        assert order.commissions() == [Money(6.0, USD)]
        assert len(order.trade_ids) == 3
        assert not order.is_inflight
        assert not order.is_open
        assert order.is_closed
        assert order.ts_last == 0

    def test_apply_order_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

        filled = OrderFilled(
            order.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            order.account_id,
            TradeId("E-1"),
            PositionId("P-1"),
            order.side,
            order.order_type,
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
        assert order.filled_qty == Quantity.from_int(100_000)
        assert order.price == Price.from_str("1.00000")
        assert order.avg_px == 1.00001
        assert order.slippage == 1.0000000000065512e-05
        assert not order.is_inflight
        assert not order.is_open
        assert order.is_closed
        assert order.ts_last == 0

    def test_apply_order_partially_filled_event_to_buy_limit_order(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

        partially = OrderFilled(
            order.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            VenueOrderId("1"),
            order.account_id,
            TradeId("E-1"),
            PositionId("P-1"),
            order.side,
            order.order_type,
            Quantity.from_int(50_000),
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
        assert order.filled_qty == Quantity.from_int(50_000)
        assert order.price == Price.from_str("1.00000")
        assert order.avg_px == 0.999999
        assert order.slippage == -1.0000000000287557e-06
        assert not order.is_inflight
        assert order.is_open
        assert not order.is_closed
        assert order.ts_last == 1_000_000_000, order.ts_last

    def test_market_order_transformation_to_limit_order(self) -> None:
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        price = Price.from_str("1.00000")

        # Act
        order = LimitOrder.transform_py(order, ts_init=1, price=price)

        # Assert
        assert order.order_type == OrderType.LIMIT
        assert order.price == price
        assert order.ts_init == 0  # Retains original order `ts_init`

    def test_limit_order_transformation_to_market_order(self) -> None:
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        # Act
        order = MarketOrder.transform_py(order, ts_init=1)

        # Assert
        assert order.order_type == OrderType.MARKET
        assert order.ts_init == 0  # Retains original order `ts_init`

    def test_limit_order_to_own_book_order(self) -> None:
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
            Price.from_str("1.00000"),
        )

        # Act
        own_order = order.to_own_book_order()

        # Assert
        assert isinstance(own_order, nautilus_pyo3.OwnBookOrder)
        assert own_order.client_order_id == nautilus_pyo3.ClientOrderId(
            "O-19700101-000000-000-001-1",
        )
        assert own_order.side == nautilus_pyo3.OrderSide.BUY
        assert own_order.price == nautilus_pyo3.Price.from_str("1.00000")
        assert own_order.size == nautilus_pyo3.Quantity.from_int(100_000)
        assert own_order.time_in_force == nautilus_pyo3.TimeInForce.GTC
        assert own_order.status == nautilus_pyo3.OrderStatus.INITIALIZED
        assert own_order.ts_last == 0
        assert own_order.ts_init == 0
