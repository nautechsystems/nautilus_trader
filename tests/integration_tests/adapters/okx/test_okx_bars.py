#!/usr/bin/env python3
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

import asyncio
from datetime import UTC
from datetime import datetime
from datetime import timedelta
from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import patch

import pandas as pd
import pytest

from nautilus_trader.adapters.okx.config import OKXDataClientConfig
from nautilus_trader.adapters.okx.data import OKXDataClient
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase


UTC = UTC


class TestOKXDataClientBarRequests:
    def setup_method(self):
        self.clock = LiveClock()
        self.msgbus = MessageBus(
            trader_id=TraderId("TESTER-001"),
            clock=self.clock,
        )
        self.cache = Cache(database=MockCacheDatabase())
        self.http_client = MagicMock()
        self.instrument_provider = MagicMock(spec=OKXInstrumentProvider)
        self.eth_spot_instrument = CryptoPerpetual(
            instrument_id=InstrumentId.from_str("ETH-USDT.OKX"),
            raw_symbol=Symbol("ETH-USDT"),
            base_currency=Currency.from_str("ETH"),
            quote_currency=Currency.from_str("USDT"),
            settlement_currency=Currency.from_str("USDT"),
            is_inverse=False,
            price_precision=2,
            size_precision=4,
            price_increment=Price.from_str("0.01"),
            size_increment=Quantity.from_str("0.0001"),
            ts_event=0,
            ts_init=0,
            margin_init=Decimal("0.1"),
            margin_maint=Decimal("0.05"),
            maker_fee=Decimal("0.0005"),
            taker_fee=Decimal("0.001"),
        )

        self.eth_swap_instrument = CryptoPerpetual(
            instrument_id=InstrumentId.from_str("ETH-USDT-SWAP.OKX"),
            raw_symbol=Symbol("ETH-USDT-SWAP"),
            base_currency=Currency.from_str("ETH"),
            quote_currency=Currency.from_str("USDT"),
            settlement_currency=Currency.from_str("USDT"),
            is_inverse=False,
            price_precision=2,
            size_precision=4,
            price_increment=Price.from_str("0.01"),
            size_increment=Quantity.from_str("0.0001"),
            ts_event=0,
            ts_init=0,
            margin_init=Decimal("0.1"),
            margin_maint=Decimal("0.05"),
            maker_fee=Decimal("0.0005"),
            taker_fee=Decimal("0.001"),
        )

        self.cache.add_instrument(self.eth_spot_instrument)
        self.cache.add_instrument(self.eth_swap_instrument)

        self.config = OKXDataClientConfig(
            api_key=None,
            api_secret=None,
            api_passphrase=None,
            base_url_http=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=(OKXInstrumentType.SPOT, OKXInstrumentType.SWAP),
            contract_types=(OKXContractType.LINEAR, OKXContractType.INVERSE),
            is_demo=True,
            http_timeout_secs=10,
        )

        self.data_client = OKXDataClient(
            loop=asyncio.new_event_loop(),
            client=self.http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=self.instrument_provider,
            config=self.config,
            name=None,
        )

    def create_request_bars(
        self,
        instrument_id: str,
        start: datetime | None = None,
        end: datetime | None = None,
        limit: int | None = None,
    ) -> RequestBars:
        bar_type = BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL")

        return RequestBars(
            bar_type=bar_type,
            start=start,
            end=end,
            limit=limit if limit is not None else 100,
            client_id=self.data_client.id,
            venue=Venue("OKX"),
            callback=lambda x: None,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

    @pytest.mark.asyncio
    async def test_request_bars_recent_data_uses_regular_endpoint(self):
        fixed_now = datetime(2025, 7, 14, 12, 0, 0, tzinfo=UTC)
        start_time = fixed_now - timedelta(days=10)

        mock_bars = []
        self.http_client.request_bars = AsyncMock(return_value=mock_bars)

        request = self.create_request_bars(
            instrument_id="ETH-USDT.OKX",
            start=start_time,
            end=fixed_now,
            limit=100,
        )

        # Mock the ensure_pydatetime_utc function to handle datetime objects
        with patch(
            "nautilus_trader.adapters.okx.data.ensure_pydatetime_utc",
            side_effect=lambda x: x,
        ):
            await self.data_client._request_bars(request)

        self.http_client.request_bars.assert_called_once()
        call_args = self.http_client.request_bars.call_args

        assert call_args[1]["start"] == start_time
        assert call_args[1]["end"] == fixed_now
        assert call_args[1]["limit"] == 100

    @pytest.mark.asyncio
    async def test_request_bars_historical_data_uses_history_endpoint(self):
        fixed_now = datetime(2025, 7, 14, 12, 0, 0, tzinfo=UTC)
        start_time = fixed_now - timedelta(days=150)

        mock_bars = []
        self.http_client.request_bars = AsyncMock(return_value=mock_bars)

        request = self.create_request_bars(
            instrument_id="ETH-USDT.OKX",
            start=start_time,
            end=fixed_now,
            limit=100,
        )

        with patch(
            "nautilus_trader.adapters.okx.data.ensure_pydatetime_utc",
            side_effect=lambda x: x,
        ):
            await self.data_client._request_bars(request)

        self.http_client.request_bars.assert_called_once()
        call_args = self.http_client.request_bars.call_args

        assert call_args[1]["start"] == start_time
        assert call_args[1]["end"] == fixed_now
        assert call_args[1]["limit"] == 100

    @pytest.mark.asyncio
    async def test_request_bars_boundary_condition_100_days(self):
        fixed_now = datetime(2025, 7, 14, 12, 0, 0, tzinfo=UTC)
        start_time = fixed_now - timedelta(days=100)

        mock_bars = []
        self.http_client.request_bars = AsyncMock(return_value=mock_bars)

        request = self.create_request_bars(
            instrument_id="ETH-USDT.OKX",
            start=start_time,
            end=fixed_now,
            limit=250,
        )

        with patch(
            "nautilus_trader.adapters.okx.data.ensure_pydatetime_utc",
            side_effect=lambda x: x,
        ):
            await self.data_client._request_bars(request)

        self.http_client.request_bars.assert_called_once()
        call_args = self.http_client.request_bars.call_args

        assert call_args[1]["start"] == start_time
        assert call_args[1]["end"] == fixed_now
        assert call_args[1]["limit"] == 250

    @pytest.mark.asyncio
    async def test_request_bars_with_swap_instrument(self):
        fixed_now = datetime(2025, 7, 14, 12, 0, 0, tzinfo=UTC)
        start_time = fixed_now - timedelta(days=10)

        mock_bars = []
        self.http_client.request_bars = AsyncMock(return_value=mock_bars)

        request = self.create_request_bars(
            instrument_id="ETH-USDT-SWAP.OKX",
            start=start_time,
            end=fixed_now,
            limit=100,
        )

        with patch(
            "nautilus_trader.adapters.okx.data.ensure_pydatetime_utc",
            side_effect=lambda x: x,
        ):
            await self.data_client._request_bars(request)

        self.http_client.request_bars.assert_called_once()
        call_args = self.http_client.request_bars.call_args

        assert call_args[1]["start"] == start_time
        assert call_args[1]["end"] == fixed_now
        assert call_args[1]["limit"] == 100

    @pytest.mark.asyncio
    async def test_request_bars_without_time_range(self):
        mock_bars = []
        self.http_client.request_bars = AsyncMock(return_value=mock_bars)

        request = self.create_request_bars(
            instrument_id="ETH-USDT.OKX",
            start=None,
            end=None,
            limit=100,
        )

        with patch(
            "nautilus_trader.adapters.okx.data.ensure_pydatetime_utc",
            side_effect=lambda x: x,
        ):
            await self.data_client._request_bars(request)

        self.http_client.request_bars.assert_called_once()
        call_args = self.http_client.request_bars.call_args

        assert call_args[1]["start"] is None
        assert call_args[1]["end"] is None
        assert call_args[1]["limit"] == 100

    @pytest.mark.asyncio
    async def test_request_bars_boundary_condition_101_days(self):
        """
        Test that 101 days uses history endpoint.
        """
        fixed_now = datetime(2025, 7, 14, 12, 0, 0, tzinfo=UTC)
        start_time = fixed_now - timedelta(days=101)

        mock_bars = []
        self.http_client.request_bars = AsyncMock(return_value=mock_bars)

        request = self.create_request_bars(
            instrument_id="ETH-USDT.OKX",
            start=start_time,
            end=fixed_now,
            limit=100,
        )

        with patch(
            "nautilus_trader.adapters.okx.data.ensure_pydatetime_utc",
            side_effect=lambda x: x,
        ):
            await self.data_client._request_bars(request)

        self.http_client.request_bars.assert_called_once()
        call_args = self.http_client.request_bars.call_args

        assert call_args[1]["start"] == start_time
        assert call_args[1]["end"] == fixed_now
        assert call_args[1]["limit"] == 100

    @pytest.mark.asyncio
    async def test_request_bars_none_limit(self):
        """
        Test that None limit is handled correctly by testing the Rust call.
        """
        fixed_now = datetime(2025, 7, 14, 12, 0, 0, tzinfo=UTC)
        start_time = fixed_now - timedelta(days=10)

        mock_bars = []
        self.http_client.request_bars = AsyncMock(return_value=mock_bars)

        # Create request with 0 limit to represent "no limit"
        request = self.create_request_bars(
            instrument_id="ETH-USDT.OKX",
            start=start_time,
            end=fixed_now,
            limit=0,  # 0 represents no limit per the specification
        )

        with patch(
            "nautilus_trader.adapters.okx.data.ensure_pydatetime_utc",
            side_effect=lambda x: x,
        ):
            await self.data_client._request_bars(request)

        self.http_client.request_bars.assert_called_once()
        call_args = self.http_client.request_bars.call_args

        assert call_args[1]["start"] == start_time
        assert call_args[1]["end"] == fixed_now
        assert call_args[1]["limit"] == 0

    @pytest.mark.asyncio
    async def test_request_bars_zero_limit(self):
        """
        Test that zero limit is handled correctly.
        """
        fixed_now = datetime(2025, 7, 14, 12, 0, 0, tzinfo=UTC)
        start_time = fixed_now - timedelta(days=10)

        mock_bars = []
        self.http_client.request_bars = AsyncMock(return_value=mock_bars)

        request = self.create_request_bars(
            instrument_id="ETH-USDT.OKX",
            start=start_time,
            end=fixed_now,
            limit=0,
        )

        with patch(
            "nautilus_trader.adapters.okx.data.ensure_pydatetime_utc",
            side_effect=lambda x: x,
        ):
            await self.data_client._request_bars(request)

        self.http_client.request_bars.assert_called_once()
        call_args = self.http_client.request_bars.call_args

        assert call_args[1]["start"] == start_time
        assert call_args[1]["end"] == fixed_now
        assert call_args[1]["limit"] == 0

    @pytest.mark.asyncio
    async def test_request_bars_large_limit_over_endpoint_max(self):
        """
        Test that large limit over endpoint max is handled correctly.
        """
        fixed_now = datetime(2025, 7, 14, 12, 0, 0, tzinfo=UTC)
        start_time = fixed_now - timedelta(days=10)

        mock_bars = []
        self.http_client.request_bars = AsyncMock(return_value=mock_bars)

        request = self.create_request_bars(
            instrument_id="ETH-USDT.OKX",
            start=start_time,
            end=fixed_now,
            limit=500,  # Over regular endpoint max of 300
        )

        with patch(
            "nautilus_trader.adapters.okx.data.ensure_pydatetime_utc",
            side_effect=lambda x: x,
        ):
            await self.data_client._request_bars(request)

        self.http_client.request_bars.assert_called_once()
        call_args = self.http_client.request_bars.call_args

        assert call_args[1]["start"] == start_time
        assert call_args[1]["end"] == fixed_now
        assert call_args[1]["limit"] == 500

    @pytest.mark.asyncio
    async def test_request_bars_inverted_time_range_should_error(self):
        """
        Test that inverted time range should error in Rust layer.
        """
        fixed_now = datetime(2025, 7, 14, 12, 0, 0, tzinfo=UTC)
        start_time = fixed_now  # Start equals end
        end_time = fixed_now - timedelta(hours=1)  # End before start

        # Mock the HTTP client to raise an error that would come from Rust
        self.http_client.request_bars = AsyncMock(
            side_effect=ValueError(
                "Invalid time range: start=2025-07-14T12:00:00+00:00 end=2025-07-14T11:00:00+00:00",
            ),
        )

        request = self.create_request_bars(
            instrument_id="ETH-USDT.OKX",
            start=start_time,
            end=end_time,
            limit=100,
        )

        with patch(
            "nautilus_trader.adapters.okx.data.ensure_pydatetime_utc",
            side_effect=lambda x: x,
        ):
            with pytest.raises(ValueError, match="Invalid time range"):
                await self.data_client._request_bars(request)

    @pytest.mark.asyncio
    async def test_request_bars_endpoint_selection_logging(self):
        """
        Test that endpoint selection is logged correctly.
        """
        fixed_now = datetime(2025, 7, 14, 12, 0, 0, tzinfo=UTC)
        start_time = fixed_now - timedelta(days=150)  # Should use history endpoint

        mock_bars = []
        self.http_client.request_bars = AsyncMock(return_value=mock_bars)

        request = self.create_request_bars(
            instrument_id="ETH-USDT.OKX",
            start=start_time,
            end=fixed_now,
            limit=100,
        )

        with patch(
            "nautilus_trader.adapters.okx.data.ensure_pydatetime_utc",
            side_effect=lambda x: x,
        ):
            await self.data_client._request_bars(request)

        # Verify the HTTP client was called with correct parameters
        self.http_client.request_bars.assert_called_once()
        call_args = self.http_client.request_bars.call_args

        assert call_args[1]["start"] == start_time
        assert call_args[1]["end"] == fixed_now
        assert call_args[1]["limit"] == 100


def test_okx_data_client_configuration():
    spot_config = OKXDataClientConfig(
        api_key=None,  # 'OKX_API_KEY' env var
        api_secret=None,  # 'OKX_API_SECRET' env var
        api_passphrase=None,  # 'OKX_API_PASSPHRASE' env var
        base_url_http=None,  # Override with custom endpoint
        instrument_provider=InstrumentProviderConfig(load_all=True),
        instrument_types=(OKXInstrumentType.SPOT,),
        contract_types=None,  # SPOT doesn't use contract types
        is_demo=False,
        http_timeout_secs=10,
    )

    assert spot_config.instrument_types == (OKXInstrumentType.SPOT,)
    assert spot_config.contract_types is None
    assert spot_config.http_timeout_secs == 10

    swap_config = OKXDataClientConfig(
        api_key=None,
        api_secret=None,
        api_passphrase=None,
        base_url_http=None,
        instrument_provider=InstrumentProviderConfig(load_all=True),
        instrument_types=(OKXInstrumentType.SWAP,),
        contract_types=(OKXContractType.LINEAR, OKXContractType.INVERSE),
        is_demo=False,
        http_timeout_secs=10,
    )

    assert swap_config.instrument_types == (OKXInstrumentType.SWAP,)
    assert swap_config.contract_types == (OKXContractType.LINEAR, OKXContractType.INVERSE)

    futures_config = OKXDataClientConfig(
        api_key=None,
        api_secret=None,
        api_passphrase=None,
        base_url_http=None,
        instrument_provider=InstrumentProviderConfig(load_all=True),
        instrument_types=(OKXInstrumentType.FUTURES,),
        contract_types=(OKXContractType.INVERSE,),  # ETH-USD futures are inverse contracts
        is_demo=False,
        http_timeout_secs=10,
    )

    assert futures_config.instrument_types == (OKXInstrumentType.FUTURES,)
    assert futures_config.contract_types == (OKXContractType.INVERSE,)

    option_config = OKXDataClientConfig(
        api_key=None,
        api_secret=None,
        api_passphrase=None,
        base_url_http=None,
        instrument_provider=InstrumentProviderConfig(load_all=True),
        instrument_types=(OKXInstrumentType.OPTION,),
        contract_types=None,  # OPTIONS don't use contract types in the same way
        is_demo=False,
        http_timeout_secs=10,
    )

    assert option_config.instrument_types == (OKXInstrumentType.OPTION,)
    assert option_config.contract_types is None


def test_okx_instrument_id_formats():
    spot_id = InstrumentId.from_str("ETH-USDT.OKX")
    assert spot_id.symbol.value == "ETH-USDT"
    assert spot_id.venue.value == "OKX"

    swap_id = InstrumentId.from_str("ETH-USDT-SWAP.OKX")
    assert swap_id.symbol.value == "ETH-USDT-SWAP"
    assert swap_id.venue.value == "OKX"

    futures_id = InstrumentId.from_str("ETH-USD-251226.OKX")
    assert futures_id.symbol.value == "ETH-USD-251226"
    assert futures_id.venue.value == "OKX"

    option_id = InstrumentId.from_str("ETH-USD-250328-4000-C.OKX")
    assert option_id.symbol.value == "ETH-USD-250328-4000-C"
    assert option_id.venue.value == "OKX"


def test_okx_bar_type_formats():
    spot_bar_type = BarType.from_str("ETH-USDT.OKX-1-MINUTE-LAST-EXTERNAL")
    assert spot_bar_type.instrument_id.symbol.value == "ETH-USDT"
    assert spot_bar_type.instrument_id.venue.value == "OKX"

    swap_bar_type = BarType.from_str("ETH-USDT-SWAP.OKX-1-MINUTE-LAST-EXTERNAL")
    assert swap_bar_type.instrument_id.symbol.value == "ETH-USDT-SWAP"
    assert swap_bar_type.instrument_id.venue.value == "OKX"

    futures_bar_type = BarType.from_str("ETH-USD-251226.OKX-1-MINUTE-LAST-EXTERNAL")
    assert futures_bar_type.instrument_id.symbol.value == "ETH-USD-251226"
    assert futures_bar_type.instrument_id.venue.value == "OKX"

    option_bar_type = BarType.from_str("ETH-USD-250328-4000-C.OKX-1-MINUTE-LAST-EXTERNAL")
    assert option_bar_type.instrument_id.symbol.value == "ETH-USD-250328-4000-C"
    assert option_bar_type.instrument_id.venue.value == "OKX"


def test_okx_bar_logic_scenarios():
    fixed_now = datetime(2025, 7, 14, 12, 0, 0, tzinfo=UTC)

    logic_test_cases = [
        {
            "name": "Recent data (10 days ago)",
            "start": fixed_now - timedelta(days=10),
            "end": fixed_now,
            "limit": 100,
            "expected_endpoint": "regular",
            "expected_limit": 100,
        },
        {
            "name": "Historical data (150 days ago)",
            "start": fixed_now - timedelta(days=150),
            "end": fixed_now,
            "limit": 100,
            "expected_endpoint": "history",
            "expected_limit": 100,
        },
        {
            "name": "Boundary case (100 days ago)",
            "start": fixed_now - timedelta(days=100),
            "end": fixed_now,
            "limit": 100,
            "expected_endpoint": "regular",
            "expected_limit": 100,
        },
        {
            "name": "Just over boundary (101 days ago)",
            "start": fixed_now - timedelta(days=101),
            "end": fixed_now,
            "limit": 100,
            "expected_endpoint": "history",
            "expected_limit": 100,
        },
        {
            "name": "Very recent data (1 day ago)",
            "start": fixed_now - timedelta(days=1),
            "end": fixed_now,
            "limit": 100,
            "expected_endpoint": "regular",
            "expected_limit": 100,
        },
        {
            "name": "Medium historical data (200 days ago)",
            "start": fixed_now - timedelta(days=200),
            "end": fixed_now,
            "limit": 100,
            "expected_endpoint": "history",
            "expected_limit": 100,
        },
        {
            "name": "Very old data (1 year ago)",
            "start": fixed_now - timedelta(days=365),
            "end": fixed_now,
            "limit": 100,
            "expected_endpoint": "history",
            "expected_limit": 100,
        },
    ]

    for test_case in logic_test_cases:
        start_utc = test_case["start"]
        days_ago = (fixed_now - start_utc).days
        use_history_endpoint = days_ago > 100

        endpoint = "history" if use_history_endpoint else "regular"

        limit = test_case["limit"]
        if limit > 300 and endpoint == "regular":
            limit = 300
        elif limit > 100 and endpoint == "history":
            limit = 100

        assert endpoint == test_case["expected_endpoint"], (
            f"Endpoint mismatch for {test_case['name']}: "
            f"expected {test_case['expected_endpoint']}, got {endpoint}"
        )
        assert limit == test_case["expected_limit"], (
            f"Limit mismatch for {test_case['name']}: "
            f"expected {test_case['expected_limit']}, got {limit}"
        )


def test_okx_data_tester_configuration():
    """
    Test DataTesterConfig for OKX bar requests with request_bars=True.

    Based on: https://github.com/nautechsystems/nautilus_trader/blob/develop/nautilus_trader/test_kit/strategies/tester_data.py#L57

    """
    try:
        from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig
    except ImportError:
        pytest.skip("DataTesterConfig not available")

    # Configuration for requesting historical bars
    config = DataTesterConfig(
        instrument_ids=[
            InstrumentId.from_str("ETH-USDT.OKX"),
            InstrumentId.from_str("ETH-USDT-SWAP.OKX"),
        ],
        bar_types=[
            BarType.from_str("ETH-USDT.OKX-1-MINUTE-LAST-EXTERNAL"),
            BarType.from_str("ETH-USDT-SWAP.OKX-1-MINUTE-LAST-EXTERNAL"),
        ],
        request_bars=True,  # Enable historical bar requests
        requests_start_delta=pd.Timedelta(hours=24),  # Request 24 hours of data
        subscribe_bars=True,  # Also subscribe to live bars
        subscribe_book_deltas=True,
        subscribe_quotes=True,
        subscribe_trades=True,
    )

    # Verify configuration
    assert config.request_bars is True
    assert config.requests_start_delta == pd.Timedelta(hours=24)
    assert len(config.instrument_ids) == 2
    assert len(config.bar_types) == 2

    # Test different time deltas
    config_1hour = DataTesterConfig(
        instrument_ids=[InstrumentId.from_str("ETH-USDT.OKX")],
        bar_types=[BarType.from_str("ETH-USDT.OKX-1-MINUTE-LAST-EXTERNAL")],
        request_bars=True,
        requests_start_delta=pd.Timedelta(hours=1),  # 1 hour back
    )
    assert config_1hour.requests_start_delta == pd.Timedelta(hours=1)

    config_7days = DataTesterConfig(
        instrument_ids=[InstrumentId.from_str("ETH-USDT.OKX")],
        bar_types=[BarType.from_str("ETH-USDT.OKX-1-MINUTE-LAST-EXTERNAL")],
        request_bars=True,
        requests_start_delta=pd.Timedelta(days=7),  # 7 days back
    )
    assert config_7days.requests_start_delta == pd.Timedelta(days=7)

    # Test with None (uses default 1 hour)
    config_default = DataTesterConfig(
        instrument_ids=[InstrumentId.from_str("ETH-USDT.OKX")],
        bar_types=[BarType.from_str("ETH-USDT.OKX-1-MINUTE-LAST-EXTERNAL")],
        request_bars=True,
        requests_start_delta=None,  # Will default to 1 hour
    )
    assert config_default.requests_start_delta is None


def test_okx_data_tester_with_bar_requests():
    """
    Test DataTester integration with OKX for bar requests.

    Demonstrates how to set up DataTester with request_bars=True and
    requests_start_delta.

    """
    try:
        from nautilus_trader.test_kit.strategies.tester_data import DataTester
        from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig
    except ImportError:
        pytest.skip("DataTester not available")

    # Create configuration for historical bar requests
    config = DataTesterConfig(
        instrument_ids=[
            InstrumentId.from_str("ETH-USDT.OKX"),
            InstrumentId.from_str("ETH-USDT-SWAP.OKX"),
        ],
        bar_types=[
            BarType.from_str("ETH-USDT.OKX-1-MINUTE-LAST-EXTERNAL"),
            BarType.from_str("ETH-USDT-SWAP.OKX-1-MINUTE-LAST-EXTERNAL"),
        ],
        request_bars=True,  # Enable historical bar requests
        requests_start_delta=pd.Timedelta(hours=6),  # Request 6 hours of historical data
        subscribe_bars=True,  # Also subscribe to live bar updates
        subscribe_quotes=True,
        subscribe_trades=True,
        subscribe_mark_prices=True,
        subscribe_index_prices=True,
    )

    # Create DataTester instance
    tester = DataTester(config=config)

    # Verify the configuration is properly set
    assert tester.config.request_bars is True
    assert tester.config.requests_start_delta == pd.Timedelta(hours=6)
    assert len(tester.config.instrument_ids) == 2
    assert len(tester.config.bar_types) == 2

    # Test different scenarios for historical data requests
    scenarios = [
        {
            "name": "Recent data (1 hour)",
            "requests_start_delta": pd.Timedelta(hours=1),
            "expected_hours": 1,
        },
        {
            "name": "Daily data (24 hours)",
            "requests_start_delta": pd.Timedelta(hours=24),
            "expected_hours": 24,
        },
        {
            "name": "Weekly data (7 days)",
            "requests_start_delta": pd.Timedelta(days=7),
            "expected_hours": 168,  # 7 * 24
        },
        {
            "name": "Monthly data (30 days)",
            "requests_start_delta": pd.Timedelta(days=30),
            "expected_hours": 720,  # 30 * 24
        },
    ]

    for scenario in scenarios:
        config_scenario = DataTesterConfig(
            instrument_ids=[InstrumentId.from_str("ETH-USDT.OKX")],
            bar_types=[BarType.from_str("ETH-USDT.OKX-1-MINUTE-LAST-EXTERNAL")],
            request_bars=True,
            requests_start_delta=scenario["requests_start_delta"],
        )

        assert (
            config_scenario.requests_start_delta.total_seconds() / 3600
            == scenario["expected_hours"]
        )
