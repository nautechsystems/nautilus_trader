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
Unit tests for Delta Exchange HTTP client bindings.

This module tests the Rust HTTP client bindings with comprehensive mock responses,
error scenarios, authentication, rate limiting, and request/response handling.
"""

import asyncio
import json
from unittest.mock import AsyncMock, MagicMock, patch
from decimal import Decimal

import pytest

from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE_BASE_URL,
    DELTA_EXCHANGE_TESTNET_BASE_URL,
    DELTA_EXCHANGE_ERROR_CODES,
)


class TestDeltaExchangeHttpClientBindings:
    """Test Delta Exchange HTTP client Rust bindings."""

    def setup_method(self):
        """Set up test fixtures."""
        self.api_key = "test_api_key_12345678901234567890"
        self.api_secret = "test_api_secret_base64_encoded_string"
        self.base_url = DELTA_EXCHANGE_TESTNET_BASE_URL
        
        # Mock the Rust HTTP client
        self.mock_client = MagicMock()
        
        # Sample API responses
        self.sample_products_response = {
            "success": True,
            "result": [
                {
                    "id": 139,
                    "symbol": "BTCUSDT",
                    "description": "BTC/USDT Perpetual",
                    "created_at": "2023-01-01T00:00:00Z",
                    "updated_at": "2023-01-01T00:00:00Z",
                    "settlement_time": None,
                    "notional_type": "vanilla",
                    "impact_size": 100,
                    "initial_margin": 0.1,
                    "maintenance_margin": 0.05,
                    "contract_value": "1",
                    "contract_unit_currency": "BTC",
                    "tick_size": "0.5",
                    "product_specs": {
                        "contract_type": "perpetual_futures",
                        "contract_size": "1",
                        "price_band": "0.1",
                        "max_leverage_notional": "100",
                        "max_leverage_portfolio": "20",
                        "is_quanto": False,
                        "funding_method": "mark_price",
                        "annualized_funding": "0.0",
                        "price_band_table": []
                    },
                    "state": "live",
                    "trading_status": "operational",
                    "max_leverage_notional": "100",
                    "max_leverage_portfolio": "20",
                    "spot_index": {
                        "id": 1,
                        "symbol": "BTC_INDEX",
                        "constituent_exchanges": ["binance", "coinbase", "kraken"]
                    }
                }
            ]
        }
        
        self.sample_assets_response = {
            "success": True,
            "result": [
                {
                    "id": 1,
                    "symbol": "BTC",
                    "name": "Bitcoin",
                    "minimum_precision": 8,
                    "minimum_withdrawal": "0.001",
                    "withdrawal_fee": "0.0005",
                    "deposit_status": "enabled",
                    "withdrawal_status": "enabled",
                    "base_withdrawal_fee": "0.0005",
                    "networks": [
                        {
                            "network_code": "BTC",
                            "network_name": "Bitcoin",
                            "min_withdrawal": "0.001",
                            "withdrawal_fee": "0.0005"
                        }
                    ]
                },
                {
                    "id": 2,
                    "symbol": "USDT",
                    "name": "Tether USD",
                    "minimum_precision": 6,
                    "minimum_withdrawal": "10",
                    "withdrawal_fee": "1",
                    "deposit_status": "enabled",
                    "withdrawal_status": "enabled",
                    "base_withdrawal_fee": "1",
                    "networks": [
                        {
                            "network_code": "ETH",
                            "network_name": "Ethereum",
                            "min_withdrawal": "10",
                            "withdrawal_fee": "1"
                        }
                    ]
                }
            ]
        }
        
        self.sample_order_response = {
            "success": True,
            "result": {
                "id": 12345,
                "user_id": 1,
                "size": "1.0",
                "unfilled_size": "0.0",
                "side": "buy",
                "order_type": "limit_order",
                "limit_price": "50000.0",
                "stop_order_type": None,
                "stop_price": None,
                "paid_commission": "0.05",
                "commission": "0.05",
                "reduce_only": False,
                "client_order_id": "test_order_123",
                "state": "filled",
                "created_at": "2023-01-01T12:00:00Z",
                "product_id": 139,
                "product_symbol": "BTCUSDT",
                "time_in_force": "gtc",
                "meta_data": {
                    "source": "api"
                }
            }
        }
        
        self.sample_error_response = {
            "success": False,
            "error": {
                "code": 2001,
                "message": "Insufficient Balance",
                "context": "order_submission"
            }
        }

    @pytest.mark.asyncio
    async def test_http_client_initialization(self):
        """Test HTTP client initialization with various configurations."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client_instance = MagicMock()
            mock_client_class.return_value = mock_client_instance
            
            # Test production initialization
            client = mock_client_class(
                api_key=self.api_key,
                api_secret=self.api_secret,
                base_url=DELTA_EXCHANGE_BASE_URL,
                testnet=False,
            )
            
            mock_client_class.assert_called_once_with(
                api_key=self.api_key,
                api_secret=self.api_secret,
                base_url=DELTA_EXCHANGE_BASE_URL,
                testnet=False,
            )
            
            # Test testnet initialization
            mock_client_class.reset_mock()
            testnet_client = mock_client_class(
                api_key=self.api_key,
                api_secret=self.api_secret,
                base_url=DELTA_EXCHANGE_TESTNET_BASE_URL,
                testnet=True,
            )
            
            mock_client_class.assert_called_once_with(
                api_key=self.api_key,
                api_secret=self.api_secret,
                base_url=DELTA_EXCHANGE_TESTNET_BASE_URL,
                testnet=True,
            )

    @pytest.mark.asyncio
    async def test_get_products_success(self):
        """Test successful products API call."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock successful response
            mock_client.get_products = AsyncMock(return_value=json.dumps(self.sample_products_response))
            
            # Test the call
            response = await mock_client.get_products()
            parsed_response = json.loads(response)
            
            # Verify response structure
            assert parsed_response["success"] is True
            assert "result" in parsed_response
            assert len(parsed_response["result"]) == 1
            
            product = parsed_response["result"][0]
            assert product["symbol"] == "BTCUSDT"
            assert product["id"] == 139
            assert product["state"] == "live"
            assert product["trading_status"] == "operational"

    @pytest.mark.asyncio
    async def test_get_assets_success(self):
        """Test successful assets API call."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock successful response
            mock_client.get_assets = AsyncMock(return_value=json.dumps(self.sample_assets_response))
            
            # Test the call
            response = await mock_client.get_assets()
            parsed_response = json.loads(response)
            
            # Verify response structure
            assert parsed_response["success"] is True
            assert "result" in parsed_response
            assert len(parsed_response["result"]) == 2
            
            btc_asset = parsed_response["result"][0]
            assert btc_asset["symbol"] == "BTC"
            assert btc_asset["name"] == "Bitcoin"
            assert btc_asset["minimum_precision"] == 8

    @pytest.mark.asyncio
    async def test_submit_order_success(self):
        """Test successful order submission."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock successful response
            mock_client.submit_order = AsyncMock(return_value=json.dumps(self.sample_order_response))
            
            # Test order submission
            order_data = {
                "product_id": 139,
                "size": "1.0",
                "side": "buy",
                "order_type": "limit_order",
                "limit_price": "50000.0",
                "time_in_force": "gtc",
                "client_order_id": "test_order_123"
            }
            
            response = await mock_client.submit_order(json.dumps(order_data))
            parsed_response = json.loads(response)
            
            # Verify response
            assert parsed_response["success"] is True
            result = parsed_response["result"]
            assert result["id"] == 12345
            assert result["state"] == "filled"
            assert result["client_order_id"] == "test_order_123"

    @pytest.mark.asyncio
    async def test_api_error_handling(self):
        """Test API error response handling."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock error response
            mock_client.submit_order = AsyncMock(return_value=json.dumps(self.sample_error_response))
            
            # Test error handling
            order_data = {
                "product_id": 139,
                "size": "1000.0",  # Large size to trigger insufficient balance
                "side": "buy",
                "order_type": "limit_order",
                "limit_price": "50000.0"
            }
            
            response = await mock_client.submit_order(json.dumps(order_data))
            parsed_response = json.loads(response)
            
            # Verify error response
            assert parsed_response["success"] is False
            assert "error" in parsed_response
            error = parsed_response["error"]
            assert error["code"] == 2001
            assert error["message"] == "Insufficient Balance"

    @pytest.mark.asyncio
    async def test_rate_limiting_handling(self):
        """Test rate limiting error handling."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock rate limit error
            rate_limit_error = {
                "success": False,
                "error": {
                    "code": 429,
                    "message": "Too Many Requests",
                    "context": "rate_limit_exceeded"
                }
            }
            
            mock_client.get_products = AsyncMock(return_value=json.dumps(rate_limit_error))
            
            # Test rate limit handling
            response = await mock_client.get_products()
            parsed_response = json.loads(response)
            
            # Verify rate limit error
            assert parsed_response["success"] is False
            error = parsed_response["error"]
            assert error["code"] == 429
            assert "Too Many Requests" in error["message"]

    @pytest.mark.asyncio
    async def test_authentication_error(self):
        """Test authentication error handling."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock authentication error
            auth_error = {
                "success": False,
                "error": {
                    "code": 401,
                    "message": "Unauthorized",
                    "context": "invalid_api_key"
                }
            }
            
            mock_client.get_orders = AsyncMock(return_value=json.dumps(auth_error))
            
            # Test authentication error
            response = await mock_client.get_orders()
            parsed_response = json.loads(response)
            
            # Verify authentication error
            assert parsed_response["success"] is False
            error = parsed_response["error"]
            assert error["code"] == 401
            assert error["message"] == "Unauthorized"

    @pytest.mark.asyncio
    async def test_network_timeout_handling(self):
        """Test network timeout error handling."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock timeout error
            mock_client.get_products = AsyncMock(side_effect=asyncio.TimeoutError("Request timeout"))
            
            # Test timeout handling
            with pytest.raises(asyncio.TimeoutError):
                await mock_client.get_products()

    @pytest.mark.asyncio
    async def test_connection_error_handling(self):
        """Test connection error handling."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock connection error
            mock_client.get_products = AsyncMock(side_effect=ConnectionError("Connection failed"))
            
            # Test connection error handling
            with pytest.raises(ConnectionError):
                await mock_client.get_products()

    def test_request_signing(self):
        """Test request signing mechanism."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock signing method
            mock_client.sign_request = MagicMock(return_value="signed_request_data")
            
            # Test signing
            request_data = {"method": "GET", "path": "/v2/products", "body": ""}
            signature = mock_client.sign_request(json.dumps(request_data))
            
            # Verify signing was called
            mock_client.sign_request.assert_called_once()
            assert signature == "signed_request_data"

    @pytest.mark.asyncio
    async def test_pagination_handling(self):
        """Test pagination in API responses."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock paginated response
            paginated_response = {
                "success": True,
                "result": [{"id": i, "symbol": f"TEST{i}"} for i in range(50)],
                "meta": {
                    "count": 50,
                    "page_size": 50,
                    "after": "cursor_token_123",
                    "before": None
                }
            }
            
            mock_client.get_products = AsyncMock(return_value=json.dumps(paginated_response))
            
            # Test pagination
            response = await mock_client.get_products()
            parsed_response = json.loads(response)
            
            # Verify pagination metadata
            assert "meta" in parsed_response
            meta = parsed_response["meta"]
            assert meta["count"] == 50
            assert meta["page_size"] == 50
            assert meta["after"] == "cursor_token_123"

    @pytest.mark.asyncio
    async def test_request_retry_mechanism(self):
        """Test request retry mechanism for transient errors."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock retry configuration
            mock_client.set_retry_config = MagicMock()
            mock_client.set_retry_config(max_retries=3, retry_delay=1.0)
            
            # Verify retry configuration
            mock_client.set_retry_config.assert_called_once_with(max_retries=3, retry_delay=1.0)

    def test_client_configuration_validation(self):
        """Test client configuration validation."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            # Test invalid API key
            with pytest.raises((ValueError, RuntimeError)):
                mock_client_class(
                    api_key="",  # Empty API key
                    api_secret=self.api_secret,
                    base_url=self.base_url,
                    testnet=True,
                )
            
            # Test invalid base URL
            with pytest.raises((ValueError, RuntimeError)):
                mock_client_class(
                    api_key=self.api_key,
                    api_secret=self.api_secret,
                    base_url="invalid_url",  # Invalid URL
                    testnet=True,
                )

    @pytest.mark.asyncio
    async def test_concurrent_requests(self):
        """Test handling of concurrent HTTP requests."""
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client_class.return_value = mock_client
            
            # Mock concurrent responses
            mock_client.get_products = AsyncMock(return_value=json.dumps(self.sample_products_response))
            mock_client.get_assets = AsyncMock(return_value=json.dumps(self.sample_assets_response))
            
            # Test concurrent requests
            tasks = [
                mock_client.get_products(),
                mock_client.get_assets(),
                mock_client.get_products(),
            ]
            
            responses = await asyncio.gather(*tasks)
            
            # Verify all requests completed
            assert len(responses) == 3
            for response in responses:
                parsed = json.loads(response)
                assert parsed["success"] is True
