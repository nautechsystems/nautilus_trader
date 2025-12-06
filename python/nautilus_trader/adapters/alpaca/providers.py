# -------------------------------------------------------------------------------------------------
#  Bot-folio Alpaca Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.alpaca.constants import ALPACA_VENUE
from nautilus_trader.adapters.alpaca.http.client import AlpacaHttpClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class AlpacaInstrumentProvider(InstrumentProvider):
    """
    Provides instruments from Alpaca.

    Parameters
    ----------
    client : AlpacaHttpClient
        The Alpaca HTTP client.
    clock : LiveClock
        The clock for the provider.
    config : InstrumentProviderConfig
        The configuration for the provider.

    """

    def __init__(
        self,
        client: AlpacaHttpClient,
        clock: LiveClock,
        config: InstrumentProviderConfig,
    ) -> None:
        super().__init__(config=config)
        self._client = client
        self._clock = clock
        self._log_warnings = config.log_warnings

    async def load_all_async(self, filters: dict | None = None) -> None:
        """Load all available instruments from Alpaca."""
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        # Fetch active US equity assets
        assets = await self._client.get_assets(status="active", asset_class="us_equity")

        for asset_data in assets:
            if not asset_data.get("tradable"):
                continue

            try:
                instrument = self._parse_equity(asset_data)
                self.add(instrument)
            except Exception as e:
                if self._log_warnings:
                    self._log.warning(f"Failed to parse asset {asset_data.get('symbol')}: {e}")

        self._log.info(f"Loaded {len(self._instruments)} Alpaca instruments")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """Load specific instruments by ID."""
        for instrument_id in instrument_ids:
            symbol = instrument_id.symbol.value

            try:
                asset_data = await self._client.get_asset(symbol)

                if not asset_data.get("tradable"):
                    if self._log_warnings:
                        self._log.warning(f"Asset {symbol} is not tradable")
                    continue

                instrument = self._parse_equity(asset_data)
                self.add(instrument)

            except Exception as e:
                if self._log_warnings:
                    self._log.warning(f"Failed to load instrument {symbol}: {e}")

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        """Load a single instrument by ID."""
        await self.load_ids_async([instrument_id], filters)

    def _parse_equity(self, data: dict[str, Any]) -> Equity:
        """Parse Alpaca asset data into a Nautilus Equity instrument."""
        symbol_str = data["symbol"]
        instrument_id = InstrumentId(
            symbol=Symbol(symbol_str),
            venue=ALPACA_VENUE,
        )

        # Alpaca doesn't provide lot size info, default to 1
        # Price precision is typically 2 decimals for USD
        price_precision = 2
        size_precision = 0  # Whole shares (fractional handled separately)

        # Default tick size for US equities
        price_increment = Price.from_str("0.01")
        size_increment = Quantity.from_str("1")

        return Equity(
            instrument_id=instrument_id,
            raw_symbol=Symbol(symbol_str),
            currency=Currency.from_str("USD"),
            price_precision=price_precision,
            price_increment=price_increment,
            lot_size=size_increment,
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
            info=data,  # Store raw data for reference
        )

