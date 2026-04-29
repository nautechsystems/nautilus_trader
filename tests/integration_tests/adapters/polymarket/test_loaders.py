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

import pkgutil
from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import Mock
from unittest.mock import patch

import msgspec.json
import pytest

from nautilus_trader.adapters.polymarket.common.parsing import parse_polymarket_instrument
from nautilus_trader.adapters.polymarket.loaders import PolymarketDataLoader
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide


@pytest.fixture(autouse=True)
def patch_fetch_fee_schedules():
    """
    Patch the Gamma `fetch_fee_schedules` helper used by
    from_market_slug/from_event_slug.

    The slug/event response fixtures in this repo do not carry `feeSchedule`,
    so the loaders fall back to a Gamma lookup. This fixture stubs that lookup
    to return empty, keeping tests hermetic.

    """
    with patch(
        "nautilus_trader.adapters.polymarket.loaders.fetch_fee_schedules",
        new=AsyncMock(return_value={}),
    ) as mocked:
        yield mocked


@pytest.fixture
def markets_list_data():
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.http_responses",
        "markets_list.json",
    )
    assert data
    return msgspec.json.decode(data)


@pytest.fixture
def market_slug_data():
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.http_responses",
        "market_slug.json",
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
def trades_data():
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.http_responses",
        "trades.json",
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
def loader(test_instrument, market_details_data):
    return PolymarketDataLoader(
        test_instrument,
        token_id=market_details_data["tokens"][0]["token_id"],
        condition_id=market_details_data["condition_id"],
    )


@pytest.mark.asyncio
async def test_fetch_markets(test_instrument, markets_list_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(markets_list_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act
    markets = await loader.fetch_markets(limit=10)

    # Assert
    mock_http_client.get.assert_called_once()
    assert len(markets) == 3
    assert markets[0]["slug"] == "fed-rate-hike-in-2025"
    assert (
        markets[0]["conditionId"]
        == "0x4319532e181605cb15b1bd677759a3bc7f7394b2fdf145195b700eeaedfd5221"
    )


@pytest.mark.asyncio
async def test_find_market_by_slug(test_instrument, market_slug_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(market_slug_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act
    market = await loader.find_market_by_slug("kamala-harris-divorce-in-2025")

    # Assert
    mock_http_client.get.assert_called_once_with(
        url="https://gamma-api.polymarket.com/markets/slug/kamala-harris-divorce-in-2025",
    )
    assert market["slug"] == "kamala-harris-divorce-in-2025"
    assert (
        market["conditionId"]
        == "0x270d5aa3b23be0d4e713361d603b187dd1919c71c74226ad867699f33972c5f2"
    )
    assert market["active"] is True


@pytest.mark.asyncio
async def test_find_market_by_slug_not_found(test_instrument):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 404
    mock_response.body = b""
    mock_http_client.get = AsyncMock(return_value=mock_response)

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act & Assert
    with pytest.raises(ValueError, match="Market with slug 'nonexistent-market' not found"):
        await loader.find_market_by_slug("nonexistent-market")


@pytest.mark.asyncio
async def test_from_market_slug_uses_slug_endpoint(
    market_slug_data,
    market_details_data,
):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)

    slug_response = Mock()
    slug_response.status = 200
    slug_response.body = msgspec.json.encode(market_slug_data)

    details_response = Mock()
    details_response.status = 200
    details_response.body = msgspec.json.encode(market_details_data)

    mock_http_client.get = AsyncMock(side_effect=[slug_response, details_response])

    # Act
    loader = await PolymarketDataLoader.from_market_slug(
        "kamala-harris-divorce-in-2025",
        http_client=mock_http_client,
    )

    # Assert
    assert loader.token_id == market_details_data["tokens"][0]["token_id"]
    assert loader.condition_id == market_slug_data["conditionId"]
    assert mock_http_client.get.call_args_list[0].kwargs["url"] == (
        "https://gamma-api.polymarket.com/markets/slug/kamala-harris-divorce-in-2025"
    )
    assert mock_http_client.get.call_args_list[1].kwargs["url"] == (
        "https://clob.polymarket.com/markets/"
        "0x270d5aa3b23be0d4e713361d603b187dd1919c71c74226ad867699f33972c5f2"
    )


@pytest.mark.asyncio
async def test_from_market_slug_populates_taker_fee_from_gamma_payload(
    market_slug_data,
    market_details_data,
    patch_fetch_fee_schedules,
):
    """
    If the slug response already carries feeSchedule, the loader uses it directly and
    does not call fetch_fee_schedules.
    """
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    slug_with_schedule = {**market_slug_data, "feeSchedule": {"rate": 0.03}}

    slug_response = Mock()
    slug_response.status = 200
    slug_response.body = msgspec.json.encode(slug_with_schedule)

    details_response = Mock()
    details_response.status = 200
    details_response.body = msgspec.json.encode(market_details_data)

    mock_http_client.get = AsyncMock(side_effect=[slug_response, details_response])

    # Act
    loader = await PolymarketDataLoader.from_market_slug(
        "kamala-harris-divorce-in-2025",
        http_client=mock_http_client,
    )

    # Assert
    assert loader.instrument.taker_fee == Decimal("0.03")
    assert loader.instrument.maker_fee == Decimal(0)
    patch_fetch_fee_schedules.assert_not_awaited()


@pytest.mark.asyncio
async def test_from_market_slug_falls_back_to_fetch_fee_schedules(
    market_slug_data,
    market_details_data,
    patch_fetch_fee_schedules,
):
    """
    When the slug response lacks feeSchedule, the loader falls back to a
    fetch_fee_schedules lookup by condition ID.
    """
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)

    slug_response = Mock()
    slug_response.status = 200
    slug_response.body = msgspec.json.encode(market_slug_data)

    details_response = Mock()
    details_response.status = 200
    details_response.body = msgspec.json.encode(market_details_data)

    mock_http_client.get = AsyncMock(side_effect=[slug_response, details_response])
    patch_fetch_fee_schedules.return_value = {
        market_slug_data["conditionId"]: {"rate": 0.072},
    }

    # Act
    loader = await PolymarketDataLoader.from_market_slug(
        "kamala-harris-divorce-in-2025",
        http_client=mock_http_client,
    )

    # Assert
    assert loader.instrument.taker_fee == Decimal("0.072")
    assert loader.instrument.maker_fee == Decimal(0)
    patch_fetch_fee_schedules.assert_awaited_once()
    assert patch_fetch_fee_schedules.await_args.kwargs["condition_ids"] == [
        market_slug_data["conditionId"],
    ]


@pytest.mark.asyncio
async def test_query_market_by_slug(market_slug_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(market_slug_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)

    # Act - no loader instance needed
    market = await PolymarketDataLoader.query_market_by_slug(
        "kamala-harris-divorce-in-2025",
        http_client=mock_http_client,
    )

    # Assert
    mock_http_client.get.assert_called_once_with(
        url="https://gamma-api.polymarket.com/markets/slug/kamala-harris-divorce-in-2025",
    )
    assert market["slug"] == "kamala-harris-divorce-in-2025"
    assert market["conditionId"] == (
        "0x270d5aa3b23be0d4e713361d603b187dd1919c71c74226ad867699f33972c5f2"
    )


@pytest.mark.asyncio
async def test_query_market_details(market_details_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(market_details_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)
    condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"

    # Act - no loader instance needed
    details = await PolymarketDataLoader.query_market_details(
        condition_id,
        http_client=mock_http_client,
    )

    # Assert
    mock_http_client.get.assert_called_once()
    assert details["condition_id"] == condition_id
    assert details["question"] == "Will Donald Trump win the 2024 US Presidential Election?"


@pytest.mark.asyncio
async def test_query_event_by_slug(event_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode([event_data])
    mock_http_client.get = AsyncMock(return_value=mock_response)

    # Act - no loader instance needed
    event = await PolymarketDataLoader.query_event_by_slug(
        "highest-temperature-in-nyc-on-january-26",
        http_client=mock_http_client,
    )

    # Assert
    mock_http_client.get.assert_called_once_with(
        url="https://gamma-api.polymarket.com/events",
        params={"slug": "highest-temperature-in-nyc-on-january-26"},
    )
    assert event["slug"] == "highest-temperature-in-nyc-on-january-26"
    assert len(event["markets"]) == 3


@pytest.mark.asyncio
async def test_fetch_market_details(test_instrument, market_details_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(market_details_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)
    condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act
    details = await loader.fetch_market_details(condition_id)

    # Assert
    mock_http_client.get.assert_called_once()
    assert details["condition_id"] == condition_id
    assert details["question"] == "Will Donald Trump win the 2024 US Presidential Election?"
    assert len(details["tokens"]) == 2


@pytest.mark.asyncio
async def test_fetch_trades(test_instrument, trades_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(trades_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)

    condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act
    trades = await loader.fetch_trades(condition_id)

    # Assert
    mock_http_client.get.assert_called_once()
    assert len(trades) == 4
    assert trades[0]["side"] == "SELL"
    assert trades[0]["price"] == 0.998
    assert trades[0]["size"] == 5.4
    assert trades[0]["timestamp"] == 1729000180


@pytest.mark.asyncio
async def test_fetch_trades_with_pagination(test_instrument):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)

    page1_data = [
        {
            "side": "BUY",
            "asset": "token123",
            "conditionId": "0xcond",
            "size": 10.0,
            "price": 0.5,
            "timestamp": 1729000060,
            "transactionHash": "0xhash1",
            "outcome": "Yes",
            "outcomeIndex": 0,
        },
    ]
    page2_data = [
        {
            "side": "SELL",
            "asset": "token123",
            "conditionId": "0xcond",
            "size": 20.0,
            "price": 0.6,
            "timestamp": 1729000000,
            "transactionHash": "0xhash2",
            "outcome": "Yes",
            "outcomeIndex": 0,
        },
    ]

    mock_response1 = Mock()
    mock_response1.status = 200
    mock_response1.body = msgspec.json.encode(page1_data)

    mock_response2 = Mock()
    mock_response2.status = 200
    mock_response2.body = msgspec.json.encode(page2_data)

    # Third response is empty to stop pagination
    mock_response3 = Mock()
    mock_response3.status = 200
    mock_response3.body = msgspec.json.encode([])

    mock_http_client.get = AsyncMock(
        side_effect=[mock_response1, mock_response2, mock_response3],
    )

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act - use limit=1 to force pagination
    trades = await loader.fetch_trades("0xcond", limit=1)

    # Assert
    assert mock_http_client.get.call_count == 3
    assert len(trades) == 2


def test_parse_trades(loader, trades_data):
    # Act - pass unfiltered data (includes both Yes and No token trades)
    trades = loader.parse_trades(trades_data)

    # Assert - only Yes token trades parsed (No token filtered out)
    assert len(trades) == 3
    for trade in trades:
        assert isinstance(trade, TradeTick)
        assert trade.instrument_id == loader.instrument.id


def test_parse_trades_aggressor_side(loader, trades_data):
    # Act
    trades = loader.parse_trades(trades_data)

    # Assert
    assert trades[0].aggressor_side == AggressorSide.SELLER
    assert trades[1].aggressor_side == AggressorSide.BUYER
    assert trades[2].aggressor_side == AggressorSide.BUYER


def test_parse_trades_uses_instrument_precision(loader, trades_data):
    # Act
    trades = loader.parse_trades(trades_data)

    # Assert
    first_trade = trades[0]
    assert first_trade.price.precision == loader.instrument.price_precision
    assert first_trade.size.precision == loader.instrument.size_precision


def test_parse_trades_uses_transaction_hash_as_trade_id(loader, trades_data):
    # Act
    trades = loader.parse_trades(trades_data)

    # Assert - last 36 chars of tx hash used (TradeId max length)
    assert str(trades[0].trade_id) == trades_data[0]["transactionHash"][-36:]


@pytest.fixture
def event_data():
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.http_responses",
        "event.json",
    )
    assert data
    return msgspec.json.decode(data)


@pytest.fixture
def events_list_data():
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.http_responses",
        "events_list.json",
    )
    assert data
    return msgspec.json.decode(data)


@pytest.mark.asyncio
async def test_fetch_event_by_slug(test_instrument, event_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode([event_data])  # API returns array
    mock_http_client.get = AsyncMock(return_value=mock_response)

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act
    event = await loader.fetch_event_by_slug("highest-temperature-in-nyc-on-january-26")

    # Assert
    mock_http_client.get.assert_called_once_with(
        url="https://gamma-api.polymarket.com/events",
        params={"slug": "highest-temperature-in-nyc-on-january-26"},
    )
    assert event["slug"] == "highest-temperature-in-nyc-on-january-26"
    assert event["title"] == "Highest temperature in NYC on January 26?"
    assert len(event["markets"]) == 3


@pytest.mark.asyncio
async def test_fetch_event_by_slug_not_found(test_instrument):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode([])  # Empty array
    mock_http_client.get = AsyncMock(return_value=mock_response)

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act & Assert
    with pytest.raises(ValueError, match="Event with slug 'nonexistent-event' not found"):
        await loader.fetch_event_by_slug("nonexistent-event")


@pytest.mark.asyncio
async def test_fetch_events(test_instrument, events_list_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode(events_list_data)
    mock_http_client.get = AsyncMock(return_value=mock_response)

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act
    events = await loader.fetch_events(active=True, closed=False, limit=10)

    # Assert
    mock_http_client.get.assert_called_once()
    assert len(events) == 3
    assert events[0]["slug"] == "nba-mavericks-vs-grizzlies"
    assert events[1]["slug"] == "nfl-falcons-vs-panthers"


@pytest.mark.asyncio
async def test_get_event_markets(test_instrument, event_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)
    mock_response = Mock()
    mock_response.status = 200
    mock_response.body = msgspec.json.encode([event_data])
    mock_http_client.get = AsyncMock(return_value=mock_response)

    loader = PolymarketDataLoader(test_instrument, http_client=mock_http_client)

    # Act
    markets = await loader.get_event_markets("highest-temperature-in-nyc-on-january-26")

    # Assert
    assert len(markets) == 3
    assert (
        markets[0]["conditionId"]
        == "0xed7d522e06d2f1f9015a468884cfdb2be7e737a33f130c1237a40f18bc739267"
    )
    assert "25F or below" in markets[0]["question"]


@pytest.mark.asyncio
async def test_from_event_slug(event_data, market_details_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)

    event_response = Mock()
    event_response.status = 200
    event_response.body = msgspec.json.encode([event_data])  # API returns array

    details_response = Mock()
    details_response.status = 200
    details_response.body = msgspec.json.encode(market_details_data)

    # Event fetch + 3 market detail fetches (one per market in event)
    mock_http_client.get = AsyncMock(
        side_effect=[event_response, details_response, details_response, details_response],
    )

    # Act
    loaders = await PolymarketDataLoader.from_event_slug(
        "highest-temperature-in-nyc-on-january-26",
        http_client=mock_http_client,
    )

    # Assert
    assert len(loaders) == 3
    for loader in loaders:
        assert loader.token_id == market_details_data["tokens"][0]["token_id"]
        assert loader.condition_id is not None
        assert loader.instrument is not None

    # Verify API calls
    assert mock_http_client.get.call_count == 4  # 1 event + 3 market details
    # First call should be to events API
    assert mock_http_client.get.call_args_list[0].kwargs["url"] == (
        "https://gamma-api.polymarket.com/events"
    )
    # Subsequent calls should be to CLOB market details API
    for i in range(1, 4):
        assert (
            "https://clob.polymarket.com/markets/"
            in (mock_http_client.get.call_args_list[i].kwargs["url"])
        )


@pytest.mark.asyncio
async def test_from_event_slug_with_token_index(event_data, market_details_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)

    event_response = Mock()
    event_response.status = 200
    event_response.body = msgspec.json.encode([event_data])

    details_response = Mock()
    details_response.status = 200
    details_response.body = msgspec.json.encode(market_details_data)

    mock_http_client.get = AsyncMock(
        side_effect=[event_response, details_response, details_response, details_response],
    )

    # Act - request second token (No outcome)
    loaders = await PolymarketDataLoader.from_event_slug(
        "highest-temperature-in-nyc-on-january-26",
        token_index=1,
        http_client=mock_http_client,
    )

    # Assert
    assert len(loaders) == 3
    for loader in loaders:
        # Should use second token (No outcome)
        assert loader.token_id == market_details_data["tokens"][1]["token_id"]


@pytest.mark.asyncio
async def test_from_event_slug_token_index_out_of_range(event_data, market_details_data):
    # Arrange
    mock_http_client = MagicMock(spec=nautilus_pyo3.HttpClient)

    event_response = Mock()
    event_response.status = 200
    event_response.body = msgspec.json.encode([event_data])

    details_response = Mock()
    details_response.status = 200
    details_response.body = msgspec.json.encode(market_details_data)

    mock_http_client.get = AsyncMock(side_effect=[event_response, details_response])

    # Act & Assert
    with pytest.raises(ValueError, match="Token index 5 out of range"):
        await PolymarketDataLoader.from_event_slug(
            "highest-temperature-in-nyc-on-january-26",
            token_index=5,
            http_client=mock_http_client,
        )
