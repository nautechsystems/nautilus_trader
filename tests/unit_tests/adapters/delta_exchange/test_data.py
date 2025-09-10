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

"""
Unit tests for Delta Exchange data client.

This module provides comprehensive tests for the DeltaExchangeDataClient,
covering all subscription types, historical data requests, message handling,
and error scenarios.
"""

import asyncio
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeDataClientConfig
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE
from nautilus_trader.adapters.delta_exchange.data import DeltaExchangeDataClient
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock, MessageBus
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import RequestTradeTicks, SubscribeQuoteTicks
from nautilus_trader.model.data import BarType, QuoteTick, TradeTick
from nautilus_trader.model.enums import AggregateSide, BookType, PriceType
from nautilus_trader.model.identifiers import InstrumentId, Symbol, Venue
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.test_kit.mocks import MockMessageBus
from nautilus_trader.test_kit.stubs import TestStubs


class TestDeltaExchangeDataClient:
    """Test suite for DeltaExchangeDataClient."""

    def setup_method(self):
        """Set up test fixtures."""
        # Create test components
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.msgbus = MockMessageBus()
        self.cache = Cache()
        
        # Create mock HTTP client
        self.mock_http_client = MagicMock(spec=nautilus_pyo3.DeltaExchangeHttpClient)
        
        # Create mock instrument provider
        self.mock_instrument_provider = MagicMock(spec=DeltaExchangeInstrumentProvider)
        
        # Create test configuration
        self.config = DeltaExchangeDataClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            testnet=True,
            default_channels=["v2_ticker", "all_trades"],
            symbol_filters=["BTC*", "ETH*"],
        )
        
        # Create test instrument
        self.instrument = CryptoPerpetual(
            instrument_id=InstrumentId(Symbol("BTCUSDT"), DELTA_EXCHANGE),
            raw_symbol=Symbol("BTCUSDT"),
            base_currency=TestStubs.currency_btc(),
            quote_currency=TestStubs.currency_usdt(),
            settlement_currency=TestStubs.currency_usdt(),
            is_inverse=False,
            price_precision=2,
            size_precision=6,
            price_increment=TestStubs.price_increment(),
            size_increment=TestStubs.size_increment(),
            margin_init=TestStubs.decimal_from_str("0.1"),
            margin_maint=TestStubs.decimal_from_str("0.05"),
            maker_fee=TestStubs.decimal_from_str("0.0002"),
            taker_fee=TestStubs.decimal_from_str("0.0005"),
            ts_event=0,
            ts_init=0,
        )
        
        # Create data client
        self.data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=self.mock_http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=self.mock_instrument_provider,
            config=self.config,
        )

    def test_init(self):
        """Test data client initialization."""
        assert self.data_client.id.value == DELTA_EXCHANGE.value
        assert self.data_client.venue == DELTA_EXCHANGE
        assert self.data_client._config == self.config
        assert self.data_client._client == self.mock_http_client
        assert not self.data_client._is_connected
        assert len(self.data_client._subscribed_instruments) == 0

    def test_stats_property(self):
        """Test stats property returns correct statistics."""
        stats = self.data_client.stats
        
        assert isinstance(stats, dict)
        assert "messages_received" in stats
        assert "messages_processed" in stats
        assert "connection_attempts" in stats
        assert "reconnections" in stats
        assert "errors" in stats
        assert "subscriptions" in stats
        assert "unsubscriptions" in stats

    @pytest.mark.asyncio
    async def test_connect_success(self):
        """Test successful connection."""
        # Mock WebSocket client
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient', return_value=mock_ws_client):
            await self.data_client._connect()
            
            assert self.data_client._is_connected
            assert self.data_client._ws_client == mock_ws_client
            mock_ws_client.connect.assert_called_once()
            mock_ws_client.set_message_handler.assert_called_once()

    @pytest.mark.asyncio
    async def test_connect_failure(self):
        """Test connection failure handling."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_ws_class:
            mock_ws_class.side_effect = Exception("Connection failed")
            
            with pytest.raises(Exception, match="Connection failed"):
                await self.data_client._connect()
            
            assert not self.data_client._is_connected
            assert self.data_client.stats["errors"] > 0

    @pytest.mark.asyncio
    async def test_disconnect(self):
        """Test disconnection."""
        # Set up connected state
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        self.data_client._ws_client = mock_ws_client
        self.data_client._is_connected = True
        
        # Add some subscription state
        self.data_client._subscribed_quote_ticks.add(self.instrument.id)
        self.data_client._symbol_to_instrument["BTCUSDT"] = self.instrument.id
        
        await self.data_client._disconnect()
        
        assert not self.data_client._is_connected
        assert self.data_client._ws_client is None
        assert len(self.data_client._subscribed_quote_ticks) == 0
        assert len(self.data_client._symbol_to_instrument) == 0
        mock_ws_client.disconnect.assert_called_once()

    @pytest.mark.asyncio
    async def test_reset(self):
        """Test client reset."""
        # Set some state
        self.data_client._stats["messages_received"] = 100
        self.data_client._connection_retry_count = 5
        
        await self.data_client._reset()
        
        assert self.data_client._stats["messages_received"] == 0
        assert self.data_client._connection_retry_count == 0

    def test_should_subscribe_symbol_with_filters(self):
        """Test symbol filtering logic."""
        # Should match BTC* filter
        assert self.data_client._should_subscribe_symbol("BTCUSDT")
        assert self.data_client._should_subscribe_symbol("BTCUSD")
        
        # Should match ETH* filter
        assert self.data_client._should_subscribe_symbol("ETHUSDT")
        
        # Should not match filters
        assert not self.data_client._should_subscribe_symbol("ADAUSDT")
        assert not self.data_client._should_subscribe_symbol("SOLUSDT")

    def test_should_subscribe_symbol_no_filters(self):
        """Test symbol filtering with no filters configured."""
        # Create client without filters
        config_no_filters = DeltaExchangeDataClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            testnet=True,
        )
        
        client_no_filters = DeltaExchangeDataClient(
            loop=self.loop,
            client=self.mock_http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=self.mock_instrument_provider,
            config=config_no_filters,
        )
        
        # Should accept all symbols
        assert client_no_filters._should_subscribe_symbol("BTCUSDT")
        assert client_no_filters._should_subscribe_symbol("ETHUSDT")
        assert client_no_filters._should_subscribe_symbol("ADAUSDT")

    def test_map_bar_aggregation_to_resolution(self):
        """Test bar aggregation to resolution mapping."""
        # Test known mappings
        assert self.data_client._map_bar_aggregation_to_resolution(60) == "1m"
        assert self.data_client._map_bar_aggregation_to_resolution(300) == "5m"
        assert self.data_client._map_bar_aggregation_to_resolution(3600) == "1h"
        assert self.data_client._map_bar_aggregation_to_resolution(86400) == "1d"
        
        # Test unknown mapping (should default to 1h)
        assert self.data_client._map_bar_aggregation_to_resolution(123) == "1h"

    @pytest.mark.asyncio
    async def test_apply_rate_limit(self):
        """Test rate limiting functionality."""
        # First request should not be rate limited
        start_time = self.clock.timestamp()
        await self.data_client._apply_rate_limit()
        end_time = self.clock.timestamp()
        
        # Should be very fast
        assert end_time - start_time < 0.1
        assert self.data_client._request_count == 1

    def test_get_cached_instrument(self):
        """Test cached instrument retrieval."""
        # Add instrument to cache and symbol mapping
        self.cache.add_instrument(self.instrument)
        self.data_client._symbol_to_instrument["BTCUSDT"] = self.instrument.id
        
        # Should retrieve cached instrument
        cached = self.data_client._get_cached_instrument("BTCUSDT")
        assert cached == self.instrument
        
        # Should return None for unknown symbol
        unknown = self.data_client._get_cached_instrument("UNKNOWN")
        assert unknown is None

    def test_repr(self):
        """Test string representation."""
        repr_str = repr(self.data_client)
        
        assert "DeltaExchangeDataClient" in repr_str
        assert f"id={self.data_client.id}" in repr_str
        assert f"venue={self.data_client.venue}" in repr_str
        assert "connected=False" in repr_str
        assert "subscriptions=0" in repr_str

    @pytest.mark.asyncio
    async def test_subscribe_quote_ticks_success(self):
        """Test successful quote tick subscription."""
        # Set up connected state
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        self.data_client._ws_client = mock_ws_client
        self.data_client._is_connected = True

        await self.data_client._subscribe_quote_ticks(self.instrument.id)

        # Verify subscription state
        assert self.instrument.id in self.data_client._subscribed_quote_ticks
        assert "BTCUSDT" in self.data_client._symbol_to_instrument
        assert self.data_client._symbol_to_instrument["BTCUSDT"] == self.instrument.id

    @pytest.mark.asyncio
    async def test_subscribe_quote_ticks_not_connected(self):
        """Test quote tick subscription when not connected."""
        # Client not connected
        self.data_client._is_connected = False

        await self.data_client._subscribe_quote_ticks(self.instrument.id)

        # Should not be subscribed
        assert self.instrument.id not in self.data_client._subscribed_quote_ticks

    @pytest.mark.asyncio
    async def test_subscribe_quote_ticks_filtered_symbol(self):
        """Test quote tick subscription with filtered symbol."""
        # Set up connected state
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        self.data_client._ws_client = mock_ws_client
        self.data_client._is_connected = True

        # Create instrument with symbol that doesn't match filters
        filtered_instrument = CryptoPerpetual(
            instrument_id=InstrumentId(Symbol("ADAUSDT"), DELTA_EXCHANGE),
            raw_symbol=Symbol("ADAUSDT"),
            base_currency=TestStubs.currency_ada(),
            quote_currency=TestStubs.currency_usdt(),
            settlement_currency=TestStubs.currency_usdt(),
            is_inverse=False,
            price_precision=4,
            size_precision=2,
            price_increment=TestStubs.price_increment(),
            size_increment=TestStubs.size_increment(),
            margin_init=TestStubs.decimal_from_str("0.1"),
            margin_maint=TestStubs.decimal_from_str("0.05"),
            maker_fee=TestStubs.decimal_from_str("0.0002"),
            taker_fee=TestStubs.decimal_from_str("0.0005"),
            ts_event=0,
            ts_init=0,
        )

        await self.data_client._subscribe_quote_ticks(filtered_instrument.id)

        # Should not be subscribed due to filtering
        assert filtered_instrument.id not in self.data_client._subscribed_quote_ticks

    @pytest.mark.asyncio
    async def test_subscribe_trade_ticks_success(self):
        """Test successful trade tick subscription."""
        # Set up connected state
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        self.data_client._ws_client = mock_ws_client
        self.data_client._is_connected = True

        await self.data_client._subscribe_trade_ticks(self.instrument.id)

        # Verify subscription state
        assert self.instrument.id in self.data_client._subscribed_trade_ticks
        assert "BTCUSDT" in self.data_client._symbol_to_instrument

    @pytest.mark.asyncio
    async def test_subscribe_order_book_deltas_success(self):
        """Test successful order book subscription."""
        # Set up connected state
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        self.data_client._ws_client = mock_ws_client
        self.data_client._is_connected = True

        await self.data_client._subscribe_order_book_deltas(
            self.instrument.id,
            BookType.L2_MBP
        )

        # Verify subscription state
        assert self.instrument.id in self.data_client._subscribed_order_books
        assert "BTCUSDT" in self.data_client._symbol_to_instrument

    @pytest.mark.asyncio
    async def test_subscribe_bars_success(self):
        """Test successful bar subscription."""
        # Set up connected state
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        self.data_client._ws_client = mock_ws_client
        self.data_client._is_connected = True

        # Create bar type
        bar_type = BarType(
            instrument_id=self.instrument.id,
            bar_spec=TestStubs.bar_spec_1min_bid(),
            aggregation_source=AggregateSide.BID,
        )

        await self.data_client._subscribe_bars(bar_type)

        # Verify subscription state
        assert bar_type in self.data_client._subscribed_bars
        assert self.data_client._subscribed_bars[bar_type] == self.instrument.id

    @pytest.mark.asyncio
    async def test_subscribe_mark_prices_success(self):
        """Test successful mark price subscription."""
        # Set up connected state
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        self.data_client._ws_client = mock_ws_client
        self.data_client._is_connected = True

        await self.data_client._subscribe_mark_prices(self.instrument.id)

        # Verify subscription state
        assert self.instrument.id in self.data_client._subscribed_mark_prices

    @pytest.mark.asyncio
    async def test_subscribe_funding_rates_success(self):
        """Test successful funding rate subscription."""
        # Set up connected state
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        self.data_client._ws_client = mock_ws_client
        self.data_client._is_connected = True

        await self.data_client._subscribe_funding_rates(self.instrument.id)

        # Verify subscription state
        assert self.instrument.id in self.data_client._subscribed_funding_rates

    @pytest.mark.asyncio
    async def test_unsubscribe_quote_ticks_success(self):
        """Test successful quote tick unsubscription."""
        # Set up connected state and existing subscription
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        self.data_client._ws_client = mock_ws_client
        self.data_client._is_connected = True
        self.data_client._subscribed_quote_ticks.add(self.instrument.id)
        self.data_client._symbol_to_instrument["BTCUSDT"] = self.instrument.id

        await self.data_client._unsubscribe_quote_ticks(self.instrument.id)

        # Verify unsubscription
        assert self.instrument.id not in self.data_client._subscribed_quote_ticks

    @pytest.mark.asyncio
    async def test_request_quote_ticks_not_available(self):
        """Test quote tick request returns empty response."""
        correlation_id = UUID4()

        await self.data_client._request_quote_ticks(
            self.instrument.id,
            limit=100,
            correlation_id=correlation_id,
        )

        # Should have sent empty response
        assert len(self.msgbus.sent) > 0

    @pytest.mark.asyncio
    async def test_request_trade_ticks_success(self):
        """Test successful trade tick request."""
        # Mock HTTP client response
        mock_trades_response = {
            "result": [
                {
                    "id": "123",
                    "symbol": "BTCUSDT",
                    "price": "50000.00",
                    "size": "0.001",
                    "side": "buy",
                    "timestamp": 1640995200,
                }
            ]
        }
        self.mock_http_client.get_trades = AsyncMock(return_value=mock_trades_response)

        # Mock parsing
        mock_trade_tick = TradeTick(
            instrument_id=self.instrument.id,
            price=TestStubs.price(),
            size=TestStubs.quantity(),
            aggressor_side=AggregateSide.BUY,
            trade_id="123",
            ts_event=1640995200000000000,
            ts_init=1640995200000000000,
        )

        with patch.object(self.data_client, '_parse_trade_tick', return_value=mock_trade_tick):
            correlation_id = UUID4()

            await self.data_client._request_trade_ticks(
                self.instrument.id,
                limit=100,
                correlation_id=correlation_id,
            )

            # Should have called HTTP client
            self.mock_http_client.get_trades.assert_called_once()

            # Should have sent response
            assert len(self.msgbus.sent) > 0

    @pytest.mark.asyncio
    async def test_request_bars_success(self):
        """Test successful bar request."""
        # Mock HTTP client response
        mock_candles_response = {
            "result": [
                {
                    "symbol": "BTCUSDT",
                    "open": "49000.00",
                    "high": "51000.00",
                    "low": "48000.00",
                    "close": "50000.00",
                    "volume": "100.0",
                    "timestamp": 1640995200,
                }
            ]
        }
        self.mock_http_client.get_candles = AsyncMock(return_value=mock_candles_response)

        # Create bar type
        bar_type = BarType(
            instrument_id=self.instrument.id,
            bar_spec=TestStubs.bar_spec_1min_bid(),
            aggregation_source=AggregateSide.BID,
        )

        # Mock parsing
        mock_bar = TestStubs.bar_5decimal()

        with patch.object(self.data_client, '_parse_bar', return_value=mock_bar):
            correlation_id = UUID4()

            await self.data_client._request_bars(
                bar_type,
                limit=100,
                correlation_id=correlation_id,
            )

            # Should have called HTTP client
            self.mock_http_client.get_candles.assert_called_once()

            # Should have sent response
            assert len(self.msgbus.sent) > 0
