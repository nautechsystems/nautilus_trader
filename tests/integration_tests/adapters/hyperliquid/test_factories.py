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

from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.config import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid.enums import HyperliquidProductType
from nautilus_trader.adapters.hyperliquid.factories import HyperliquidLiveExecClientFactory
from nautilus_trader.core.nautilus_pyo3 import HyperliquidEnvironment


class TestHyperliquidDataClientConfig:
    def test_default_config(self):
        # Arrange & Act
        config = HyperliquidDataClientConfig()

        # Assert
        assert config.base_url_ws is None
        assert config.environment is None
        assert config.http_timeout_secs == 10
        assert config.bbo_redundancy == 4

    def test_custom_bbo_redundancy(self):
        # Arrange & Act
        config = HyperliquidDataClientConfig(bbo_redundancy=2)

        # Assert
        assert config.bbo_redundancy == 2

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
        assert config.ws_post_timeout_secs == 10
        assert config.include_builder_attribution is True

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

    def test_ws_post_timeout_config(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig(ws_post_timeout_secs=7)

        # Assert
        assert config.ws_post_timeout_secs == 7

    def test_include_builder_attribution_can_be_disabled(self):
        # Arrange & Act
        config = HyperliquidExecClientConfig(include_builder_attribution=False)

        # Assert
        assert config.include_builder_attribution is False

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


def test_exec_factory_passes_rust_resolved_account_address(monkeypatch):
    # Arrange
    http_client = MagicMock()
    provider = MagicMock()
    execution_client = MagicMock()
    resolve_account_address = MagicMock(return_value="0xvault")
    http_kwargs = {}
    execution_kwargs = {}

    def get_http_client(**kwargs):
        http_kwargs.update(kwargs)
        return http_client

    def create_execution_client(**kwargs):
        execution_kwargs.update(kwargs)
        return execution_client

    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.factories.get_cached_hyperliquid_http_client",
        get_http_client,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.factories.get_cached_hyperliquid_instrument_provider",
        lambda **_: provider,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.factories.HyperliquidExecutionClient",
        create_execution_client,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.factories.nautilus_pyo3.hyperliquid_resolve_execution_account_address",
        resolve_account_address,
        raising=False,
    )

    config = HyperliquidExecClientConfig(
        private_key="0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        vault_address=" 0xvault ",
    )

    # Act
    client = HyperliquidLiveExecClientFactory.create(
        loop=MagicMock(),
        name="HYPERLIQUID",
        config=config,
        msgbus=MagicMock(),
        cache=MagicMock(),
        clock=MagicMock(),
    )

    # Assert
    assert client is execution_client
    assert http_kwargs["account_address"] is None
    assert http_kwargs["vault_address"] == " 0xvault "
    assert http_kwargs["include_builder_attribution"] is True
    assert execution_kwargs["client"] is http_client
    assert execution_kwargs["instrument_provider"] is provider
    assert execution_kwargs["account_address"] == "0xvault"
    resolve_account_address.assert_called_once_with(
        private_key=config.private_key,
        vault_address=config.vault_address,
        account_address=config.account_address,
        environment=HyperliquidEnvironment.MAINNET,
    )


def test_exec_factory_propagates_account_address_resolution_errors(monkeypatch):
    # Arrange
    get_http_client = MagicMock()
    get_provider = MagicMock()
    create_execution_client = MagicMock()
    resolve_account_address = MagicMock(side_effect=ValueError("invalid vault address"))

    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.factories.get_cached_hyperliquid_http_client",
        get_http_client,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.factories.get_cached_hyperliquid_instrument_provider",
        get_provider,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.factories.HyperliquidExecutionClient",
        create_execution_client,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.factories.nautilus_pyo3.hyperliquid_resolve_execution_account_address",
        resolve_account_address,
        raising=False,
    )

    config = HyperliquidExecClientConfig(
        private_key="0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        vault_address="0xinvalid",
    )

    # Act, Assert
    with pytest.raises(ValueError, match="invalid vault address"):
        HyperliquidLiveExecClientFactory.create(
            loop=MagicMock(),
            name="HYPERLIQUID",
            config=config,
            msgbus=MagicMock(),
            cache=MagicMock(),
            clock=MagicMock(),
        )

    get_http_client.assert_not_called()
    get_provider.assert_not_called()
    create_execution_client.assert_not_called()
    resolve_account_address.assert_called_once_with(
        private_key=config.private_key,
        vault_address=config.vault_address,
        account_address=config.account_address,
        environment=HyperliquidEnvironment.MAINNET,
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
