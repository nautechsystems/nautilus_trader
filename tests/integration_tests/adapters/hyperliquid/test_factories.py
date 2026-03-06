import pytest

from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.config import HyperliquidExecClientConfig


class TestHyperliquidDataClientConfig:
    def test_default_config(self):
        # Arrange & Act
        config = HyperliquidDataClientConfig()

        # Assert
        assert config.base_url_ws is None
        assert config.testnet is False
        assert config.http_timeout_secs == 10

    def test_testnet_config(self):
        # Arrange & Act
        config = HyperliquidDataClientConfig(testnet=True)

        # Assert
        assert config.testnet is True

    def test_custom_http_timeout(self):
        # Arrange & Act
        config = HyperliquidDataClientConfig(http_timeout_secs=30)

        # Assert
        assert config.http_timeout_secs == 30

    def test_custom_base_urls(self):
        # Arrange & Act
        config = HyperliquidDataClientConfig(
            base_url_ws="wss://custom.ws.com",
        )

        # Assert
        assert config.base_url_ws == "wss://custom.ws.com"

    def test_proxy_config(self):
        # Arrange & Act
        config = HyperliquidDataClientConfig(
            http_proxy_url="http://proxy:8080",
        )

        # Assert
        assert config.http_proxy_url == "http://proxy:8080"


class TestHyperliquidExecClientConfig:
    def test_default_config(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig()

        # Assert
        assert config.private_key is None
        assert config.vault_address is None
        assert config.testnet is False
        assert config.http_timeout_secs == 10

    def test_with_private_key(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig(
            private_key="0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        )

        # Assert
        assert config.private_key is not None

    def test_with_vault_address(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig(
            vault_address="0xabcdef1234567890abcdef1234567890abcdef12",
        )

        # Assert
        assert config.vault_address == "0xabcdef1234567890abcdef1234567890abcdef12"

    def test_testnet_config(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig(testnet=True)

        # Assert
        assert config.testnet is True

    def test_retry_config(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig(
            max_retries=5,
            retry_delay_initial_ms=100,
            retry_delay_max_ms=5000,
        )

        # Assert
        assert config.max_retries == 5
        assert config.retry_delay_initial_ms == 100
        assert config.retry_delay_max_ms == 5000

    def test_custom_base_urls(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig(
            base_url_ws="wss://custom.ws.com",
        )

        # Assert
        assert config.base_url_ws == "wss://custom.ws.com"


class TestConfigValidation:
    @pytest.mark.parametrize(
        ("testnet", "expected_testnet"),
        [
            (False, False),
            (True, True),
        ],
    )
    def test_data_client_testnet_setting(self, testnet, expected_testnet):
        # Arrange & Act
        config = HyperliquidDataClientConfig(testnet=testnet)

        # Assert
        assert config.testnet == expected_testnet

    @pytest.mark.parametrize(
        ("testnet", "expected_testnet"),
        [
            (False, False),
            (True, True),
        ],
    )
    def test_exec_client_testnet_setting(self, testnet, expected_testnet):
        # Arrange & Act
        config = HyperliquidExecClientConfig(testnet=testnet)

        # Assert
        assert config.testnet == expected_testnet
