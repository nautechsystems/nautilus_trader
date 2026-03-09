import pytest

from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.config import HyperliquidExecClientConfig
from nautilus_trader.core import nautilus_pyo3


TEST_PRIVATE_KEY = "0x" + ("11" * 32)
TEST_VAULT_ADDRESS = "0x" + ("22" * 20)


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

    def test_trade_xyz_dex_config(self):
        # Arrange & Act
        config = HyperliquidDataClientConfig(dex="xyz")

        # Assert
        assert config.dex == "xyz"


class TestHyperliquidExecClientConfig:
    def test_default_config(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig()

        # Assert
        assert config.private_key is None
        assert config.account_address is None
        assert config.vault_address is None
        assert config.dex is None
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

    def test_with_account_address_and_dex(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig(
            account_address="0xabcdef1234567890abcdef1234567890abcdef12",
            dex="xyz",
        )

        # Assert
        assert config.account_address == "0xabcdef1234567890abcdef1234567890abcdef12"
        assert config.dex == "xyz"

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

    def test_pyo3_positional_signature_is_backward_compatible(self):
        # Arrange & Act
        config = nautilus_pyo3.HyperliquidExecClientConfig(
            TEST_PRIVATE_KEY,
            TEST_VAULT_ADDRESS,
            None,
            True,
            None,
            None,
            None,
            None,
            10,
        )

        # Assert
        config_repr = repr(config)
        assert TEST_PRIVATE_KEY in config_repr
        assert TEST_VAULT_ADDRESS in config_repr
        assert "account_address: None" in config_repr
        assert "is_testnet: true" in config_repr
        assert "http_timeout_secs: 10" in config_repr


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
