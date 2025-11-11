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

from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProvider
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
async def test_gamma_markets_filters_specific_token_ids(mock_clob_client, live_clock):
    """
    Test that Gamma API loader only loads explicitly requested token_ids.

    When requesting specific instruments like POLYMARKET-123.YES, it should only load
    that specific token, not both YES and NO tokens from the market.

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

        # Act
        await provider.load_ids_async([yes_instrument_id])

        # Assert: Only YES token should be loaded, not NO
        instruments = provider.list_all()
        assert len(instruments) == 1

        instrument = instruments[0]
        assert instrument.id == yes_instrument_id
        assert instrument.outcome == "Yes"


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
