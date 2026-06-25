# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common import Clock
from nautilus_trader.common import OrderFactory
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderListId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.model import TrailingOffsetType
from nautilus_trader.model import TriggerType


INSTRUMENT_ID = InstrumentId.from_str("BTCUSDT.BINANCE")
TRIGGER_INSTRUMENT_ID = InstrumentId.from_str("ETHUSDT.BINANCE")
QUANTITY = Quantity.from_int(100)
DISPLAY_QTY = Quantity.from_int(50)
EXPIRE_TIME = 1_700_000_000_000_000_000
EXEC_ALGORITHM_ID = ExecAlgorithmId("VWAP")
EXEC_ALGORITHM_PARAMS = {"speed": "fast"}


def test_order_factory_generates_and_resets_ids():
    factory = _factory()

    client_order_id = factory.generate_client_order_id()
    order_list_id = factory.generate_order_list_id()

    assert client_order_id == ClientOrderId("O-19700101-000000-001-001-1")
    assert order_list_id == OrderListId("OL-19700101-000000-001-001-1")
    assert factory.get_client_order_id_count() == 1
    assert factory.get_order_list_id_count() == 1

    factory.reset()

    assert factory.get_client_order_id_count() == 0
    assert factory.get_order_list_id_count() == 0


def test_order_factory_does_not_expose_internal_count_setters():
    factory = _factory()

    assert not hasattr(factory, "set_client_order_id_count")
    assert not hasattr(factory, "set_order_list_id_count")


def test_order_factory_creates_single_order_types_with_forwarded_parameters():
    factory = _factory()

    market_id = ClientOrderId("O-MARKET")
    market = factory.market(
        INSTRUMENT_ID,
        OrderSide.BUY,
        QUANTITY,
        time_in_force=TimeInForce.FOK,
        reduce_only=True,
        quote_quantity=True,
        exec_algorithm_id=EXEC_ALGORITHM_ID,
        exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        tags=["market", "factory"],
        client_order_id=market_id,
    )

    limit_id = ClientOrderId("O-LIMIT")
    limit = factory.limit(
        INSTRUMENT_ID,
        OrderSide.BUY,
        QUANTITY,
        Price.from_str("50000.00"),
        time_in_force=TimeInForce.GTD,
        expire_time=EXPIRE_TIME,
        post_only=True,
        reduce_only=True,
        quote_quantity=False,
        display_qty=DISPLAY_QTY,
        emulation_trigger=TriggerType.LAST_PRICE,
        trigger_instrument_id=TRIGGER_INSTRUMENT_ID,
        exec_algorithm_id=EXEC_ALGORITHM_ID,
        exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        tags=["limit", "factory"],
        client_order_id=limit_id,
    )

    stop_market_id = ClientOrderId("O-STOP-MARKET")
    stop_market = factory.stop_market(
        INSTRUMENT_ID,
        OrderSide.SELL,
        QUANTITY,
        Price.from_str("45000.00"),
        trigger_type=TriggerType.LAST_PRICE,
        time_in_force=TimeInForce.GTD,
        expire_time=EXPIRE_TIME,
        reduce_only=True,
        quote_quantity=False,
        display_qty=DISPLAY_QTY,
        emulation_trigger=TriggerType.LAST_PRICE,
        trigger_instrument_id=TRIGGER_INSTRUMENT_ID,
        exec_algorithm_id=EXEC_ALGORITHM_ID,
        exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        tags=["stop-market", "factory"],
        client_order_id=stop_market_id,
    )

    stop_limit_id = ClientOrderId("O-STOP-LIMIT")
    stop_limit = factory.stop_limit(
        INSTRUMENT_ID,
        OrderSide.SELL,
        QUANTITY,
        Price.from_str("44900.00"),
        Price.from_str("45000.00"),
        trigger_type=TriggerType.LAST_PRICE,
        time_in_force=TimeInForce.GTD,
        expire_time=EXPIRE_TIME,
        post_only=True,
        reduce_only=True,
        quote_quantity=False,
        display_qty=DISPLAY_QTY,
        emulation_trigger=TriggerType.LAST_PRICE,
        trigger_instrument_id=TRIGGER_INSTRUMENT_ID,
        exec_algorithm_id=EXEC_ALGORITHM_ID,
        exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        tags=["stop-limit", "factory"],
        client_order_id=stop_limit_id,
    )

    market_to_limit_id = ClientOrderId("O-MARKET-TO-LIMIT")
    market_to_limit = factory.market_to_limit(
        INSTRUMENT_ID,
        OrderSide.BUY,
        QUANTITY,
        time_in_force=TimeInForce.GTD,
        expire_time=EXPIRE_TIME,
        reduce_only=True,
        quote_quantity=False,
        display_qty=DISPLAY_QTY,
        exec_algorithm_id=EXEC_ALGORITHM_ID,
        exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        tags=["market-to-limit", "factory"],
        client_order_id=market_to_limit_id,
    )

    market_if_touched_id = ClientOrderId("O-MARKET-IF-TOUCHED")
    market_if_touched = factory.market_if_touched(
        INSTRUMENT_ID,
        OrderSide.BUY,
        QUANTITY,
        Price.from_str("48000.00"),
        trigger_type=TriggerType.LAST_PRICE,
        time_in_force=TimeInForce.GTD,
        expire_time=EXPIRE_TIME,
        reduce_only=True,
        quote_quantity=False,
        emulation_trigger=TriggerType.LAST_PRICE,
        trigger_instrument_id=TRIGGER_INSTRUMENT_ID,
        exec_algorithm_id=EXEC_ALGORITHM_ID,
        exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        tags=["market-if-touched", "factory"],
        client_order_id=market_if_touched_id,
    )

    limit_if_touched_id = ClientOrderId("O-LIMIT-IF-TOUCHED")
    limit_if_touched = factory.limit_if_touched(
        INSTRUMENT_ID,
        OrderSide.BUY,
        QUANTITY,
        Price.from_str("48100.00"),
        Price.from_str("48000.00"),
        trigger_type=TriggerType.LAST_PRICE,
        time_in_force=TimeInForce.GTD,
        expire_time=EXPIRE_TIME,
        post_only=True,
        reduce_only=True,
        quote_quantity=False,
        display_qty=DISPLAY_QTY,
        emulation_trigger=TriggerType.LAST_PRICE,
        trigger_instrument_id=TRIGGER_INSTRUMENT_ID,
        exec_algorithm_id=EXEC_ALGORITHM_ID,
        exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        tags=["limit-if-touched", "factory"],
        client_order_id=limit_if_touched_id,
    )

    trailing_stop_market_id = ClientOrderId("O-TRAILING-STOP-MARKET")
    trailing_stop_market = factory.trailing_stop_market(
        INSTRUMENT_ID,
        OrderSide.SELL,
        QUANTITY,
        Decimal("0.50"),
        trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
        activation_price=Price.from_str("45500.00"),
        trigger_price=Price.from_str("45000.00"),
        trigger_type=TriggerType.LAST_PRICE,
        time_in_force=TimeInForce.GTD,
        expire_time=EXPIRE_TIME,
        reduce_only=True,
        quote_quantity=False,
        display_qty=DISPLAY_QTY,
        emulation_trigger=TriggerType.LAST_PRICE,
        trigger_instrument_id=TRIGGER_INSTRUMENT_ID,
        exec_algorithm_id=EXEC_ALGORITHM_ID,
        exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        tags=["trailing-stop-market", "factory"],
        client_order_id=trailing_stop_market_id,
    )

    trailing_stop_limit_id = ClientOrderId("O-TRAILING-STOP-LIMIT")
    trailing_stop_limit = factory.trailing_stop_limit(
        INSTRUMENT_ID,
        OrderSide.SELL,
        QUANTITY,
        Price.from_str("44900.00"),
        Decimal("10.00"),
        Decimal("0.50"),
        trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
        activation_price=Price.from_str("45500.00"),
        trigger_price=Price.from_str("45000.00"),
        trigger_type=TriggerType.LAST_PRICE,
        time_in_force=TimeInForce.GTD,
        expire_time=EXPIRE_TIME,
        post_only=False,
        reduce_only=True,
        quote_quantity=False,
        display_qty=DISPLAY_QTY,
        emulation_trigger=TriggerType.LAST_PRICE,
        trigger_instrument_id=TRIGGER_INSTRUMENT_ID,
        exec_algorithm_id=EXEC_ALGORITHM_ID,
        exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        tags=["trailing-stop-limit", "factory"],
        client_order_id=trailing_stop_limit_id,
    )

    _assert_order_base(
        market,
        OrderType.MARKET,
        OrderSide.BUY,
        market_id,
        ["market", "factory"],
        TimeInForce.FOK,
        True,
        True,
    )

    _assert_order_base(
        limit,
        OrderType.LIMIT,
        OrderSide.BUY,
        limit_id,
        ["limit", "factory"],
        TimeInForce.GTD,
        True,
        False,
    )
    assert limit.price == Price.from_str("50000.00")
    assert limit.expire_time == EXPIRE_TIME
    assert limit.is_post_only is True
    assert limit.display_qty == DISPLAY_QTY
    _assert_trigger_fields(limit)

    _assert_order_base(
        stop_market,
        OrderType.STOP_MARKET,
        OrderSide.SELL,
        stop_market_id,
        ["stop-market", "factory"],
        TimeInForce.GTD,
        True,
        False,
    )
    assert stop_market.trigger_price == Price.from_str("45000.00")
    assert stop_market.trigger_type == TriggerType.LAST_PRICE
    assert stop_market.display_qty == DISPLAY_QTY
    _assert_trigger_fields(stop_market)

    _assert_order_base(
        stop_limit,
        OrderType.STOP_LIMIT,
        OrderSide.SELL,
        stop_limit_id,
        ["stop-limit", "factory"],
        TimeInForce.GTD,
        True,
        False,
    )
    assert stop_limit.price == Price.from_str("44900.00")
    assert stop_limit.trigger_price == Price.from_str("45000.00")
    assert stop_limit.is_post_only is True
    assert stop_limit.display_qty == DISPLAY_QTY
    _assert_trigger_fields(stop_limit)

    _assert_order_base(
        market_to_limit,
        OrderType.MARKET_TO_LIMIT,
        OrderSide.BUY,
        market_to_limit_id,
        ["market-to-limit", "factory"],
        TimeInForce.GTD,
        True,
        False,
    )
    assert market_to_limit.expire_time == EXPIRE_TIME
    assert market_to_limit.display_qty == DISPLAY_QTY

    _assert_order_base(
        market_if_touched,
        OrderType.MARKET_IF_TOUCHED,
        OrderSide.BUY,
        market_if_touched_id,
        ["market-if-touched", "factory"],
        TimeInForce.GTD,
        True,
        False,
    )
    assert market_if_touched.trigger_price == Price.from_str("48000.00")
    assert market_if_touched.trigger_type == TriggerType.LAST_PRICE
    _assert_trigger_fields(market_if_touched)

    _assert_order_base(
        limit_if_touched,
        OrderType.LIMIT_IF_TOUCHED,
        OrderSide.BUY,
        limit_if_touched_id,
        ["limit-if-touched", "factory"],
        TimeInForce.GTD,
        True,
        False,
    )
    assert limit_if_touched.price == Price.from_str("48100.00")
    assert limit_if_touched.trigger_price == Price.from_str("48000.00")
    assert limit_if_touched.is_post_only is True
    assert limit_if_touched.display_qty == DISPLAY_QTY
    _assert_trigger_fields(limit_if_touched)

    _assert_order_base(
        trailing_stop_market,
        OrderType.TRAILING_STOP_MARKET,
        OrderSide.SELL,
        trailing_stop_market_id,
        ["trailing-stop-market", "factory"],
        TimeInForce.GTD,
        True,
        False,
    )
    assert trailing_stop_market.trigger_price == Price.from_str("45000.00")
    assert trailing_stop_market.activation_price == Price.from_str("45500.00")
    assert trailing_stop_market.trailing_offset == Decimal("0.50")
    assert trailing_stop_market.trailing_offset_type == TrailingOffsetType.BASIS_POINTS
    assert trailing_stop_market.display_qty == DISPLAY_QTY
    _assert_trigger_fields(trailing_stop_market)

    _assert_order_base(
        trailing_stop_limit,
        OrderType.TRAILING_STOP_LIMIT,
        OrderSide.SELL,
        trailing_stop_limit_id,
        ["trailing-stop-limit", "factory"],
        TimeInForce.GTD,
        True,
        False,
    )
    assert trailing_stop_limit.price == Price.from_str("44900.00")
    assert trailing_stop_limit.trigger_price == Price.from_str("45000.00")
    assert trailing_stop_limit.activation_price == Price.from_str("45500.00")
    assert trailing_stop_limit.limit_offset == Decimal("10.00")
    assert trailing_stop_limit.trailing_offset == Decimal("0.50")
    assert trailing_stop_limit.trailing_offset_type == TrailingOffsetType.BASIS_POINTS
    assert trailing_stop_limit.display_qty == DISPLAY_QTY
    _assert_trigger_fields(trailing_stop_limit)


def test_order_factory_bracket_forwards_leg_parameters():
    factory = _factory()
    entry_id = ClientOrderId("O-BRACKET-ENTRY")
    tp_id = ClientOrderId("O-BRACKET-TP")
    sl_id = ClientOrderId("O-BRACKET-SL")

    orders = factory.bracket(
        INSTRUMENT_ID,
        OrderSide.BUY,
        QUANTITY,
        quote_quantity=False,
        emulation_trigger=TriggerType.LAST_PRICE,
        trigger_instrument_id=TRIGGER_INSTRUMENT_ID,
        contingency_type=ContingencyType.OCO,
        entry_order_type=OrderType.STOP_LIMIT,
        entry_price=Price.from_str("50100.00"),
        entry_trigger_price=Price.from_str("50000.00"),
        expire_time=EXPIRE_TIME,
        time_in_force=TimeInForce.GTD,
        entry_post_only=True,
        entry_exec_algorithm_id=EXEC_ALGORITHM_ID,
        entry_exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        entry_tags=["entry", "factory"],
        entry_client_order_id=entry_id,
        tp_order_type=OrderType.LIMIT_IF_TOUCHED,
        tp_price=Price.from_str("55000.00"),
        tp_trigger_price=Price.from_str("55100.00"),
        tp_trigger_type=TriggerType.LAST_PRICE,
        tp_time_in_force=TimeInForce.FOK,
        tp_post_only=False,
        tp_exec_algorithm_id=EXEC_ALGORITHM_ID,
        tp_exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        tp_tags=["tp", "factory"],
        tp_client_order_id=tp_id,
        sl_order_type=OrderType.TRAILING_STOP_MARKET,
        sl_trigger_price=Price.from_str("45000.00"),
        sl_trigger_type=TriggerType.BID_ASK,
        sl_activation_price=Price.from_str("45500.00"),
        sl_trailing_offset=Decimal("0.75"),
        sl_trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
        sl_time_in_force=TimeInForce.IOC,
        sl_exec_algorithm_id=EXEC_ALGORITHM_ID,
        sl_exec_algorithm_params=EXEC_ALGORITHM_PARAMS,
        sl_tags=["sl", "factory"],
        sl_client_order_id=sl_id,
    )

    entry, stop_loss, take_profit = orders

    assert len(orders) == 3
    _assert_order_base(
        entry,
        OrderType.STOP_LIMIT,
        OrderSide.BUY,
        entry_id,
        ["entry", "factory"],
        TimeInForce.GTD,
        False,
        False,
    )
    assert entry.price == Price.from_str("50100.00")
    assert entry.trigger_price == Price.from_str("50000.00")
    assert entry.expire_time == EXPIRE_TIME
    assert entry.is_post_only is True
    assert entry.contingency_type == ContingencyType.OTO
    assert entry.linked_order_ids == [sl_id, tp_id]
    assert entry.parent_order_id is None
    _assert_trigger_fields(entry)

    _assert_order_base(
        stop_loss,
        OrderType.TRAILING_STOP_MARKET,
        OrderSide.SELL,
        sl_id,
        ["sl", "factory"],
        TimeInForce.IOC,
        True,
        False,
    )
    assert stop_loss.trigger_price == Price.from_str("45000.00")
    assert stop_loss.trigger_type == TriggerType.BID_ASK
    assert stop_loss.activation_price == Price.from_str("45500.00")
    assert stop_loss.trailing_offset == Decimal("0.75")
    assert stop_loss.trailing_offset_type == TrailingOffsetType.BASIS_POINTS
    assert stop_loss.contingency_type == ContingencyType.OCO
    assert stop_loss.linked_order_ids == [tp_id]
    assert stop_loss.parent_order_id == entry_id
    _assert_trigger_fields(stop_loss)

    _assert_order_base(
        take_profit,
        OrderType.LIMIT_IF_TOUCHED,
        OrderSide.SELL,
        tp_id,
        ["tp", "factory"],
        TimeInForce.FOK,
        True,
        False,
    )
    assert take_profit.price == Price.from_str("55000.00")
    assert take_profit.trigger_price == Price.from_str("55100.00")
    assert take_profit.trigger_type == TriggerType.LAST_PRICE
    assert take_profit.is_post_only is False
    assert take_profit.contingency_type == ContingencyType.OCO
    assert take_profit.linked_order_ids == [sl_id]
    assert take_profit.parent_order_id == entry_id
    _assert_trigger_fields(take_profit)


def test_order_factory_checked_errors_raise_value_error():
    factory = _factory()

    with pytest.raises(
        ValueError,
        match="TrailingStopMarket requires either trigger_price or activation_price",
    ):
        factory.trailing_stop_market(
            INSTRUMENT_ID,
            OrderSide.SELL,
            QUANTITY,
            Decimal("0.50"),
        )

    with pytest.raises(ValueError, match="`tp_price` is required for a LIMIT take-profit"):
        factory.bracket(
            INSTRUMENT_ID,
            OrderSide.BUY,
            QUANTITY,
            sl_trigger_price=Price.from_str("45000.00"),
        )


def _factory() -> OrderFactory:
    return OrderFactory(
        TraderId("TRADER-001"),
        StrategyId("S-001"),
        Clock.new_test(),
    )


def _assert_trigger_fields(order):
    assert order.emulation_trigger == TriggerType.LAST_PRICE
    assert order.trigger_instrument_id == TRIGGER_INSTRUMENT_ID


def _assert_order_base(
    order,
    order_type,
    order_side,
    client_order_id,
    tags,
    time_in_force,
    reduce_only,
    quote_quantity,
):
    assert order.instrument_id == INSTRUMENT_ID
    assert order.order_type == order_type
    assert order.side == order_side
    assert order.quantity == QUANTITY
    assert order.time_in_force == time_in_force
    assert order.is_reduce_only is reduce_only
    assert order.is_quote_quantity is quote_quantity
    _assert_algorithm_fields(order, client_order_id, tags)


def _assert_algorithm_fields(order, client_order_id, tags):
    assert order.client_order_id == client_order_id
    assert order.exec_algorithm_id == EXEC_ALGORITHM_ID
    assert order.exec_algorithm_params == EXEC_ALGORITHM_PARAMS
    assert order.exec_spawn_id == client_order_id
    assert order.tags == tags
