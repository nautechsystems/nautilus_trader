from decimal import Decimal
from typing import Any

import pandas as pd

from nautilus_trader.model.enums import AssetClass
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Symbol
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Currency
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class FuturesSpread(Instrument):
    """
    Represents a generic deliverable futures spread instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    raw_symbol : Symbol
        The raw/local/native symbol for the instrument, assigned by the venue.
    asset_class : AssetClass
        The futures spread asset class.
    currency : Currency
        The futures spread currency.
    price_precision : int
        The price decimal precision.
    price_increment : Decimal
        The minimum price increment (tick size).
    multiplier : Quantity
        The contract multiplier.
    lot_size : Quantity
        The rounded lot unit size (standard/board).
    underlying : str
        The underlying asset.
    strategy_type : str
        The strategy type for the spread.
    activation_ns : uint64_t
        UNIX timestamp (nanoseconds) for contract activation.
    expiration_ns : uint64_t
        UNIX timestamp (nanoseconds) for contract expiration.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.
    margin_init : Decimal, optional
        The initial (order) margin requirement in percentage of order value.
    margin_maint : Decimal, optional
        The maintenance (position) margin in percentage of position value.
    maker_fee : Decimal, optional
        The fee rate for liquidity makers as a percentage of order value.
    taker_fee : Decimal, optional
        The fee rate for liquidity takers as a percentage of order value.
    exchange : str, optional
        The exchange ISO 10383 Market Identifier Code (MIC) where the instrument trades.
    info : dict[str, object], optional
        The additional instrument information.

    Raises
    ------
    ValueError
        If `strategy_type` is not a valid string.
    ValueError
        If `multiplier` is not positive (> 0).
    ValueError
        If `price_precision` is negative (< 0).
    ValueError
        If `tick_size` is not positive (> 0).
    ValueError
        If `lot_size` is not positive (> 0).
    ValueError
        If `margin_init` is negative (< 0).
    ValueError
        If `margin_maint` is negative (< 0).
    ValueError
        If `exchange` is not ``None`` and not a valid string.

    """

    exchange: str | None
    underlying: str
    strategy_type: str
    activation_ns: int
    expiration_ns: int

    def __init__(
        self,
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        currency: Currency,
        price_precision: int,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        underlying: str,
        strategy_type: str,
        activation_ns: int,
        expiration_ns: int,
        ts_event: int,
        ts_init: int,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        maker_fee: Decimal | None = None,
        taker_fee: Decimal | None = None,
        exchange: str | None = None,
        info: dict[str, Any] | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    @property
    def activation_utc(self) -> pd.Timestamp:
        """
        Return the contract activation timestamp (UTC).

        Returns
        -------
        pd.Timestamp
            tz-aware UTC.

        """
        ...
    @property
    def expiration_utc(self) -> pd.Timestamp:
        """
        Return the contract expriation timestamp (UTC).

        Returns
        -------
        pd.Timestamp
            tz-aware UTC.

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> FuturesSpread:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        FuturesSpread

        """
        ...
    @staticmethod
    def to_dict(obj: FuturesSpread) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_instrument: Instrument) -> FuturesSpread:
        """
        Return legacy Cython futures spread instrument converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_instrument : nautilus_pyo3.FuturesSpread
            The pyo3 Rust futures spread instrument to convert from.

        Returns
        -------
        FuturesSpread

        """
        ...
