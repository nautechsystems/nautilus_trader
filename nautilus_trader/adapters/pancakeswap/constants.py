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

from dataclasses import dataclass
from functools import lru_cache
from typing import Final

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.identifiers import Venue


PANCAKESWAP_V2: Final[str] = "PancakeSwapV2"
PANCAKESWAP_V2_BSC_VENUE: Final[Venue] = Venue("Bsc:PancakeSwapV2")
PANCAKESWAP_V2_BSC_CHAIN_ID: Final[int] = 56
PANCAKESWAP_V2_BSC_TESTNET_CHAIN_ID: Final[int] = 97
_DEFAULTS_BY_CHAIN_ID: Final[dict[int, tuple[str, str, str]]] = {
    56: (
        "0x10ED43C718714eb63d5aA57B78B54704E256024E",
        "0xcA143Ce32Fe78f1f7019d7d551a6402fC5350c73",
        "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c",
    ),
    97: (
        "0xD99D1c33F9fC3444f8101754aBC46c52416550D1",
        "0x6725F303b657a9451d8BA641348b6761A6CC7a17",
        "0xae13d989dac2f0debff460ac112a837c89baa7cd",
    ),
}


@dataclass(frozen=True)
class PancakeSwapV2Defaults:
    """Default router/factory/WBNB addresses for a supported chain."""

    chain_id: int
    router_address: str
    factory_address: str
    wnative_address: str


def _defaults_from_python_map(chain_id: int) -> tuple[str, str, str]:
    defaults = _DEFAULTS_BY_CHAIN_ID.get(chain_id)
    if defaults is None:
        raise ValueError(f"No PancakeSwapV2 defaults configured for chain_id={chain_id}")
    return defaults


def _defaults_from_rust_export(chain_id: int) -> tuple[str, str, str] | None:
    blockchain_module = getattr(nautilus_pyo3, "blockchain", None)
    if blockchain_module is None:
        return None
    if not hasattr(blockchain_module, "pancakeswap_v2_defaults_for_chain_id"):
        return None
    return blockchain_module.pancakeswap_v2_defaults_for_chain_id(chain_id)


@lru_cache(maxsize=8)
def get_pancakeswap_v2_defaults(chain_id: int) -> PancakeSwapV2Defaults:
    """Return canonical PancakeSwap V2 addresses with Rust-export preference."""
    defaults = _defaults_from_rust_export(chain_id)
    if defaults is None:
        defaults = _defaults_from_python_map(chain_id)

    router, factory, wnative = defaults
    return PancakeSwapV2Defaults(
        chain_id=chain_id,
        router_address=router,
        factory_address=factory,
        wnative_address=wnative,
    )


PANCAKESWAP_V2_BSC_MAINNET_DEFAULTS: Final[PancakeSwapV2Defaults] = get_pancakeswap_v2_defaults(
    PANCAKESWAP_V2_BSC_CHAIN_ID,
)
PANCAKESWAP_V2_BSC_TESTNET_DEFAULTS: Final[PancakeSwapV2Defaults] = get_pancakeswap_v2_defaults(
    PANCAKESWAP_V2_BSC_TESTNET_CHAIN_ID,
)
