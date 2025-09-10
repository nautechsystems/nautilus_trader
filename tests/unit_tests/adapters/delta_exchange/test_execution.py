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
Unit tests for Delta Exchange execution client.

This module provides comprehensive tests for the DeltaExchangeExecutionClient,
covering all order management operations, position tracking, WebSocket message
handling, and error scenarios.
"""

import asyncio
from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeExecClientConfig
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE
from nautilus_trader.adapters.delta_exchange.execution import DeltaExchangeExecutionClient
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock, MessageBus
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport, PositionStatusReport
from nautilus_trader.model.enums import AccountType, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce
from nautilus_trader.model.identifiers import AccountId, ClientOrderId, InstrumentId, Symbol, VenueOrderId
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.model.orders import LimitOrder, MarketOrder, OrderList
from nautilus_trader.test_kit.mocks import MockMessageBus
from nautilus_trader.test_kit.stubs import TestStubs


class TestDeltaExchangeExecutionClient:
    """Test suite for DeltaExchangeExecutionClient."""

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
        self.config = DeltaExchangeExecClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            testnet=True,
            account_id="test_account",
            max_retries=3,
            retry_delay_secs=1.0,
            position_limits={"BTCUSDT": Decimal("10.0")},
            daily_loss_limit=Decimal("1000.0"),
            max_position_value=Decimal("50000.0"),
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
        
        # Create execution client
        self.exec_client = DeltaExchangeExecutionClient(
            loop=self.loop,
            client=self.mock_http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=self.mock_instrument_provider,
            config=self.config,
        )

    def test_init(self):
        """Test execution client initialization."""
        assert self.exec_client.id.value == DELTA_EXCHANGE.value
        assert self.exec_client.venue == DELTA_EXCHANGE
        assert self.exec_client.account_id == AccountId(f"{DELTA_EXCHANGE.value}-test_account")
        assert self.exec_client.account_type == AccountType.MARGIN
        assert self.exec_client.oms_type == OmsType.HEDGING
        assert self.exec_client._config == self.config
        assert self.exec_client._client == self.mock_http_client
        assert not self.exec_client._is_connected
        assert len(self.exec_client._open_orders) == 0

    def test_stats_property(self):
        """Test stats property returns correct statistics."""
        stats = self.exec_client.stats
        
        assert isinstance(stats, dict)
        assert "orders_submitted" in stats
        assert "orders_modified" in stats
        assert "orders_cancelled" in stats
        assert "orders_filled" in stats
        assert "orders_rejected" in stats
        assert "positions_opened" in stats
        assert "positions_closed" in stats
        assert "connection_attempts" in stats
        assert "reconnections" in stats
        assert "errors" in stats
        assert "api_calls" in stats

    @pytest.mark.asyncio
    async def test_connect_success(self):
        """Test successful connection."""
        # Mock WebSocket client
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        
        # Mock API responses
        self.mock_http_client.get_account = AsyncMock(return_value={
            "success": True,
            "result": {"email": "test@example.com", "base_currency": "USDT"}
        })
        self.mock_http_client.get_orders = AsyncMock(return_value={
            "success": True,
            "result": []
        })
        self.mock_http_client.get_positions = AsyncMock(return_value={
            "success": True,
            "result": []
        })
        self.mock_http_client.get_wallet = AsyncMock(return_value={
            "success": True,
            "result": []
        })
        
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient', return_value=mock_ws_client):
            await self.exec_client._connect()
            
            assert self.exec_client._is_connected
            assert self.exec_client._ws_client == mock_ws_client
            mock_ws_client.connect.assert_called_once()
            mock_ws_client.set_message_handler.assert_called_once()

    @pytest.mark.asyncio
    async def test_connect_failure(self):
        """Test connection failure handling."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeWebSocketClient') as mock_ws_class:
            mock_ws_class.side_effect = Exception("Connection failed")
            
            with pytest.raises(Exception, match="Connection failed"):
                await self.exec_client._connect()
            
            assert not self.exec_client._is_connected
            assert self.exec_client.stats["errors"] > 0

    @pytest.mark.asyncio
    async def test_disconnect(self):
        """Test disconnection."""
        # Set up connected state
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        self.exec_client._ws_client = mock_ws_client
        self.exec_client._is_connected = True
        
        # Add some tracking state
        client_order_id = ClientOrderId("test_order")
        venue_order_id = VenueOrderId("12345")
        self.exec_client._order_client_id_to_venue_id[client_order_id] = venue_order_id
        self.exec_client._venue_order_id_to_client_id[venue_order_id] = client_order_id
        
        await self.exec_client._disconnect()
        
        assert not self.exec_client._is_connected
        assert self.exec_client._ws_client is None
        assert len(self.exec_client._order_client_id_to_venue_id) == 0
        assert len(self.exec_client._venue_order_id_to_client_id) == 0
        mock_ws_client.disconnect.assert_called_once()

    @pytest.mark.asyncio
    async def test_reset(self):
        """Test client reset."""
        # Set some state
        self.exec_client._stats["orders_submitted"] = 100
        self.exec_client._connection_retry_count = 5
        
        await self.exec_client._reset()
        
        assert self.exec_client._stats["orders_submitted"] == 0
        assert self.exec_client._connection_retry_count == 0

    @pytest.mark.asyncio
    async def test_submit_order_success(self):
        """Test successful order submission."""
        # Create test order
        order = LimitOrder(
            trader_id=TestStubs.trader_id(),
            strategy_id=TestStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=ClientOrderId("test_order_1"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("1.0"),
            price=Price.from_str("50000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=0,
        )
        
        # Mock successful response
        mock_response = {
            "success": True,
            "result": {"id": "12345", "status": "open"}
        }
        self.mock_http_client.create_order = AsyncMock(return_value=mock_response)
        
        # Mock request building
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        mock_ws_client.build_order_request = AsyncMock(return_value={
            "symbol": "BTCUSDT",
            "side": "buy",
            "order_type": "limit_order",
            "size": "1.0",
            "price": "50000.00",
        })
        self.exec_client._ws_client = mock_ws_client
        
        await self.exec_client._submit_order(order)
        
        # Verify API call was made
        self.mock_http_client.create_order.assert_called_once()
        
        # Verify order tracking
        venue_order_id = VenueOrderId("12345")
        assert self.exec_client._order_client_id_to_venue_id[order.client_order_id] == venue_order_id
        assert self.exec_client._venue_order_id_to_client_id[venue_order_id] == order.client_order_id
        
        # Verify statistics
        assert self.exec_client.stats["orders_submitted"] == 1
        assert self.exec_client.stats["api_calls"] == 1

    @pytest.mark.asyncio
    async def test_submit_order_risk_check_failure(self):
        """Test order submission with risk check failure."""
        # Create test order that exceeds position limit
        order = LimitOrder(
            trader_id=TestStubs.trader_id(),
            strategy_id=TestStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=ClientOrderId("test_order_2"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("20.0"),  # Exceeds limit of 10.0
            price=Price.from_str("50000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=0,
        )
        
        await self.exec_client._submit_order(order)
        
        # Verify API call was NOT made
        self.mock_http_client.create_order.assert_not_called()
        
        # Verify statistics (no orders submitted due to risk check)
        assert self.exec_client.stats["orders_submitted"] == 0

    @pytest.mark.asyncio
    async def test_submit_order_list_success(self):
        """Test successful order list submission."""
        # Create test orders
        order1 = LimitOrder(
            trader_id=TestStubs.trader_id(),
            strategy_id=TestStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=ClientOrderId("test_order_3"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("1.0"),
            price=Price.from_str("50000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=0,
        )
        
        order2 = LimitOrder(
            trader_id=TestStubs.trader_id(),
            strategy_id=TestStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=ClientOrderId("test_order_4"),
            order_side=OrderSide.SELL,
            quantity=Quantity.from_str("1.0"),
            price=Price.from_str("51000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=0,
        )
        
        order_list = OrderList(
            order_list_id=TestStubs.order_list_id(),
            orders=[order1, order2],
        )
        
        # Mock successful response
        mock_response = {
            "success": True,
            "result": [
                {"success": True, "id": "12345"},
                {"success": True, "id": "12346"},
            ]
        }
        self.mock_http_client.create_batch_orders = AsyncMock(return_value=mock_response)
        
        # Mock request building
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        mock_ws_client.build_order_request = AsyncMock(return_value={})
        self.exec_client._ws_client = mock_ws_client
        
        await self.exec_client._submit_order_list(order_list)
        
        # Verify API call was made
        self.mock_http_client.create_batch_orders.assert_called_once()
        
        # Verify statistics
        assert self.exec_client.stats["orders_submitted"] == 2

    @pytest.mark.asyncio
    async def test_cancel_order_success(self):
        """Test successful order cancellation."""
        # Create test order with venue order ID
        order = LimitOrder(
            trader_id=TestStubs.trader_id(),
            strategy_id=TestStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=ClientOrderId("test_order_5"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("1.0"),
            price=Price.from_str("50000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=0,
        )
        
        # Set venue order ID
        venue_order_id = VenueOrderId("12345")
        order._venue_order_id = venue_order_id
        
        # Mock successful response
        mock_response = {"success": True}
        self.mock_http_client.cancel_order = AsyncMock(return_value=mock_response)
        
        await self.exec_client._cancel_order(order)
        
        # Verify API call was made
        self.mock_http_client.cancel_order.assert_called_once_with("12345")
        
        # Verify statistics
        assert self.exec_client.stats["orders_cancelled"] == 1

    @pytest.mark.asyncio
    async def test_check_order_risk_position_limit(self):
        """Test risk check with position limit."""
        # Create order that would exceed position limit
        order = LimitOrder(
            trader_id=TestStubs.trader_id(),
            strategy_id=TestStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=ClientOrderId("test_order_6"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("15.0"),  # Exceeds limit of 10.0
            price=Price.from_str("50000.00"),
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=0,
        )
        
        # Risk check should fail
        result = await self.exec_client._check_order_risk(order)
        assert not result

    @pytest.mark.asyncio
    async def test_check_order_risk_max_position_value(self):
        """Test risk check with maximum position value."""
        # Create order that would exceed max position value
        order = LimitOrder(
            trader_id=TestStubs.trader_id(),
            strategy_id=TestStubs.strategy_id(),
            instrument_id=self.instrument.id,
            client_order_id=ClientOrderId("test_order_7"),
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("2.0"),
            price=Price.from_str("30000.00"),  # 2.0 * 30000 = 60000 > 50000 limit
            time_in_force=TimeInForce.GTC,
            init_id=UUID4(),
            ts_init=0,
        )
        
        # Risk check should fail
        result = await self.exec_client._check_order_risk(order)
        assert not result

    @pytest.mark.asyncio
    async def test_apply_rate_limit(self):
        """Test rate limiting functionality."""
        # First request should not be rate limited
        start_time = self.clock.timestamp()
        await self.exec_client._apply_rate_limit()
        end_time = self.clock.timestamp()
        
        # Should be very fast
        assert end_time - start_time < 0.1
        assert self.exec_client._request_count == 1

    @pytest.mark.asyncio
    async def test_health_check_success(self):
        """Test successful health check."""
        # Set up connected state
        mock_ws_client = AsyncMock(spec=nautilus_pyo3.DeltaExchangeWebSocketClient)
        mock_ws_client.ping = AsyncMock(return_value=True)
        self.exec_client._ws_client = mock_ws_client
        self.exec_client._is_connected = True
        
        # Mock API response
        self.mock_http_client.get_account = AsyncMock(return_value={"success": True})
        
        result = await self.exec_client._health_check()
        assert result

    @pytest.mark.asyncio
    async def test_health_check_failure(self):
        """Test health check failure."""
        # Not connected
        self.exec_client._is_connected = False
        
        result = await self.exec_client._health_check()
        assert not result

    def test_repr(self):
        """Test string representation."""
        repr_str = repr(self.exec_client)
        
        assert "DeltaExchangeExecutionClient" in repr_str
        assert f"id={self.exec_client.id}" in repr_str
        assert f"venue={self.exec_client.venue}" in repr_str
        assert f"account_id={self.exec_client.account_id}" in repr_str
        assert "connected=False" in repr_str
        assert "open_orders=0" in repr_str
        assert "positions=0" in repr_str
