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

from typing import TYPE_CHECKING

from nautilus_trader.adapters.pancakeswap.constants import PANCAKESWAP_V2
from nautilus_trader.adapters.pancakeswap.constants import PANCAKESWAP_V2_BSC_CHAIN_ID
from nautilus_trader.adapters.pancakeswap.constants import PANCAKESWAP_V2_BSC_MAINNET_DEFAULTS
from nautilus_trader.adapters.pancakeswap.constants import PANCAKESWAP_V2_BSC_TESTNET_CHAIN_ID
from nautilus_trader.adapters.pancakeswap.constants import PANCAKESWAP_V2_BSC_TESTNET_DEFAULTS
from nautilus_trader.adapters.pancakeswap.constants import PANCAKESWAP_V2_BSC_VENUE
from nautilus_trader.adapters.pancakeswap.constants import PancakeSwapV2Defaults
from nautilus_trader.adapters.pancakeswap.constants import get_pancakeswap_v2_defaults
from nautilus_trader.adapters.pancakeswap.providers import PancakeSwapInstrumentProvider
from nautilus_trader.adapters.pancakeswap.providers import PancakeSwapInstrumentProviderConfig
from nautilus_trader.adapters.pancakeswap.providers import PancakeSwapPoolConfig


if TYPE_CHECKING:
    from nautilus_trader.adapters.pancakeswap.config import PancakeSwapV2ExecClientConfig
    from nautilus_trader.adapters.pancakeswap.factories import PancakeSwapV2ExecClientFactory


__all__ = [
    "PANCAKESWAP_V2",
    "PANCAKESWAP_V2_BSC_CHAIN_ID",
    "PANCAKESWAP_V2_BSC_MAINNET_DEFAULTS",
    "PANCAKESWAP_V2_BSC_TESTNET_CHAIN_ID",
    "PANCAKESWAP_V2_BSC_TESTNET_DEFAULTS",
    "PANCAKESWAP_V2_BSC_VENUE",
    "PancakeSwapInstrumentProvider",
    "PancakeSwapInstrumentProviderConfig",
    "PancakeSwapPoolConfig",
    "PancakeSwapV2Defaults",
    "PancakeSwapV2ExecClientConfig",
    "PancakeSwapV2ExecClientFactory",
    "get_pancakeswap_v2_defaults",
]


def __getattr__(name: str):  # type: ignore[no-untyped-def]
    if name == "PancakeSwapV2ExecClientConfig":
        from nautilus_trader.adapters.pancakeswap.config import PancakeSwapV2ExecClientConfig

        return PancakeSwapV2ExecClientConfig
    if name == "PancakeSwapV2ExecClientFactory":
        from nautilus_trader.adapters.pancakeswap.factories import PancakeSwapV2ExecClientFactory

        return PancakeSwapV2ExecClientFactory
    raise AttributeError(f"module 'nautilus_trader.adapters.pancakeswap' has no attribute '{name}'")
