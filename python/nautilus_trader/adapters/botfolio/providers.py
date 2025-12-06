# -------------------------------------------------------------------------------------------------
#  Bot-folio Local Paper Trading Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.botfolio.constants import BOTFOLIO_VENUE
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BotfolioInstrumentProvider(InstrumentProvider):
    """
    Provides instruments for Botfolio local paper trading.

    For simplicity, this provider creates instruments on-demand based on
    symbol naming conventions:
    - Symbols containing "/" are treated as currency pairs (e.g., BTC/USD)
    - Symbols ending with "-USD" or "-USDT" are treated as crypto
    - Other symbols are treated as US equities

    Parameters
    ----------
    clock : LiveClock
        The clock for the provider.
    config : InstrumentProviderConfig
        The configuration for the provider.

    """

    def __init__(
        self,
        clock: LiveClock,
        config: InstrumentProviderConfig,
    ) -> None:
        super().__init__(config=config)
        self._clock = clock
        self._log_warnings = config.log_warnings

    async def load_all_async(self, filters: dict | None = None) -> None:
        """
        Load all available instruments.

        For Botfolio, instruments are created on-demand, so this is a no-op.
        """
        self._log.info("Botfolio instruments are created on-demand")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """Load specific instruments by ID."""
        for instrument_id in instrument_ids:
            await self.load_async(instrument_id, filters)

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        """Load a single instrument by ID."""
        # Check if already loaded
        if instrument_id in self._instruments:
            return

        symbol_str = instrument_id.symbol.value

        try:
            instrument = self._create_instrument(symbol_str)
            self.add(instrument)
            self._log.debug(f"Created instrument: {instrument_id}")
        except Exception as e:
            if self._log_warnings:
                self._log.warning(f"Failed to create instrument {symbol_str}: {e}")

    def _create_instrument(self, symbol_str: str) -> Equity | CurrencyPair:
        """
        Create an instrument based on symbol naming conventions.

        Parameters
        ----------
        symbol_str : str
            The symbol string.

        Returns
        -------
        Equity | CurrencyPair
            The created instrument.

        """
        instrument_id = InstrumentId(
            symbol=Symbol(symbol_str),
            venue=BOTFOLIO_VENUE,
        )

        # Determine instrument type based on symbol
        if "/" in symbol_str:
            # Currency pair (e.g., BTC/USD, EUR/USD)
            return self._create_currency_pair(instrument_id, symbol_str)
        elif symbol_str.endswith("-USD") or symbol_str.endswith("-USDT"):
            # Crypto (e.g., BTC-USD, ETH-USDT)
            return self._create_currency_pair(instrument_id, symbol_str)
        else:
            # Default to equity
            return self._create_equity(instrument_id, symbol_str)

    def _create_equity(self, instrument_id: InstrumentId, symbol_str: str) -> Equity:
        """Create an equity instrument."""
        return Equity(
            instrument_id=instrument_id,
            raw_symbol=Symbol(symbol_str),
            currency=Currency.from_str("USD"),
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            lot_size=Quantity.from_str("1"),
            max_quantity=None,
            min_quantity=Quantity.from_str("1"),
            max_price=None,
            min_price=Price.from_str("0.01"),
            margin_init=Decimal("0"),
            margin_maint=Decimal("0"),
            maker_fee=Decimal("0"),
            taker_fee=Decimal("0"),
            ts_event=self._clock.timestamp_ns(),
            ts_init=self._clock.timestamp_ns(),
            info={},
        )

    def _create_currency_pair(
        self, instrument_id: InstrumentId, symbol_str: str
    ) -> CurrencyPair:
        """Create a currency pair instrument (crypto or forex)."""
        # Parse base and quote currencies
        if "/" in symbol_str:
            base_str, quote_str = symbol_str.split("/")
        elif "-" in symbol_str:
            base_str, quote_str = symbol_str.rsplit("-", 1)
        else:
            # Default to USD quote
            base_str = symbol_str
            quote_str = "USD"

        # Normalize quote currency
        if quote_str == "USDT":
            quote_str = "USD"  # Treat USDT as USD for simplicity

        base_currency = Currency.from_str(base_str)
        quote_currency = Currency.from_str(quote_str)

        # Determine precision based on asset type
        if base_str in ("BTC", "ETH", "SOL", "AVAX", "DOT", "LINK"):
            # Major crypto - 2 decimal places for price
            price_precision = 2
            size_precision = 8
        else:
            # Default
            price_precision = 5
            size_precision = 8

        return CurrencyPair(
            instrument_id=instrument_id,
            raw_symbol=Symbol(symbol_str),
            base_currency=base_currency,
            quote_currency=quote_currency,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=Price.from_str(f"0.{'0' * (price_precision - 1)}1"),
            size_increment=Quantity.from_str(f"0.{'0' * (size_precision - 1)}1"),
            lot_size=None,
            max_quantity=None,
            min_quantity=Quantity.from_str("0.00000001"),
            max_price=None,
            min_price=Price.from_str(f"0.{'0' * (price_precision - 1)}1"),
            margin_init=Decimal("0"),
            margin_maint=Decimal("0"),
            maker_fee=Decimal("0"),
            taker_fee=Decimal("0"),
            ts_event=self._clock.timestamp_ns(),
            ts_init=self._clock.timestamp_ns(),
            info={},
        )

