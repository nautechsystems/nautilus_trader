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

import pytest

from nautilus_trader.adapters.pancakeswap.config import PancakeSwapV2ExecClientConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import TraderId


HAS_BLOCKCHAIN_BINDINGS = hasattr(nautilus_pyo3, "blockchain")


def _base_values() -> dict[str, object]:
    return {
        "trader_id": TraderId("TESTER-001"),
        "client_id": AccountId("SIM-001"),
        "wallet_address": "0x49E96E255bA418d08E66c35b588E2f2F3766E1d0",
        "http_rpc_url": "https://bsc.example.com",
        "signer_endpoint": "https://signer.example.com",
    }


def test_exec_config_requires_signer_endpoint() -> None:
    values = _base_values()
    values["signer_endpoint"] = ""

    with pytest.raises(ValueError):
        PancakeSwapV2ExecClientConfig(**values)


def test_exec_config_requires_positive_confirmation_and_polling_params() -> None:
    values = _base_values()
    values["execution_confirmations_required"] = 0

    with pytest.raises(ValueError, match="execution_confirmations_required"):
        PancakeSwapV2ExecClientConfig(**values)


@pytest.mark.skipif(not HAS_BLOCKCHAIN_BINDINGS, reason="Requires nautilus_pyo3 built with defi")
def test_exec_config_merges_tokens_with_wnative() -> None:
    values = _base_values()
    values["tokens"] = (
        "0x55d398326f99059fF775485246999027B3197955",
        "0x8AC76a51cc950d9822d68b83fE1Ad97B32Cd580d",
    )

    config = PancakeSwapV2ExecClientConfig(**values)
    pyo3_config = config.to_pyo3()

    assert pyo3_config.wallet_wnative_address is not None
    assert pyo3_config.tokens is not None
    assert pyo3_config.wallet_wnative_address in pyo3_config.tokens


def test_exec_config_defaults_to_preapproved_allowance_policy() -> None:
    config = PancakeSwapV2ExecClientConfig(**_base_values())

    assert config.execution_require_preapproved_allowance is True


def test_exec_config_to_pyo3_requires_defi_bindings_when_unavailable() -> None:
    if HAS_BLOCKCHAIN_BINDINGS:
        pytest.skip("DeFi bindings are available in this environment")

    config = PancakeSwapV2ExecClientConfig(**_base_values())
    with pytest.raises(RuntimeError, match="defi"):
        config.to_pyo3()
