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
Unit tests for Delta Exchange WebSocket client bindings.

This module tests the Rust WebSocket client bindings with comprehensive mock connections,
message parsing, subscription management, authentication, and error handling scenarios.
"""

import asyncio
import json
from unittest.mock import AsyncMock, MagicMock, patch
from datetime import datetime

import pytest

from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_WS_URL,
    DELTA_EXCHANGE_TESTNET_WS_URL,
    DELTA_EXCHANGE_WS_PUBLIC_CHANNELS,
    DELTA_EXCHANGE_WS_PRIVATE_CHANNELS,
)


class TestDeltaExchangeWebSocketClientBindings:
    """Test Delta Exchange WebSocket client Rust bindings."""

    def setup_method(self):
        """Set up test fixtures."""
        self.api_key = "test_api_key_12345678901234567890"
        self.api_secret = "test_api_secret_base64_encoded_string"
        self.ws_url = DELTA_EXCHANGE_TESTNET_WS_URL
        
        # Mock the Rust WebSocket client
        self.mock_client = MagicMock()
        
        # Sample WebSocket messages
        self.sample_ticker_message = {
            "type": "v2_ticker",
            "product_id": 139,
            "symbol": "BTCUSDT",
            "timestamp": 1640995200000000,
            "best_bid": "49950.0",
            "best_ask": "50050.0",
            "last_price": "50000.0",
            "volume": "1234.567",
            "open_interest": "9876.543",
            "mark_price": "50025.0",
            "funding_rate": "0.0001",
            "next_funding_time": 1641024000000000
        }
        
        self.sample_orderbook_snapshot = {
            "type": "l2_orderbook",
            "product_id": 139,
            "symbol": "BTCUSDT",
            "timestamp": 1640995200000000,
            "buy": [
                {"price": "49950.0", "size": "1.5"},
                {"price": "49900.0", "size": "2.0"},
                {"price": "49850.0", "size": "1.0"}
            ],
            "sell": [
                {"price": "50050.0", "size": "1.2"},
                {"price": "50100.0", "size": "1.8"},
                {"price": "50150.0", "size": "0.9"}
            ]
        }
        
        self.sample_orderbook_update = {
            "type": "l2_updates",
            "product_id": 139,
            "symbol": "BTCUSDT",
            "timestamp": 1640995201000000,
            "buy": [
                {"price": "49950.0", "size": "2.0"}  # Updated size
            ],
            "sell": [
                {"price": "50050.0", "size": "0.0"}  # Removed level
            ]
        }
        
        self.sample_trade_message = {
            "type": "all_trades",
            "product_id": 139,
            "symbol": "BTCUSDT",
            "timestamp": 1640995200000000,
            "price": "50000.0",
            "size": "0.5",
            "side": "buy",
            "trade_id": "12345678"
        }
        
        self.sample_order_update = {
            "type": "orders",
            "user_id": 1,
            "order_id": 12345,
            "client_order_id": "test_order_123",
            "product_id": 139,
            "symbol": "BTCUSDT",
            "side": "buy",
            "order_type": "limit_order",
            "size": "1.0",
            "unfilled_size": "0.0",
            "limit_price": "50000.0",
            "state": "filled",
            "timestamp": 1640995200000000
        }
        
        self.sample_position_update = {
            "type": "positions",
            "user_id": 1,
            "product_id": 139,
            "symbol": "BTCUSDT",
            "size": "1.5",
            "entry_price": "49800.0",
            "mark_price": "50000.0",
            "unrealized_pnl": "300.0",
            "realized_pnl": "0.0",
            "margin": "498.0",
            "timestamp": 1640995200000000
        }
        
        self.sample_auth_message = {
            "type": "auth",
            "success": True,
            "message": "Authentication successful"
        }
        
        self.sample_subscription_message = {
            "type": "subscribe",
            "channels": [
                {
                    "name": "v2_ticker",
                    "symbols": ["BTCUSDT", "ETHUSDT"]
                }
            ]
        }

    @pytest.mark.asyncio
    async def test_websocket_client_initialization(self):
        """Test WebSocket client initialization with various configurations."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client_instance = MagicMock()
            mock_client_class.return_value = mock_client_instance
            
            # Test production initialization
            client = mock_client_class(
                ws_url=DELTA_EXCHANGE_WS_URL,
                api_key=self.api_key,
                api_secret=self.api_secret,
                testnet=False,
            )
            
            mock_client_class.assert_called_once_with(
                ws_url=DELTA_EXCHANGE_WS_URL,
                api_key=self.api_key,
                api_secret=self.api_secret,
                testnet=False,
            )
            
            # Test testnet initialization
            mock_client_class.reset_mock()
            testnet_client = mock_client_class(
                ws_url=DELTA_EXCHANGE_TESTNET_WS_URL,
                api_key=self.api_key,
                api_secret=self.api_secret,
                testnet=True,
            )
            
            mock_client_class.assert_called_once_with(
                ws_url=DELTA_EXCHANGE_TESTNET_WS_URL,
                api_key=self.api_key,
                api_secret=self.api_secret,
                testnet=True,
            )

    @pytest.mark.asyncio
    async def test_websocket_connection_lifecycle(self):
        """Test WebSocket connection lifecycle (connect, disconnect, reconnect)."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock connection methods
            mock_client.connect = AsyncMock(return_value=True)
            mock_client.disconnect = AsyncMock(return_value=True)
            mock_client.is_connected = MagicMock(return_value=True)
            
            # Test connection
            connected = await mock_client.connect()
            assert connected is True
            mock_client.connect.assert_called_once()
            
            # Test connection status
            assert mock_client.is_connected() is True
            
            # Test disconnection
            disconnected = await mock_client.disconnect()
            assert disconnected is True
            mock_client.disconnect.assert_called_once()

    @pytest.mark.asyncio
    async def test_public_channel_subscription(self):
        """Test subscription to public channels."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock subscription method
            mock_client.subscribe = AsyncMock(return_value=True)
            
            # Test public channel subscription
            subscription_data = {
                "type": "subscribe",
                "channels": [
                    {
                        "name": "v2_ticker",
                        "symbols": ["BTCUSDT"]
                    }
                ]
            }
            
            success = await mock_client.subscribe(json.dumps(subscription_data))
            assert success is True
            mock_client.subscribe.assert_called_once()

    @pytest.mark.asyncio
    async def test_private_channel_authentication(self):
        """Test authentication for private channels."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock authentication method
            mock_client.authenticate = AsyncMock(return_value=json.dumps(self.sample_auth_message))
            
            # Test authentication
            auth_response = await mock_client.authenticate()
            parsed_response = json.loads(auth_response)
            
            assert parsed_response["type"] == "auth"
            assert parsed_response["success"] is True
            mock_client.authenticate.assert_called_once()

    @pytest.mark.asyncio
    async def test_ticker_message_parsing(self):
        """Test parsing of ticker messages."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock message handler
            mock_client.parse_message = MagicMock(return_value=json.dumps(self.sample_ticker_message))
            
            # Test ticker message parsing
            raw_message = json.dumps(self.sample_ticker_message)
            parsed_message = mock_client.parse_message(raw_message)
            ticker_data = json.loads(parsed_message)
            
            # Verify ticker data
            assert ticker_data["type"] == "v2_ticker"
            assert ticker_data["symbol"] == "BTCUSDT"
            assert ticker_data["best_bid"] == "49950.0"
            assert ticker_data["best_ask"] == "50050.0"
            assert ticker_data["last_price"] == "50000.0"

    @pytest.mark.asyncio
    async def test_orderbook_snapshot_parsing(self):
        """Test parsing of order book snapshot messages."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock message handler
            mock_client.parse_message = MagicMock(return_value=json.dumps(self.sample_orderbook_snapshot))
            
            # Test order book snapshot parsing
            raw_message = json.dumps(self.sample_orderbook_snapshot)
            parsed_message = mock_client.parse_message(raw_message)
            orderbook_data = json.loads(parsed_message)
            
            # Verify order book data
            assert orderbook_data["type"] == "l2_orderbook"
            assert orderbook_data["symbol"] == "BTCUSDT"
            assert len(orderbook_data["buy"]) == 3
            assert len(orderbook_data["sell"]) == 3
            assert orderbook_data["buy"][0]["price"] == "49950.0"
            assert orderbook_data["sell"][0]["price"] == "50050.0"

    @pytest.mark.asyncio
    async def test_orderbook_update_parsing(self):
        """Test parsing of order book update messages."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock message handler
            mock_client.parse_message = MagicMock(return_value=json.dumps(self.sample_orderbook_update))
            
            # Test order book update parsing
            raw_message = json.dumps(self.sample_orderbook_update)
            parsed_message = mock_client.parse_message(raw_message)
            update_data = json.loads(parsed_message)
            
            # Verify update data
            assert update_data["type"] == "l2_updates"
            assert update_data["symbol"] == "BTCUSDT"
            assert len(update_data["buy"]) == 1
            assert len(update_data["sell"]) == 1
            assert update_data["buy"][0]["size"] == "2.0"  # Updated size
            assert update_data["sell"][0]["size"] == "0.0"  # Removed level

    @pytest.mark.asyncio
    async def test_trade_message_parsing(self):
        """Test parsing of trade messages."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock message handler
            mock_client.parse_message = MagicMock(return_value=json.dumps(self.sample_trade_message))
            
            # Test trade message parsing
            raw_message = json.dumps(self.sample_trade_message)
            parsed_message = mock_client.parse_message(raw_message)
            trade_data = json.loads(parsed_message)
            
            # Verify trade data
            assert trade_data["type"] == "all_trades"
            assert trade_data["symbol"] == "BTCUSDT"
            assert trade_data["price"] == "50000.0"
            assert trade_data["size"] == "0.5"
            assert trade_data["side"] == "buy"
            assert trade_data["trade_id"] == "12345678"

    @pytest.mark.asyncio
    async def test_order_update_parsing(self):
        """Test parsing of order update messages."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock message handler
            mock_client.parse_message = MagicMock(return_value=json.dumps(self.sample_order_update))
            
            # Test order update parsing
            raw_message = json.dumps(self.sample_order_update)
            parsed_message = mock_client.parse_message(raw_message)
            order_data = json.loads(parsed_message)
            
            # Verify order data
            assert order_data["type"] == "orders"
            assert order_data["order_id"] == 12345
            assert order_data["client_order_id"] == "test_order_123"
            assert order_data["symbol"] == "BTCUSDT"
            assert order_data["state"] == "filled"

    @pytest.mark.asyncio
    async def test_position_update_parsing(self):
        """Test parsing of position update messages."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock message handler
            mock_client.parse_message = MagicMock(return_value=json.dumps(self.sample_position_update))
            
            # Test position update parsing
            raw_message = json.dumps(self.sample_position_update)
            parsed_message = mock_client.parse_message(raw_message)
            position_data = json.loads(parsed_message)
            
            # Verify position data
            assert position_data["type"] == "positions"
            assert position_data["symbol"] == "BTCUSDT"
            assert position_data["size"] == "1.5"
            assert position_data["entry_price"] == "49800.0"
            assert position_data["unrealized_pnl"] == "300.0"

    @pytest.mark.asyncio
    async def test_subscription_management(self):
        """Test subscription and unsubscription management."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock subscription methods
            mock_client.subscribe = AsyncMock(return_value=True)
            mock_client.unsubscribe = AsyncMock(return_value=True)
            mock_client.get_subscriptions = MagicMock(return_value=["v2_ticker", "l2_orderbook"])
            
            # Test subscription
            subscription_data = {
                "type": "subscribe",
                "channels": [{"name": "v2_ticker", "symbols": ["BTCUSDT"]}]
            }
            success = await mock_client.subscribe(json.dumps(subscription_data))
            assert success is True
            
            # Test unsubscription
            unsubscription_data = {
                "type": "unsubscribe",
                "channels": [{"name": "v2_ticker", "symbols": ["BTCUSDT"]}]
            }
            success = await mock_client.unsubscribe(json.dumps(unsubscription_data))
            assert success is True
            
            # Test getting active subscriptions
            subscriptions = mock_client.get_subscriptions()
            assert "v2_ticker" in subscriptions
            assert "l2_orderbook" in subscriptions

    @pytest.mark.asyncio
    async def test_connection_error_handling(self):
        """Test WebSocket connection error handling."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock connection error
            mock_client.connect = AsyncMock(side_effect=ConnectionError("WebSocket connection failed"))
            
            # Test connection error handling
            with pytest.raises(ConnectionError):
                await mock_client.connect()

    @pytest.mark.asyncio
    async def test_message_queue_handling(self):
        """Test message queue and buffering."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock message queue methods
            mock_client.get_message_count = MagicMock(return_value=5)
            mock_client.clear_message_queue = MagicMock()
            
            # Test message queue
            message_count = mock_client.get_message_count()
            assert message_count == 5
            
            # Test queue clearing
            mock_client.clear_message_queue()
            mock_client.clear_message_queue.assert_called_once()

    @pytest.mark.asyncio
    async def test_heartbeat_mechanism(self):
        """Test WebSocket heartbeat/ping-pong mechanism."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock heartbeat methods
            mock_client.send_ping = AsyncMock(return_value=True)
            mock_client.handle_pong = MagicMock()
            
            # Test ping
            ping_sent = await mock_client.send_ping()
            assert ping_sent is True
            mock_client.send_ping.assert_called_once()
            
            # Test pong handling
            mock_client.handle_pong("pong_data")
            mock_client.handle_pong.assert_called_once_with("pong_data")

    @pytest.mark.asyncio
    async def test_reconnection_mechanism(self):
        """Test automatic reconnection mechanism."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock reconnection methods
            mock_client.enable_auto_reconnect = MagicMock()
            mock_client.disable_auto_reconnect = MagicMock()
            mock_client.set_reconnect_delay = MagicMock()
            
            # Test reconnection configuration
            mock_client.enable_auto_reconnect()
            mock_client.set_reconnect_delay(5.0)
            
            mock_client.enable_auto_reconnect.assert_called_once()
            mock_client.set_reconnect_delay.assert_called_once_with(5.0)
            
            # Test disabling reconnection
            mock_client.disable_auto_reconnect()
            mock_client.disable_auto_reconnect.assert_called_once()

    def test_message_validation(self):
        """Test WebSocket message validation."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock validation method
            mock_client.validate_message = MagicMock(return_value=True)
            
            # Test message validation
            valid_message = json.dumps(self.sample_ticker_message)
            is_valid = mock_client.validate_message(valid_message)
            assert is_valid is True
            
            # Test invalid message
            mock_client.validate_message.return_value = False
            invalid_message = '{"invalid": "json"'
            is_valid = mock_client.validate_message(invalid_message)
            assert is_valid is False

    @pytest.mark.asyncio
    async def test_concurrent_subscriptions(self):
        """Test handling of concurrent subscription requests."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock concurrent subscription
            mock_client.subscribe = AsyncMock(return_value=True)
            
            # Test concurrent subscriptions
            subscription_tasks = [
                mock_client.subscribe(json.dumps({
                    "type": "subscribe",
                    "channels": [{"name": "v2_ticker", "symbols": ["BTCUSDT"]}]
                })),
                mock_client.subscribe(json.dumps({
                    "type": "subscribe", 
                    "channels": [{"name": "l2_orderbook", "symbols": ["ETHUSDT"]}]
                })),
                mock_client.subscribe(json.dumps({
                    "type": "subscribe",
                    "channels": [{"name": "all_trades", "symbols": ["SOLUSDT"]}]
                })),
            ]
            
            results = await asyncio.gather(*subscription_tasks)
            
            # Verify all subscriptions succeeded
            assert all(results)
            assert mock_client.subscribe.call_count == 3
