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

from datetime import UTC
from datetime import datetime
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestOrderBookDepth
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3


ES_ID = nautilus_pyo3.InstrumentId.from_str("ESZ21.GLBX")
NQ_ID = nautilus_pyo3.InstrumentId.from_str("NQZ21.GLBX")
GLBX = Venue("GLBX")

ES_CY = InstrumentId.from_str("ESZ21.GLBX")
NQ_CY = InstrumentId.from_str("NQZ21.GLBX")


def test_resolve_ids_single_instrument_from_command(databento_client):
    command = SimpleNamespace(
        instrument_id=ES_CY,
        params={},
    )

    result = databento_client._resolve_instrument_ids_and_dataset(command)

    assert result is not None
    instrument_ids, dataset = result
    assert instrument_ids == [ES_CY]
    assert dataset == "GLBX.MDP3"


def test_resolve_ids_batch_from_params(databento_client):
    command = SimpleNamespace(
        instrument_id=ES_CY,
        params={"instrument_ids": [ES_CY, NQ_CY]},
    )

    result = databento_client._resolve_instrument_ids_and_dataset(command)

    assert result is not None
    instrument_ids, dataset = result
    assert instrument_ids == [ES_CY, NQ_CY]
    assert dataset == "GLBX.MDP3"


def test_resolve_ids_multi_dataset_returns_none(databento_client, mock_loader):
    mock_loader.get_dataset_for_venue = lambda v: "GLBX.MDP3" if v == GLBX else "XNAS.ITCH"

    command = SimpleNamespace(
        instrument_id=ES_CY,
        params={
            "instrument_ids": [
                ES_CY,
                InstrumentId.from_str("AAPL.XNAS"),
            ],
        },
    )

    result = databento_client._resolve_instrument_ids_and_dataset(command)

    assert result is None


def test_resolve_ids_empty_list_falls_back_to_instrument_id(databento_client):
    """
    Empty list is falsy, so falls back to command.instrument_id.
    """
    command = SimpleNamespace(
        instrument_id=ES_CY,
        params={"instrument_ids": []},
    )

    result = databento_client._resolve_instrument_ids_and_dataset(command)

    assert result is not None
    instrument_ids, dataset = result
    assert instrument_ids == [ES_CY]
    assert dataset == "GLBX.MDP3"


def test_resolve_ids_duplicate_instrument_ids_preserved(databento_client):
    """
    Duplicates in params are passed through without deduplication.
    """
    command = SimpleNamespace(
        instrument_id=ES_CY,
        params={"instrument_ids": [ES_CY, ES_CY, NQ_CY]},
    )

    result = databento_client._resolve_instrument_ids_and_dataset(command)

    assert result is not None
    instrument_ids, dataset = result
    assert instrument_ids == [ES_CY, ES_CY, NQ_CY]
    assert dataset == "GLBX.MDP3"


@pytest.mark.asyncio
async def test_request_quote_ticks_single_instrument(
    databento_client,
    mock_http_client,
    data_responses,
):
    pyo3_quotes = [
        TestDataProviderPyo3.quote_tick(instrument_id=ES_ID),
        TestDataProviderPyo3.quote_tick(instrument_id=ES_ID),
    ]
    mock_http_client.get_range_quotes = AsyncMock(return_value=pyo3_quotes)

    request = RequestQuoteTicks(
        instrument_id=ES_CY,
        start=datetime(2024, 1, 1, tzinfo=UTC),
        end=datetime(2024, 1, 2, tzinfo=UTC),
        limit=0,
        client_id=ClientId("DATABENTO"),
        venue=GLBX,
        callback=None,
        request_id=UUID4(),
        ts_init=0,
        params={},
    )

    await databento_client._request_quote_ticks(request)

    assert len(data_responses) == 1
    response = data_responses[0]
    assert response.data_type.type == QuoteTick
    assert response.data_type.metadata["instrument_id"] == ES_CY
    assert response.correlation_id == request.id
    assert response.venue == GLBX
    assert response.start == request.start
    assert response.end == request.end
    assert len(response.data) == 2
    assert all(isinstance(q, QuoteTick) for q in response.data)
    assert all(q.instrument_id == ES_CY for q in response.data)

    call_kwargs = mock_http_client.get_range_quotes.call_args.kwargs
    assert len(call_kwargs["instrument_ids"]) == 1
    assert str(call_kwargs["instrument_ids"][0]) == str(ES_ID)
    assert call_kwargs["dataset"] == "GLBX.MDP3"
    assert call_kwargs["schema"] == "mbp-1"


@pytest.mark.asyncio
async def test_request_quote_ticks_multi_instrument_single_response(
    databento_client,
    mock_http_client,
    data_responses,
):
    """
    Multi-instrument batch emits a single response to preserve correlation_id callback
    delivery.
    """
    pyo3_quotes = [
        TestDataProviderPyo3.quote_tick(instrument_id=ES_ID),
        TestDataProviderPyo3.quote_tick(instrument_id=NQ_ID),
        TestDataProviderPyo3.quote_tick(instrument_id=ES_ID),
    ]
    mock_http_client.get_range_quotes = AsyncMock(return_value=pyo3_quotes)

    request = RequestQuoteTicks(
        instrument_id=ES_CY,
        start=datetime(2024, 1, 1, tzinfo=UTC),
        end=datetime(2024, 1, 2, tzinfo=UTC),
        limit=0,
        client_id=ClientId("DATABENTO"),
        venue=GLBX,
        callback=None,
        request_id=UUID4(),
        ts_init=0,
        params={"instrument_ids": [ES_CY, NQ_CY]},
    )

    await databento_client._request_quote_ticks(request)

    # Single response per request for correct callback delivery
    assert len(data_responses) == 1
    response = data_responses[0]
    assert response.data_type.type == QuoteTick
    assert response.data_type.metadata["instrument_id"] == ES_CY
    assert response.correlation_id == request.id
    assert response.start == request.start
    assert response.end == request.end
    assert len(response.data) == 3
    assert all(isinstance(q, QuoteTick) for q in response.data)

    instrument_ids = {q.instrument_id for q in response.data}
    assert instrument_ids == {ES_CY, NQ_CY}

    call_kwargs = mock_http_client.get_range_quotes.call_args.kwargs
    pyo3_ids = call_kwargs["instrument_ids"]
    assert len(pyo3_ids) == 2
    assert {str(pid) for pid in pyo3_ids} == {str(ES_ID), str(NQ_ID)}


@pytest.mark.asyncio
async def test_request_quote_ticks_empty_response(
    databento_client,
    mock_http_client,
    data_responses,
):
    mock_http_client.get_range_quotes = AsyncMock(return_value=[])

    request = RequestQuoteTicks(
        instrument_id=ES_CY,
        start=datetime(2024, 1, 1, tzinfo=UTC),
        end=datetime(2024, 1, 2, tzinfo=UTC),
        limit=0,
        client_id=ClientId("DATABENTO"),
        venue=GLBX,
        callback=None,
        request_id=UUID4(),
        ts_init=0,
        params={"instrument_ids": [ES_CY, NQ_CY]},
    )

    await databento_client._request_quote_ticks(request)

    # Empty result still emits one response with empty data
    assert len(data_responses) == 1
    response = data_responses[0]
    assert response.correlation_id == request.id
    assert len(response.data) == 0
    mock_http_client.get_range_quotes.assert_awaited_once()


@pytest.mark.asyncio
async def test_request_quote_ticks_partial_instruments(
    databento_client,
    mock_http_client,
    data_responses,
):
    """
    Only ES data returned despite requesting ES + NQ.
    """
    pyo3_quotes = [
        TestDataProviderPyo3.quote_tick(instrument_id=ES_ID),
        TestDataProviderPyo3.quote_tick(instrument_id=ES_ID),
    ]
    mock_http_client.get_range_quotes = AsyncMock(return_value=pyo3_quotes)

    request = RequestQuoteTicks(
        instrument_id=ES_CY,
        start=datetime(2024, 1, 1, tzinfo=UTC),
        end=datetime(2024, 1, 2, tzinfo=UTC),
        limit=0,
        client_id=ClientId("DATABENTO"),
        venue=GLBX,
        callback=None,
        request_id=UUID4(),
        ts_init=0,
        params={"instrument_ids": [ES_CY, NQ_CY]},
    )

    await databento_client._request_quote_ticks(request)

    assert len(data_responses) == 1
    response = data_responses[0]
    assert response.correlation_id == request.id
    assert len(response.data) == 2
    assert all(q.instrument_id == ES_CY for q in response.data)


@pytest.mark.asyncio
async def test_request_trade_ticks_multi_instrument_single_response(
    databento_client,
    mock_http_client,
    data_responses,
):
    pyo3_trades = [
        TestDataProviderPyo3.trade_tick(instrument_id=ES_ID),
        TestDataProviderPyo3.trade_tick(instrument_id=NQ_ID),
        TestDataProviderPyo3.trade_tick(instrument_id=ES_ID),
    ]
    mock_http_client.get_range_trades = AsyncMock(return_value=pyo3_trades)

    request = RequestTradeTicks(
        instrument_id=ES_CY,
        start=datetime(2024, 1, 1, tzinfo=UTC),
        end=datetime(2024, 1, 2, tzinfo=UTC),
        limit=0,
        client_id=ClientId("DATABENTO"),
        venue=GLBX,
        callback=None,
        request_id=UUID4(),
        ts_init=0,
        params={"instrument_ids": [ES_CY, NQ_CY]},
    )

    await databento_client._request_trade_ticks(request)

    assert len(data_responses) == 1
    response = data_responses[0]
    assert response.data_type.type == TradeTick
    assert response.data_type.metadata["instrument_id"] == ES_CY
    assert response.correlation_id == request.id
    assert response.start == request.start
    assert response.end == request.end
    assert len(response.data) == 3

    instrument_ids = {t.instrument_id for t in response.data}
    assert instrument_ids == {ES_CY, NQ_CY}

    call_kwargs = mock_http_client.get_range_trades.call_args.kwargs
    assert len(call_kwargs["instrument_ids"]) == 2


@pytest.mark.asyncio
async def test_request_bars_multi_bar_type_single_response(
    databento_client,
    mock_http_client,
    data_responses,
):
    es_bar_type = nautilus_pyo3.BarType(
        ES_ID,
        nautilus_pyo3.BarSpecification(
            1,
            nautilus_pyo3.BarAggregation("MINUTE"),
            nautilus_pyo3.PriceType("LAST"),
        ),
    )
    nq_bar_type = nautilus_pyo3.BarType(
        NQ_ID,
        nautilus_pyo3.BarSpecification(
            1,
            nautilus_pyo3.BarAggregation("MINUTE"),
            nautilus_pyo3.PriceType("LAST"),
        ),
    )

    pyo3_bars = [
        nautilus_pyo3.Bar(
            bar_type=es_bar_type,
            open=nautilus_pyo3.Price.from_str("4500.00"),
            high=nautilus_pyo3.Price.from_str("4510.00"),
            low=nautilus_pyo3.Price.from_str("4490.00"),
            close=nautilus_pyo3.Price.from_str("4505.00"),
            volume=nautilus_pyo3.Quantity.from_int(100),
            ts_event=0,
            ts_init=0,
        ),
        nautilus_pyo3.Bar(
            bar_type=es_bar_type,
            open=nautilus_pyo3.Price.from_str("4505.00"),
            high=nautilus_pyo3.Price.from_str("4515.00"),
            low=nautilus_pyo3.Price.from_str("4495.00"),
            close=nautilus_pyo3.Price.from_str("4510.00"),
            volume=nautilus_pyo3.Quantity.from_int(80),
            ts_event=1,
            ts_init=1,
        ),
        nautilus_pyo3.Bar(
            bar_type=nq_bar_type,
            open=nautilus_pyo3.Price.from_str("15500.00"),
            high=nautilus_pyo3.Price.from_str("15510.00"),
            low=nautilus_pyo3.Price.from_str("15490.00"),
            close=nautilus_pyo3.Price.from_str("15505.00"),
            volume=nautilus_pyo3.Quantity.from_int(50),
            ts_event=0,
            ts_init=0,
        ),
    ]
    mock_http_client.get_range_bars = AsyncMock(return_value=pyo3_bars)

    es_cy_bar_type = BarType.from_str("ESZ21.GLBX-1-MINUTE-LAST-EXTERNAL")
    nq_cy_bar_type = BarType.from_str("NQZ21.GLBX-1-MINUTE-LAST-EXTERNAL")

    request = RequestBars(
        bar_type=es_cy_bar_type,
        start=datetime(2024, 1, 1, tzinfo=UTC),
        end=datetime(2024, 1, 2, tzinfo=UTC),
        limit=0,
        client_id=ClientId("DATABENTO"),
        venue=GLBX,
        callback=None,
        request_id=UUID4(),
        ts_init=0,
        params={"bar_types": [es_cy_bar_type, nq_cy_bar_type]},
    )

    await databento_client._request_bars(request)

    assert len(data_responses) == 1
    response = data_responses[0]
    assert response.data_type.type == Bar
    assert response.data_type.metadata["bar_type"] == es_cy_bar_type
    assert response.correlation_id == request.id
    assert response.venue == GLBX
    assert response.start == request.start
    assert response.end == request.end
    assert len(response.data) == 3
    assert all(isinstance(b, Bar) for b in response.data)

    bar_types = {b.bar_type for b in response.data}
    assert bar_types == {es_cy_bar_type, nq_cy_bar_type}

    call_kwargs = mock_http_client.get_range_bars.call_args.kwargs
    assert len(call_kwargs["instrument_ids"]) == 2
    assert call_kwargs["dataset"] == "GLBX.MDP3"


@pytest.mark.asyncio
async def test_request_order_book_depth_multi_instrument_single_response(
    databento_client,
    mock_http_client,
    data_responses,
):
    pyo3_depths = [
        TestDataProviderPyo3.order_book_depth10(instrument_id=ES_ID),
        TestDataProviderPyo3.order_book_depth10(instrument_id=NQ_ID),
    ]
    mock_http_client.get_order_book_depth10 = AsyncMock(return_value=pyo3_depths)

    request = RequestOrderBookDepth(
        instrument_id=ES_CY,
        start=datetime(2024, 1, 1, tzinfo=UTC),
        end=datetime(2024, 1, 2, tzinfo=UTC),
        limit=0,
        depth=10,
        client_id=ClientId("DATABENTO"),
        venue=GLBX,
        callback=None,
        request_id=UUID4(),
        ts_init=0,
        params={"instrument_ids": [ES_CY, NQ_CY]},
    )

    await databento_client._request_order_book_depth(request)

    assert len(data_responses) == 1
    response = data_responses[0]
    assert response.data_type.type == OrderBookDepth10
    assert response.data_type.metadata["instrument_id"] == ES_CY
    assert response.correlation_id == request.id
    assert response.start == request.start
    assert response.end == request.end
    assert len(response.data) == 2

    instrument_ids = {d.instrument_id for d in response.data}
    assert instrument_ids == {ES_CY, NQ_CY}

    call_kwargs = mock_http_client.get_order_book_depth10.call_args.kwargs
    assert len(call_kwargs["instrument_ids"]) == 2
    assert call_kwargs["depth"] == 10
