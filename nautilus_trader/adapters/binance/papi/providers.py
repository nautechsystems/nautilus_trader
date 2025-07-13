# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument


class BinancePortfolioMarginInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading instruments from the Binance Portfolio Margin exchange.

    Portfolio Margin is a unified account that allows trading across spot, margin,
    and futures (both USDT-M and COIN-M) markets with cross-collateralization.
    This provider aggregates instruments from all these markets.

    Parameters
    ----------
    client : BinanceHttpClient
        The client for the provider.
    clock : LiveClock
        The clock for the provider.
    account_type : BinanceAccountType, default PORTFOLIO_MARGIN
        The Binance account type for the provider.
    config : InstrumentProviderConfig, optional
        The configuration for the provider.
    venue : Venue, default BINANCE_VENUE
        The venue for the provider.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        clock: LiveClock,
        account_type: BinanceAccountType = BinanceAccountType.PORTFOLIO_MARGIN,
        config: InstrumentProviderConfig | None = None,
        venue: Venue = BINANCE_VENUE,
    ) -> None:
        super().__init__(config=config)

        PyCondition.is_true(
            account_type.is_portfolio_margin,
            "account_type was not PORTFOLIO_MARGIN",
        )

        self._clock = clock
        self._client = client
        self._account_type = account_type
        self._venue = venue

        # Initialize constituent providers for different market types
        # Portfolio Margin can trade spot, margin, and futures instruments
        self._spot_provider = BinanceSpotInstrumentProvider(
            client=client,
            clock=clock,
            account_type=BinanceAccountType.SPOT,  # Use SPOT for spot instruments
            is_testnet=False,  # Portfolio margin is not available on testnet
            config=config,
            venue=venue,
        )

        self._futures_provider = BinanceFuturesInstrumentProvider(
            client=client,
            clock=clock,
            account_type=BinanceAccountType.USDT_FUTURE,  # Use USDT_FUTURE for futures instruments
            config=config,
            venue=venue,
        )

        self._log_warnings = config.log_warnings if config else True

        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

    async def load_all_async(self, filters: dict | None = None) -> None:
        """
        Load all instruments from Portfolio Margin markets asynchronously.

        This will load instruments from spot, margin, and futures markets that are
        available for Portfolio Margin trading.

        Parameters
        ----------
        filters : dict, optional
            The filters to apply to the loaded instruments.

        """
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all Portfolio Margin instruments{filters_str}")

        # Load instruments from all constituent markets
        await self._spot_provider.load_all_async(filters)
        await self._futures_provider.load_all_async(filters)

        # Copy loaded instruments from constituent providers
        for instrument in self._spot_provider.get_all().values():
            self.add(instrument)

        for instrument in self._futures_provider.get_all().values():
            self.add(instrument)

        self._log.info(
            f"Loaded {len(self._instruments)} Portfolio Margin instruments "
            f"({len(self._spot_provider.get_all())} spot, "
            f"{len(self._futures_provider.get_all())} futures)"
        )

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load the instruments for the given IDs asynchronously.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : dict, optional
            The filters to apply to the loaded instruments.

        """
        if not instrument_ids:
            self._log.info("No instrument IDs given for loading")
            return

        # Separate IDs by market type based on symbol naming conventions
        spot_ids = []
        futures_ids = []

        for instrument_id in instrument_ids:
            symbol_str = instrument_id.symbol.value
            # Futures symbols typically contain month codes or "PERP"
            if any(x in symbol_str for x in ["PERP", "H", "M", "U", "Z"]):
                futures_ids.append(instrument_id)
            else:
                spot_ids.append(instrument_id)

        # Load instruments from appropriate providers
        if spot_ids:
            await self._spot_provider.load_ids_async(spot_ids, filters)
            for instrument_id in spot_ids:
                instrument = self._spot_provider.get(instrument_id)
                if instrument is not None:
                    self.add(instrument)

        if futures_ids:
            await self._futures_provider.load_ids_async(futures_ids, filters)
            for instrument_id in futures_ids:
                instrument = self._futures_provider.get(instrument_id)
                if instrument is not None:
                    self.add(instrument)

        self._log.info(f"Loaded {len(instrument_ids)} Portfolio Margin instruments")

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        """
        Load the instrument for the given ID asynchronously.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.
        filters : dict, optional
            The filters to apply to the loaded instrument.

        """
        await self.load_ids_async([instrument_id], filters)

    def get_all(self) -> dict[InstrumentId, Instrument]:
        """
        Return all loaded instruments.

        Returns
        -------
        dict[InstrumentId, Instrument]

        """
        return self._instruments.copy()

    def currencies(self) -> list[str]:
        """
        Return a list of all currencies available from Portfolio Margin markets.

        Returns
        -------
        list[str]

        """
        currencies = set()
        currencies.update(self._spot_provider.currencies())
        currencies.update(self._futures_provider.currencies())
        return sorted(currencies)
