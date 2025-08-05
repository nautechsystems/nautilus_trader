from decimal import Decimal
from typing import Any

import pandas as pd

from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import Symbol
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Currency
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity

class CryptoFuture(Instrument):
    """
    Represents a deliverable futures contract instrument, with crypto assets
    as underlying and for settlement.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the instrument.
    raw_symbol : Symbol
        The raw/local/native symbol for the instrument, assigned by the venue.
    underlying : Currency
        The underlying asset.
    quote_currency : Currency
        The contract quote currency.
    settlement_currency : Currency
        The settlement currency.
    is_inverse : bool
        If the instrument costing is inverse (quantity expressed in quote currency units).
    activation_ns : uint64_t
        UNIX timestamp (nanoseconds) for contract activation.
    expiration_ns : uint64_t
        UNIX timestamp (nanoseconds) for contract expiration.
    price_precision : int
        The price decimal precision.
    size_precision : int
        The trading size decimal precision.
    price_increment : Price
        The minimum price increment (tick size).
    size_increment : Quantity
        The minimum size increment.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.
    multiplier : Quantity, default 1
        The contract multiplier.
    lot_size : Quantity
        The rounded lot unit size (standard/board).
    max_quantity : Quantity, optional
        The maximum allowable order quantity.
    min_quantity : Quantity, optional
        The minimum allowable order quantity.
    max_notional : Money, optional
        The maximum allowable order notional value.
    min_notional : Money, optional
        The minimum allowable order notional value.
    max_price : Price, optional
        The maximum allowable quoted price.
    min_price : Price, optional
        The minimum allowable quoted price.
    margin_init : Decimal, optional
        The initial (order) margin requirement in percentage of order value.
    margin_maint : Decimal, optional
        The maintenance (position) margin in percentage of position value.
    maker_fee : Decimal, optional
        The fee rate for liquidity makers as a percentage of order value.
    taker_fee : Decimal, optional
        The fee rate for liquidity takers as a percentage of order value.
    info : dict[str, object], optional
        The additional instrument information.

    Raises
    ------
    ValueError
        If `price_precision` is negative (< 0).
    ValueError
        If `size_precision` is negative (< 0).
    ValueError
        If `price_increment` is not positive (> 0).
    ValueError
        If `size_increment` is not positive (> 0).
    ValueError
        If `price_precision` is not equal to price_increment.precision.
    ValueError
        If `size_increment` is not equal to size_increment.precision.
    ValueError
        If `lot size` is not positive (> 0).
    ValueError
        If `max_quantity` is not positive (> 0).
    ValueError
        If `min_quantity` is negative (< 0).
    ValueError
        If `max_notional` is not positive (> 0).
    ValueError
        If `min_notional` is negative (< 0).
    ValueError
        If `max_price` is not positive (> 0).
    ValueError
        If `min_price` is negative (< 0).
    ValueError
        If `margin_init` is negative (< 0).
    ValueError
        If `margin_maint` is negative (< 0).

    """

    underlying: Currency
    settlement_currency: Currency
    activation_ns: int
    expiration_ns: int

    def __init__(
        self,
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        underlying: Currency,
        quote_currency: Currency,
        settlement_currency: Currency,
        is_inverse: bool,
        activation_ns: int,
        expiration_ns: int,
        price_precision: int,
        size_precision: int,
        price_increment: Price,
        size_increment: Quantity,
        ts_event: int,
        ts_init: int,
        multiplier: Quantity = ...,
        lot_size: Quantity = ...,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_notional: Money | None = None,
        min_notional: Money | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        maker_fee: Decimal | None = None,
        taker_fee: Decimal | None = None,
        info: dict = ...,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def get_base_currency(self) -> Currency:
        """
        Return the instruments base currency (underlying).

        Returns
        -------
        Currency

        """
        ...
    def get_settlement_currency(self) -> Currency:
        """
        Return the currency used to settle a trade of the instrument.

        Returns
        -------
        Currency

        """
        ...
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
    def from_dict(values: dict) -> CryptoFuture:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        CryptoFuture

        """
        ...
    @staticmethod
    def to_dict(obj: CryptoFuture) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_instrument: NautilusPyO3CryptoFuture) -> CryptoFuture:
        """
        Return legacy Cython crypto future instrument converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_instrument : nautilus_pyo3.CryptoFuture
            The pyo3 Rust option contract instrument to convert from.

        Returns
        -------
        CryptoFuture

        """
        ...
