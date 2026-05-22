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
"""
Regression tests for HistoricInteractiveBrokersClient.

The wrapper used to return the provider's entire cumulative instrument cache
on every call to ``request_instruments``. That leaked previously-loaded
instruments back into the return value and silently masked qualification
failures for the contracts a caller actually asked for.

"""

from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.historical import HistoricInteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.model.identifiers import InstrumentId


def _make_client_with_provider(instruments_cache, loaded_ids):
    """
    Build a HistoricInteractiveBrokersClient with mocked internals so the
    request_instruments method can be exercised without an IB connection.
    """
    client = HistoricInteractiveBrokersClient.__new__(HistoricInteractiveBrokersClient)
    provider = MagicMock(spec=InteractiveBrokersInstrumentProvider)
    provider._instruments = instruments_cache
    provider.load_ids_with_return_async = AsyncMock(return_value=loaded_ids)
    client._data_client = MagicMock()
    client._data_client.instrument_provider = provider
    return client, provider


@pytest.mark.asyncio
async def test_request_instruments_returns_only_just_loaded_instruments():
    """
    request_instruments should return only the instruments loaded by this call, not the
    cumulative provider cache from prior calls.
    """
    aapl_id = InstrumentId.from_str("AAPL.NASDAQ")
    spy_id = InstrumentId.from_str("SPY.ARCA")
    msft_id = InstrumentId.from_str("MSFT.NASDAQ")
    aapl = MagicMock(id=aapl_id)
    spy = MagicMock(id=spy_id)
    msft = MagicMock(id=msft_id)

    # Provider already has AAPL and MSFT from prior calls; SPY is the one
    # just loaded by the current request.
    client, provider = _make_client_with_provider(
        instruments_cache={aapl_id: aapl, spy_id: spy, msft_id: msft},
        loaded_ids=[spy_id],
    )

    instruments = await client.request_instruments(instrument_ids=["SPY.ARCA"])

    assert [inst.id for inst in instruments] == [spy_id]
    provider.load_ids_with_return_async.assert_awaited_once()


@pytest.mark.asyncio
async def test_request_instruments_returns_empty_when_qualification_fails():
    """
    When the provider cannot qualify the requested contract, the wrapper must return an
    empty list rather than leaking the prior cache.
    """
    aapl_id = InstrumentId.from_str("AAPL.NASDAQ")
    aapl = MagicMock(id=aapl_id)

    # Provider has AAPL cached from earlier; new request fails to load
    # anything (load_ids_with_return_async returns []).
    client, _ = _make_client_with_provider(
        instruments_cache={aapl_id: aapl},
        loaded_ids=[],
    )

    instruments = await client.request_instruments(
        contracts=[IBContract(secType="STK", symbol="DOESNTEXIST", exchange="SMART")],
    )

    assert instruments == []


@pytest.mark.asyncio
async def test_request_instruments_accepts_string_and_object_ids():
    """
    Both raw string IDs and InstrumentId objects should be passed through to the
    provider as InstrumentId instances.
    """
    aapl_id = InstrumentId.from_str("AAPL.NASDAQ")
    spy_id = InstrumentId.from_str("SPY.ARCA")
    aapl = MagicMock(id=aapl_id)
    spy = MagicMock(id=spy_id)

    client, provider = _make_client_with_provider(
        instruments_cache={aapl_id: aapl, spy_id: spy},
        loaded_ids=[aapl_id, spy_id],
    )

    instruments = await client.request_instruments(
        instrument_ids=["AAPL.NASDAQ", spy_id],
    )

    returned_ids = sorted(inst.id for inst in instruments)
    assert returned_ids == sorted([aapl_id, spy_id])
    call_args = provider.load_ids_with_return_async.await_args
    requested = call_args.args[0] if call_args.args else call_args.kwargs["instrument_ids"]
    assert all(isinstance(item, InstrumentId) for item in requested)
