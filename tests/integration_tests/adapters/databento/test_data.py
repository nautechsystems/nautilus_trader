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
from datetime import UTC
from datetime import datetime
from types import SimpleNamespace
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.databento.types import DatabentoImbalance
from nautilus_trader.adapters.databento.types import DatabentoStatistics
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestOrderBookDeltas
from nautilus_trader.data.messages import RequestOrderBookDepth
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3


ES_ID = nautilus_pyo3.InstrumentId.from_str("ESZ21.GLBX")
NQ_ID = nautilus_pyo3.InstrumentId.from_str("NQZ21.GLBX")
GLBX = Venue("GLBX")

ES_CY = InstrumentId.from_str("ESZ21.GLBX")
NQ_CY = InstrumentId.from_str("NQZ21.GLBX")


def _prepare_live_client(databento_client, instrument_provider):
    live_client = MagicMock()
    databento_client._live_clients["GLBX.MDP3"] = live_client
    databento_client._has_subscribed["GLBX.MDP3"] = True
    instrument_provider.find.return_value = SimpleNamespace(price_precision=5)
    return live_client


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
async def test_subscribe_imbalance_passes_price_precision(
    databento_client,
    instrument_provider,
):
    live_client = _prepare_live_client(databento_client, instrument_provider)
    data_type = DataType(DatabentoImbalance, metadata={"instrument_id": ES_CY})

    await databento_client._subscribe_imbalance(data_type)

    call_kwargs = live_client.subscribe.call_args.kwargs
    assert call_kwargs["schema"] == "imbalance"
    assert [str(instrument_id) for instrument_id in call_kwargs["instrument_ids"]] == [str(ES_ID)]
    assert call_kwargs["price_precisions"] == [5]


@pytest.mark.asyncio
async def test_subscribe_statistics_passes_price_precision(
    databento_client,
    instrument_provider,
):
    live_client = _prepare_live_client(databento_client, instrument_provider)
    data_type = DataType(DatabentoStatistics, metadata={"instrument_id": ES_CY})

    await databento_client._subscribe_statistics(data_type)

    call_kwargs = live_client.subscribe.call_args.kwargs
    assert call_kwargs["schema"] == "statistics"
    assert [str(instrument_id) for instrument_id in call_kwargs["instrument_ids"]] == [str(ES_ID)]
    assert call_kwargs["price_precisions"] == [5]


@pytest.mark.asyncio
async def test_subscribe_parent_symbols_passes_parent_stype(
    databento_client,
    instrument_provider,
):
    live_client = _prepare_live_client(databento_client, instrument_provider)
    parent_symbols = {
        InstrumentId.from_str("ES.FUT.GLBX"),
        InstrumentId.from_str("NQ.FUT.GLBX"),
    }

    await databento_client._subscribe_parent_symbols("GLBX.MDP3", parent_symbols)

    call_kwargs = live_client.subscribe.call_args.kwargs
    assert call_kwargs["schema"] == "definition"
    assert [str(instrument_id) for instrument_id in call_kwargs["instrument_ids"]] == [
        "ES.FUT.GLBX",
        "NQ.FUT.GLBX",
    ]
    assert call_kwargs["stype_in"] == "parent"


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas_passes_price_precisions(
    databento_client,
    instrument_provider,
):
    live_client = MagicMock()
    live_client.start = AsyncMock()
    databento_client._live_clients_mbo["GLBX.MDP3"] = live_client
    instrument_provider.find.return_value = SimpleNamespace(price_precision=5)

    await databento_client._subscribe_order_book_deltas_batch([ES_CY, NQ_CY])
    await asyncio.gather(*databento_client._live_client_futures)

    call_kwargs = live_client.subscribe.call_args.kwargs
    assert call_kwargs["schema"] == "mbo"
    assert [str(instrument_id) for instrument_id in call_kwargs["instrument_ids"]] == [
        str(ES_ID),
        str(NQ_ID),
    ]
    assert call_kwargs["price_precisions"] == [5, 5]


@pytest.mark.asyncio
async def test_subscribe_order_book_snapshots_passes_price_precisions(
    databento_client,
    instrument_provider,
):
    live_client = _prepare_live_client(databento_client, instrument_provider)
    databento_client._instrument_ids["GLBX.MDP3"].update([ES_CY, NQ_CY])
    command = SubscribeOrderBook(
        instrument_id=ES_CY,
        book_data_type=OrderBookDepth10,
        book_type=BookType.L2_MBP,
        depth=10,
        client_id=ClientId("DATABENTO"),
        venue=GLBX,
        command_id=UUID4(),
        ts_init=0,
        params={"instrument_ids": [ES_CY, NQ_CY]},
    )

    await databento_client._subscribe_order_book_snapshots(command)

    call_kwargs = live_client.subscribe.call_args.kwargs
    assert call_kwargs["schema"] == "mbp-10"
    assert [str(instrument_id) for instrument_id in call_kwargs["instrument_ids"]] == [
        str(ES_ID),
        str(NQ_ID),
    ]
    assert call_kwargs["price_precisions"] == [5, 5]


@pytest.mark.asyncio
async def test_subscribe_bars_passes_price_precisions(
    databento_client,
    instrument_provider,
):
    live_client = _prepare_live_client(databento_client, instrument_provider)
    es_bar_type = BarType(
        ES_CY,
        BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST),
    )
    nq_bar_type = BarType(
        NQ_CY,
        BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST),
    )
    command = SubscribeBars(
        bar_type=es_bar_type,
        client_id=ClientId("DATABENTO"),
        venue=GLBX,
        command_id=UUID4(),
        ts_init=0,
        params={"bar_types": [es_bar_type, nq_bar_type]},
    )

    await databento_client._subscribe_bars(command)

    call_kwargs = live_client.subscribe.call_args.kwargs
    assert call_kwargs["schema"] == "ohlcv-1m"
    assert [str(instrument_id) for instrument_id in call_kwargs["instrument_ids"]] == [
        str(ES_ID),
        str(NQ_ID),
    ]
    assert call_kwargs["price_precisions"] == [5, 5]


@pytest.mark.asyncio
async def test_request_quote_ticks_single_instrument(
    databento_client,
    mock_http_client,
    instrument_provider,
    data_responses,
):
    instrument_provider.find.return_value = SimpleNamespace(price_precision=5)

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
    assert call_kwargs["price_precision"] == 5


@pytest.mark.asyncio
async def test_request_quote_ticks_groups_by_price_precision(
    databento_client,
    mock_http_client,
    instrument_provider,
    data_responses,
):
    # Each entry consumed once during _seed_http_price_precisions and once
    # during _price_precision_groups, so values repeat per instrument.
    instrument_provider.find.side_effect = [
        SimpleNamespace(price_precision=2),
        SimpleNamespace(price_precision=5),
        SimpleNamespace(price_precision=2),
        SimpleNamespace(price_precision=5),
    ]
    mock_http_client.get_range_quotes = AsyncMock(
        side_effect=[
            [TestDataProviderPyo3.quote_tick(instrument_id=ES_ID, ts_event=2, ts_init=2)],
            [TestDataProviderPyo3.quote_tick(instrument_id=NQ_ID, ts_event=1, ts_init=1)],
        ],
    )

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

    assert mock_http_client.get_range_quotes.await_count == 2
    first_call = mock_http_client.get_range_quotes.await_args_list[0].kwargs
    second_call = mock_http_client.get_range_quotes.await_args_list[1].kwargs
    assert first_call["price_precision"] == 2
    assert [str(instrument_id) for instrument_id in first_call["instrument_ids"]] == [str(ES_ID)]
    assert second_call["price_precision"] == 5
    assert [str(instrument_id) for instrument_id in second_call["instrument_ids"]] == [
        str(NQ_ID),
    ]

    assert len(data_responses) == 1
    response = data_responses[0]
    assert [quote.ts_event for quote in response.data] == [1, 2]


@pytest.mark.asyncio
async def test_request_quote_ticks_uses_resolved_precision_when_one_instrument_missing(
    databento_client,
    mock_http_client,
    instrument_provider,
    data_responses,
):
    # Each entry consumed once during _seed_http_price_precisions and once
    # during _price_precision_groups, so values repeat per instrument.
    instrument_provider.find.side_effect = [
        None,
        SimpleNamespace(price_precision=5),
        None,
        SimpleNamespace(price_precision=5),
    ]
    mock_http_client.get_range_quotes = AsyncMock(
        side_effect=[
            [TestDataProviderPyo3.quote_tick(instrument_id=ES_ID)],
            [TestDataProviderPyo3.quote_tick(instrument_id=NQ_ID)],
        ],
    )

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

    assert mock_http_client.get_range_quotes.await_count == 2
    missing_call = mock_http_client.get_range_quotes.await_args_list[0].kwargs
    resolved_call = mock_http_client.get_range_quotes.await_args_list[1].kwargs
    assert "price_precision" not in missing_call
    assert [str(instrument_id) for instrument_id in missing_call["instrument_ids"]] == [str(ES_ID)]
    assert resolved_call["price_precision"] == 5
    assert [str(instrument_id) for instrument_id in resolved_call["instrument_ids"]] == [
        str(NQ_ID),
    ]

    assert len(data_responses) == 1
    assert len(data_responses[0].data) == 2


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
async def test_request_trade_ticks_single_instrument_passes_price_precision(
    databento_client,
    mock_http_client,
    instrument_provider,
    data_responses,
):
    instrument_provider.find.return_value = SimpleNamespace(price_precision=5)

    pyo3_trades = [
        TestDataProviderPyo3.trade_tick(instrument_id=ES_ID),
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
        params={},
    )

    await databento_client._request_trade_ticks(request)

    assert len(data_responses) == 1
    response = data_responses[0]
    assert response.data_type.type == TradeTick
    assert response.data_type.metadata["instrument_id"] == ES_CY
    assert response.correlation_id == request.id
    assert len(response.data) == 2

    call_kwargs = mock_http_client.get_range_trades.call_args.kwargs
    assert len(call_kwargs["instrument_ids"]) == 1
    assert str(call_kwargs["instrument_ids"][0]) == str(ES_ID)
    assert call_kwargs["dataset"] == "GLBX.MDP3"
    assert call_kwargs["price_precision"] == 5


@pytest.mark.asyncio
async def test_request_trade_ticks_groups_by_price_precision(
    databento_client,
    mock_http_client,
    instrument_provider,
    data_responses,
):
    # Each entry consumed once during _seed_http_price_precisions and once
    # during _price_precision_groups, so values repeat per instrument.
    instrument_provider.find.side_effect = [
        SimpleNamespace(price_precision=2),
        SimpleNamespace(price_precision=5),
        SimpleNamespace(price_precision=2),
        SimpleNamespace(price_precision=5),
    ]
    mock_http_client.get_range_trades = AsyncMock(
        side_effect=[
            [TestDataProviderPyo3.trade_tick(instrument_id=ES_ID, ts_event=2, ts_init=2)],
            [TestDataProviderPyo3.trade_tick(instrument_id=NQ_ID, ts_event=1, ts_init=1)],
        ],
    )

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

    assert mock_http_client.get_range_trades.await_count == 2
    first_call = mock_http_client.get_range_trades.await_args_list[0].kwargs
    second_call = mock_http_client.get_range_trades.await_args_list[1].kwargs
    assert first_call["price_precision"] == 2
    assert [str(instrument_id) for instrument_id in first_call["instrument_ids"]] == [str(ES_ID)]
    assert second_call["price_precision"] == 5
    assert [str(instrument_id) for instrument_id in second_call["instrument_ids"]] == [
        str(NQ_ID),
    ]

    assert len(data_responses) == 1
    response = data_responses[0]
    assert [trade.ts_event for trade in response.data] == [1, 2]


@pytest.mark.asyncio
async def test_request_trade_ticks_uses_resolved_precision_when_one_instrument_missing(
    databento_client,
    mock_http_client,
    instrument_provider,
    data_responses,
):
    # Each entry consumed once during _seed_http_price_precisions and once
    # during _price_precision_groups, so values repeat per instrument.
    instrument_provider.find.side_effect = [
        None,
        SimpleNamespace(price_precision=5),
        None,
        SimpleNamespace(price_precision=5),
    ]
    mock_http_client.get_range_trades = AsyncMock(
        side_effect=[
            [TestDataProviderPyo3.trade_tick(instrument_id=ES_ID)],
            [TestDataProviderPyo3.trade_tick(instrument_id=NQ_ID)],
        ],
    )

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

    assert mock_http_client.get_range_trades.await_count == 2
    missing_call = mock_http_client.get_range_trades.await_args_list[0].kwargs
    resolved_call = mock_http_client.get_range_trades.await_args_list[1].kwargs
    assert "price_precision" not in missing_call
    assert [str(instrument_id) for instrument_id in missing_call["instrument_ids"]] == [str(ES_ID)]
    assert resolved_call["price_precision"] == 5
    assert [str(instrument_id) for instrument_id in resolved_call["instrument_ids"]] == [
        str(NQ_ID),
    ]

    assert len(data_responses) == 1
    assert len(data_responses[0].data) == 2


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


def test_seed_http_price_precisions_calls_set_for_known_instruments(
    databento_client,
    mock_http_client,
    instrument_provider,
):
    instrument_provider.find.side_effect = [
        SimpleNamespace(price_precision=2),
        SimpleNamespace(price_precision=5),
    ]

    databento_client._seed_http_price_precisions([ES_CY, NQ_CY])

    assert mock_http_client.set_price_precision.call_args_list == [
        (("ESZ21", 2),),
        (("NQZ21", 5),),
    ]


def test_seed_http_price_precisions_skips_unknown_instruments(
    databento_client,
    mock_http_client,
    instrument_provider,
):
    instrument_provider.find.side_effect = [
        None,
        SimpleNamespace(price_precision=5),
    ]

    databento_client._seed_http_price_precisions([ES_CY, NQ_CY])

    assert mock_http_client.set_price_precision.call_args_list == [
        (("NQZ21", 5),),
    ]


@pytest.mark.asyncio
async def test_request_imbalance_seeds_http_client_before_call(
    databento_client,
    mock_http_client,
    instrument_provider,
):
    instrument_provider.find.return_value = SimpleNamespace(price_precision=5)
    mock_http_client.get_range_imbalance = AsyncMock(return_value=[])
    data_type = DataType(DatabentoImbalance, metadata={"instrument_id": ES_CY})

    await databento_client._request_imbalance(data_type, UUID4())

    mock_http_client.set_price_precision.assert_called_once_with("ESZ21", 5)
    mock_http_client.get_range_imbalance.assert_awaited_once()

    # set_price_precision must come before the HTTP call
    method_names = [call[0] for call in mock_http_client.method_calls]
    assert method_names.index("set_price_precision") < method_names.index(
        "get_range_imbalance",
    )


@pytest.mark.asyncio
async def test_request_statistics_seeds_http_client_before_call(
    databento_client,
    mock_http_client,
    instrument_provider,
):
    instrument_provider.find.return_value = SimpleNamespace(price_precision=5)
    mock_http_client.get_range_statistics = AsyncMock(return_value=[])
    data_type = DataType(DatabentoStatistics, metadata={"instrument_id": ES_CY})

    await databento_client._request_statistics(data_type, UUID4())

    mock_http_client.set_price_precision.assert_called_once_with("ESZ21", 5)
    mock_http_client.get_range_statistics.assert_awaited_once()

    method_names = [call[0] for call in mock_http_client.method_calls]
    assert method_names.index("set_price_precision") < method_names.index(
        "get_range_statistics",
    )


@pytest.mark.asyncio
async def test_request_order_book_depth_seeds_http_client_before_call(
    databento_client,
    mock_http_client,
    instrument_provider,
):
    instrument_provider.find.return_value = SimpleNamespace(price_precision=5)
    mock_http_client.get_order_book_depth10 = AsyncMock(return_value=[])

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
        params={},
    )

    await databento_client._request_order_book_depth(request)

    mock_http_client.set_price_precision.assert_called_once_with("ESZ21", 5)
    mock_http_client.get_order_book_depth10.assert_awaited_once()

    method_names = [call[0] for call in mock_http_client.method_calls]
    assert method_names.index("set_price_precision") < method_names.index(
        "get_order_book_depth10",
    )


@pytest.mark.asyncio
async def test_request_order_book_deltas_seeds_http_client_before_call(
    databento_client,
    mock_http_client,
    instrument_provider,
):
    instrument_provider.find.return_value = SimpleNamespace(price_precision=5)
    mock_http_client.get_range_order_book_deltas = AsyncMock(return_value=[])

    request = RequestOrderBookDeltas(
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

    await databento_client._request_order_book_deltas(request)

    mock_http_client.set_price_precision.assert_called_once_with("ESZ21", 5)
    mock_http_client.get_range_order_book_deltas.assert_awaited_once()

    method_names = [call[0] for call in mock_http_client.method_calls]
    assert method_names.index("set_price_precision") < method_names.index(
        "get_range_order_book_deltas",
    )


@pytest.mark.asyncio
async def test_request_bars_seeds_http_client_before_call(
    databento_client,
    mock_http_client,
    instrument_provider,
):
    instrument_provider.find.return_value = SimpleNamespace(price_precision=5)

    bar_type = BarType.from_str("ESZ21.GLBX-1-MINUTE-LAST-EXTERNAL")
    request = RequestBars(
        bar_type=bar_type,
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

    await databento_client._request_bars(request)

    mock_http_client.set_price_precision.assert_called_once_with("ESZ21", 5)
    mock_http_client.get_range_bars.assert_awaited_once()

    method_names = [call[0] for call in mock_http_client.method_calls]
    assert method_names.index("set_price_precision") < method_names.index(
        "get_range_bars",
    )
