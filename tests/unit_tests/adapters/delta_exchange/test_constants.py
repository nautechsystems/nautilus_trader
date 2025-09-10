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
Unit tests for Delta Exchange constants and enumerations.

This module tests all Delta Exchange constants, enumerations, mappings, and
validation patterns to ensure they are correctly defined and provide proper
type safety and integration with Nautilus Trader.
"""

import re
from decimal import Decimal

import pytest

from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE,
    DELTA_EXCHANGE_ALL_CONSTANTS,
    DELTA_EXCHANGE_API_KEY_PATTERN,
    DELTA_EXCHANGE_API_SECRET_PATTERN,
    DELTA_EXCHANGE_CLIENT_ID,
    DELTA_EXCHANGE_CURRENCY_MAP,
    DELTA_EXCHANGE_DEFAULT_CONFIG,
    DELTA_EXCHANGE_ENVIRONMENTS,
    DELTA_EXCHANGE_ERROR_CODES,
    DELTA_EXCHANGE_FEATURE_FLAGS,
    DELTA_EXCHANGE_HTTP_URLS,
    DELTA_EXCHANGE_LOGGER_NAMES,
    DELTA_EXCHANGE_MAX_ORDER_PRICE,
    DELTA_EXCHANGE_MIN_ORDER_PRICE,
    DELTA_EXCHANGE_ORDER_STATES,
    DELTA_EXCHANGE_ORDER_TYPES,
    DELTA_EXCHANGE_PERFORMANCE_METRICS,
    DELTA_EXCHANGE_PRODUCT_TYPES,
    DELTA_EXCHANGE_SUPPORTED_ORDER_SIDES,
    DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES,
    DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE,
    DELTA_EXCHANGE_SYMBOL_PATTERN,
    DELTA_EXCHANGE_TIME_IN_FORCE,
    DELTA_EXCHANGE_TO_NAUTILUS_ORDER_SIDE,
    DELTA_EXCHANGE_TO_NAUTILUS_ORDER_STATUS,
    DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE,
    DELTA_EXCHANGE_TO_NAUTILUS_TIME_IN_FORCE,
    DELTA_EXCHANGE_VENUE,
    DELTA_EXCHANGE_WS_ALL_CHANNELS,
    DELTA_EXCHANGE_WS_PRIVATE_CHANNELS,
    DELTA_EXCHANGE_WS_PUBLIC_CHANNELS,
    DELTA_EXCHANGE_WS_URLS,
    DeltaExchangeOrderSide,
    DeltaExchangeOrderStatus,
    DeltaExchangeOrderType,
    DeltaExchangeProductType,
    DeltaExchangeTimeInForce,
    DeltaExchangeTradingStatus,
    NAUTILUS_TO_DELTA_EXCHANGE_ORDER_SIDE,
    NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE,
    NAUTILUS_TO_DELTA_EXCHANGE_TIME_IN_FORCE,
)
from nautilus_trader.model.enums import OrderSide, OrderStatus, OrderType, TimeInForce
from nautilus_trader.model.identifiers import ClientId, Venue


class TestDeltaExchangeVenueConstants:
    """Test Delta Exchange venue and client identifier constants."""

    def test_venue_identifier(self):
        """Test venue identifier is correctly defined."""
        assert DELTA_EXCHANGE_VENUE == Venue("DELTA_EXCHANGE")
        assert isinstance(DELTA_EXCHANGE_VENUE, Venue)
        assert str(DELTA_EXCHANGE_VENUE) == "DELTA_EXCHANGE"

    def test_client_identifier(self):
        """Test client identifier is correctly defined."""
        assert DELTA_EXCHANGE_CLIENT_ID == ClientId("DELTA_EXCHANGE")
        assert isinstance(DELTA_EXCHANGE_CLIENT_ID, ClientId)
        assert str(DELTA_EXCHANGE_CLIENT_ID) == "DELTA_EXCHANGE"

    def test_backward_compatibility(self):
        """Test backward compatibility with legacy constants."""
        assert DELTA_EXCHANGE == DELTA_EXCHANGE_VENUE


class TestDeltaExchangeURLConstants:
    """Test Delta Exchange URL constants."""

    def test_http_urls(self):
        """Test HTTP URL constants are valid."""
        assert "production" in DELTA_EXCHANGE_HTTP_URLS
        assert "testnet" in DELTA_EXCHANGE_HTTP_URLS
        assert "sandbox" in DELTA_EXCHANGE_HTTP_URLS
        
        for env, url in DELTA_EXCHANGE_HTTP_URLS.items():
            assert url.startswith("https://")
            assert "delta.exchange" in url

    def test_websocket_urls(self):
        """Test WebSocket URL constants are valid."""
        assert "production" in DELTA_EXCHANGE_WS_URLS
        assert "testnet" in DELTA_EXCHANGE_WS_URLS
        assert "sandbox" in DELTA_EXCHANGE_WS_URLS
        
        for env, url in DELTA_EXCHANGE_WS_URLS.items():
            assert url.startswith("wss://")
            assert "delta.exchange" in url

    def test_environments(self):
        """Test environment constants."""
        assert len(DELTA_EXCHANGE_ENVIRONMENTS) == 3
        assert "production" in DELTA_EXCHANGE_ENVIRONMENTS
        assert "testnet" in DELTA_EXCHANGE_ENVIRONMENTS
        assert "sandbox" in DELTA_EXCHANGE_ENVIRONMENTS


class TestDeltaExchangeWebSocketConstants:
    """Test Delta Exchange WebSocket constants."""

    def test_public_channels(self):
        """Test public WebSocket channels."""
        assert len(DELTA_EXCHANGE_WS_PUBLIC_CHANNELS) > 0
        assert "v2_ticker" in DELTA_EXCHANGE_WS_PUBLIC_CHANNELS
        assert "l2_orderbook" in DELTA_EXCHANGE_WS_PUBLIC_CHANNELS
        assert "all_trades" in DELTA_EXCHANGE_WS_PUBLIC_CHANNELS

    def test_private_channels(self):
        """Test private WebSocket channels."""
        assert len(DELTA_EXCHANGE_WS_PRIVATE_CHANNELS) > 0
        assert "orders" in DELTA_EXCHANGE_WS_PRIVATE_CHANNELS
        assert "positions" in DELTA_EXCHANGE_WS_PRIVATE_CHANNELS
        assert "margins" in DELTA_EXCHANGE_WS_PRIVATE_CHANNELS

    def test_all_channels(self):
        """Test all channels collection."""
        expected_count = len(DELTA_EXCHANGE_WS_PUBLIC_CHANNELS) + len(DELTA_EXCHANGE_WS_PRIVATE_CHANNELS)
        assert len(DELTA_EXCHANGE_WS_ALL_CHANNELS) == expected_count
        
        for channel in DELTA_EXCHANGE_WS_PUBLIC_CHANNELS:
            assert channel in DELTA_EXCHANGE_WS_ALL_CHANNELS
        
        for channel in DELTA_EXCHANGE_WS_PRIVATE_CHANNELS:
            assert channel in DELTA_EXCHANGE_WS_ALL_CHANNELS


class TestDeltaExchangeEnumerations:
    """Test Delta Exchange enumeration classes."""

    def test_product_type_enum(self):
        """Test DeltaExchangeProductType enumeration."""
        assert DeltaExchangeProductType.PERPETUAL_FUTURES.value == "perpetual_futures"
        assert DeltaExchangeProductType.CALL_OPTIONS.value == "call_options"
        assert DeltaExchangeProductType.PUT_OPTIONS.value == "put_options"
        
        # Test properties
        assert DeltaExchangeProductType.PERPETUAL_FUTURES.is_perpetual
        assert not DeltaExchangeProductType.CALL_OPTIONS.is_perpetual
        assert DeltaExchangeProductType.CALL_OPTIONS.is_option
        assert DeltaExchangeProductType.PUT_OPTIONS.is_option
        assert not DeltaExchangeProductType.PERPETUAL_FUTURES.is_option

    def test_order_type_enum(self):
        """Test DeltaExchangeOrderType enumeration."""
        assert DeltaExchangeOrderType.LIMIT_ORDER.value == "limit_order"
        assert DeltaExchangeOrderType.MARKET_ORDER.value == "market_order"
        assert DeltaExchangeOrderType.STOP_LOSS_ORDER.value == "stop_loss_order"
        assert DeltaExchangeOrderType.TAKE_PROFIT_ORDER.value == "take_profit_order"
        
        # Test properties
        assert DeltaExchangeOrderType.MARKET_ORDER.is_market
        assert not DeltaExchangeOrderType.LIMIT_ORDER.is_market
        assert DeltaExchangeOrderType.LIMIT_ORDER.is_limit
        assert not DeltaExchangeOrderType.MARKET_ORDER.is_limit
        assert DeltaExchangeOrderType.STOP_LOSS_ORDER.is_stop
        assert DeltaExchangeOrderType.TAKE_PROFIT_ORDER.is_stop
        assert not DeltaExchangeOrderType.LIMIT_ORDER.is_stop

    def test_order_status_enum(self):
        """Test DeltaExchangeOrderStatus enumeration."""
        assert DeltaExchangeOrderStatus.OPEN.value == "open"
        assert DeltaExchangeOrderStatus.PENDING.value == "pending"
        assert DeltaExchangeOrderStatus.CLOSED.value == "closed"
        assert DeltaExchangeOrderStatus.CANCELLED.value == "cancelled"
        
        # Test properties
        assert DeltaExchangeOrderStatus.OPEN.is_active
        assert DeltaExchangeOrderStatus.PENDING.is_active
        assert DeltaExchangeOrderStatus.PARTIALLY_FILLED.is_active
        assert not DeltaExchangeOrderStatus.CLOSED.is_active
        
        assert DeltaExchangeOrderStatus.CLOSED.is_terminal
        assert DeltaExchangeOrderStatus.CANCELLED.is_terminal
        assert DeltaExchangeOrderStatus.REJECTED.is_terminal
        assert not DeltaExchangeOrderStatus.OPEN.is_terminal

    def test_time_in_force_enum(self):
        """Test DeltaExchangeTimeInForce enumeration."""
        assert DeltaExchangeTimeInForce.GTC.value == "gtc"
        assert DeltaExchangeTimeInForce.IOC.value == "ioc"
        assert DeltaExchangeTimeInForce.FOK.value == "fok"
        assert DeltaExchangeTimeInForce.GTD.value == "gtd"
        
        # Test properties
        assert DeltaExchangeTimeInForce.IOC.is_immediate
        assert DeltaExchangeTimeInForce.FOK.is_immediate
        assert not DeltaExchangeTimeInForce.GTC.is_immediate
        assert not DeltaExchangeTimeInForce.GTD.is_immediate

    def test_order_side_enum(self):
        """Test DeltaExchangeOrderSide enumeration."""
        assert DeltaExchangeOrderSide.BUY.value == "buy"
        assert DeltaExchangeOrderSide.SELL.value == "sell"

    def test_trading_status_enum(self):
        """Test DeltaExchangeTradingStatus enumeration."""
        assert DeltaExchangeTradingStatus.ACTIVE.value == "active"
        assert DeltaExchangeTradingStatus.INACTIVE.value == "inactive"
        assert DeltaExchangeTradingStatus.EXPIRED.value == "expired"
        
        # Test properties
        assert DeltaExchangeTradingStatus.ACTIVE.is_tradable
        assert not DeltaExchangeTradingStatus.INACTIVE.is_tradable
        assert not DeltaExchangeTradingStatus.EXPIRED.is_tradable


class TestDeltaExchangeDataModelMappings:
    """Test Delta Exchange to Nautilus data model mappings."""

    def test_order_type_mappings(self):
        """Test order type mappings are complete and bidirectional."""
        # Test Delta Exchange to Nautilus mapping
        assert DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE["limit_order"] == OrderType.LIMIT
        assert DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE["market_order"] == OrderType.MARKET
        assert DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE["stop_loss_order"] == OrderType.STOP_MARKET
        
        # Test Nautilus to Delta Exchange mapping
        assert NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE[OrderType.LIMIT] == "limit_order"
        assert NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE[OrderType.MARKET] == "market_order"
        assert NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE[OrderType.STOP_MARKET] == "stop_loss_order"
        
        # Test bidirectional consistency
        for delta_type, nautilus_type in DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE.items():
            if nautilus_type in NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE:
                assert NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE[nautilus_type] == delta_type

    def test_order_status_mappings(self):
        """Test order status mappings."""
        assert DELTA_EXCHANGE_TO_NAUTILUS_ORDER_STATUS["open"] == OrderStatus.ACCEPTED
        assert DELTA_EXCHANGE_TO_NAUTILUS_ORDER_STATUS["pending"] == OrderStatus.PENDING_NEW
        assert DELTA_EXCHANGE_TO_NAUTILUS_ORDER_STATUS["closed"] == OrderStatus.FILLED
        assert DELTA_EXCHANGE_TO_NAUTILUS_ORDER_STATUS["cancelled"] == OrderStatus.CANCELED

    def test_time_in_force_mappings(self):
        """Test time-in-force mappings are complete and bidirectional."""
        # Test Delta Exchange to Nautilus mapping
        assert DELTA_EXCHANGE_TO_NAUTILUS_TIME_IN_FORCE["gtc"] == TimeInForce.GTC
        assert DELTA_EXCHANGE_TO_NAUTILUS_TIME_IN_FORCE["ioc"] == TimeInForce.IOC
        assert DELTA_EXCHANGE_TO_NAUTILUS_TIME_IN_FORCE["fok"] == TimeInForce.FOK
        
        # Test Nautilus to Delta Exchange mapping
        assert NAUTILUS_TO_DELTA_EXCHANGE_TIME_IN_FORCE[TimeInForce.GTC] == "gtc"
        assert NAUTILUS_TO_DELTA_EXCHANGE_TIME_IN_FORCE[TimeInForce.IOC] == "ioc"
        assert NAUTILUS_TO_DELTA_EXCHANGE_TIME_IN_FORCE[TimeInForce.FOK] == "fok"
        
        # Test bidirectional consistency
        for delta_tif, nautilus_tif in DELTA_EXCHANGE_TO_NAUTILUS_TIME_IN_FORCE.items():
            assert NAUTILUS_TO_DELTA_EXCHANGE_TIME_IN_FORCE[nautilus_tif] == delta_tif

    def test_order_side_mappings(self):
        """Test order side mappings are complete and bidirectional."""
        # Test Delta Exchange to Nautilus mapping
        assert DELTA_EXCHANGE_TO_NAUTILUS_ORDER_SIDE["buy"] == OrderSide.BUY
        assert DELTA_EXCHANGE_TO_NAUTILUS_ORDER_SIDE["sell"] == OrderSide.SELL
        
        # Test Nautilus to Delta Exchange mapping
        assert NAUTILUS_TO_DELTA_EXCHANGE_ORDER_SIDE[OrderSide.BUY] == "buy"
        assert NAUTILUS_TO_DELTA_EXCHANGE_ORDER_SIDE[OrderSide.SELL] == "sell"
        
        # Test bidirectional consistency
        for delta_side, nautilus_side in DELTA_EXCHANGE_TO_NAUTILUS_ORDER_SIDE.items():
            assert NAUTILUS_TO_DELTA_EXCHANGE_ORDER_SIDE[nautilus_side] == delta_side


class TestDeltaExchangeSupportedTypes:
    """Test Delta Exchange supported Nautilus types."""

    def test_supported_order_types(self):
        """Test supported order types."""
        assert OrderType.MARKET in DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES
        assert OrderType.LIMIT in DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES
        assert OrderType.STOP_MARKET in DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES
        assert OrderType.STOP_LIMIT in DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES

    def test_supported_time_in_force(self):
        """Test supported time-in-force values."""
        assert TimeInForce.GTC in DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE
        assert TimeInForce.IOC in DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE
        assert TimeInForce.FOK in DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE
        assert TimeInForce.GTD in DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE

    def test_supported_order_sides(self):
        """Test supported order sides."""
        assert OrderSide.BUY in DELTA_EXCHANGE_SUPPORTED_ORDER_SIDES
        assert OrderSide.SELL in DELTA_EXCHANGE_SUPPORTED_ORDER_SIDES


class TestDeltaExchangeValidationPatterns:
    """Test Delta Exchange validation patterns."""

    def test_api_key_pattern(self):
        """Test API key validation pattern."""
        pattern = re.compile(DELTA_EXCHANGE_API_KEY_PATTERN)
        
        # Valid API keys
        assert pattern.match("abcd1234efgh5678ijkl9012mnop3456")
        assert pattern.match("ABCD1234EFGH5678IJKL9012MNOP3456")
        assert pattern.match("aBcD1234eFgH5678iJkL9012mNoP3456qRsT7890")
        
        # Invalid API keys
        assert not pattern.match("short")
        assert not pattern.match("contains-special-chars!")
        assert not pattern.match("")

    def test_api_secret_pattern(self):
        """Test API secret validation pattern."""
        pattern = re.compile(DELTA_EXCHANGE_API_SECRET_PATTERN)
        
        # Valid API secrets
        assert pattern.match("YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY=")
        assert pattern.match("dGVzdC1zZWNyZXQtc3RyaW5nLWZvci12YWxpZGF0aW9u")
        
        # Invalid API secrets
        assert not pattern.match("short")
        assert not pattern.match("invalid-chars!")
        assert not pattern.match("")

    def test_symbol_pattern(self):
        """Test symbol validation pattern."""
        pattern = re.compile(DELTA_EXCHANGE_SYMBOL_PATTERN)
        
        # Valid symbols
        assert pattern.match("BTCUSDT")
        assert pattern.match("ETH_USDT")
        assert pattern.match("BTC-PERPETUAL")
        assert pattern.match("ETH_CALL_2024")
        
        # Invalid symbols
        assert not pattern.match("btc/usdt")
        assert not pattern.match("BTC USDT")
        assert not pattern.match("")


class TestDeltaExchangeErrorCodes:
    """Test Delta Exchange error codes."""

    def test_error_codes_defined(self):
        """Test error codes are properly defined."""
        assert 400 in DELTA_EXCHANGE_ERROR_CODES
        assert 401 in DELTA_EXCHANGE_ERROR_CODES
        assert 429 in DELTA_EXCHANGE_ERROR_CODES
        assert 500 in DELTA_EXCHANGE_ERROR_CODES
        
        # Test custom error codes
        assert 1001 in DELTA_EXCHANGE_ERROR_CODES
        assert 2001 in DELTA_EXCHANGE_ERROR_CODES
        assert 3001 in DELTA_EXCHANGE_ERROR_CODES
        assert 4001 in DELTA_EXCHANGE_ERROR_CODES

    def test_error_code_messages(self):
        """Test error code messages are descriptive."""
        for code, message in DELTA_EXCHANGE_ERROR_CODES.items():
            assert isinstance(message, str)
            assert len(message) > 0
            assert message != ""


class TestDeltaExchangeConfiguration:
    """Test Delta Exchange configuration constants."""

    def test_default_config(self):
        """Test default configuration values."""
        assert "testnet" in DELTA_EXCHANGE_DEFAULT_CONFIG
        assert "sandbox" in DELTA_EXCHANGE_DEFAULT_CONFIG
        assert "enable_private_channels" in DELTA_EXCHANGE_DEFAULT_CONFIG
        assert "product_types" in DELTA_EXCHANGE_DEFAULT_CONFIG
        assert "max_retries" in DELTA_EXCHANGE_DEFAULT_CONFIG

    def test_feature_flags(self):
        """Test feature flags."""
        assert "enable_portfolio_margins" in DELTA_EXCHANGE_FEATURE_FLAGS
        assert "enable_mmp" in DELTA_EXCHANGE_FEATURE_FLAGS
        assert "enable_batch_orders" in DELTA_EXCHANGE_FEATURE_FLAGS
        assert "enable_options_trading" in DELTA_EXCHANGE_FEATURE_FLAGS

    def test_currency_map(self):
        """Test currency mappings."""
        assert "USDT" in DELTA_EXCHANGE_CURRENCY_MAP
        assert "BTC" in DELTA_EXCHANGE_CURRENCY_MAP
        assert "ETH" in DELTA_EXCHANGE_CURRENCY_MAP
        
        for delta_currency, standard_currency in DELTA_EXCHANGE_CURRENCY_MAP.items():
            assert isinstance(delta_currency, str)
            assert isinstance(standard_currency, str)
            assert len(delta_currency) > 0
            assert len(standard_currency) > 0


class TestDeltaExchangeBackwardCompatibility:
    """Test backward compatibility with legacy constants."""

    def test_legacy_product_types(self):
        """Test legacy product types list."""
        assert len(DELTA_EXCHANGE_PRODUCT_TYPES) == len(DeltaExchangeProductType)
        for product_type in DeltaExchangeProductType:
            assert product_type.value in DELTA_EXCHANGE_PRODUCT_TYPES

    def test_legacy_order_types(self):
        """Test legacy order types list."""
        assert len(DELTA_EXCHANGE_ORDER_TYPES) == len(DeltaExchangeOrderType)
        for order_type in DeltaExchangeOrderType:
            assert order_type.value in DELTA_EXCHANGE_ORDER_TYPES

    def test_legacy_order_states(self):
        """Test legacy order states list."""
        assert len(DELTA_EXCHANGE_ORDER_STATES) == len(DeltaExchangeOrderStatus)
        for order_status in DeltaExchangeOrderStatus:
            assert order_status.value in DELTA_EXCHANGE_ORDER_STATES

    def test_legacy_time_in_force(self):
        """Test legacy time-in-force list."""
        assert len(DELTA_EXCHANGE_TIME_IN_FORCE) == len(DeltaExchangeTimeInForce)
        for tif in DeltaExchangeTimeInForce:
            assert tif.value in DELTA_EXCHANGE_TIME_IN_FORCE


class TestDeltaExchangeComprehensiveConstants:
    """Test comprehensive constants collection."""

    def test_all_constants_structure(self):
        """Test the comprehensive constants collection structure."""
        assert "venue" in DELTA_EXCHANGE_ALL_CONSTANTS
        assert "client_id" in DELTA_EXCHANGE_ALL_CONSTANTS
        assert "http_urls" in DELTA_EXCHANGE_ALL_CONSTANTS
        assert "ws_urls" in DELTA_EXCHANGE_ALL_CONSTANTS
        assert "public_channels" in DELTA_EXCHANGE_ALL_CONSTANTS
        assert "private_channels" in DELTA_EXCHANGE_ALL_CONSTANTS
        assert "product_types" in DELTA_EXCHANGE_ALL_CONSTANTS
        assert "order_types" in DELTA_EXCHANGE_ALL_CONSTANTS
        assert "supported_order_types" in DELTA_EXCHANGE_ALL_CONSTANTS
        assert "order_type_mappings" in DELTA_EXCHANGE_ALL_CONSTANTS
        assert "default_config" in DELTA_EXCHANGE_ALL_CONSTANTS

    def test_all_constants_completeness(self):
        """Test that all constants collection is comprehensive."""
        constants = DELTA_EXCHANGE_ALL_CONSTANTS
        
        # Test venue and identifiers
        assert constants["venue"] == DELTA_EXCHANGE_VENUE
        assert constants["client_id"] == DELTA_EXCHANGE_CLIENT_ID
        
        # Test URLs
        assert constants["http_urls"] == DELTA_EXCHANGE_HTTP_URLS
        assert constants["ws_urls"] == DELTA_EXCHANGE_WS_URLS
        
        # Test channels
        assert constants["public_channels"] == DELTA_EXCHANGE_WS_PUBLIC_CHANNELS
        assert constants["private_channels"] == DELTA_EXCHANGE_WS_PRIVATE_CHANNELS
        
        # Test supported types
        assert constants["supported_order_types"] == DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES
        assert constants["supported_time_in_force"] == DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE
        
        # Test configuration
        assert constants["default_config"] == DELTA_EXCHANGE_DEFAULT_CONFIG
        assert constants["feature_flags"] == DELTA_EXCHANGE_FEATURE_FLAGS


class TestDeltaExchangeNumericConstants:
    """Test Delta Exchange numeric constants and limits."""

    def test_price_limits(self):
        """Test price limit constants."""
        min_price = Decimal(DELTA_EXCHANGE_MIN_ORDER_PRICE)
        max_price = Decimal(DELTA_EXCHANGE_MAX_ORDER_PRICE)
        
        assert min_price > 0
        assert max_price > min_price
        assert min_price == Decimal("0.000001")
        assert max_price == Decimal("1000000000.0")

    def test_performance_metrics(self):
        """Test performance metrics list."""
        assert len(DELTA_EXCHANGE_PERFORMANCE_METRICS) > 0
        assert "http_request_count" in DELTA_EXCHANGE_PERFORMANCE_METRICS
        assert "ws_connection_count" in DELTA_EXCHANGE_PERFORMANCE_METRICS
        assert "order_submission_latency" in DELTA_EXCHANGE_PERFORMANCE_METRICS

    def test_logger_names(self):
        """Test logger names dictionary."""
        assert "adapter" in DELTA_EXCHANGE_LOGGER_NAMES
        assert "data" in DELTA_EXCHANGE_LOGGER_NAMES
        assert "execution" in DELTA_EXCHANGE_LOGGER_NAMES
        assert "factories" in DELTA_EXCHANGE_LOGGER_NAMES
        
        for component, logger_name in DELTA_EXCHANGE_LOGGER_NAMES.items():
            assert logger_name.startswith("nautilus_trader.adapters.delta_exchange")
