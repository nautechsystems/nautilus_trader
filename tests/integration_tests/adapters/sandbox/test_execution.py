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

import asyncio
from unittest.mock import MagicMock

import pytest

from nautilus_trader.backtest.engine import SimulatedExchange
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.data import InstrumentClose
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderPendingCancel
from nautilus_trader.model.events import OrderPendingUpdate
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


def _make_quote_tick(instrument):
    return QuoteTick(
        instrument_id=instrument.id,
        bid_price=instrument.make_price(10),
        ask_price=instrument.make_price(10),
        bid_size=instrument.make_qty(100),
        ask_size=instrument.make_qty(100),
        ts_init=0,
        ts_event=0,
    )


def _make_equity_with_increment(instrument: Equity, price_increment: str) -> Equity:
    info = instrument.info.copy() if isinstance(instrument.info, dict) else instrument.info
    return Equity(
        instrument_id=instrument.id,
        raw_symbol=instrument.raw_symbol,
        currency=instrument.quote_currency,
        price_precision=instrument.price_precision,
        price_increment=Price.from_str(price_increment),
        lot_size=instrument.lot_size,
        ts_event=instrument.ts_event + 1,
        ts_init=instrument.ts_init + 1,
        max_quantity=instrument.max_quantity,
        min_quantity=instrument.min_quantity,
        margin_init=instrument.margin_init,
        margin_maint=instrument.margin_maint,
        maker_fee=instrument.maker_fee,
        taker_fee=instrument.taker_fee,
        isin=instrument.isin,
        tick_scheme_name=instrument.tick_scheme_name,
        info=info,
    )


@pytest.mark.asyncio()
async def test_connect(exec_client):
    exec_client.connect()
    await asyncio.sleep(0)
    assert isinstance(exec_client.exchange, SimulatedExchange)
    assert exec_client.is_connected


@pytest.mark.asyncio()
async def test_submit_order_success(exec_client, instrument, strategy, events):
    # Arrange
    exec_client.connect()
    order = TestExecStubs.limit_order(instrument=instrument)

    # Act
    strategy.submit_order(order=order)
    exec_client.on_data(_make_quote_tick(instrument))

    # Assert
    print(events)
    _, submitted, accepted, filled, _ = events
    assert isinstance(submitted, OrderSubmitted)
    assert isinstance(accepted, OrderAccepted)
    assert isinstance(filled, OrderFilled)
    assert accepted.venue_order_id.value.startswith("SANDBOX-")


@pytest.mark.asyncio()
async def test_submit_orders_list_success(
    exec_client,
    instrument,
    strategy,
    events,
):
    # Arrange
    exec_client.connect()
    factory = OrderFactory(
        trader_id=TraderId("TESTER-000"),
        strategy_id=StrategyId("S-001"),
        clock=TestClock(),
    )
    first_order = TestExecStubs.limit_order(
        instrument=instrument,
        client_order_id=factory.generate_client_order_id(),
    )
    second_order = TestExecStubs.limit_order(
        instrument=instrument,
        client_order_id=factory.generate_client_order_id(),
    )
    order_list = OrderList(
        order_list_id=factory.generate_order_list_id(),
        orders=[first_order, second_order],
    )

    # Act
    strategy.submit_order_list(order_list=order_list)
    exec_client.on_data(_make_quote_tick(instrument))

    # Assert
    print(events)
    (
        _,  # first initialized
        _,  # second initialized
        first_submitted,
        second_submitted,
        first_accepted,
        second_accepted,
        first_filled,
        _,  # position opened
        second_filled,
        _,  # position changed
    ) = events
    assert isinstance(first_submitted, OrderSubmitted)
    assert isinstance(second_submitted, OrderSubmitted)
    assert isinstance(first_accepted, OrderAccepted)
    assert isinstance(second_accepted, OrderAccepted)
    assert isinstance(first_filled, OrderFilled)
    assert isinstance(second_filled, OrderFilled)
    assert first_accepted.venue_order_id.value.startswith("SANDBOX-")
    assert second_accepted.venue_order_id.value.startswith("SANDBOX-")


@pytest.mark.asyncio()
async def test_modify_order_success(exec_client, strategy, instrument, events):
    # Arrange
    exec_client.connect()
    order = TestExecStubs.limit_order(
        instrument=instrument,
        price=instrument.make_price(0.01),
    )
    strategy.submit_order(order)
    exec_client.on_data(_make_quote_tick(instrument))

    # Act
    strategy.modify_order(
        order=order,
        price=instrument.make_price(0.01),
        quantity=instrument.make_qty(200),
    )
    exec_client.on_data(_make_quote_tick(instrument))

    # Assert
    initialised, submitted, accepted, pending_update, updated = events
    assert isinstance(pending_update, OrderPendingUpdate)
    assert isinstance(updated, OrderUpdated)
    assert updated.price == Price.from_str("0.01")


@pytest.mark.asyncio()
async def test_modify_order_error_no_venue_id(exec_client, strategy, instrument):
    # Arrange
    exec_client.connect()
    order = TestExecStubs.limit_order(
        instrument=instrument,
        price=instrument.make_price(0.01),
    )
    strategy.submit_order(order)
    exec_client.on_data(_make_quote_tick(instrument))

    # Act
    client_order_id = ClientOrderId("NOT-AN-ID")
    command = TestCommandStubs.modify_order_command(
        instrument_id=order.instrument_id,
        client_order_id=client_order_id,
        price=instrument.make_price(0.01),
        quantity=instrument.make_qty(200),
    )
    exec_client.modify_order(command)
    exec_client.on_data(_make_quote_tick(instrument))

    # Assert
    order_client_ids = [o.client_order_id for o in strategy.cache.orders()]
    assert client_order_id not in order_client_ids


@pytest.mark.asyncio()
async def test_cancel_order_success(exec_client, cache, strategy, instrument, events):
    # Arrange
    exec_client.connect()
    order = TestExecStubs.limit_order(
        instrument=instrument,
        price=instrument.make_price(0.01),
    )
    strategy.submit_order(order)
    exec_client.on_data(_make_quote_tick(instrument))

    # Act
    strategy.cancel_order(order)
    exec_client.on_data(_make_quote_tick(instrument))

    # Assert
    _, _, _, pending_cancel, cancelled = events
    assert isinstance(pending_cancel, OrderPendingCancel)
    assert isinstance(cancelled, OrderCanceled)


@pytest.mark.asyncio()
async def test_cancel_order_fail(exec_client, cache, strategy, instrument, events):
    # Arrange
    exec_client.connect()
    order = TestExecStubs.limit_order(
        instrument=instrument,
        price=instrument.make_price(0.01),
    )
    strategy.submit_order(order)

    # Act
    client_order_id = ClientOrderId("111")
    venue_order_id = VenueOrderId("1")
    command = TestCommandStubs.cancel_order_command(
        instrument_id=order.instrument_id,
        client_order_id=client_order_id,
    )
    exec_client.cancel_order(command)
    exec_client.on_data(_make_quote_tick(instrument))

    # Assert
    client_order_ids = [o.client_order_id for o in strategy.cache.orders()]
    assert client_order_id not in client_order_ids
    venue_order_ids = [o.venue_order_id for o in strategy.cache.orders()]
    assert venue_order_id not in venue_order_ids


@pytest.mark.asyncio()
async def test_on_data_updates_exchange_instrument(exec_client, instrument):
    # Arrange
    exec_client.connect()
    matching_engine = exec_client.exchange.get_matching_engine(instrument.id)
    assert matching_engine is not None
    updated_instrument = _make_equity_with_increment(instrument, "0.02")

    # Act
    exec_client.on_data(updated_instrument)

    # Assert
    assert matching_engine.instrument.price_increment == Price.from_str("0.02")
    assert matching_engine.instrument.ts_init == updated_instrument.ts_init


@pytest.mark.asyncio()
async def test_on_data_forwards_instrument_status(exec_client, instrument):
    # Arrange
    exec_client.connect()
    mock_exchange = MagicMock(spec=SimulatedExchange)
    mock_exchange.process_instrument_status = MagicMock()
    mock_exchange.process = MagicMock()
    exec_client.exchange = mock_exchange
    status = InstrumentStatus(
        instrument_id=instrument.id,
        action=MarketStatusAction.TRADING,
        ts_event=1,
        ts_init=1,
    )

    # Act
    exec_client.on_data(status)

    # Assert
    mock_exchange.process_instrument_status.assert_called_once_with(status)
    mock_exchange.process.assert_called_once_with(status.ts_init)


@pytest.mark.asyncio()
async def test_on_data_forwards_instrument_close(exec_client, instrument):
    # Arrange
    exec_client.connect()
    mock_exchange = MagicMock(spec=SimulatedExchange)
    mock_exchange.process_instrument_close = MagicMock()
    mock_exchange.process = MagicMock()
    exec_client.exchange = mock_exchange
    close = InstrumentClose(
        instrument_id=instrument.id,
        close_price=Price.from_str("123.45"),
        close_type=InstrumentCloseType.CONTRACT_EXPIRED,
        ts_event=1,
        ts_init=1,
    )

    # Act
    exec_client.on_data(close)

    # Assert
    mock_exchange.process_instrument_close.assert_called_once_with(close)
    mock_exchange.process.assert_called_once_with(close.ts_init)
