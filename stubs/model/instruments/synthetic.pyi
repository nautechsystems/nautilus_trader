from nautilus_trader.core.nautilus_pyo3 import Data, InstrumentId, Price, Symbol, Venue
from typing import Any

class SyntheticInstrument(Data):
    """
    Represents a synthetic instrument with prices derived from component instruments using a
    formula.

    The `id` for the synthetic will become `{symbol}.{SYNTH}`.

    Parameters
    ----------
    symbol : Symbol
        The symbol for the synethic instrument.
    price_precision : uint8_t
        The price precision for the synthetic instrument.
    components : list[InstrumentId]
        The component instruments for the synthetic instrument.
    formula : str
        The derivation formula for the synthetic instrument.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `price_precision` is greater than 9.
    OverflowError
        If `price_precision` is negative (< 0).
    ValueError
        If the `components` list does not contain at least 2 instrument IDs.
    ValueError
        If the `formula` is not a valid string.
    ValueError
        If the `formula` is not a valid expression.

    Warnings
    --------
    All component instruments should already be defined and exist in the cache prior to defining
    a new synthetic instrument.

    """

    id: InstrumentId
    def __init__(
        self,
        symbol: Symbol,
        price_precision: int,
        components: list[InstrumentId],
        formula: str,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    def __eq__(self, other: SyntheticInstrument) -> bool: ...
    def __hash__(self) -> int: ...
    @property
    def price_precision(self) -> int:
        """
        Return the precision for the synthetic instrument.

        Returns
        -------
        int

        """
        ...
    @property
    def price_increment(self) -> Price:
        """
        Return the minimum price increment (tick size) for the synthetic instrument.

        Returns
        -------
        Price

        """
        ...
    @property
    def components(self) -> list[InstrumentId]:
        """
        Return the components of the synthetic instrument.

        Returns
        -------
        list[InstrumentId]

        """
        ...
    @property
    def formula(self) -> str:
        """
        Return the synthetic instrument internal derivation formula.

        Returns
        -------
        str

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        ...
    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        ...
    def change_formula(self, formula: str) -> None:
        """
        Change the internal derivation formula for the synthetic instrument.

        Parameters
        ----------
        formula : str
            The derivation formula to change to.

        Raises
        ------
        ValueError
            If the `formula` is not a valid string.
        ValueError
            If the `formula` is not a valid expression.

        """
        ...
    def calculate(self, inputs: list[float]) -> Price:
        """
        Calculate the price of the synthetic instrument from the given `inputs`.

        Parameters
        ----------
        inputs : list[double]

        Returns
        -------
        Price

        Raises
        ------
        ValueError
            If `inputs` is empty, contains a NaN value, or length is different from components count.
        RuntimeError
            If an internal error occurs when calculating the price.

        """
        ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> SyntheticInstrument:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        SyntheticInstrument

        """
        ...
    @staticmethod
    def to_dict(obj: SyntheticInstrument) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...