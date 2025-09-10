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
Unit tests for Delta Exchange factory classes.

This module provides comprehensive tests for all Delta Exchange factory classes,
covering client creation, configuration validation, caching mechanisms, error
handling, and resource management.
"""

import asyncio
from unittest.mock import MagicMock, patch

import pytest

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeDataClientConfig
from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeExecClientConfig
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE
from nautilus_trader.adapters.delta_exchange.data import DeltaExchangeDataClient
from nautilus_trader.adapters.delta_exchange.execution import DeltaExchangeExecutionClient
from nautilus_trader.adapters.delta_exchange.factories import (
    DeltaExchangeLiveDataClientFactory,
    DeltaExchangeLiveDataEngineFactory,
    DeltaExchangeLiveExecClientFactory,
    DeltaExchangeLiveExecEngineFactory,
    clear_delta_exchange_caches,
    create_delta_exchange_clients,
    create_production_factories,
    create_testnet_factories,
    get_cached_delta_exchange_http_client,
    get_cached_delta_exchange_instrument_provider,
    get_cached_delta_exchange_ws_client,
    get_delta_exchange_factory_info,
    validate_factory_environment,
)
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.test_kit.mocks import MockMessageBus


class TestDeltaExchangeFactoryCaching:
    """Test suite for Delta Exchange factory caching mechanisms."""

    def setup_method(self):
        """Set up test fixtures."""
        # Clear caches before each test
        clear_delta_exchange_caches()

    def test_http_client_caching(self):
        """Test HTTP client caching functionality."""
        # Create first client
        client1 = get_cached_delta_exchange_http_client(
            api_key="test_key",
            api_secret="test_secret",
            testnet=True,
        )
        
        # Create second client with same parameters
        client2 = get_cached_delta_exchange_http_client(
            api_key="test_key",
            api_secret="test_secret",
            testnet=True,
        )
        
        # Should be the same cached instance
        assert client1 is client2

    def test_http_client_different_params(self):
        """Test HTTP client caching with different parameters."""
        # Create clients with different parameters
        client1 = get_cached_delta_exchange_http_client(
            api_key="test_key1",
            api_secret="test_secret1",
            testnet=True,
        )
        
        client2 = get_cached_delta_exchange_http_client(
            api_key="test_key2",
            api_secret="test_secret2",
            testnet=True,
        )
        
        # Should be different instances
        assert client1 is not client2

    def test_ws_client_caching(self):
        """Test WebSocket client caching functionality."""
        # Create first client
        client1 = get_cached_delta_exchange_ws_client(
            api_key="test_key",
            api_secret="test_secret",
            testnet=True,
        )
        
        # Create second client with same parameters
        client2 = get_cached_delta_exchange_ws_client(
            api_key="test_key",
            api_secret="test_secret",
            testnet=True,
        )
        
        # Should be the same cached instance
        assert client1 is client2

    def test_instrument_provider_caching(self):
        """Test instrument provider caching functionality."""
        # Mock HTTP client and clock
        mock_client = MagicMock(spec=nautilus_pyo3.DeltaExchangeHttpClient)
        mock_clock = MagicMock(spec=LiveClock)
        
        # Create first provider
        provider1 = get_cached_delta_exchange_instrument_provider(
            client=mock_client,
            clock=mock_clock,
            config=DeltaExchangeDataClientConfig().instrument_provider,
        )
        
        # Create second provider with same parameters
        provider2 = get_cached_delta_exchange_instrument_provider(
            client=mock_client,
            clock=mock_clock,
            config=DeltaExchangeDataClientConfig().instrument_provider,
        )
        
        # Should be the same cached instance
        assert provider1 is provider2

    def test_cache_clearing(self):
        """Test cache clearing functionality."""
        # Create cached clients
        get_cached_delta_exchange_http_client(api_key="test", testnet=True)
        get_cached_delta_exchange_ws_client(api_key="test", testnet=True)
        
        # Get cache info before clearing
        info_before = get_delta_exchange_factory_info()
        assert info_before["http_client_cache"]["currsize"] > 0
        assert info_before["ws_client_cache"]["currsize"] > 0
        
        # Clear caches
        clear_delta_exchange_caches()
        
        # Get cache info after clearing
        info_after = get_delta_exchange_factory_info()
        assert info_after["http_client_cache"]["currsize"] == 0
        assert info_after["ws_client_cache"]["currsize"] == 0


class TestDeltaExchangeDataClientFactory:
    """Test suite for DeltaExchangeDataClientFactory."""

    def setup_method(self):
        """Set up test fixtures."""
        self.loop = asyncio.get_event_loop()
        self.msgbus = MockMessageBus()
        self.cache = Cache()
        self.clock = LiveClock()
        
        # Clear caches
        clear_delta_exchange_caches()

    def test_create_data_client_success(self):
        """Test successful data client creation."""
        config = DeltaExchangeDataClientConfig.testnet(
            api_key="test_key",
            api_secret="test_secret",
        )
        
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient'):
            client = DeltaExchangeLiveDataClientFactory.create(
                loop=self.loop,
                name="test_client",
                config=config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
            )
            
            assert isinstance(client, DeltaExchangeDataClient)
            assert client.id.value == "test_client"

    def test_create_data_client_invalid_config(self):
        """Test data client creation with invalid configuration."""
        # Config with both testnet and sandbox enabled
        config = DeltaExchangeDataClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            testnet=True,
            sandbox=True,  # Invalid combination
        )
        
        with pytest.raises(RuntimeError, match="Data client creation failed"):
            DeltaExchangeLiveDataClientFactory.create(
                loop=self.loop,
                name="test_client",
                config=config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
            )

    def test_create_data_client_missing_credentials_for_private(self):
        """Test data client creation with missing credentials for private channels."""
        config = DeltaExchangeDataClientConfig(
            enable_private_channels=True,
            # Missing API credentials
        )
        
        with pytest.raises(RuntimeError, match="Data client creation failed"):
            DeltaExchangeLiveDataClientFactory.create(
                loop=self.loop,
                name="test_client",
                config=config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
            )


class TestDeltaExchangeExecClientFactory:
    """Test suite for DeltaExchangeExecClientFactory."""

    def setup_method(self):
        """Set up test fixtures."""
        self.loop = asyncio.get_event_loop()
        self.msgbus = MockMessageBus()
        self.cache = Cache()
        self.clock = LiveClock()
        
        # Clear caches
        clear_delta_exchange_caches()

    def test_create_exec_client_success(self):
        """Test successful execution client creation."""
        config = DeltaExchangeExecClientConfig.testnet(
            api_key="test_key",
            api_secret="test_secret",
            account_id="test_account",
        )
        
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient'):
            client = DeltaExchangeLiveExecClientFactory.create(
                loop=self.loop,
                name="test_exec_client",
                config=config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
            )
            
            assert isinstance(client, DeltaExchangeExecutionClient)
            assert client.id.value == "test_exec_client"

    def test_create_exec_client_missing_credentials(self):
        """Test execution client creation with missing credentials."""
        config = DeltaExchangeExecClientConfig(
            # Missing required credentials
            account_id="test_account",
        )
        
        with pytest.raises(RuntimeError, match="Execution client creation failed"):
            DeltaExchangeLiveExecClientFactory.create(
                loop=self.loop,
                name="test_exec_client",
                config=config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
            )

    def test_create_exec_client_missing_account_id(self):
        """Test execution client creation with missing account ID."""
        config = DeltaExchangeExecClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            # Missing account_id
        )
        
        with pytest.raises(RuntimeError, match="Execution client creation failed"):
            DeltaExchangeLiveExecClientFactory.create(
                loop=self.loop,
                name="test_exec_client",
                config=config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
            )


class TestDeltaExchangeEngineFactories:
    """Test suite for Delta Exchange engine factories."""

    def test_data_engine_factory_create_config(self):
        """Test data engine factory configuration creation."""
        config = DeltaExchangeLiveDataEngineFactory.create_config(
            api_key="test_key",
            api_secret="test_secret",
            testnet=True,
        )
        
        assert "data_clients" in config
        assert DELTA_EXCHANGE.value in config["data_clients"]
        assert config["data_clients"][DELTA_EXCHANGE.value]["factory"] == DeltaExchangeLiveDataClientFactory

    def test_exec_engine_factory_create_config(self):
        """Test execution engine factory configuration creation."""
        config = DeltaExchangeLiveExecEngineFactory.create_config(
            api_key="test_key",
            api_secret="test_secret",
            account_id="test_account",
            testnet=True,
        )
        
        assert "exec_clients" in config
        assert DELTA_EXCHANGE.value in config["exec_clients"]
        assert config["exec_clients"][DELTA_EXCHANGE.value]["factory"] == DeltaExchangeLiveExecClientFactory

    def test_register_with_node(self):
        """Test factory registration with trading node."""
        mock_node = MagicMock()
        
        # Test data factory registration
        DeltaExchangeLiveDataEngineFactory.register_with_node(mock_node)
        mock_node.add_data_client_factory.assert_called_once_with(
            DELTA_EXCHANGE.value, 
            DeltaExchangeLiveDataClientFactory
        )
        
        # Test exec factory registration
        DeltaExchangeLiveExecEngineFactory.register_with_node(mock_node)
        mock_node.add_exec_client_factory.assert_called_once_with(
            DELTA_EXCHANGE.value, 
            DeltaExchangeLiveExecClientFactory
        )


class TestDeltaExchangeFactoryUtilities:
    """Test suite for Delta Exchange factory utility functions."""

    def test_create_testnet_factories(self):
        """Test testnet factory creation."""
        data_factory, exec_factory = create_testnet_factories(
            api_key="test_key",
            api_secret="test_secret",
            account_id="test_account",
        )
        
        assert isinstance(data_factory, DeltaExchangeLiveDataClientFactory)
        assert isinstance(exec_factory, DeltaExchangeLiveExecClientFactory)

    def test_create_production_factories(self):
        """Test production factory creation."""
        data_factory, exec_factory = create_production_factories(
            api_key="prod_key",
            api_secret="prod_secret",
            account_id="prod_account",
        )
        
        assert isinstance(data_factory, DeltaExchangeLiveDataClientFactory)
        assert isinstance(exec_factory, DeltaExchangeLiveExecClientFactory)

    def test_validate_factory_environment(self):
        """Test factory environment validation."""
        results = validate_factory_environment()
        
        # Check that validation returns expected keys
        expected_keys = [
            "rust_http_client",
            "rust_ws_client", 
            "data_config",
            "exec_config",
            "data_client",
            "exec_client",
            "instrument_provider",
            "data_factory",
            "exec_factory",
        ]
        
        for key in expected_keys:
            assert key in results

    def test_get_factory_info(self):
        """Test factory information retrieval."""
        info = get_delta_exchange_factory_info()
        
        assert "http_client_cache" in info
        assert "ws_client_cache" in info
        assert "instrument_provider_cache" in info
        assert "supported_environments" in info
        assert "factory_classes" in info
        
        # Check supported environments
        assert "production" in info["supported_environments"]
        assert "testnet" in info["supported_environments"]
        assert "sandbox" in info["supported_environments"]

    def test_create_delta_exchange_clients_both(self):
        """Test creating both data and execution clients."""
        data_config = DeltaExchangeDataClientConfig.testnet(
            api_key="test_key",
            api_secret="test_secret",
        )
        exec_config = DeltaExchangeExecClientConfig.testnet(
            api_key="test_key",
            api_secret="test_secret",
            account_id="test_account",
        )
        
        loop = asyncio.get_event_loop()
        msgbus = MockMessageBus()
        cache = Cache()
        clock = LiveClock()
        
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient'):
            data_client, exec_client = create_delta_exchange_clients(
                data_config=data_config,
                exec_config=exec_config,
                loop=loop,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
            )
            
            assert isinstance(data_client, DeltaExchangeDataClient)
            assert isinstance(exec_client, DeltaExchangeExecutionClient)

    def test_create_delta_exchange_clients_data_only(self):
        """Test creating only data client."""
        data_config = DeltaExchangeDataClientConfig.testnet(
            api_key="test_key",
            api_secret="test_secret",
        )
        
        loop = asyncio.get_event_loop()
        msgbus = MockMessageBus()
        cache = Cache()
        clock = LiveClock()
        
        with patch('nautilus_trader.core.nautilus_pyo3.DeltaExchangeHttpClient'):
            data_client, exec_client = create_delta_exchange_clients(
                data_config=data_config,
                loop=loop,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
            )
            
            assert isinstance(data_client, DeltaExchangeDataClient)
            assert exec_client is None
