# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

"""
LMEX instrument provider — loads ``CurrencyPair`` definitions from the REST API.
"""

from __future__ import annotations

import math
from decimal import Decimal

from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
from nautilus_trader.adapters.lmex.http.market import LmexMarketHttpAPI
from nautilus_trader.adapters.lmex.schemas.market import LmexMarketSummary
from nautilus_trader.common.component import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def _decimal_places(value: float) -> int:
    """
    Count the number of significant decimal places in a float.

    Uses string conversion to avoid IEEE-754 rounding artefacts.

    Parameters
    ----------
    value : float
        A positive increment value (e.g. ``0.01``, ``1e-05``).

    Returns
    -------
    int
        Number of decimal places (0 for whole-number increments).

    Examples
    --------
    >>> _decimal_places(0.1)
    1
    >>> _decimal_places(0.00001)
    5
    >>> _decimal_places(1.0)
    0

    """
    if value <= 0:
        return 0
    # Use -log10 to estimate, then verify via string for edge cases
    raw_places = max(0, -int(math.floor(math.log10(value))))
    # Cross-check: format with enough digits to catch e.g. 0.00001
    formatted = f"{value:.{raw_places + 2}f}".rstrip("0")
    if "." in formatted:
        return len(formatted.split(".")[1])
    return 0


class LmexInstrumentProvider(InstrumentProvider):
    """
    Provides NautilusTrader instrument definitions sourced from the LMEX REST API.

    Calls ``GET /api/v3.2/market_summary`` and converts each active spot market
    into a ``CurrencyPair`` instrument.

    Parameters
    ----------
    market_api : LmexMarketHttpAPI
        The LMEX market HTTP API wrapper.
    config : InstrumentProviderConfig, optional
        Standard instrument provider configuration.

    """

    def __init__(
        self,
        market_api: LmexMarketHttpAPI,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._market_api = market_api
        self._log: Logger = Logger(type(self).__name__)
        self._log_warnings: bool = config.log_warnings if config else True

    async def load_all_async(self, filters: dict | None = None) -> None:
        """
        Load all active LMEX instruments into the provider cache.

        Fetches the full market summary from the LMEX REST API and converts
        each active spot pair to a ``CurrencyPair`` instrument.

        Parameters
        ----------
        filters : dict, optional
            Optional filter keys:

            - ``"symbol"`` : str — restrict to a single trading pair.
            - ``"spot_only"`` : bool — when ``True`` (default) skip futures
              instruments.

        """
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        spot_only: bool = True if filters is None else filters.get("spot_only", True)
        single_symbol: str | None = filters.get("symbol") if filters else None

        summaries = await self._market_api.get_market_summary(symbol=single_symbol)

        loaded = 0
        skipped = 0
        for summary in summaries:
            if not summary.active:
                skipped += 1
                continue
            if spot_only and summary.futures:
                skipped += 1
                continue
            try:
                instrument = self._parse_instrument(summary)
                self.add(instrument=instrument)
                loaded += 1
            except Exception as exc:
                if self._log_warnings:
                    self._log.warning(
                        f"Failed to parse instrument {summary.symbol!r}: {exc}"
                    )
                skipped += 1

        self._log.info(f"Loaded {loaded} instruments ({skipped} skipped)")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load specific instruments by ``InstrumentId``.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : dict, optional
            Passed through to ``load_all_async``; typically unused here.

        """
        if not instrument_ids:
            self._log.warning("No instrument IDs provided for loading")
            return

        # Validate venue
        for iid in instrument_ids:
            if iid.venue != LMEX_VENUE:
                raise ValueError(
                    f"Instrument {iid} does not belong to venue LMEX "
                    f"(got {iid.venue})"
                )

        # Fetch each symbol individually to avoid loading the full 2K+ list
        for iid in instrument_ids:
            symbol_str = iid.symbol.value
            try:
                summaries = await self._market_api.get_market_summary(symbol=symbol_str)
                for summary in summaries:
                    if summary.symbol == symbol_str:
                        instrument = self._parse_instrument(summary)
                        self.add(instrument=instrument)
                        break
                else:
                    self._log.warning(f"Symbol {symbol_str!r} not found in market_summary")
            except Exception as exc:
                self._log.warning(f"Failed to load instrument {iid}: {exc}")

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        """
        Load a single instrument by ``InstrumentId``.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to load.
        filters : dict, optional
            Ignored.

        """
        await self.load_ids_async([instrument_id], filters)

    # ------------------------------------------------------------------
    # Internal parsing
    # ------------------------------------------------------------------

    def _parse_instrument(self, s: LmexMarketSummary) -> CurrencyPair:
        """
        Convert a ``LmexMarketSummary`` into a ``CurrencyPair``.

        Parameters
        ----------
        s : LmexMarketSummary
            Raw market summary entry from the LMEX API.

        Returns
        -------
        CurrencyPair

        Raises
        ------
        ValueError
            If any required field is zero or invalid.

        """
        from nautilus_trader.model.currencies import Currency  # noqa: PLC0415

        price_precision = _decimal_places(s.minPriceIncrement)
        size_precision = _decimal_places(s.minSizeIncrement)

        instrument_id = InstrumentId(Symbol(s.symbol), LMEX_VENUE)

        try:
            base_currency = Currency.from_str(s.base)
        except Exception:
            base_currency = Currency(
                code=s.base,
                precision=size_precision,
                iso4217=0,
                name=s.base,
                currency_type=3,  # CurrencyType.CRYPTO
            )

        try:
            quote_currency = Currency.from_str(s.quote)
        except Exception:
            quote_currency = Currency(
                code=s.quote,
                precision=price_precision,
                iso4217=0,
                name=s.quote,
                currency_type=3,  # CurrencyType.CRYPTO
            )

        price_increment = Price(s.minPriceIncrement, price_precision)
        size_increment = Quantity(s.minSizeIncrement, size_precision)
        max_quantity = (
            Quantity(s.maxOrderSize, size_precision) if s.maxOrderSize > 0 else None
        )
        min_quantity = (
            Quantity(s.minOrderSize, size_precision) if s.minOrderSize > 0 else None
        )

        return CurrencyPair(
            instrument_id=instrument_id,
            raw_symbol=Symbol(s.symbol),
            base_currency=base_currency,
            quote_currency=quote_currency,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=price_increment,
            size_increment=size_increment,
            ts_event=0,
            ts_init=0,
            max_quantity=max_quantity,
            min_quantity=min_quantity,
            info={
                "symbol": s.symbol,
                "active": s.active,
                "futures": s.futures,
                "minValidPrice": s.minValidPrice,
            },
        )
