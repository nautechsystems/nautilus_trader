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

from typing import Any

from nautilus_trader.adapters.pancakeswap.config import PancakeSwapV2ExecClientConfig
from nautilus_trader.core import nautilus_pyo3


def _require_blockchain_bindings() -> object:
    blockchain_module = getattr(nautilus_pyo3, "blockchain", None)
    if blockchain_module is None:
        raise RuntimeError(
            "PancakeSwap execution bindings require `nautilus_pyo3` built with the `defi` feature",
        )
    return blockchain_module


class PancakeSwapV2ExecClientFactory:
    """
    Thin Python wrapper for the Rust blockchain execution factory.

    This wrapper keeps Python surface ergonomic while preserving canonical Rust
    execution/config extraction through the global PyO3 registry.
    """

    @staticmethod
    def create() -> nautilus_pyo3.blockchain.BlockchainExecutionClientFactory:
        """Create the canonical PyO3 blockchain execution factory."""
        blockchain_module = _require_blockchain_bindings()
        return blockchain_module.BlockchainExecutionClientFactory()

    @staticmethod
    def create_config(
        config: PancakeSwapV2ExecClientConfig,
    ) -> nautilus_pyo3.blockchain.BlockchainExecutionClientConfig:
        """Convert wrapper config into canonical PyO3 config."""
        return config.to_pyo3()

    @staticmethod
    def add_to_builder(
        builder: Any,
        config: PancakeSwapV2ExecClientConfig,
        name: str | None = None,
    ) -> Any:
        """
        Add PancakeSwap execution client to a PyO3 live-node builder.

        Parameters
        ----------
        builder : Any
            A ``nautilus_pyo3.LiveNodeBuilderPy`` instance.
        config : PancakeSwapV2ExecClientConfig
            User-facing wrapper config.
        name : str, optional
            Optional execution client name override.

        Returns
        -------
        Any
            The updated builder.

        """
        factory = PancakeSwapV2ExecClientFactory.create()
        pyo3_config = PancakeSwapV2ExecClientFactory.create_config(config)
        return builder.add_exec_client(name, factory, pyo3_config)
