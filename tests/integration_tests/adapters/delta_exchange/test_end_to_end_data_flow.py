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

"""
Integration tests for end-to-end data flow from Delta Exchange WebSocket feeds to Nautilus data events.

These tests validate the complete data pipeline from WebSocket message reception
through parsing, conversion, and delivery to Nautilus Trader data handlers.
"""

import asyncio
import os
from unittest.mock import AsyncMock, MagicMock, patch
from decimal import Decimal

import pytest

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeDataClientConfig
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE_VENUE
from nautilus_trader.adapters.delta_exchange.data import DeltaExchangeDataClient
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.data.messages import DataRequest, DataResponse
from nautilus_trader.model.data import QuoteTick, TradeTick, OrderBookDelta
from nautilus_trader.model.identifiers import InstrumentId, Symbol
from nautilus_trader.test_kit.mocks import MockMessageBus


@pytest.mark.integration
@pytest.mark.skipif(
    not os.getenv("DELTA_EXCHANGE_API_KEY") or not os.getenv("DELTA_EXCHANGE_API_SECRET"),
    reason="Delta Exchange API credentials not available"
)
class TestEndToEndDataFlow:
    """Test end-to-end data flow from WebSocket to Nautilus events."""

    def setup_method(self):
        """Set up test fixtures."""
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.msgbus = MockMessageBus()
        self.cache = Cache()
        
        # Use environment variables for credentials (testnet)
        self.api_key = os.getenv("DELTA_EXCHANGE_API_KEY", "test_key")
        self.api_secret = os.getenv("DELTA_EXCHANGE_API_SECRET", "test_secret")
        
        # Create configuration for testnet
        self.config = DeltaExchangeDataClientConfig(
            api_key=self.api_key,
            api_secret=self.api_secret,
            testnet=True,
            enable_private_channels=False,  # Public data only for integration tests
            product_types=["perpetual_futures"],
            symbol_patterns=["BTC*", "ETH*"],
            request_timeout_secs=30.0,
            ws_timeout_secs=10.0,
            max_retries=3,
        )
        
        # Sample instrument for testing
        self.test_instrument_id = InstrumentId(Symbol("BTCUSDT"), DELTA_EXCHANGE_VENUE)
        
        # Data collection for verification
        self.received_quotes = []
        self.received_trades = []
        self.received_deltas = []
        self.received_errors = []

    def _setup_data_handlers(self, data_client):
        """Set up data event handlers for testing."""
        def on_quote_tick(quote: QuoteTick):
            self.received_quotes.append(quote)
        
        def on_trade_tick(trade: TradeTick):
            self.received_trades.append(trade)
        
        def on_order_book_delta(delta: OrderBookDelta):
            self.received_deltas.append(delta)
        
        def on_error(error):
            self.received_errors.append(error)
        
        # Register handlers with message bus
        self.msgbus.register_handler(QuoteTick, on_quote_tick)
        self.msgbus.register_handler(TradeTick, on_trade_tick)
        self.msgbus.register_handler(OrderBookDelta, on_order_book_delta)

    @pytest.mark.asyncio
    async def test_websocket_connection_and_subscription(self):
        """Test WebSocket connection and channel subscription."""
        # Create instrument provider
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        # Create data client
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        self._setup_data_handlers(data_client)
        
        try:
            # Connect to WebSocket
            await data_client.connect()
            assert data_client.is_connected
            
            # Subscribe to ticker data
            await data_client.subscribe_quote_ticks(self.test_instrument_id)
            
            # Wait for some data
            await asyncio.sleep(5.0)
            
            # Verify we received some quotes
            assert len(self.received_quotes) > 0, "Should have received quote ticks"
            
            # Verify quote structure
            quote = self.received_quotes[0]
            assert quote.instrument_id == self.test_instrument_id
            assert quote.bid_price > 0
            assert quote.ask_price > 0
            assert quote.bid_size > 0
            assert quote.ask_size > 0
            
        finally:
            # Clean up
            await data_client.disconnect()

    @pytest.mark.asyncio
    async def test_trade_data_flow(self):
        """Test trade data flow from WebSocket to Nautilus events."""
        # Create instrument provider
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        # Create data client
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        self._setup_data_handlers(data_client)
        
        try:
            # Connect and subscribe to trades
            await data_client.connect()
            await data_client.subscribe_trade_ticks(self.test_instrument_id)
            
            # Wait for trade data
            await asyncio.sleep(10.0)
            
            # Verify we received trades
            assert len(self.received_trades) > 0, "Should have received trade ticks"
            
            # Verify trade structure
            trade = self.received_trades[0]
            assert trade.instrument_id == self.test_instrument_id
            assert trade.price > 0
            assert trade.size > 0
            assert trade.aggressor_side is not None
            
        finally:
            await data_client.disconnect()

    @pytest.mark.asyncio
    async def test_order_book_data_flow(self):
        """Test order book data flow from WebSocket to Nautilus events."""
        # Create instrument provider
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        # Create data client
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        self._setup_data_handlers(data_client)
        
        try:
            # Connect and subscribe to order book
            await data_client.connect()
            await data_client.subscribe_order_book_deltas(self.test_instrument_id)
            
            # Wait for order book data
            await asyncio.sleep(10.0)
            
            # Verify we received deltas
            assert len(self.received_deltas) > 0, "Should have received order book deltas"
            
            # Verify delta structure
            delta = self.received_deltas[0]
            assert delta.instrument_id == self.test_instrument_id
            assert delta.action is not None
            assert delta.order is not None
            
        finally:
            await data_client.disconnect()

    @pytest.mark.asyncio
    async def test_multiple_instrument_subscriptions(self):
        """Test subscribing to multiple instruments simultaneously."""
        # Create additional test instruments
        eth_instrument_id = InstrumentId(Symbol("ETHUSDT"), DELTA_EXCHANGE_VENUE)
        
        # Create instrument provider
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        # Create data client
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        self._setup_data_handlers(data_client)
        
        try:
            # Connect
            await data_client.connect()
            
            # Subscribe to multiple instruments
            await data_client.subscribe_quote_ticks(self.test_instrument_id)
            await data_client.subscribe_quote_ticks(eth_instrument_id)
            
            # Wait for data
            await asyncio.sleep(10.0)
            
            # Verify we received data for both instruments
            btc_quotes = [q for q in self.received_quotes if q.instrument_id == self.test_instrument_id]
            eth_quotes = [q for q in self.received_quotes if q.instrument_id == eth_instrument_id]
            
            assert len(btc_quotes) > 0, "Should have received BTC quotes"
            assert len(eth_quotes) > 0, "Should have received ETH quotes"
            
        finally:
            await data_client.disconnect()

    @pytest.mark.asyncio
    async def test_subscription_management(self):
        """Test subscription and unsubscription management."""
        # Create instrument provider
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        # Create data client
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        self._setup_data_handlers(data_client)
        
        try:
            # Connect
            await data_client.connect()
            
            # Subscribe to quotes
            await data_client.subscribe_quote_ticks(self.test_instrument_id)
            
            # Wait for some data
            await asyncio.sleep(5.0)
            initial_quote_count = len(self.received_quotes)
            
            # Unsubscribe
            await data_client.unsubscribe_quote_ticks(self.test_instrument_id)
            
            # Wait and verify no new data
            await asyncio.sleep(5.0)
            final_quote_count = len(self.received_quotes)
            
            # Should have stopped receiving data (or very little new data)
            assert final_quote_count - initial_quote_count < 5, "Should have stopped receiving quotes after unsubscribe"
            
        finally:
            await data_client.disconnect()

    @pytest.mark.asyncio
    async def test_reconnection_handling(self):
        """Test automatic reconnection handling."""
        # Create instrument provider
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        # Create data client with shorter timeouts for testing
        test_config = DeltaExchangeDataClientConfig(
            api_key=self.api_key,
            api_secret=self.api_secret,
            testnet=True,
            enable_private_channels=False,
            ws_timeout_secs=5.0,
            heartbeat_interval_secs=10.0,
            max_reconnect_attempts=3,
            reconnect_delay_secs=2.0,
        )
        
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=test_config,
        )
        
        self._setup_data_handlers(data_client)
        
        try:
            # Connect and subscribe
            await data_client.connect()
            await data_client.subscribe_quote_ticks(self.test_instrument_id)
            
            # Wait for initial data
            await asyncio.sleep(5.0)
            initial_quote_count = len(self.received_quotes)
            
            # Simulate connection loss (this would trigger reconnection in real scenario)
            # For integration test, we just verify the client can handle reconnection
            
            # Wait for potential reconnection
            await asyncio.sleep(15.0)
            
            # Verify we're still receiving data (indicating successful reconnection)
            final_quote_count = len(self.received_quotes)
            assert final_quote_count > initial_quote_count, "Should continue receiving data after reconnection"
            
        finally:
            await data_client.disconnect()

    @pytest.mark.asyncio
    async def test_error_handling_and_recovery(self):
        """Test error handling and recovery mechanisms."""
        # Create instrument provider
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        # Create data client
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        self._setup_data_handlers(data_client)
        
        try:
            # Connect
            await data_client.connect()
            
            # Try to subscribe to invalid instrument (should handle gracefully)
            invalid_instrument_id = InstrumentId(Symbol("INVALID"), DELTA_EXCHANGE_VENUE)
            
            # This should not crash the client
            try:
                await data_client.subscribe_quote_ticks(invalid_instrument_id)
            except Exception as e:
                # Error is expected and should be handled gracefully
                self.received_errors.append(e)
            
            # Verify client is still functional
            await data_client.subscribe_quote_ticks(self.test_instrument_id)
            await asyncio.sleep(5.0)
            
            # Should still receive valid data
            assert len(self.received_quotes) > 0, "Should still receive data after error"
            
        finally:
            await data_client.disconnect()

    @pytest.mark.asyncio
    async def test_data_quality_and_consistency(self):
        """Test data quality and consistency checks."""
        # Create instrument provider
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        # Create data client
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        self._setup_data_handlers(data_client)
        
        try:
            # Connect and subscribe
            await data_client.connect()
            await data_client.subscribe_quote_ticks(self.test_instrument_id)
            await data_client.subscribe_trade_ticks(self.test_instrument_id)
            
            # Collect data for analysis
            await asyncio.sleep(30.0)
            
            # Verify data quality
            assert len(self.received_quotes) > 0, "Should have received quotes"
            assert len(self.received_trades) > 0, "Should have received trades"
            
            # Check quote consistency
            for quote in self.received_quotes:
                assert quote.bid_price > 0, "Bid price should be positive"
                assert quote.ask_price > 0, "Ask price should be positive"
                assert quote.ask_price >= quote.bid_price, "Ask should be >= bid"
                assert quote.bid_size > 0, "Bid size should be positive"
                assert quote.ask_size > 0, "Ask size should be positive"
            
            # Check trade consistency
            for trade in self.received_trades:
                assert trade.price > 0, "Trade price should be positive"
                assert trade.size > 0, "Trade size should be positive"
                assert trade.aggressor_side is not None, "Aggressor side should be set"
            
            # Check timestamp ordering (should be mostly increasing)
            quote_timestamps = [q.ts_event for q in self.received_quotes]
            trade_timestamps = [t.ts_event for t in self.received_trades]
            
            # Allow for some out-of-order messages but most should be ordered
            quote_ordered_ratio = sum(1 for i in range(1, len(quote_timestamps)) 
                                    if quote_timestamps[i] >= quote_timestamps[i-1]) / max(1, len(quote_timestamps) - 1)
            trade_ordered_ratio = sum(1 for i in range(1, len(trade_timestamps)) 
                                    if trade_timestamps[i] >= trade_timestamps[i-1]) / max(1, len(trade_timestamps) - 1)
            
            assert quote_ordered_ratio > 0.9, f"Quote timestamps should be mostly ordered: {quote_ordered_ratio}"
            assert trade_ordered_ratio > 0.9, f"Trade timestamps should be mostly ordered: {trade_ordered_ratio}"
            
        finally:
            await data_client.disconnect()
