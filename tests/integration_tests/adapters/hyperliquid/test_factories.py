# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.config import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid.enums import HyperliquidProductType
from nautilus_trader.core.nautilus_pyo3 import HyperliquidEnvironment


class TestHyperliquidDataClientConfig:
    def test_default_config(self):
        # Arrange & Act
        config = HyperliquidDataClientConfig()

        # Assert
        assert config.base_url_ws is None
        assert config.environment is None
        assert config.http_timeout_secs == 10

    def test_testnet_config(self):
        # Arrange & Act
        config = HyperliquidDataClientConfig(environment=HyperliquidEnvironment.TESTNET)

        # Assert
        assert config.environment == HyperliquidEnvironment.TESTNET

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
            proxy_url="http://proxy:8080",
        )

        # Assert
        assert config.proxy_url == "http://proxy:8080"

    def test_with_product_types(self):
        # Arrange & Act
        config = HyperliquidDataClientConfig(
            product_types=(
                HyperliquidProductType.PERP,
                HyperliquidProductType.PERP_HIP3,
            ),
        )

        # Assert
        assert config.product_types == (
            HyperliquidProductType.PERP,
            HyperliquidProductType.PERP_HIP3,
        )


class TestHyperliquidExecClientConfig:
    def test_default_config(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig()

        # Assert
        assert config.private_key is None
        assert config.vault_address is None
        assert config.environment is None
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

    def test_default_has_no_account_address(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig()

        # Assert
        assert config.account_address is None

    def test_with_account_address(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig(
            account_address="0xabcdef1234567890abcdef1234567890abcdef12",
        )

        # Assert
        assert config.account_address == "0xabcdef1234567890abcdef1234567890abcdef12"

    def test_testnet_config(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig(environment=HyperliquidEnvironment.TESTNET)

        # Assert
        assert config.environment == HyperliquidEnvironment.TESTNET

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

    def test_with_product_types(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig(
            product_types=(
                HyperliquidProductType.PERP,
                HyperliquidProductType.PERP_HIP3,
            ),
        )

        # Assert
        assert config.product_types == (
            HyperliquidProductType.PERP,
            HyperliquidProductType.PERP_HIP3,
        )


class TestConfigValidation:
    @pytest.mark.parametrize(
        ("environment", "expected_environment"),
        [
            (None, None),
            (HyperliquidEnvironment.TESTNET, HyperliquidEnvironment.TESTNET),
        ],
    )
    def test_data_client_environment_setting(self, environment, expected_environment):
        # Arrange & Act
        config = HyperliquidDataClientConfig(environment=environment)

        # Assert
        assert config.environment == expected_environment

    @pytest.mark.parametrize(
        ("environment", "expected_environment"),
        [
            (None, None),
            (HyperliquidEnvironment.TESTNET, HyperliquidEnvironment.TESTNET),
        ],
    )
    def test_exec_client_environment_setting(self, environment, expected_environment):
        # Arrange & Act
        config = HyperliquidExecClientConfig(environment=environment)

        # Assert
        assert config.environment == expected_environment
