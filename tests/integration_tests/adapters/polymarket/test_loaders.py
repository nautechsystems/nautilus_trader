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

import pkgutil
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import Mock

import msgspec.json
import pytest

from nautilus_trader.adapters.polymarket.common.parsing import parse_polymarket_instrument
from nautilus_trader.adapters.polymarket.loaders import PolymarketDataLoader
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide


@pytest.fixture
def markets_list_data():
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.http_responses",
        "markets_list.json",
    )
    assert data
    return msgspec.json.decode(data)


@pytest.fixture
def market_details_data():
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.http_responses",
        "market.json",
    )
    assert data
    return msgspec.json.decode(data)


@pytest.fixture
def orderbook_history_data():
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.http_responses",
        "orderbook_history.json",
    )
    assert data
    return msgspec.json.decode(data)


@pytest.fixture
def price_history_data():
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.http_responses",
        "price_history.json",
    )
    assert data
    return msgspec.json.decode(data)


@pytest.fixture
def test_instrument(market_details_data):
    token = market_details_data["tokens"][0]
    return parse_polymarket_instrument(
        market_info=market_details_data,
        token_id=token["token_id"],
        outcome=token["outcome"],
        ts_init=0,
    )


@pytest.fixture
def loader(test_instrument):
    return PolymarketDataLoader(test_instrument)


@pytest.mark.asyncio
async def test_fetch_markets(markets_list_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(markets_list_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)

    # Act
    markets = await PolymarketDataLoader.fetch_markets(
        limit=10,
        http_client=mock_http_client,
    )

    # Assert
    mock_http_client.get.assert_called_once()
    assert len(markets) == 3
    assert markets[0]["slug"] == "fed-rate-hike-in-2025"
    assert markets[0]["conditionId"] == "0x4319532e181605cb15b1bd677759a3bc7f7394b2fdf145195b700eeaedfd5221"


@pytest.mark.asyncio
async def test_find_market_by_slug(markets_list_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(markets_list_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)

    # Act
    market = await PolymarketDataLoader.find_market_by_slug(
        "btc-price-above-100k",
        http_client=mock_http_client,
    )

    # Assert
    assert market["slug"] == "btc-price-above-100k"
    assert market["conditionId"] == "0xabc123"
    assert market["active"] is True


@pytest.mark.asyncio
async def test_find_market_by_slug_not_found(markets_list_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(markets_list_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)

    # Act & Assert
    with pytest.raises(ValueError, match="not found in active markets"):
        await PolymarketDataLoader.find_market_by_slug(
            "nonexistent-market",
            http_client=mock_http_client,
        )


@pytest.mark.asyncio
async def test_fetch_market_details(market_details_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(market_details_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)
    condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"

    # Act
    details = await PolymarketDataLoader.fetch_market_details(
        condition_id,
        http_client=mock_http_client,
    )

    # Assert
    mock_http_client.get.assert_called_once()
    assert details["condition_id"] == condition_id
    assert details["question"] == "Will Donald Trump win the 2024 US Presidential Election?"
    assert len(details["tokens"]) == 2


@pytest.mark.asyncio
async def test_fetch_orderbook_history(test_instrument, orderbook_history_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(orderbook_history_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)

    token_id = "60487116984468020978247225474488676749601001829886755968952521846780452448915"
    start_ms = 1729000000000
    end_ms = 1729000180000

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act
    snapshots = await loader.fetch_orderbook_history(token_id, start_ms, end_ms)

    # Assert
    mock_http_client.get.assert_called_once()
    assert len(snapshots) == 3
    assert snapshots[0]["timestamp"] == 1729000000000
    assert len(snapshots[0]["bids"]) == 3
    assert len(snapshots[0]["asks"]) == 3


@pytest.mark.asyncio
async def test_fetch_orderbook_history_with_pagination(test_instrument):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    page1_data = {
        "snapshots": [{"timestamp": 1729000000000, "bids": [], "asks": []}],
        "pagination": {"has_more": True, "pagination_key": "key123"},
    }
    page2_data = {
        "snapshots": [{"timestamp": 1729000060000, "bids": [], "asks": []}],
        "pagination": {"has_more": False},
    }

    mock_response1 = Mock()
    mock_response1.status = 200
    mock_response1.body = msgspec.json.encode(page1_data)

    mock_response2 = Mock()
    mock_response2.status = 200
    mock_response2.body = msgspec.json.encode(page2_data)

    mock_http_client.get = AsyncMock(side_effect=[mock_response1, mock_response2])

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act
    snapshots = await loader.fetch_orderbook_history("token123", 1729000000000, 1729000120000)

    # Assert
    assert mock_http_client.get.call_count == 2
    assert len(snapshots) == 2


@pytest.mark.asyncio
async def test_fetch_price_history(test_instrument, price_history_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(price_history_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)

    token_id = "60487116984468020978247225474488676749601001829886755968952521846780452448915"
    start_time_ms = 1729000000000
    end_time_ms = 1729000600000

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act
    history = await loader.fetch_price_history(token_id, start_time_ms, end_time_ms)

    # Assert
    mock_http_client.get.assert_called_once()
    assert len(history) == 10
    assert history[0]["t"] == 1729000000
    assert history[0]["p"] == 0.51


def test_parse_orderbook_snapshots(loader, orderbook_history_data):
    # Arrange
    snapshots = orderbook_history_data["snapshots"]

    # Act
    deltas_list = loader.parse_orderbook_snapshots(snapshots)

    # Assert
    assert len(deltas_list) == 3
    for deltas in deltas_list:
        assert isinstance(deltas, OrderBookDeltas)
        assert deltas.instrument_id == loader.instrument.id
        # Each snapshot should have: 1 CLEAR + 3 bids + 3 asks = 7 deltas
        assert len(deltas.deltas) == 7


def test_parse_orderbook_snapshots_uses_instrument_precision(
    loader,
    orderbook_history_data,
):
    # Arrange
    snapshots = orderbook_history_data["snapshots"]

    # Act
    deltas_list = loader.parse_orderbook_snapshots(snapshots)

    # Assert
    first_deltas = deltas_list[0]
    # Skip CLEAR delta, check first ADD delta
    first_order_delta = first_deltas.deltas[1]

    assert first_order_delta.order.price.precision == loader.instrument.price_precision
    assert first_order_delta.order.size.precision == loader.instrument.size_precision


def test_parse_price_history(loader, price_history_data):
    # Arrange
    history = price_history_data["history"]

    # Act
    trades = loader.parse_price_history(history)

    # Assert
    assert len(trades) == 10
    for trade in trades:
        assert isinstance(trade, TradeTick)
        assert trade.instrument_id == loader.instrument.id


def test_parse_price_history_aggressor_side_logic(loader, price_history_data):
    # Arrange
    history = price_history_data["history"]

    # Act
    trades = loader.parse_price_history(history)

    # Assert
    # First trade should have NO_AGGRESSOR (no previous price)
    assert trades[0].aggressor_side == AggressorSide.NO_AGGRESSOR

    # Second trade: price went from 0.51 to 0.52 (up) -> BUYER
    assert trades[1].aggressor_side == AggressorSide.BUYER

    # Fourth trade: price went from 0.53 to 0.52 (down) -> SELLER
    assert trades[3].aggressor_side == AggressorSide.SELLER


def test_parse_price_history_uses_instrument_precision(
    loader,
    price_history_data,
):
    # Arrange
    history = price_history_data["history"]

    # Act
    trades = loader.parse_price_history(history)

    # Assert
    first_trade = trades[0]
    assert first_trade.price.precision == loader.instrument.price_precision
    assert first_trade.size.precision == loader.instrument.size_precision
