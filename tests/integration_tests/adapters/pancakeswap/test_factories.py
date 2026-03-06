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

from __future__ import annotations

from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.pancakeswap.config import PancakeSwapV2ExecClientConfig
from nautilus_trader.adapters.pancakeswap.constants import PANCAKESWAP_V2_BSC_MAINNET_DEFAULTS
from nautilus_trader.adapters.pancakeswap.constants import get_pancakeswap_v2_defaults
from nautilus_trader.adapters.pancakeswap.factories import PancakeSwapV2ExecClientFactory
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import TraderId


HAS_BLOCKCHAIN_BINDINGS = hasattr(nautilus_pyo3, "blockchain")


def _make_config(**overrides) -> PancakeSwapV2ExecClientConfig:
    values = {
        "trader_id": TraderId("TESTER-001"),
        "client_id": AccountId("SIM-001"),
        "wallet_address": "0x49E96E255bA418d08E66c35b588E2f2F3766E1d0",
        "http_rpc_url": "https://bsc.example.com",
        "signer_endpoint": "https://signer.example.com",
        "tokens": (
            "0x55d398326f99059fF775485246999027B3197955",
            "0x8AC76a51cc950d9822d68b83fE1Ad97B32Cd580d",
        ),
    }
    values.update(overrides)
    return PancakeSwapV2ExecClientConfig(**values)


def test_defaults_loaded_from_canonical_exports_or_fallbacks() -> None:
    defaults = get_pancakeswap_v2_defaults(56)

    assert defaults.router_address == PANCAKESWAP_V2_BSC_MAINNET_DEFAULTS.router_address
    assert defaults.factory_address == PANCAKESWAP_V2_BSC_MAINNET_DEFAULTS.factory_address
    assert defaults.wnative_address == PANCAKESWAP_V2_BSC_MAINNET_DEFAULTS.wnative_address
    if HAS_BLOCKCHAIN_BINDINGS:
        router, factory, wnative = nautilus_pyo3.blockchain.pancakeswap_v2_defaults_for_chain_id(56)
        assert defaults.router_address == router
        assert defaults.factory_address == factory
        assert defaults.wnative_address == wnative


@pytest.mark.skipif(not HAS_BLOCKCHAIN_BINDINGS, reason="Requires nautilus_pyo3 built with defi")
def test_exec_config_to_pyo3_wires_bsc_pancakeswap_venue() -> None:
    config = _make_config()

    pyo3_config = config.to_pyo3()

    assert str(pyo3_config.venue) == "Bsc:PancakeSwapV2"
    assert (
        pyo3_config.execution_router_address == PANCAKESWAP_V2_BSC_MAINNET_DEFAULTS.router_address
    )
    assert pyo3_config.wallet_wnative_address == PANCAKESWAP_V2_BSC_MAINNET_DEFAULTS.wnative_address
    assert pyo3_config.signer_endpoint == "https://signer.example.com"


def test_exec_config_rejects_unsafe_router_override_without_flag() -> None:
    with pytest.raises(ValueError, match="allow_unsafe_address_override"):
        _make_config(router_address="0x16b9a8284fA6fd1D4B1A97fA50a84d9E1f4dA0b1")


@pytest.mark.skipif(not HAS_BLOCKCHAIN_BINDINGS, reason="Requires nautilus_pyo3 built with defi")
def test_exec_config_accepts_unsafe_router_override_with_flag() -> None:
    config = _make_config(
        router_address="0x16b9a8284fA6fd1D4B1A97fA50a84d9E1f4dA0b1",
        factory_address="0x6725F303b657a9451d8BA641348b6761A6CC7a17",
        wnative_address="0xae13d989dac2f0debff460ac112a837c89baa7cd",
        allow_unsafe_address_override=True,
        chain_id=97,
    )

    pyo3_config = config.to_pyo3()

    assert pyo3_config.execution_router_address == "0x16b9a8284fA6fd1D4B1A97fA50a84d9E1f4dA0b1"
    assert pyo3_config.wallet_wnative_address == "0xae13d989dac2f0debff460ac112a837c89baa7cd"


@pytest.mark.skipif(not HAS_BLOCKCHAIN_BINDINGS, reason="Requires nautilus_pyo3 built with defi")
def test_factory_wraps_blockchain_execution_factory() -> None:
    factory = PancakeSwapV2ExecClientFactory.create()

    assert factory.name() == "BLOCKCHAIN"
    assert factory.config_type() == "BlockchainExecutionClientConfig"


@pytest.mark.skipif(not HAS_BLOCKCHAIN_BINDINGS, reason="Requires nautilus_pyo3 built with defi")
def test_factory_add_to_builder_uses_pyo3_factory_and_config() -> None:
    config = _make_config()

    builder = MagicMock()
    builder.add_exec_client.return_value = builder

    result = PancakeSwapV2ExecClientFactory.add_to_builder(
        builder=builder,
        config=config,
        name="BLOCKCHAIN",
    )

    assert result is builder
    assert builder.add_exec_client.call_count == 1

    args = builder.add_exec_client.call_args[0]
    assert args[0] == "BLOCKCHAIN"
    assert args[1].name() == "BLOCKCHAIN"
    assert str(args[2].venue) == "Bsc:PancakeSwapV2"


@pytest.mark.skipif(not HAS_BLOCKCHAIN_BINDINGS, reason="Requires nautilus_pyo3 built with defi")
def test_blockchain_pyo3_module_exports_execution_types() -> None:
    assert hasattr(nautilus_pyo3, "blockchain")
    assert hasattr(nautilus_pyo3.blockchain, "BlockchainExecutionClientConfig")
    assert hasattr(nautilus_pyo3.blockchain, "BlockchainExecutionClientFactory")


def test_factory_create_requires_defi_bindings_when_unavailable() -> None:
    if HAS_BLOCKCHAIN_BINDINGS:
        pytest.skip("DeFi bindings are available in this environment")

    with pytest.raises(RuntimeError, match="defi"):
        PancakeSwapV2ExecClientFactory.create()
