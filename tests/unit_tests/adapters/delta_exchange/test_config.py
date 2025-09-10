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
Unit tests for Delta Exchange configuration classes.

This module provides comprehensive testing for all Delta Exchange configuration
classes, validation functions, environment variable handling, and edge cases.
"""

import os
import tempfile
from decimal import Decimal
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.delta_exchange.config import (
    DeltaExchangeDataClientConfig,
    DeltaExchangeExecClientConfig,
    DeltaExchangeInstrumentProviderConfig,
    _get_env_credentials,
    _validate_api_credentials,
    _validate_url,
    _validate_environment_settings,
    _validate_risk_management_settings,
    _validate_timeout_settings,
)
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE,
    DELTA_EXCHANGE_BASE_URL,
    DELTA_EXCHANGE_TESTNET_BASE_URL,
    DELTA_EXCHANGE_TESTNET_WS_URL,
    DELTA_EXCHANGE_WS_URL,
    DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES,
    DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE,
)
from nautilus_trader.config import InvalidConfiguration
from nautilus_trader.model.enums import OrderType, TimeInForce


class TestValidationFunctions:
    """Test validation utility functions."""

    def test_validate_url_valid_http(self):
        """Test URL validation with valid HTTP URLs."""
        _validate_url("https://api.example.com", "test")
        _validate_url("http://localhost:8080", "test")
        _validate_url("https://192.168.1.1:3000/api", "test")

    def test_validate_url_valid_websocket(self):
        """Test URL validation with valid WebSocket URLs."""
        _validate_url("wss://socket.example.com", "test")
        _validate_url("ws://localhost:8080/ws", "test")

    def test_validate_url_invalid(self):
        """Test URL validation with invalid URLs."""
        with pytest.raises(InvalidConfiguration):
            _validate_url("invalid-url", "test")
        
        with pytest.raises(InvalidConfiguration):
            _validate_url("ftp://example.com", "test")

    def test_validate_url_none(self):
        """Test URL validation with None (should pass)."""
        _validate_url(None, "test")  # Should not raise

    def test_validate_api_credentials_valid(self):
        """Test API credentials validation with valid credentials."""
        _validate_api_credentials("valid_api_key_123", "valid_api_secret_with_sufficient_length")

    def test_validate_api_credentials_invalid_key(self):
        """Test API credentials validation with invalid key."""
        with pytest.raises(InvalidConfiguration):
            _validate_api_credentials("short", "valid_api_secret_with_sufficient_length")

    def test_validate_api_credentials_invalid_secret(self):
        """Test API credentials validation with invalid secret."""
        with pytest.raises(InvalidConfiguration):
            _validate_api_credentials("valid_api_key_123", "short")

    def test_validate_api_credentials_none(self):
        """Test API credentials validation with None values."""
        _validate_api_credentials(None, None)  # Should not raise

    @patch.dict(os.environ, {
        "DELTA_EXCHANGE_API_KEY": "prod_key",
        "DELTA_EXCHANGE_API_SECRET": "prod_secret",
        "DELTA_EXCHANGE_TESTNET_API_KEY": "test_key",
        "DELTA_EXCHANGE_TESTNET_API_SECRET": "test_secret",
        "DELTA_EXCHANGE_SANDBOX_API_KEY": "sandbox_key",
        "DELTA_EXCHANGE_SANDBOX_API_SECRET": "sandbox_secret",
    })
    def test_get_env_credentials(self):
        """Test environment credential retrieval."""
        # Production credentials
        key, secret = _get_env_credentials(testnet=False, sandbox=False)
        assert key == "prod_key"
        assert secret == "prod_secret"

        # Testnet credentials
        key, secret = _get_env_credentials(testnet=True, sandbox=False)
        assert key == "test_key"
        assert secret == "test_secret"

        # Sandbox credentials
        key, secret = _get_env_credentials(testnet=False, sandbox=True)
        assert key == "sandbox_key"
        assert secret == "sandbox_secret"


class TestDeltaExchangeDataClientConfig:
    """Test DeltaExchangeDataClientConfig."""

    def test_default_config(self):
        """Test default configuration."""
        config = DeltaExchangeDataClientConfig()
        
        assert config.venue == DELTA_EXCHANGE
        assert config.api_key is None
        assert config.api_secret is None
        assert config.http_timeout_secs == 60
        assert config.ws_timeout_secs == 30
        assert not config.testnet
        assert not config.sandbox
        assert config.auto_reconnect
        assert config.rate_limit_requests_per_second == 75

    def test_custom_config(self):
        """Test custom configuration."""
        config = DeltaExchangeDataClientConfig(
            api_key="test_key",
            api_secret="test_secret_with_sufficient_length",
            http_timeout_secs=120,
            ws_timeout_secs=60,
            heartbeat_interval_secs=60,
            default_channels=["v2_ticker", "l2_orderbook"],
            symbol_filters=["BTC*", "ETH*"],
            rate_limit_requests_per_second=50,
        )
        
        assert config.api_key == "test_key"
        assert config.api_secret == "test_secret_with_sufficient_length"
        assert config.http_timeout_secs == 120
        assert config.ws_timeout_secs == 60
        assert config.heartbeat_interval_secs == 60
        assert config.default_channels == ["v2_ticker", "l2_orderbook"]
        assert config.symbol_filters == ["BTC*", "ETH*"]
        assert config.rate_limit_requests_per_second == 50

    def test_testnet_factory(self):
        """Test testnet factory method."""
        config = DeltaExchangeDataClientConfig.testnet(
            api_key="test_key",
            api_secret="test_secret_with_sufficient_length",
        )
        
        assert config.testnet
        assert not config.sandbox
        assert config.get_effective_http_url() == DELTA_EXCHANGE_TESTNET_BASE_URL
        assert config.get_effective_ws_url() == DELTA_EXCHANGE_TESTNET_WS_URL

    def test_sandbox_factory(self):
        """Test sandbox factory method."""
        config = DeltaExchangeDataClientConfig.sandbox(
            api_key="test_key",
            api_secret="test_secret_with_sufficient_length",
        )
        
        assert not config.testnet
        assert config.sandbox
        assert config.get_effective_http_url() == DELTA_EXCHANGE_TESTNET_BASE_URL
        assert config.get_effective_ws_url() == DELTA_EXCHANGE_TESTNET_WS_URL

    def test_production_factory(self):
        """Test production factory method."""
        config = DeltaExchangeDataClientConfig.production(
            api_key="test_key",
            api_secret="test_secret_with_sufficient_length",
        )
        
        assert not config.testnet
        assert not config.sandbox
        assert config.get_effective_http_url() == DELTA_EXCHANGE_BASE_URL
        assert config.get_effective_ws_url() == DELTA_EXCHANGE_WS_URL

    def test_invalid_testnet_and_sandbox(self):
        """Test invalid configuration with both testnet and sandbox."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeDataClientConfig(testnet=True, sandbox=True)

    def test_invalid_timeout(self):
        """Test invalid timeout values."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeDataClientConfig(http_timeout_secs=0)
        
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeDataClientConfig(http_timeout_secs=500)

    def test_invalid_heartbeat_interval(self):
        """Test invalid heartbeat interval."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeDataClientConfig(heartbeat_interval_secs=5)
        
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeDataClientConfig(heartbeat_interval_secs=500)

    def test_invalid_channel(self):
        """Test invalid channel in default_channels."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeDataClientConfig(default_channels=["invalid_channel"])

    def test_invalid_rate_limit(self):
        """Test invalid rate limit."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeDataClientConfig(rate_limit_requests_per_second=150)

    def test_has_credentials(self):
        """Test credential availability check."""
        config = DeltaExchangeDataClientConfig()
        assert not config.has_credentials()
        
        config = DeltaExchangeDataClientConfig(
            api_key="test_key",
            api_secret="test_secret_with_sufficient_length",
        )
        assert config.has_credentials()

    @patch.dict(os.environ, {
        "DELTA_EXCHANGE_API_KEY": "env_key",
        "DELTA_EXCHANGE_API_SECRET": "env_secret_with_sufficient_length",
    })
    def test_environment_credentials(self):
        """Test loading credentials from environment."""
        config = DeltaExchangeDataClientConfig()
        
        assert config.get_effective_api_key() == "env_key"
        assert config.get_effective_api_secret() == "env_secret_with_sufficient_length"
        assert config.has_credentials()


class TestDeltaExchangeExecClientConfig:
    """Test DeltaExchangeExecClientConfig."""

    def test_default_config(self):
        """Test default configuration."""
        config = DeltaExchangeExecClientConfig()
        
        assert config.venue == DELTA_EXCHANGE
        assert config.default_time_in_force == TimeInForce.GTC
        assert not config.post_only_default
        assert not config.reduce_only_default
        assert config.margin_mode == "cross"
        assert config.default_leverage == 1.0
        assert config.max_leverage == 100.0
        assert config.enable_client_order_id_generation
        assert config.client_order_id_prefix == "NAUTILUS"

    def test_custom_config(self):
        """Test custom configuration."""
        config = DeltaExchangeExecClientConfig(
            api_key="test_key",
            api_secret="test_secret_with_sufficient_length",
            default_time_in_force=TimeInForce.IOC,
            post_only_default=True,
            margin_mode="isolated",
            default_leverage=10.0,
            max_leverage=50.0,
            max_order_size=1000.0,
            max_position_size=5000.0,
            client_order_id_prefix="CUSTOM",
        )
        
        assert config.default_time_in_force == TimeInForce.IOC
        assert config.post_only_default
        assert config.margin_mode == "isolated"
        assert config.default_leverage == 10.0
        assert config.max_leverage == 50.0
        assert config.max_order_size == 1000.0
        assert config.max_position_size == 5000.0
        assert config.client_order_id_prefix == "CUSTOM"

    def test_invalid_margin_mode(self):
        """Test invalid margin mode."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeExecClientConfig(margin_mode="invalid")

    def test_invalid_leverage(self):
        """Test invalid leverage configuration."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeExecClientConfig(default_leverage=10.0, max_leverage=5.0)
        
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeExecClientConfig(max_leverage=300.0)

    def test_invalid_retry_config(self):
        """Test invalid retry configuration."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeExecClientConfig(
                retry_delay_initial_ms=5000,
                retry_delay_max_ms=1000,
            )

    def test_invalid_client_order_id_prefix(self):
        """Test invalid client order ID prefix."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeExecClientConfig(client_order_id_prefix="")
        
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeExecClientConfig(client_order_id_prefix="A" * 25)

    def test_validate_risk_parameters(self):
        """Test risk parameter validation."""
        config = DeltaExchangeExecClientConfig(
            max_order_size=1000.0,
            max_position_size=5000.0,
            max_notional_per_order=10000.0,
        )
        
        assert config.validate_risk_parameters()

    def test_invalid_risk_parameters(self):
        """Test invalid risk parameters."""
        config = DeltaExchangeExecClientConfig(max_order_size=-100.0)
        
        with pytest.raises(InvalidConfiguration):
            config.validate_risk_parameters()

    def test_get_order_retry_config(self):
        """Test order retry configuration retrieval."""
        config = DeltaExchangeExecClientConfig(
            max_retries=5,
            retry_delay_initial_ms=2000,
            retry_delay_max_ms=15000,
        )
        
        retry_config = config.get_order_retry_config()
        assert retry_config["max_retries"] == 5
        assert retry_config["initial_delay_ms"] == 2000
        assert retry_config["max_delay_ms"] == 15000


class TestDeltaExchangeInstrumentProviderConfig:
    """Test DeltaExchangeInstrumentProviderConfig."""

    def test_default_config(self):
        """Test default configuration."""
        config = DeltaExchangeInstrumentProviderConfig()
        
        assert config.load_active_only
        assert not config.load_expired
        assert config.cache_validity_hours == 24
        assert config.update_instruments_interval_mins == 60
        assert config.enable_auto_refresh
        assert config.enable_instrument_caching
        assert config.cache_file_prefix == "delta_exchange_instruments"

    def test_custom_config(self):
        """Test custom configuration."""
        config = DeltaExchangeInstrumentProviderConfig(
            product_types=["perpetual_futures"],
            symbol_filters=["BTC*", "ETH*"],
            cache_validity_hours=12,
            update_instruments_interval_mins=30,
            max_concurrent_requests=10,
            cache_file_prefix="custom_instruments",
        )
        
        assert config.product_types == ["perpetual_futures"]
        assert config.symbol_filters == ["BTC*", "ETH*"]
        assert config.cache_validity_hours == 12
        assert config.update_instruments_interval_mins == 30
        assert config.max_concurrent_requests == 10
        assert config.cache_file_prefix == "custom_instruments"

    def test_invalid_product_types(self):
        """Test invalid product types."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeInstrumentProviderConfig(product_types=["invalid_type"])

    def test_invalid_update_interval(self):
        """Test invalid update interval."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeInstrumentProviderConfig(update_instruments_interval_mins=0)
        
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeInstrumentProviderConfig(update_instruments_interval_mins=2000)

    def test_invalid_cache_validity(self):
        """Test invalid cache validity."""
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeInstrumentProviderConfig(cache_validity_hours=0)
        
        with pytest.raises(InvalidConfiguration):
            DeltaExchangeInstrumentProviderConfig(cache_validity_hours=200)

    def test_get_product_type_filters(self):
        """Test product type filter retrieval."""
        config = DeltaExchangeInstrumentProviderConfig()
        filters = config.get_product_type_filters()
        assert "perpetual_futures" in filters
        assert "call_options" in filters
        assert "put_options" in filters
        
        config = DeltaExchangeInstrumentProviderConfig(product_types=["perpetual_futures"])
        filters = config.get_product_type_filters()
        assert filters == ["perpetual_futures"]

    def test_should_load_instrument(self):
        """Test instrument loading filter."""
        config = DeltaExchangeInstrumentProviderConfig(
            product_types=["perpetual_futures"],
            symbol_filters=["BTC*"],
            load_active_only=True,
        )
        
        # Should load
        assert config.should_load_instrument("BTCUSD", "perpetual_futures", "active")
        
        # Should not load - wrong product type
        assert not config.should_load_instrument("BTCUSD", "call_options", "active")
        
        # Should not load - wrong symbol
        assert not config.should_load_instrument("ETHUSD", "perpetual_futures", "active")
        
        # Should not load - inactive
        assert not config.should_load_instrument("BTCUSD", "perpetual_futures", "inactive")

    def test_get_cache_file_path(self):
        """Test cache file path generation."""
        config = DeltaExchangeInstrumentProviderConfig()
        path = config.get_cache_file_path()
        assert "delta_exchange_instruments.json" in path
        
        config = DeltaExchangeInstrumentProviderConfig(testnet=True)
        path = config.get_cache_file_path()
        assert "delta_exchange_instruments_testnet.json" in path
        
        config = DeltaExchangeInstrumentProviderConfig(sandbox=True)
        path = config.get_cache_file_path()
        assert "delta_exchange_instruments_sandbox.json" in path

    def test_is_cache_valid(self):
        """Test cache validity check."""
        config = DeltaExchangeInstrumentProviderConfig(enable_instrument_caching=False)
        assert not config.is_cache_valid()
        
        # Test with non-existent cache file
        config = DeltaExchangeInstrumentProviderConfig()
        assert not config.is_cache_valid()
        
        # Test with fresh cache file
        with tempfile.NamedTemporaryFile(delete=False) as tmp:
            config = DeltaExchangeInstrumentProviderConfig(cache_directory=os.path.dirname(tmp.name))
            # Create a fresh cache file
            cache_path = config.get_cache_file_path()
            with open(cache_path, 'w') as f:
                f.write('{}')
            
            assert config.is_cache_valid()

            # Clean up
            os.unlink(cache_path)


class TestConfigurationEdgeCases:
    """Test edge cases and error scenarios for configuration classes."""

    def test_validate_environment_settings_valid(self):
        """Test environment settings validation with valid configurations."""
        # Valid production settings
        _validate_environment_settings(testnet=False, sandbox=False)

        # Valid testnet settings
        _validate_environment_settings(testnet=True, sandbox=False)

        # Valid sandbox settings
        _validate_environment_settings(testnet=False, sandbox=True)

    def test_validate_environment_settings_invalid(self):
        """Test environment settings validation with invalid configurations."""
        # Invalid: both testnet and sandbox
        with pytest.raises(InvalidConfiguration, match="Cannot use both testnet and sandbox"):
            _validate_environment_settings(testnet=True, sandbox=True)

    def test_validate_risk_management_settings_valid(self):
        """Test risk management settings validation with valid values."""
        # Valid settings
        _validate_risk_management_settings(
            position_limits={"BTCUSDT": Decimal("10.0")},
            daily_loss_limit=Decimal("1000.0"),
            max_position_value=Decimal("100000.0"),
        )

        # Empty position limits (valid)
        _validate_risk_management_settings(
            position_limits={},
            daily_loss_limit=Decimal("1000.0"),
            max_position_value=Decimal("100000.0"),
        )

    def test_validate_risk_management_settings_invalid(self):
        """Test risk management settings validation with invalid values."""
        # Invalid: negative daily loss limit
        with pytest.raises(InvalidConfiguration, match="Daily loss limit must be positive"):
            _validate_risk_management_settings(
                position_limits={},
                daily_loss_limit=Decimal("-1000.0"),
                max_position_value=Decimal("100000.0"),
            )

        # Invalid: zero max position value
        with pytest.raises(InvalidConfiguration, match="Max position value must be positive"):
            _validate_risk_management_settings(
                position_limits={},
                daily_loss_limit=Decimal("1000.0"),
                max_position_value=Decimal("0.0"),
            )

        # Invalid: negative position limit
        with pytest.raises(InvalidConfiguration, match="Position limit must be positive"):
            _validate_risk_management_settings(
                position_limits={"BTCUSDT": Decimal("-10.0")},
                daily_loss_limit=Decimal("1000.0"),
                max_position_value=Decimal("100000.0"),
            )

    def test_validate_timeout_settings_valid(self):
        """Test timeout settings validation with valid values."""
        # Valid timeout settings
        _validate_timeout_settings(
            request_timeout_secs=60.0,
            ws_timeout_secs=10.0,
            heartbeat_interval_secs=30.0,
        )

    def test_validate_timeout_settings_invalid(self):
        """Test timeout settings validation with invalid values."""
        # Invalid: negative request timeout
        with pytest.raises(InvalidConfiguration, match="Request timeout must be positive"):
            _validate_timeout_settings(
                request_timeout_secs=-60.0,
                ws_timeout_secs=10.0,
                heartbeat_interval_secs=30.0,
            )

        # Invalid: zero WebSocket timeout
        with pytest.raises(InvalidConfiguration, match="WebSocket timeout must be positive"):
            _validate_timeout_settings(
                request_timeout_secs=60.0,
                ws_timeout_secs=0.0,
                heartbeat_interval_secs=30.0,
            )

        # Invalid: negative heartbeat interval
        with pytest.raises(InvalidConfiguration, match="Heartbeat interval must be positive"):
            _validate_timeout_settings(
                request_timeout_secs=60.0,
                ws_timeout_secs=10.0,
                heartbeat_interval_secs=-30.0,
            )

    def test_data_client_config_extreme_values(self):
        """Test data client configuration with extreme values."""
        # Test with very large values
        config = DeltaExchangeDataClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            request_timeout_secs=3600.0,  # 1 hour
            ws_timeout_secs=300.0,        # 5 minutes
            max_retries=100,
            retry_delay_secs=60.0,
        )
        assert config.request_timeout_secs == 3600.0
        assert config.max_retries == 100

    def test_exec_client_config_extreme_values(self):
        """Test execution client configuration with extreme values."""
        # Test with very large position limits
        large_limits = {f"SYMBOL{i}": Decimal("1000000.0") for i in range(100)}

        config = DeltaExchangeExecClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            account_id="test_account",
            position_limits=large_limits,
            daily_loss_limit=Decimal("10000000.0"),
            max_position_value=Decimal("100000000.0"),
        )
        assert len(config.position_limits) == 100
        assert config.daily_loss_limit == Decimal("10000000.0")

    def test_configuration_serialization(self):
        """Test configuration serialization and deserialization."""
        # Test data client config serialization
        data_config = DeltaExchangeDataClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            testnet=True,
            product_types=["perpetual_futures"],
        )

        # Convert to dict and back
        config_dict = data_config.dict()
        assert config_dict["api_key"] == "test_key"
        assert config_dict["testnet"] is True

        # Test exec client config serialization
        exec_config = DeltaExchangeExecClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            account_id="test_account",
            position_limits={"BTCUSDT": Decimal("10.0")},
        )

        config_dict = exec_config.dict()
        assert config_dict["account_id"] == "test_account"
        assert "position_limits" in config_dict

    def test_configuration_inheritance(self):
        """Test configuration inheritance and method resolution."""
        # Test that configs inherit from proper base classes
        data_config = DeltaExchangeDataClientConfig(
            api_key="test_key",
            api_secret="test_secret",
        )

        # Should have inherited methods
        assert hasattr(data_config, 'dict')
        assert hasattr(data_config, 'json')

        exec_config = DeltaExchangeExecClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            account_id="test_account",
        )

        # Should have inherited methods
        assert hasattr(exec_config, 'dict')
        assert hasattr(exec_config, 'json')

    def test_configuration_immutability(self):
        """Test that configurations are properly immutable where expected."""
        config = DeltaExchangeDataClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            product_types=["perpetual_futures"],
        )

        # Test that we can't modify the config after creation
        with pytest.raises((AttributeError, TypeError)):
            config.api_key = "new_key"
