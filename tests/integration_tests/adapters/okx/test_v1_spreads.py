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

import asyncio
from types import SimpleNamespace
from typing import cast
from unittest.mock import AsyncMock

import pytest

import nautilus_trader.adapters.okx.providers as okx_providers
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SPREAD_ID = InstrumentId.from_str("BCH-USDT_BCH-USDT-SWAP.OKX")


class FakeOKXHttpClient:
    def __init__(self) -> None:
        self.request_instruments_calls: list[tuple[OKXInstrumentType, str | None]] = []
        self.request_spread_instruments_calls = 0

    async def request_instruments(self, instrument_type, instrument_family):
        self.request_instruments_calls.append((instrument_type, instrument_family))
        return ["spot-instrument"], [("BTC-USDT", 1)]

    async def request_spread_instruments(self):
        self.request_spread_instruments_calls += 1
        return ["spread-instrument"]


def test_provider_load_all_loads_spreads_when_configured(monkeypatch) -> None:
    client = FakeOKXHttpClient()
    provider = OKXInstrumentProvider(
        client=cast(nautilus_pyo3.OKXHttpClient, client),
        instrument_types=(OKXInstrumentType.SPOT,),
        load_spreads=True,
    )
    monkeypatch.setattr(okx_providers, "instruments_from_pyo3", lambda instruments: [])

    asyncio.run(provider.load_all_async())

    assert client.request_instruments_calls == [(OKXInstrumentType.SPOT, None)]
    assert client.request_spread_instruments_calls == 1
    assert provider.instruments_pyo3() == ["spot-instrument", "spread-instrument"]


@pytest.mark.asyncio
async def test_submit_order_routes_spread_to_http(exec_client, monkeypatch) -> None:
    client = exec_client
    submit_order_http = AsyncMock()
    submit_order_websocket = AsyncMock()
    submit_algo_order_http = AsyncMock()
    monkeypatch.setattr(client, "_submit_order_http", submit_order_http)
    monkeypatch.setattr(client, "_submit_order_websocket", submit_order_websocket)
    monkeypatch.setattr(client, "_submit_algo_order_http", submit_algo_order_http)

    order = TestExecStubs.limit_order(
        instrument=SimpleNamespace(id=SPREAD_ID),
        order_side=OrderSide.BUY,
        client_order_id=ClientOrderId("O-SPRD-001"),
        price=Price.from_str("0.01"),
        quantity=Quantity.from_str("1"),
    )
    command = SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        position_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    await client._submit_order(command)

    submit_order_http.assert_awaited_once_with(command)
    submit_order_websocket.assert_not_called()
    submit_algo_order_http.assert_not_called()


@pytest.mark.asyncio
async def test_cancel_order_routes_spread_to_http(exec_client, monkeypatch) -> None:
    client = exec_client
    cancel_order_http = AsyncMock()
    monkeypatch.setattr(client, "_cancel_order_http", cancel_order_http)

    order = TestExecStubs.limit_order(
        instrument=SimpleNamespace(id=SPREAD_ID),
        order_side=OrderSide.BUY,
        client_order_id=ClientOrderId("O-SPRD-002"),
        price=Price.from_str("0.01"),
        quantity=Quantity.from_str("1"),
    )
    client._cache.add_order(order, None, None)
    command = CancelOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=SPREAD_ID,
        client_order_id=order.client_order_id,
        venue_order_id=None,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    await client._cancel_order(command)

    cancel_order_http.assert_awaited_once_with(command, order)


@pytest.mark.asyncio
async def test_cancel_all_orders_routes_spread_to_http(exec_client, monkeypatch) -> None:
    client = exec_client
    cancel_all_orders_http = AsyncMock()
    cancel_all_orders_mass_cancel = AsyncMock()
    cancel_all_orders_individually = AsyncMock()
    monkeypatch.setattr(client, "_cancel_all_orders_http", cancel_all_orders_http)
    monkeypatch.setattr(client, "_cancel_all_orders_mass_cancel", cancel_all_orders_mass_cancel)
    monkeypatch.setattr(client, "_cancel_all_orders_individually", cancel_all_orders_individually)

    command = CancelAllOrders(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=SPREAD_ID,
        order_side=OrderSide.NO_ORDER_SIDE,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
    )

    await client._cancel_all_orders(command)

    cancel_all_orders_http.assert_awaited_once_with(command)
    cancel_all_orders_mass_cancel.assert_not_called()
    cancel_all_orders_individually.assert_not_called()
