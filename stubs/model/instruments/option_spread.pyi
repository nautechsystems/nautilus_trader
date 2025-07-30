from decimal import Decimal
import pandas as pd
import pytz
from nautilus_trader.core.nautilus_pyo3 import AssetClass
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import Instrument
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.nautilus_pyo3 import OptionSpread as NautilusPyO3OptionSpread
from typing import Any
class OptionSpread(Instrument):
    """
    Represents a generic option spread instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    raw_symbol : Symbol
        The raw/local/native symbol for the instrument, assigned by the venue.
    asset_class : AssetClass
        The option spread asset class.
    currency : Currency
        The option spread currency.
    price_precision : int
        The price decimal precision.
    price_increment : Price
        The minimum price increment (tick size).
    multiplier : Quantity
        The option multiplier.
    lot_size : Quantity
        The rounded lot unit size (standard/board).
    underlying : str
        The underlying asset.
    strategy_type : str
        The strategy type of the spread.
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
        info: dict[Any, Any] | None = None,
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
    def from_dict(values: dict[str, Any]) -> OptionSpread:
        """
        Return an instrument from the given initialization values.
        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.
        Returns
        -------
        OptionSpread
        """
        ...
    @staticmethod
    def to_dict(obj: OptionSpread) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.
        Returns
        -------
        dict[str, object]
        """
        ...
    @staticmethod
    def from_pyo3(pyo3_instrument: NautilusPyO3OptionSpread) -> OptionSpread:
        """
        Return legacy Cython option spread instrument converted from the given pyo3 Rust object.
        Parameters
        ----------
        pyo3_instrument : nautilus_pyo3.OptionSpread
            The pyo3 Rust option spread instrument to convert from.
        Returns
        -------
        OptionSpread
        """
        ...