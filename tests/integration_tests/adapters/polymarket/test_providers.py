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

from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProvider
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProviderConfig
from nautilus_trader.common.component import LiveClock
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId


@pytest.fixture
def mock_clob_client():
    """
    Create a mock ClobClient for testing.

    Note: The ClobClient methods are synchronous and called via asyncio.to_thread,
    so we mock them as regular synchronous methods.

    """
    return MagicMock()


@pytest.fixture
def live_clock():
    """
    Create a LiveClock for testing.
    """
    return LiveClock()


@pytest.fixture
def instrument_provider(mock_clob_client, live_clock):
    """
    Create a PolymarketInstrumentProvider for testing.
    """
    return PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
    )


# Sample market data with different states
ACTIVE_OPEN_MARKET = {
    "enable_order_book": True,
    "active": True,
    "closed": False,
    "archived": False,
    "accepting_orders": True,
    "minimum_order_size": 5,
    "minimum_tick_size": 0.001,
    "condition_id": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
    "question_id": "0xe3b1bc389210504ebcb9cffe4b0ed06ccac50561e0f24abb6379984cec030f00",
    "question": "Will market A resolve Yes?",
    "market_slug": "market-a",
    "end_date_iso": "2025-12-31T00:00:00Z",
    "maker_base_fee": 0,
    "taker_base_fee": 0,
    "tokens": [
        {
            "token_id": "11111111111111111111111111111111111111111111111111111111111111111",
            "outcome": "Yes",
            "price": 0.5,
            "winner": False,
        },
        {
            "token_id": "22222222222222222222222222222222222222222222222222222222222222222",
            "outcome": "No",
            "price": 0.5,
            "winner": False,
        },
    ],
    "tags": ["Test"],
}

ACTIVE_CLOSED_MARKET = {
    "enable_order_book": True,
    "active": True,
    "closed": True,
    "archived": False,
    "accepting_orders": False,
    "minimum_order_size": 5,
    "minimum_tick_size": 0.001,
    "condition_id": "0xaa22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
    "question_id": "0xb3b1bc389210504ebcb9cffe4b0ed06ccac50561e0f24abb6379984cec030f00",
    "question": "Will market B resolve Yes?",
    "market_slug": "market-b",
    "end_date_iso": "2024-06-01T00:00:00Z",
    "maker_base_fee": 0,
    "taker_base_fee": 0,
    "tokens": [
        {
            "token_id": "33333333333333333333333333333333333333333333333333333333333333333",
            "outcome": "Yes",
            "price": 1.0,
            "winner": True,
        },
        {
            "token_id": "44444444444444444444444444444444444444444444444444444444444444444",
            "outcome": "No",
            "price": 0.0,
            "winner": False,
        },
    ],
    "tags": ["Test"],
}

INACTIVE_CLOSED_MARKET = {
    "enable_order_book": False,
    "active": False,
    "closed": True,
    "archived": False,
    "accepting_orders": False,
    "minimum_order_size": 5,
    "minimum_tick_size": 0.001,
    "condition_id": "0xcc22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
    "question_id": "0xc3b1bc389210504ebcb9cffe4b0ed06ccac50561e0f24abb6379984cec030f00",
    "question": "Will market C resolve Yes?",
    "market_slug": "market-c",
    "end_date_iso": "2024-01-01T00:00:00Z",
    "maker_base_fee": 0,
    "taker_base_fee": 0,
    "tokens": [
        {
            "token_id": "55555555555555555555555555555555555555555555555555555555555555555",
            "outcome": "Yes",
            "price": 0.0,
            "winner": False,
        },
        {
            "token_id": "66666666666666666666666666666666666666666666666666666666666666666",
            "outcome": "No",
            "price": 1.0,
            "winner": True,
        },
    ],
    "tags": ["Test"],
}

INACTIVE_OPEN_MARKET = {
    "enable_order_book": False,
    "active": False,
    "closed": False,
    "archived": False,
    "accepting_orders": False,
    "minimum_order_size": 5,
    "minimum_tick_size": 0.001,
    "condition_id": "0xbb22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
    "question_id": "0xd3b1bc389210504ebcb9cffe4b0ed06ccac50561e0f24abb6379984cec030f00",
    "question": "Will market D resolve Yes?",
    "market_slug": "market-d",
    "end_date_iso": "2025-06-01T00:00:00Z",
    "maker_base_fee": 0,
    "taker_base_fee": 0,
    "tokens": [
        {
            "token_id": "77777777777777777777777777777777777777777777777777777777777777777",
            "outcome": "Yes",
            "price": 0.5,
            "winner": False,
        },
        {
            "token_id": "88888888888888888888888888888888888888888888888888888888888888888",
            "outcome": "No",
            "price": 0.5,
            "winner": False,
        },
    ],
    "tags": ["Test"],
}


@pytest.mark.asyncio
async def test_load_markets_with_is_active_filter_excludes_closed_markets(
    instrument_provider,
    mock_clob_client,
):
    """
    Test that when is_active filter is True, only truly active markets are included.

    This is a regression test ensuring that markets are filtered by BOTH the
    'active' and 'closed' fields. Markets must have active=True AND closed=False
    to be included when is_active=True filter is used.

    Test cases:
    - active=True, closed=False: ✅ Include (truly active)
    - active=True, closed=True: ❌ Exclude (closed/disputed)
    - active=False, closed=False: ❌ Exclude (suspended/paused)
    - active=False, closed=True: ❌ Exclude (inactive and closed)

    """
    # Arrange: Mock get_markets to return markets with different states
    mock_clob_client.get_markets.return_value = {
        "data": [
            ACTIVE_OPEN_MARKET,
            ACTIVE_CLOSED_MARKET,
            INACTIVE_OPEN_MARKET,
            INACTIVE_CLOSED_MARKET,
        ],
        "next_cursor": "LTE=",
    }

    # Act: Load markets with is_active filter
    await instrument_provider._load_markets([], filters={"is_active": True})

    # Assert: Only the active open market should be loaded (2 instruments, one per token)
    instruments = instrument_provider.list_all()
    assert len(instruments) == 2  # Only 2 tokens from ACTIVE_OPEN_MARKET

    condition_ids = {instr.info["condition_id"] for instr in instruments}
    assert ACTIVE_OPEN_MARKET["condition_id"] in condition_ids
    assert ACTIVE_CLOSED_MARKET["condition_id"] not in condition_ids
    assert INACTIVE_OPEN_MARKET["condition_id"] not in condition_ids
    assert INACTIVE_CLOSED_MARKET["condition_id"] not in condition_ids


@pytest.mark.asyncio
async def test_load_markets_without_filter_includes_all_markets(
    instrument_provider,
    mock_clob_client,
):
    """
    Test that when no is_active filter is provided, all markets are loaded.
    """
    # Arrange
    mock_clob_client.get_markets.return_value = {
        "data": [
            ACTIVE_OPEN_MARKET,
            ACTIVE_CLOSED_MARKET,
            INACTIVE_OPEN_MARKET,
            INACTIVE_CLOSED_MARKET,
        ],
        "next_cursor": "LTE=",
    }

    # Act: Load markets without filter
    await instrument_provider._load_markets([], filters={})

    # Assert: All markets should be loaded (8 instruments total, 2 per market)
    instruments = instrument_provider.list_all()
    assert len(instruments) == 8

    condition_ids = {instr.info["condition_id"] for instr in instruments}
    assert ACTIVE_OPEN_MARKET["condition_id"] in condition_ids
    assert ACTIVE_CLOSED_MARKET["condition_id"] in condition_ids
    assert INACTIVE_OPEN_MARKET["condition_id"] in condition_ids
    assert INACTIVE_CLOSED_MARKET["condition_id"] in condition_ids


@pytest.mark.asyncio
async def test_load_markets_seq_with_is_active_filter_excludes_closed_markets(
    instrument_provider,
    mock_clob_client,
):
    """
    Test that _load_markets_seq correctly filters markets using both active and closed.

    This ensures both code paths (bulk load and sequential load) check both the 'active'
    and 'closed' fields when is_active=True.

    """
    # Arrange
    instrument_id = InstrumentId.from_str(
        f"{ACTIVE_CLOSED_MARKET['condition_id']}-"
        f"{ACTIVE_CLOSED_MARKET['tokens'][0]['token_id']}.POLYMARKET",
    )

    mock_clob_client.get_market.return_value = ACTIVE_CLOSED_MARKET

    # Act: Load specific instrument with is_active filter
    await instrument_provider._load_markets_seq([instrument_id], filters={"is_active": True})

    # Assert: The closed market should not be loaded
    instruments = instrument_provider.list_all()
    assert len(instruments) == 0


@pytest.mark.asyncio
async def test_load_markets_seq_without_filter_includes_closed_markets(
    instrument_provider,
    mock_clob_client,
):
    # Arrange
    instrument_id = InstrumentId.from_str(
        f"{ACTIVE_CLOSED_MARKET['condition_id']}-"
        f"{ACTIVE_CLOSED_MARKET['tokens'][0]['token_id']}.POLYMARKET",
    )

    mock_clob_client.get_market.return_value = ACTIVE_CLOSED_MARKET

    # Act: Load specific instrument without filter
    await instrument_provider._load_markets_seq([instrument_id], filters={})

    # Assert: The closed market should be loaded (2 instruments, one per token)
    instruments = instrument_provider.list_all()
    assert len(instruments) == 2

    condition_ids = {instr.info["condition_id"] for instr in instruments}
    assert ACTIVE_CLOSED_MARKET["condition_id"] in condition_ids


@pytest.mark.asyncio
async def test_gamma_markets_loads_all_sibling_tokens(mock_clob_client, live_clock):
    """
    Test that Gamma API loader loads all sibling tokens for a market.

    When requesting only the YES token, the loader must also load the NO token because
    WebSocket price_change messages include updates for all tokens in a market
    regardless of which was subscribed to.

    """
    # Arrange
    config = InstrumentProviderConfig(use_gamma_markets=True)
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    yes_instrument_id = InstrumentId.from_str(
        f"{ACTIVE_OPEN_MARKET['condition_id']}-"
        f"{ACTIVE_OPEN_MARKET['tokens'][0]['token_id']}.POLYMARKET",
    )
    no_instrument_id = InstrumentId.from_str(
        f"{ACTIVE_OPEN_MARKET['condition_id']}-"
        f"{ACTIVE_OPEN_MARKET['tokens'][1]['token_id']}.POLYMARKET",
    )

    gamma_market = {
        "conditionId": ACTIVE_OPEN_MARKET["condition_id"],
        "clobTokenIds": f'["{ACTIVE_OPEN_MARKET["tokens"][0]["token_id"]}", "{ACTIVE_OPEN_MARKET["tokens"][1]["token_id"]}"]',
        "outcomes": '["Yes", "No"]',
        "outcomePrices": '["0.5", "0.5"]',
        "question": ACTIVE_OPEN_MARKET["question"],
        "endDateIso": "2025-12-31",
        "orderPriceMinTickSize": 0.001,
        "orderMinSize": 5,
        "active": True,
        "closed": False,
        "enableOrderBook": True,
    }

    with patch("nautilus_trader.adapters.polymarket.providers.list_markets") as mock_list_markets:

        async def mock_async_list_markets(*args, **kwargs):
            return [gamma_market]

        mock_list_markets.side_effect = mock_async_list_markets

        # Act: Request only YES token
        await provider.load_ids_async([yes_instrument_id])

        # Assert: Both YES and NO tokens should be loaded
        instruments = provider.list_all()
        assert len(instruments) == 2

        instrument_ids = {instr.id for instr in instruments}
        assert yes_instrument_id in instrument_ids
        assert no_instrument_id in instrument_ids


@pytest.mark.asyncio
async def test_gamma_markets_deduplicates_condition_ids(mock_clob_client, live_clock):
    """
    Test that Gamma API loader deduplicates condition IDs before limit check.

    When loading both YES and NO tokens from the same markets (common case), condition
    IDs should be deduplicated so that 60 markets with 2 tokens each (120 instruments)
    uses the filtered query instead of bulk load.

    """
    # Arrange
    config = InstrumentProviderConfig(use_gamma_markets=True)
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    # Create 60 instrument pairs (both YES and NO tokens from same market)
    instrument_ids = []
    for i in range(60):
        condition_id = f"0x{'1' * 63}{i:x}"
        yes_token_id = f"1{i:063d}"
        no_token_id = f"2{i:063d}"

        instrument_ids.append(
            InstrumentId.from_str(f"{condition_id}-{yes_token_id}.POLYMARKET"),
        )
        instrument_ids.append(
            InstrumentId.from_str(f"{condition_id}-{no_token_id}.POLYMARKET"),
        )

    with patch("nautilus_trader.adapters.polymarket.providers.list_markets") as mock_list_markets:

        async def mock_async_list_markets(*args, **kwargs):
            return []

        mock_list_markets.side_effect = mock_async_list_markets

        # Act
        await provider.load_ids_async(instrument_ids)

        # Assert: Should use filtered query, not bulk load
        call_args = mock_list_markets.call_args
        filters = call_args[1]["filters"]

        # Verify condition_ids filter was applied (means we used targeted query)
        assert "condition_ids" in filters
        # Verify we deduplicated: 120 instruments -> 60 unique condition_ids
        assert len(filters["condition_ids"]) == 60


@pytest.mark.asyncio
async def test_load_all_async_uses_gamma_api_when_configured(mock_clob_client, live_clock):
    """
    Test that load_all_async uses Gamma API when use_gamma_markets=True.

    This ensures time-based filters like end_date_min/end_date_max are passed to the
    Gamma API for server-side filtering.

    """
    config = PolymarketInstrumentProviderConfig(use_gamma_markets=True)
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    gamma_market = {
        "conditionId": ACTIVE_OPEN_MARKET["condition_id"],
        "clobTokenIds": f'["{ACTIVE_OPEN_MARKET["tokens"][0]["token_id"]}", "{ACTIVE_OPEN_MARKET["tokens"][1]["token_id"]}"]',
        "outcomes": '["Yes", "No"]',
        "outcomePrices": '["0.5", "0.5"]',
        "question": ACTIVE_OPEN_MARKET["question"],
        "endDateIso": "2025-12-31",
        "orderPriceMinTickSize": 0.001,
        "orderMinSize": 5,
        "active": True,
        "closed": False,
        "enableOrderBook": True,
    }

    with patch("nautilus_trader.adapters.polymarket.providers.list_markets") as mock_list_markets:

        async def mock_async_list_markets(*args, **kwargs):
            return [gamma_market]

        mock_list_markets.side_effect = mock_async_list_markets

        filters = {
            "is_active": True,
            "end_date_min": "2025-01-24T18:00:00+00:00",
            "end_date_max": "2025-01-24T20:00:00+00:00",
        }

        # Act
        await provider.load_all_async(filters=filters)

        # Assert: Gamma API was called with filters
        mock_list_markets.assert_called_once()
        call_kwargs = mock_list_markets.call_args[1]
        assert call_kwargs["filters"]["is_active"] is True
        assert call_kwargs["filters"]["end_date_min"] == "2025-01-24T18:00:00+00:00"
        assert call_kwargs["filters"]["end_date_max"] == "2025-01-24T20:00:00+00:00"

        # Assert: Instruments were loaded
        instruments = provider.list_all()
        assert len(instruments) == 2


@pytest.mark.asyncio
async def test_load_all_async_uses_clob_api_when_gamma_not_configured(
    instrument_provider,
    mock_clob_client,
):
    """
    Test that load_all_async uses CLOB API when use_gamma_markets=False (default).
    """
    mock_clob_client.get_markets.return_value = {
        "data": [ACTIVE_OPEN_MARKET],
        "next_cursor": "LTE=",
    }

    # Act
    await instrument_provider.load_all_async(filters={"is_active": True})

    # Assert: CLOB API was called
    mock_clob_client.get_markets.assert_called()

    instruments = instrument_provider.list_all()
    assert len(instruments) == 2


# =====================================================================================
# event_slug_builder tests
# =====================================================================================

# Sample event data for event_slug_builder tests
SAMPLE_EVENT = {
    "id": "185377",
    "slug": "highest-temperature-in-nyc-on-january-26",
    "title": "Highest temperature in NYC on January 26?",
    "active": True,
    "closed": False,
    "markets": [
        {
            "conditionId": "0xed7d522e06d2f1f9015a468884cfdb2be7e737a33f130c1237a40f18bc739267",
            "question": "Will the highest temperature be 25F or below?",
            "slug": "highest-temperature-in-nyc-on-january-26-25forbelow",
            "clobTokenIds": '["111111111111111111111111111111111111111111111111111111111111111111", "222222222222222222222222222222222222222222222222222222222222222222"]',
            "outcomes": '["Yes", "No"]',
            "outcomePrices": '["0.3", "0.7"]',
            "endDateIso": "2025-01-27",
            "orderPriceMinTickSize": 0.001,
            "orderMinSize": 5,
            "active": True,
            "closed": False,
            "enableOrderBook": True,
        },
        {
            "conditionId": "0x35c268a9ba1b27115c9b42415a667177d9959310d2c9b3131bca9e7942a13f3a",
            "question": "Will the highest temperature be 26-27F?",
            "slug": "highest-temperature-in-nyc-on-january-26-26-27f",
            "clobTokenIds": '["333333333333333333333333333333333333333333333333333333333333333333", "444444444444444444444444444444444444444444444444444444444444444444"]',
            "outcomes": '["Yes", "No"]',
            "outcomePrices": '["0.5", "0.5"]',
            "endDateIso": "2025-01-27",
            "orderPriceMinTickSize": 0.001,
            "orderMinSize": 5,
            "active": True,
            "closed": False,
            "enableOrderBook": True,
        },
    ],
}


def sample_slug_builder() -> list[str]:
    """
    Sample slug builder for testing.
    """
    return ["highest-temperature-in-nyc-on-january-26"]


def multi_slug_builder() -> list[str]:
    """
    Sample slug builder that returns multiple slugs.
    """
    return [
        "highest-temperature-in-nyc-on-january-26",
        "highest-temperature-in-london-on-january-26",
    ]


def empty_slug_builder() -> list[str]:
    """
    Sample slug builder that returns empty list.
    """
    return []


@pytest.mark.asyncio
async def test_load_all_async_uses_event_slug_builder_when_configured(
    mock_clob_client,
    live_clock,
):
    """
    Test that load_all_async uses event_slug_builder when configured.

    The event_slug_builder should take priority over both use_gamma_markets and the
    default CLOB API path.

    """
    # Arrange
    config = PolymarketInstrumentProviderConfig(
        event_slug_builder="tests.integration_tests.adapters.polymarket.test_providers:sample_slug_builder",
    )
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    with patch(
        "nautilus_trader.adapters.polymarket.providers.PolymarketDataLoader._fetch_event_by_slug",
    ) as mock_fetch_event:
        mock_fetch_event.return_value = SAMPLE_EVENT

        # Act
        await provider.load_all_async()

        # Assert
        mock_fetch_event.assert_called_once()
        call_args = mock_fetch_event.call_args
        assert call_args[1]["slug"] == "highest-temperature-in-nyc-on-january-26"

        instruments = provider.list_all()
        assert len(instruments) == 4  # 2 markets x 2 tokens


@pytest.mark.asyncio
async def test_event_slug_builder_takes_priority_over_gamma_markets(
    mock_clob_client,
    live_clock,
):
    """
    Test that event_slug_builder takes priority over use_gamma_markets.
    """
    # Arrange
    config = PolymarketInstrumentProviderConfig(
        event_slug_builder="tests.integration_tests.adapters.polymarket.test_providers:sample_slug_builder",
        use_gamma_markets=True,  # This should be ignored
    )
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    with (
        patch(
            "nautilus_trader.adapters.polymarket.providers.PolymarketDataLoader._fetch_event_by_slug",
        ) as mock_fetch_event,
        patch(
            "nautilus_trader.adapters.polymarket.providers.list_markets",
        ) as mock_list_markets,
    ):
        mock_fetch_event.return_value = SAMPLE_EVENT

        # Act
        await provider.load_all_async()

        # Assert
        mock_fetch_event.assert_called_once()
        mock_list_markets.assert_not_called()


@pytest.mark.asyncio
async def test_event_slug_builder_handles_multiple_slugs(
    mock_clob_client,
    live_clock,
):
    """
    Test that event_slug_builder correctly handles multiple slugs.
    """
    # Arrange
    config = PolymarketInstrumentProviderConfig(
        event_slug_builder="tests.integration_tests.adapters.polymarket.test_providers:multi_slug_builder",
    )
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    second_event = {
        **SAMPLE_EVENT,
        "slug": "highest-temperature-in-london-on-january-26",
        "markets": [
            {
                **SAMPLE_EVENT["markets"][0],
                "conditionId": "0xaabbcc",
                "clobTokenIds": '["555555555555555555555555555555555555555555555555555555555555555555", "666666666666666666666666666666666666666666666666666666666666666666"]',
            },
        ],
    }

    with patch(
        "nautilus_trader.adapters.polymarket.providers.PolymarketDataLoader._fetch_event_by_slug",
    ) as mock_fetch_event:
        mock_fetch_event.side_effect = [SAMPLE_EVENT, second_event]

        # Act
        await provider.load_all_async()

        # Assert
        assert mock_fetch_event.call_count == 2
        instruments = provider.list_all()
        assert len(instruments) == 6  # 4 from first event + 2 from second


@pytest.mark.asyncio
async def test_event_slug_builder_handles_empty_slug_list(
    mock_clob_client,
    live_clock,
):
    """
    Test that event_slug_builder handles empty slug list gracefully.
    """
    # Arrange
    config = PolymarketInstrumentProviderConfig(
        event_slug_builder="tests.integration_tests.adapters.polymarket.test_providers:empty_slug_builder",
    )
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    with patch(
        "nautilus_trader.adapters.polymarket.providers.PolymarketDataLoader._fetch_event_by_slug",
    ) as mock_fetch_event:
        # Act
        await provider.load_all_async()

        # Assert
        mock_fetch_event.assert_not_called()
        instruments = provider.list_all()
        assert len(instruments) == 0


@pytest.mark.asyncio
async def test_event_slug_builder_continues_on_event_not_found(
    mock_clob_client,
    live_clock,
):
    """
    Test that event_slug_builder continues processing when an event is not found.
    """
    # Arrange
    config = PolymarketInstrumentProviderConfig(
        event_slug_builder="tests.integration_tests.adapters.polymarket.test_providers:multi_slug_builder",
    )
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    with patch(
        "nautilus_trader.adapters.polymarket.providers.PolymarketDataLoader._fetch_event_by_slug",
    ) as mock_fetch_event:
        mock_fetch_event.side_effect = [
            ValueError("Event not found"),
            SAMPLE_EVENT,
        ]

        # Act
        await provider.load_all_async()

        # Assert
        assert mock_fetch_event.call_count == 2
        instruments = provider.list_all()
        assert len(instruments) == 4  # Only from successful event


@pytest.mark.asyncio
async def test_event_slug_builder_skips_markets_without_condition_id(
    mock_clob_client,
    live_clock,
):
    """
    Test that event_slug_builder skips markets without conditionId.
    """
    # Arrange
    config = PolymarketInstrumentProviderConfig(
        event_slug_builder="tests.integration_tests.adapters.polymarket.test_providers:sample_slug_builder",
    )
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    event_with_bad_market = {
        **SAMPLE_EVENT,
        "markets": [
            SAMPLE_EVENT["markets"][0],  # Valid market
            {
                "question": "Bad market without conditionId",
                "slug": "bad-market",
            },  # Invalid market
        ],
    }

    with patch(
        "nautilus_trader.adapters.polymarket.providers.PolymarketDataLoader._fetch_event_by_slug",
    ) as mock_fetch_event:
        mock_fetch_event.return_value = event_with_bad_market

        # Act
        await provider.load_all_async()

        # Assert
        instruments = provider.list_all()
        assert len(instruments) == 2  # Only 1 valid market x 2 tokens


@pytest.mark.asyncio
async def test_event_slug_builder_invalid_module_path_raises(
    mock_clob_client,
    live_clock,
):
    """
    Test that an invalid module path in event_slug_builder raises ModuleNotFoundError.

    If a user misconfigures the path with a typo in the module name, the error should
    propagate immediately (fail fast).

    """
    # Arrange
    config = PolymarketInstrumentProviderConfig(
        event_slug_builder="nonexistent.module:build_slugs",
    )
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    # Act & Assert
    with pytest.raises(ModuleNotFoundError):
        await provider.load_all_async()


@pytest.mark.asyncio
async def test_event_slug_builder_invalid_function_path_raises(
    mock_clob_client,
    live_clock,
):
    """
    Test that an invalid function name in event_slug_builder raises AttributeError.

    If a user misconfigures the path with a typo in the function name, the error should
    propagate immediately (fail fast).

    """
    # Arrange
    config = PolymarketInstrumentProviderConfig(
        event_slug_builder="tests.integration_tests.adapters.polymarket.test_providers:nonexistent_function",
    )
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    # Act & Assert
    with pytest.raises(AttributeError):
        await provider.load_all_async()


@pytest.mark.asyncio
async def test_event_slug_builder_callable_raises_exception(
    mock_clob_client,
    live_clock,
):
    """
    Test that an exception raised by the slug builder callable propagates up.

    If the slug builder itself fails (e.g., network error fetching slug data), the error
    should propagate immediately rather than being silently swallowed.

    """
    # Arrange
    config = PolymarketInstrumentProviderConfig(
        event_slug_builder="tests.integration_tests.adapters.polymarket.test_providers:sample_slug_builder",
    )
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    with patch(
        "nautilus_trader.adapters.polymarket.providers.resolve_path",
    ) as mock_resolve:
        mock_resolve.return_value = MagicMock(side_effect=RuntimeError("Builder failed"))

        # Act & Assert
        with pytest.raises(RuntimeError, match="Builder failed"):
            await provider.load_all_async()


@pytest.mark.asyncio
async def test_event_slug_builder_handles_generic_exception_during_fetch(
    mock_clob_client,
    live_clock,
):
    """
    Test that a generic (non-ValueError) exception during event fetch is logged and
    skipped.

    Unlike ValueError (event not found), other exceptions like ConnectionError should be
    logged at error level with traceback but still allow processing of remaining slugs.

    """
    # Arrange
    config = PolymarketInstrumentProviderConfig(
        event_slug_builder="tests.integration_tests.adapters.polymarket.test_providers:multi_slug_builder",
    )
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    with patch(
        "nautilus_trader.adapters.polymarket.providers.PolymarketDataLoader._fetch_event_by_slug",
    ) as mock_fetch_event:
        mock_fetch_event.side_effect = [
            ConnectionError("Network timeout"),
            SAMPLE_EVENT,
        ]

        # Act
        await provider.load_all_async()

        # Assert: Second slug still processed despite first failing
        assert mock_fetch_event.call_count == 2
        instruments = provider.list_all()
        assert len(instruments) == 4  # Only from successful event


@pytest.mark.asyncio
async def test_event_slug_builder_skips_empty_token_id(
    mock_clob_client,
    live_clock,
):
    """
    Test that tokens with empty token_id are skipped with a warning.
    """
    # Arrange
    config = PolymarketInstrumentProviderConfig(
        event_slug_builder="tests.integration_tests.adapters.polymarket.test_providers:sample_slug_builder",
    )
    provider = PolymarketInstrumentProvider(
        client=mock_clob_client,
        clock=live_clock,
        config=config,
    )

    event_with_empty_token = {
        **SAMPLE_EVENT,
        "markets": [
            {
                **SAMPLE_EVENT["markets"][0],
                "clobTokenIds": '["", "222222222222222222222222222222222222222222222222222222222222222222"]',
            },
        ],
    }

    with patch(
        "nautilus_trader.adapters.polymarket.providers.PolymarketDataLoader._fetch_event_by_slug",
    ) as mock_fetch_event:
        mock_fetch_event.return_value = event_with_empty_token

        # Act
        await provider.load_all_async()

        # Assert: Only 1 valid token loaded (the one with non-empty token_id)
        instruments = provider.list_all()
        assert len(instruments) == 1
