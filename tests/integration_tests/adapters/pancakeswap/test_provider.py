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

from nautilus_trader.adapters.pancakeswap.providers import PancakeSwapInstrumentProvider
from nautilus_trader.adapters.pancakeswap.providers import PancakeSwapInstrumentProviderConfig
from nautilus_trader.adapters.pancakeswap.providers import PancakeSwapPoolConfig
from nautilus_trader.adapters.pancakeswap.symbol import pool_instrument_id
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue


def test_provider_load_all_builds_pool_instrument_with_dex_venue() -> None:
    config = PancakeSwapInstrumentProviderConfig(
        chain="Bsc",
        dex_type="PancakeSwapV2",
        pools=(
            PancakeSwapPoolConfig(
                pool_address="0x16b9a8284fA6fd1D4B1A97fA50a84d9E1f4dA0b1",
                token0_address="0x55d398326f99059fF775485246999027B3197955",
                token0_symbol="USDT",
                token0_decimals=18,
                token1_address="0x8AC76a51cc950d9822d68b83fE1Ad97B32Cd580d",
                token1_symbol="USDC",
                token1_decimals=18,
                factory_pair_address="0x16b9a8284fA6fd1D4B1A97fA50a84d9E1f4dA0b1",
            ),
        ),
    )
    provider = PancakeSwapInstrumentProvider(config=config)

    provider.load_all()

    expected_id = pool_instrument_id(
        "0x16b9a8284fA6fd1D4B1A97fA50a84d9E1f4dA0b1",
        chain="Bsc",
        dex_type="PancakeSwapV2",
    )
    instrument = provider.find(expected_id)

    assert provider.count == 1
    assert instrument is not None
    assert instrument.id.symbol.value == expected_id.symbol.value
    assert instrument.id.venue == Venue("Bsc:PancakeSwapV2")
    assert instrument.price_precision == 16
    assert instrument.size_precision == 16
    assert instrument.base_currency.precision == 16
    assert instrument.quote_currency.precision == 16


def test_provider_load_ids_only_loads_requested_pool() -> None:
    config = PancakeSwapInstrumentProviderConfig(
        pools=(
            PancakeSwapPoolConfig(
                pool_address="0x16b9a8284fA6fd1D4B1A97fA50a84d9E1f4dA0b1",
                token0_address="0x55d398326f99059fF775485246999027B3197955",
                token0_symbol="USDT",
                token0_decimals=18,
                token1_address="0x8AC76a51cc950d9822d68b83fE1Ad97B32Cd580d",
                token1_symbol="USDC",
                token1_decimals=18,
            ),
            PancakeSwapPoolConfig(
                pool_address="0x19B53E6f8eB6A3f1897dC4D7A1166f9DfD95A2d8",
                token0_address="0x55d398326f99059fF775485246999027B3197955",
                token0_symbol="USDT",
                token0_decimals=18,
                token1_address="0x7130d2A12B9BCbfae4f2634d864A1Ee1Ce3Ead9c",
                token1_symbol="BTCB",
                token1_decimals=18,
            ),
        ),
    )
    provider = PancakeSwapInstrumentProvider(config=config)

    first_id = pool_instrument_id(
        "0x16b9a8284fA6fd1D4B1A97fA50a84d9E1f4dA0b1",
        chain="Bsc",
        dex_type="PancakeSwapV2",
    )
    second_id = pool_instrument_id(
        "0x19B53E6f8eB6A3f1897dC4D7A1166f9DfD95A2d8",
        chain="Bsc",
        dex_type="PancakeSwapV2",
    )

    provider.load_ids([first_id])

    assert provider.count == 1
    assert provider.find(first_id) is not None
    assert provider.find(second_id) is None


def test_provider_rejects_invalid_pool_address() -> None:
    config = PancakeSwapInstrumentProviderConfig(
        pools=(
            PancakeSwapPoolConfig(
                pool_address="0x1234",
                token0_address="0x55d398326f99059fF775485246999027B3197955",
                token0_symbol="USDT",
                token0_decimals=18,
                token1_address="0x8AC76a51cc950d9822d68b83fE1Ad97B32Cd580d",
                token1_symbol="USDC",
                token1_decimals=18,
            ),
        ),
    )
    provider = PancakeSwapInstrumentProvider(config=config)

    with pytest.raises(ValueError, match="Invalid pool address"):
        provider.load_all()


def test_provider_rejects_factory_pair_mismatch() -> None:
    config = PancakeSwapInstrumentProviderConfig(
        pools=(
            PancakeSwapPoolConfig(
                pool_address="0x16b9a8284fA6fd1D4B1A97fA50a84d9E1f4dA0b1",
                token0_address="0x55d398326f99059fF775485246999027B3197955",
                token0_symbol="USDT",
                token0_decimals=18,
                token1_address="0x8AC76a51cc950d9822d68b83fE1Ad97B32Cd580d",
                token1_symbol="USDC",
                token1_decimals=18,
                factory_pair_address="0x16b9a8284fA6fd1D4B1A97fA50a84d9E1f4dA0b2",
            ),
        ),
    )
    provider = PancakeSwapInstrumentProvider(config=config)

    with pytest.raises(ValueError, match=r"factory\.getPair"):
        provider.load_all()


def test_pool_instrument_id_validates_and_normalizes_address() -> None:
    instrument_id = pool_instrument_id(
        "0x16b9a8284fA6fd1D4B1A97fA50a84d9E1f4dA0b1",
        chain="Bsc",
        dex_type="PancakeSwapV2",
    )

    assert isinstance(instrument_id, InstrumentId)
    assert instrument_id.venue == Venue("Bsc:PancakeSwapV2")
    assert instrument_id.symbol.value.startswith("0x")
