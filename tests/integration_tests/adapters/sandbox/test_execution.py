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

import pytest

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderPendingCancel
from nautilus_trader.model.events import OrderPendingUpdate
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
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


@pytest.mark.asyncio()
async def test_connect(exec_client):
    exec_client.connect()
    await asyncio.sleep(0)
    assert isinstance(exec_client.exchange, SimulatedExchange)
    assert exec_client.is_connected


@pytest.mark.skip(reason="Sandbox WIP")
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
    _, submitted, _, accepted, _, filled, _ = events
    assert isinstance(submitted, OrderSubmitted)
    assert isinstance(accepted, OrderAccepted)
    assert isinstance(filled, OrderFilled)
    assert accepted.venue_order_id.value.startswith("SANDBOX-")


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
    initialised, submitted, _, accepted, pending_update, _, updated = events
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
    _, _, _, _, pending_cancel, _, cancelled = events
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
