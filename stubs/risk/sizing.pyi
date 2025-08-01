from decimal import Decimal


class PositionSizer:
    """
    The base class for all position sizers.

    Parameters
    ----------
    instrument : Instrument
        The instrument for position sizing.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    instrument: Instrument

    def __init__(self, instrument: Instrument) -> None: ...
    def update_instrument(self, instrument: Instrument) -> None:
        """
        Update the internal instrument with the given instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the update.

        Raises
        ------
        ValueError
            If `instrument` does not equal the currently held instrument.

        """
        ...
    def calculate(
        self,
        entry: Price,
        stop_loss: Price,
        equity: Money,
        risk: Decimal,
        commission_rate: Decimal = Decimal(0),
        exchange_rate: Decimal = Decimal(1),
        hard_limit: Decimal | None = None,
        unit_batch_size: Decimal = Decimal(1),
        units: int = 1,
    ) -> Quantity:
        """Abstract method (implement in subclass)."""
        ...


class FixedRiskSizer(PositionSizer):
    """
    Provides position sizing calculations based on a given risk.

    Parameters
    ----------
    instrument : Instrument
        The instrument for position sizing.
    """

    def __init__(self, instrument: Instrument) -> None: ...
    def calculate(
        self,
        entry: Price,
        stop_loss: Price,
        equity: Money,
        risk: Decimal,
        commission_rate: Decimal = Decimal(0),
        exchange_rate: Decimal = Decimal(1),
        hard_limit: Decimal | None = None,
        unit_batch_size: Decimal = Decimal(1),
        units: int = 1,
    ) -> Quantity:
        """
        Calculate the position size quantity.

        Parameters
        ----------
        entry : Price
            The entry price.
        stop_loss : Price
            The stop loss price.
        equity : Money
            The account equity.
        risk : Decimal
            The risk percentage.
        exchange_rate : Decimal
            The exchange rate for the instrument quote currency vs account currency.
        commission_rate : Decimal
            The commission rate (>= 0).
        hard_limit : Decimal, optional
            The hard limit for the total quantity (>= 0).
        unit_batch_size : Decimal
            The unit batch size (> 0).
        units : int
            The number of units to batch the position into (> 0).

        Raises
        ------
        ValueError
            If `risk_bp` is not positive (> 0).
        ValueError
            If `xrate` is not positive (> 0).
        ValueError
            If `commission_rate` is negative (< 0).
        ValueError
            If `hard_limit` is not ``None`` and is not positive (> 0).
        ValueError
            If `unit_batch_size` is not positive (> 0).
        ValueError
            If `units` is not positive (> 0).

        Returns
        -------
        Quantity

        """
        ...

