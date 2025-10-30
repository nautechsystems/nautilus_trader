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
from unittest.mock import Mock
from unittest.mock import patch

import msgspec
import pytest

from nautilus_trader.adapters.polymarket.common.parsing import parse_instrument
from nautilus_trader.adapters.polymarket.loaders import PolymarketDataLoader
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide


@pytest.fixture
def loader():
    return PolymarketDataLoader()


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
    return parse_instrument(
        market_info=market_details_data,
        token_id=token["token_id"],
        outcome=token["outcome"],
        ts_init=0,
    )


def test_fetch_markets(loader, markets_list_data):
    # Arrange
    mock_response = Mock()
    mock_response.json.return_value = markets_list_data
    mock_response.raise_for_status = Mock()

    # Act
    with patch("requests.get", return_value=mock_response) as mock_get:
        markets = loader.fetch_markets(limit=10)

    # Assert
    mock_get.assert_called_once()
    assert len(markets) == 3
    assert markets[0]["slug"] == "fed-rate-hike-in-2025"
    assert markets[0]["conditionId"] == "0x4319532e181605cb15b1bd677759a3bc7f7394b2fdf145195b700eeaedfd5221"


def test_find_market_by_slug(loader, markets_list_data):
    # Arrange
    mock_response = Mock()
    mock_response.json.return_value = markets_list_data
    mock_response.raise_for_status = Mock()

    # Act
    with patch("requests.get", return_value=mock_response):
        market = loader.find_market_by_slug("btc-price-above-100k")

    # Assert
    assert market["slug"] == "btc-price-above-100k"
    assert market["conditionId"] == "0xabc123"
    assert market["active"] is True


def test_find_market_by_slug_not_found(loader, markets_list_data):
    # Arrange
    mock_response = Mock()
    mock_response.json.return_value = markets_list_data
    mock_response.raise_for_status = Mock()

    # Act & Assert
    with patch("requests.get", return_value=mock_response):
        with pytest.raises(ValueError, match="not found in active markets"):
            loader.find_market_by_slug("nonexistent-market")


def test_fetch_market_details(loader, market_details_data):
    # Arrange
    mock_response = Mock()
    mock_response.json.return_value = market_details_data
    mock_response.raise_for_status = Mock()
    condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"

    # Act
    with patch("requests.get", return_value=mock_response) as mock_get:
        details = loader.fetch_market_details(condition_id)

    # Assert
    mock_get.assert_called_once()
    assert details["condition_id"] == condition_id
    assert details["question"] == "Will Donald Trump win the 2024 US Presidential Election?"
    assert len(details["tokens"]) == 2


def test_fetch_orderbook_history(loader, orderbook_history_data):
    # Arrange
    mock_response = Mock()
    mock_response.json.return_value = orderbook_history_data
    mock_response.raise_for_status = Mock()

    token_id = "60487116984468020978247225474488676749601001829886755968952521846780452448915"
    start_ms = 1729000000000
    end_ms = 1729000180000

    # Act
    with patch("requests.get", return_value=mock_response) as mock_get:
        snapshots = loader.fetch_orderbook_history(token_id, start_ms, end_ms)

    # Assert
    mock_get.assert_called_once()
    assert len(snapshots) == 3
    assert snapshots[0]["timestamp"] == 1729000000000
    assert len(snapshots[0]["bids"]) == 3
    assert len(snapshots[0]["asks"]) == 3


def test_fetch_orderbook_history_with_pagination(loader):
    # Arrange
    page1_data = {
        "snapshots": [{"timestamp": 1729000000000, "bids": [], "asks": []}],
        "pagination": {"has_more": True, "pagination_key": "key123"},
    }
    page2_data = {
        "snapshots": [{"timestamp": 1729000060000, "bids": [], "asks": []}],
        "pagination": {"has_more": False},
    }

    mock_response1 = Mock()
    mock_response1.json.return_value = page1_data
    mock_response1.raise_for_status = Mock()

    mock_response2 = Mock()
    mock_response2.json.return_value = page2_data
    mock_response2.raise_for_status = Mock()

    # Act
    with patch("requests.get", side_effect=[mock_response1, mock_response2]) as mock_get:
        snapshots = loader.fetch_orderbook_history("token123", 1729000000000, 1729000120000)

    # Assert
    assert mock_get.call_count == 2
    assert len(snapshots) == 2


def test_fetch_price_history(loader, price_history_data):
    # Arrange
    mock_response = Mock()
    mock_response.json.return_value = price_history_data
    mock_response.raise_for_status = Mock()

    token_id = "60487116984468020978247225474488676749601001829886755968952521846780452448915"
    start_s = 1729000000
    end_s = 1729000600

    # Act
    with patch("requests.get", return_value=mock_response) as mock_get:
        history = loader.fetch_price_history(token_id, start_s, end_s)

    # Assert
    mock_get.assert_called_once()
    assert len(history) == 10
    assert history[0]["t"] == 1729000000
    assert history[0]["p"] == 0.51


def test_parse_orderbook_snapshots(loader, orderbook_history_data, test_instrument):
    # Arrange
    snapshots = orderbook_history_data["snapshots"]

    # Act
    deltas_list = loader.parse_orderbook_snapshots(snapshots, test_instrument)

    # Assert
    assert len(deltas_list) == 3
    for deltas in deltas_list:
        assert isinstance(deltas, OrderBookDeltas)
        assert deltas.instrument_id == test_instrument.id
        # Each snapshot should have: 1 CLEAR + 3 bids + 3 asks = 7 deltas
        assert len(deltas.deltas) == 7


def test_parse_orderbook_snapshots_uses_instrument_precision(
    loader,
    orderbook_history_data,
    test_instrument,
):
    # Arrange
    snapshots = orderbook_history_data["snapshots"]

    # Act
    deltas_list = loader.parse_orderbook_snapshots(snapshots, test_instrument)

    # Assert
    first_deltas = deltas_list[0]
    # Skip CLEAR delta, check first ADD delta
    first_order_delta = first_deltas.deltas[1]

    assert first_order_delta.order.price.precision == test_instrument.price_precision
    assert first_order_delta.order.size.precision == test_instrument.size_precision


def test_parse_price_history(loader, price_history_data, test_instrument):
    # Arrange
    history = price_history_data["history"]

    # Act
    trades = loader.parse_price_history(history, test_instrument)

    # Assert
    assert len(trades) == 10
    for trade in trades:
        assert isinstance(trade, TradeTick)
        assert trade.instrument_id == test_instrument.id


def test_parse_price_history_aggressor_side_logic(loader, price_history_data, test_instrument):
    # Arrange
    history = price_history_data["history"]

    # Act
    trades = loader.parse_price_history(history, test_instrument)

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
    test_instrument,
):
    # Arrange
    history = price_history_data["history"]

    # Act
    trades = loader.parse_price_history(history, test_instrument)

    # Assert
    first_trade = trades[0]
    assert first_trade.price.precision == test_instrument.price_precision
    assert first_trade.size.precision == test_instrument.size_precision
