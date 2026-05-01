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

from types import SimpleNamespace
from typing import cast
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.dydx.data import DydxDataClient
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId


class TestDydxDataClient:
    _http_client: MagicMock
    _bars_timestamp_on_close: bool
    _log: MagicMock
    _handle_data_response: MagicMock
    _handle_trade_ticks: MagicMock
    _handle_funding_rates: MagicMock
    _handle_bars: MagicMock

    async def _request_order_book_snapshot(self, request: SimpleNamespace) -> None:
        await DydxDataClient._request_order_book_snapshot(cast(DydxDataClient, self), request)

    async def _request_trade_ticks(self, request: SimpleNamespace) -> None:
        await DydxDataClient._request_trade_ticks(cast(DydxDataClient, self), request)

    async def _request_funding_rates(self, request: SimpleNamespace) -> None:
        await DydxDataClient._request_funding_rates(cast(DydxDataClient, self), request)

    async def _request_bars(self, request: SimpleNamespace) -> None:
        await DydxDataClient._request_bars(cast(DydxDataClient, self), request)


def make_data_client(http_client: MagicMock) -> TestDydxDataClient:
    client = TestDydxDataClient()
    client._http_client = http_client
    client._bars_timestamp_on_close = True
    client._log = MagicMock()
    client._handle_data_response = MagicMock()
    client._handle_trade_ticks = MagicMock()
    client._handle_funding_rates = MagicMock()
    client._handle_bars = MagicMock()
    return client


def make_request(**kwargs) -> SimpleNamespace:
    return SimpleNamespace(
        id=UUID4(),
        start=None,
        end=None,
        limit=0,
        params=None,
        data_type=MagicMock(),
        **kwargs,
    )


@pytest.mark.asyncio
async def test_request_order_book_snapshot_sends_empty_response_on_error() -> None:
    http_client = MagicMock()
    http_client.request_orderbook_snapshot = AsyncMock(side_effect=RuntimeError("boom"))
    client = make_data_client(http_client)
    instrument_id = InstrumentId.from_str("BTC-USD-PERP.DYDX")
    request = make_request(instrument_id=instrument_id)

    await client._request_order_book_snapshot(request)

    client._handle_data_response.assert_called_once_with(
        data_type=request.data_type,
        data=[],
        correlation_id=request.id,
        start=None,
        end=None,
        params=None,
    )


@pytest.mark.asyncio
async def test_request_trade_ticks_sends_empty_response_on_error() -> None:
    http_client = MagicMock()
    http_client.request_trade_ticks = AsyncMock(side_effect=RuntimeError("boom"))
    client = make_data_client(http_client)
    instrument_id = InstrumentId.from_str("BTC-USD-PERP.DYDX")
    request = make_request(instrument_id=instrument_id)

    await client._request_trade_ticks(request)

    client._handle_trade_ticks.assert_called_once_with(
        instrument_id,
        [],
        request.id,
        None,
        None,
        None,
    )


@pytest.mark.asyncio
async def test_request_funding_rates_sends_empty_response_on_error() -> None:
    http_client = MagicMock()
    http_client.request_funding_rates = AsyncMock(side_effect=RuntimeError("boom"))
    client = make_data_client(http_client)
    instrument_id = InstrumentId.from_str("BTC-USD-PERP.DYDX")
    request = make_request(instrument_id=instrument_id)

    await client._request_funding_rates(request)

    client._handle_funding_rates.assert_called_once_with(
        instrument_id,
        [],
        request.id,
        None,
        None,
        None,
    )


@pytest.mark.asyncio
async def test_request_bars_sends_empty_response_on_error() -> None:
    http_client = MagicMock()
    http_client.request_bars = AsyncMock(side_effect=RuntimeError("boom"))
    client = make_data_client(http_client)
    bar_type = BarType.from_str("BTC-USD-PERP.DYDX-1-MINUTE-LAST-EXTERNAL")
    request = make_request(bar_type=bar_type)

    await client._request_bars(request)

    client._handle_bars.assert_called_once_with(
        bar_type,
        [],
        request.id,
        None,
        None,
        None,
    )
