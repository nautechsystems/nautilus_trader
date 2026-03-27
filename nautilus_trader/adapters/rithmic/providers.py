"""Instrument provider for Rithmic."""

from __future__ import annotations

import time
from typing import TYPE_CHECKING, Optional

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import InstrumentId, Symbol, Venue
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.objects import Currency, Price, Quantity

from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic.config import to_binding_environment

if TYPE_CHECKING:
    from nautilus_trader.model.instruments import Instrument


RITHMIC_VENUE = Venue("RITHMIC")
INDEX_PRODUCTS = {
    "ES",
    "NQ",
    "YM",
    "RTY",
    "MES",
    "MNQ",
    "MYM",
    "M2K",
}
KNOWN_EXCHANGES = {
    "CME",
    "CBOT",
    "NYMEX",
    "COMEX",
    "ICE",
    "ICE_US",
    "EUREX",
    "MGEX",
}


def split_exchange_from_symbol(symbol: str) -> tuple[str, str | None]:
    """Split an exchange suffix from a symbol when encoded as `SYMBOL.EXCHANGE` or `SYMBOL:EXCHANGE`."""
    for separator in (".", ":"):
        if separator not in symbol:
            continue

        base, _, suffix = symbol.rpartition(separator)
        if suffix in KNOWN_EXCHANGES:
            return base, suffix

    return symbol, None


def normalize_rithmic_symbol(symbol: str) -> str:
    """Return the bare venue symbol without any encoded exchange suffix."""
    base, _ = split_exchange_from_symbol(symbol)
    return base


def resolve_exchange_hint(symbol: str, filters: Optional[dict] = None) -> Optional[str]:
    """Resolve an exchange from request filters first, then from a symbol suffix."""
    if filters:
        exchange = filters.get("exchange")
        if exchange:
            return exchange

        exchanges = filters.get("exchanges")
        if isinstance(exchanges, (list, tuple)) and exchanges:
            return exchanges[0]

    _, exchange = split_exchange_from_symbol(symbol)
    return exchange


class RithmicInstrumentProvider(InstrumentProvider):
    """
    Provides instrument definitions from Rithmic.

    Parameters
    ----------
    config : RithmicDataClientConfig
        The configuration for the provider.
    """

    def __init__(self, config: RithmicDataClientConfig) -> None:
        super().__init__(config=config.instrument_provider)
        self._config = config
        self._gateway = None
        self._provider = None
        self._uses_external_gateway = False

    @property
    def venue(self) -> Venue:
        """Return the venue."""
        return RITHMIC_VENUE

    async def load_all_async(self, filters: Optional[dict] = None) -> None:
        """
        Load all instruments from Rithmic.

        Parameters
        ----------
        filters : dict, optional
            Filters to apply when loading instruments.
        """
        await self._ensure_provider_connected()
        tradeable_only = self._tradeable_only(filters)

        await self._provider.load_all_async()
        for instrument in self._provider.instruments():
            if tradeable_only and not instrument.is_tradeable:
                continue
            self._cache_instrument(self._convert_instrument(instrument))

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: Optional[dict] = None,
    ) -> None:
        """
        Load specific instruments by ID.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : dict, optional
            Filters to apply when loading instruments.
        """
        await self._ensure_provider_connected()
        tradeable_only = self._tradeable_only(filters)

        for instrument_id in instrument_ids:
            symbol = normalize_rithmic_symbol(instrument_id.symbol.value)
            exchange = self._resolve_exchange(instrument_id.symbol.value, filters)
            if exchange is None:
                self._log.warning(f"Missing exchange for instrument {instrument_id}, skipping")
                continue

            loaded = await self._provider.load_instrument_async(symbol, exchange)
            if tradeable_only and not loaded.is_tradeable:
                continue
            self._cache_instrument(self._convert_instrument(loaded))

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: Optional[dict] = None,
    ) -> None:
        """
        Load a specific instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.
        filters : dict, optional
            Filters to apply when loading the instrument.
        """
        await self._ensure_provider_connected()
        tradeable_only = self._tradeable_only(filters)

        symbol = normalize_rithmic_symbol(instrument_id.symbol.value)
        exchange = self._resolve_exchange(instrument_id.symbol.value, filters)
        if exchange is None:
            raise ValueError(f"Missing exchange for instrument {instrument_id}")

        loaded = await self._provider.load_instrument_async(symbol, exchange)
        if tradeable_only and not loaded.is_tradeable:
            return
        self._cache_instrument(self._convert_instrument(loaded))

    def find(self, instrument_id: InstrumentId) -> Optional["Instrument"]:
        """
        Find an instrument by ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to find.

        Returns
        -------
        Instrument or None
        """
        instrument = self._instruments.get(instrument_id)
        if instrument is not None:
            return instrument

        symbol = normalize_rithmic_symbol(instrument_id.symbol.value)
        if symbol == instrument_id.symbol.value:
            return None

        normalized_id = InstrumentId.from_str(f"{symbol}.{RITHMIC_VENUE.value}")
        return self._instruments.get(normalized_id)

    def get_all(self) -> dict[InstrumentId, "Instrument"]:
        """
        Return all loaded instruments.

        Returns
        -------
        dict[InstrumentId, Instrument]
        """
        return self._instruments.copy()

    def bind_gateway(self, gateway) -> None:
        """Bind the provider to an existing connected gateway owned elsewhere."""
        try:
            from nautilus_trader.adapters.rithmic.bindings import (
                RithmicInstrumentProvider as RustInstrumentProvider,
            )
        except ImportError as e:
            raise ImportError(
                "Failed to import Rust bindings. Make sure the native extension is built."
            ) from e

        self._gateway = gateway
        self._provider = RustInstrumentProvider(gateway)
        self._uses_external_gateway = True

    def clear_gateway_binding(self) -> None:
        """Clear any externally managed gateway binding."""
        if self._uses_external_gateway:
            self._gateway = None
            self._provider = None
            self._uses_external_gateway = False

    async def _ensure_provider_connected(self) -> None:
        if self._gateway is None:
            try:
                from nautilus_trader.adapters.rithmic.bindings import RithmicGateway
                from nautilus_trader.adapters.rithmic.bindings import (
                    RithmicInstrumentProvider as RustInstrumentProvider,
                )
            except ImportError as e:
                raise ImportError(
                    "Failed to import Rust bindings. Make sure the native extension is built."
                ) from e

            self._gateway = RithmicGateway(
                environment=to_binding_environment(self._config.environment),
                username=self._config.username,
                password=self._config.password,
                system_name=self._config.system_name,
                app_name=self._config.app_name,
                app_version=self._config.app_version,
                fcm_id=self._config.fcm_id or "",
                ib_id=self._config.ib_id or "",
                account_id="",
                server=self._config.server,
                alt_server=self._config.alt_server,
                enable_ticker=True,
                enable_order=False,
                enable_pnl=False,
                enable_history=False,
            )
            self._provider = RustInstrumentProvider(self._gateway)
            self._uses_external_gateway = False

        if not self._gateway.is_connected():
            if self._uses_external_gateway:
                raise RuntimeError("Bound Rithmic gateway is not connected")
            await self._gateway.connect()

    def _cache_instrument(self, instrument: "Instrument") -> None:
        self.add(instrument)

        currency = getattr(instrument, "currency", None)
        if currency is not None:
            self.add_currency(currency)

    def _tradeable_only(self, filters: Optional[dict]) -> bool:
        if filters is None:
            return False
        return bool(filters.get("tradeable_only"))

    def _resolve_exchange(self, symbol: str, filters: Optional[dict]) -> Optional[str]:
        return resolve_exchange_hint(symbol, filters)

    def _convert_instrument(self, instrument) -> FuturesContract:
        if instrument.exchange:
            instrument_id = InstrumentId.from_str(
                f"{instrument.symbol}.{instrument.exchange}.{RITHMIC_VENUE.value}"
            )
        else:
            instrument_id = InstrumentId.from_str(f"{instrument.symbol}.{RITHMIC_VENUE.value}")
        raw_symbol = Symbol(instrument.symbol)
        asset_class = self._infer_asset_class(instrument.product_code, instrument.description)
        currency = Currency.from_str(instrument.currency)
        price_precision = instrument.price_precision
        price_increment = Price(instrument.tick_size, price_precision)
        multiplier = Quantity.from_str(str(instrument.point_value))
        lot_size = Quantity.from_str(str(instrument.contract_size))
        underlying = instrument.product_code or instrument.symbol
        ts_event = time.time_ns()
        expiration_ns = instrument.expiration_ts or 0
        info = {
            "exchange": instrument.exchange,
            "product_code": instrument.product_code,
            "description": instrument.description,
            "is_tradeable": instrument.is_tradeable,
        }

        return FuturesContract(
            instrument_id=instrument_id,
            raw_symbol=raw_symbol,
            asset_class=asset_class,
            currency=currency,
            price_precision=price_precision,
            price_increment=price_increment,
            multiplier=multiplier,
            lot_size=lot_size,
            underlying=underlying,
            activation_ns=0,
            expiration_ns=expiration_ns,
            ts_event=ts_event,
            ts_init=ts_event,
            exchange=instrument.exchange,
            info=info,
        )

    def _infer_asset_class(self, product_code: str, description: str) -> AssetClass:
        if product_code in INDEX_PRODUCTS:
            return AssetClass.INDEX

        if description:
            description_upper = description.upper()
            if any(token in description_upper for token in ("S&P", "NASDAQ", "DOW", "RUSSELL")):
                return AssetClass.INDEX

        return AssetClass.COMMODITY
