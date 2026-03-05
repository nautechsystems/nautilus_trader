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

from decimal import Decimal

import msgspec

from nautilus_trader.adapters.pancakeswap.symbol import normalize_address
from nautilus_trader.adapters.pancakeswap.symbol import pool_instrument_id
from nautilus_trader.adapters.pancakeswap.symbol import validate_factory_pair_address
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


MAX_NAUTILUS_PRECISION = 16


class PancakeSwapPoolConfig(msgspec.Struct, frozen=True, kw_only=True):
    """
    Configuration for a single PancakeSwap pool instrument.

    Parameters
    ----------
    pool_address : str
        The pool contract address.
    token0_address : str
        The token0 contract address.
    token0_symbol : str
        The token0 display symbol/code.
    token0_decimals : int
        The token0 decimals from on-chain metadata.
    token1_address : str
        The token1 contract address.
    token1_symbol : str
        The token1 display symbol/code.
    token1_decimals : int
        The token1 decimals from on-chain metadata.
    factory_pair_address : str, optional
        Optional `factory.getPair(token0, token1)` result used to validate onboarding config.

    """

    pool_address: str
    token0_address: str
    token0_symbol: str
    token0_decimals: int
    token1_address: str
    token1_symbol: str
    token1_decimals: int
    factory_pair_address: str | None = None

    def __post_init__(self) -> None:
        PyCondition.valid_string(self.pool_address, "pool_address")
        PyCondition.valid_string(self.token0_address, "token0_address")
        PyCondition.valid_string(self.token0_symbol, "token0_symbol")
        PyCondition.valid_string(self.token1_address, "token1_address")
        PyCondition.valid_string(self.token1_symbol, "token1_symbol")

        if self.token0_decimals < 0:
            raise ValueError(f"token0_decimals must be non-negative, was {self.token0_decimals}")
        if self.token1_decimals < 0:
            raise ValueError(f"token1_decimals must be non-negative, was {self.token1_decimals}")


class PancakeSwapInstrumentProviderConfig(InstrumentProviderConfig, frozen=True, kw_only=True):
    """
    Configuration for ``PancakeSwapInstrumentProvider``.

    Parameters
    ----------
    chain : str, default "Bsc"
        Chain name component for DEX venues.
    dex_type : str, default "PancakeSwapV2"
        DEX type component for DEX venues.
    pools : tuple[PancakeSwapPoolConfig, ...], optional
        Config-driven pool universe.

    """

    chain: str = "Bsc"
    dex_type: str = "PancakeSwapV2"
    pools: tuple[PancakeSwapPoolConfig, ...] = ()

    def __post_init__(self) -> None:
        if self.pools and not self.load_all:
            msgspec.structs.force_setattr(self, "load_all", True)


class PancakeSwapInstrumentProvider(InstrumentProvider):
    """
    Config-driven PancakeSwap instrument provider.

    This MVP provider converts a small, operator-managed list of pools into Nautilus
    ``CurrencyPair`` instruments using DEX venue semantics (`<Chain>:<DexType>`) and
    pool-address instrument symbols.

    """

    def __init__(
        self,
        config: PancakeSwapInstrumentProviderConfig | None = None,
    ) -> None:
        if config is None:
            config = PancakeSwapInstrumentProviderConfig()

        super().__init__(config=config)
        self._config = config

    async def load_all_async(self, filters: dict | None = None) -> None:
        pools = self._config.pools
        if filters is not None and "pools" in filters:
            pools = tuple(filters["pools"])

        self._log.info(f"Loading {len(pools)} PancakeSwap pools from config")

        for pool in pools:
            instrument = self._build_instrument(pool)
            self.add(instrument)
            self.add_currency(instrument.base_currency)
            self.add_currency(instrument.quote_currency)

    def _build_instrument(self, pool: PancakeSwapPoolConfig) -> CurrencyPair:
        instrument_id = pool_instrument_id(
            pool.pool_address,
            chain=self._config.chain,
            dex_type=self._config.dex_type,
        )
        normalized_pool_address = instrument_id.symbol.value

        validate_factory_pair_address(
            normalized_pool_address,
            pool.factory_pair_address,
            chain=self._config.chain,
            dex_type=self._config.dex_type,
        )

        token0_address = normalize_address(
            pool.token0_address,
            chain=self._config.chain,
            dex_type=self._config.dex_type,
            label="token0 address",
        )
        token1_address = normalize_address(
            pool.token1_address,
            chain=self._config.chain,
            dex_type=self._config.dex_type,
            label="token1 address",
        )

        base_symbol = pool.token0_symbol.upper()
        quote_symbol = pool.token1_symbol.upper()

        base_currency = Currency(
            code=base_symbol,
            precision=min(pool.token0_decimals, MAX_NAUTILUS_PRECISION),
            iso4217=0,
            name=f"{base_symbol}:{token0_address}",
            currency_type=CurrencyType.CRYPTO,
        )
        quote_currency = Currency(
            code=quote_symbol,
            precision=min(pool.token1_decimals, MAX_NAUTILUS_PRECISION),
            iso4217=0,
            name=f"{quote_symbol}:{token1_address}",
            currency_type=CurrencyType.CRYPTO,
        )

        size_precision = min(pool.token0_decimals, MAX_NAUTILUS_PRECISION)
        price_precision = min(pool.token1_decimals, MAX_NAUTILUS_PRECISION)

        price_increment = Price.from_str(_increment_string(price_precision))
        size_increment = Quantity.from_str(_increment_string(size_precision))

        return CurrencyPair(
            instrument_id=instrument_id,
            raw_symbol=Symbol(normalized_pool_address),
            base_currency=base_currency,
            quote_currency=quote_currency,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=price_increment,
            size_increment=size_increment,
            ts_event=0,
            ts_init=0,
            maker_fee=Decimal(0),
            taker_fee=Decimal(0),
        )


def _increment_string(precision: int) -> str:
    if precision <= 0:
        return "1"

    return format(Decimal(1).scaleb(-precision), f".{precision}f")
