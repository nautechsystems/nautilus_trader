# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderPendingCancel
from nautilus_trader.model.events.order import OrderPendingUpdate
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


def _make_quote_tick(instrument):
    return QuoteTick(
        instrument_id=instrument.id,
        bid=Price.from_int(10),
        ask=Price.from_int(10),
        bid_size=Quantity.from_int(100),
        ask_size=Quantity.from_int(100),
        ts_init=0,
        ts_event=0,
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
    order = TestExecStubs.limit_order(instrument_id=instrument.id)

    # Act
    strategy.submit_order(order=order)
    exec_client.on_data(_make_quote_tick(instrument))

    # Assert
    _, submitted, _, accepted, _, filled, _ = events
    assert isinstance(submitted, OrderSubmitted)
    assert isinstance(accepted, OrderAccepted)
    assert isinstance(filled, OrderFilled)
    assert accepted.venue_order_id == VenueOrderId("SANDBOX-1-001")


@pytest.mark.asyncio()
async def test_modify_order_success(exec_client, strategy, instrument, events):
    # Arrange
    exec_client.connect()
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        price=Price.from_str("0.01"),
    )
    strategy.submit_order(order)
    exec_client.on_data(_make_quote_tick(instrument))

    # Act
    strategy.modify_order(
        order=order,
        price=Price.from_str("0.01"),
        quantity=Quantity.from_int(200),
    )
    exec_client.on_data(_make_quote_tick(instrument))

    # Assert
    initialised, submitted, _, accepted, pending_update, _, updated = events
    assert isinstance(pending_update, OrderPendingUpdate)
    assert isinstance(updated, OrderUpdated)
    assert updated.price == Price.from_str("0.01")


@pytest.mark.skip(reason="WIP and lets not use capfd for tests")
@pytest.mark.no_ci()  # Relies on capfd, which is unreliable on CI
@pytest.mark.asyncio()
async def test_modify_order_error_no_venue_id(exec_client, strategy, instrument, events, capfd):
    # Arrange
    exec_client.connect()
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        price=Price.from_str("0.01"),
    )
    strategy.submit_order(order)
    exec_client.on_data(_make_quote_tick(instrument))

    # Act
    command = TestCommandStubs.modify_order_command(
        instrument_id=order.instrument_id,
        client_order_id=ClientOrderId("NOT-AN-ID"),
        price=Price.from_str("0.01"),
        quantity=Quantity.from_int(200),
    )
    exec_client.modify_order(command)
    exec_client.on_data(_make_quote_tick(instrument))

    # Assert
    out, err = capfd.readouterr()
    assert "ClientOrderId('NOT-AN-ID') not found" in err


@pytest.mark.asyncio()
async def test_cancel_order_success(exec_client, cache, strategy, instrument, events):
    # Arrange
    exec_client.connect()
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        price=Price.from_str("0.01"),
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


@pytest.mark.skip(reason="WIP and lets not use capfd for tests")
@pytest.mark.no_ci()  # Relies on capfd, which is unreliable on CI
@pytest.mark.asyncio()
async def test_cancel_order_fail(exec_client, cache, strategy, instrument, events, capfd):
    # Arrange
    exec_client.connect()
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        price=Price.from_str("0.01"),
    )
    strategy.submit_order(order)

    # Act
    command = TestCommandStubs.cancel_order_command(
        instrument_id=order.instrument_id,
        client_order_id=ClientOrderId("111"),
    )
    exec_client.cancel_order(command)
    exec_client.on_data(_make_quote_tick(instrument))

    # Assert
    out, err = capfd.readouterr()
    assert (
        "Cannot apply event to any order: ClientOrderId('111') and VenueOrderId('1') not found in the cache."
        in err
    )
