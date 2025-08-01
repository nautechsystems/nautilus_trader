from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.nautilus_pyo3 import Venue

class Identifier:
    """
    The abstract base class for all identifiers.
    """

    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __lt__(self, other: Identifier) -> bool: ...
    def __le__(self, other: Identifier) -> bool: ...
    def __gt__(self, other: Identifier) -> bool: ...
    def __ge__(self, other: Identifier) -> bool: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def value(self) -> str:
        """
        Return the identifier (ID) value.

        Returns
        -------
        str

        """
        ...


class Symbol(Identifier):
    """
    Represents a valid ticker symbol ID for a tradable instrument.

    Parameters
    ----------
    value : str
        The ticker symbol ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.

    Warnings
    --------
    The ID value must be unique for a trading venue.

    References
    ----------
    https://en.wikipedia.org/wiki/Ticker_symbol
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: Symbol) -> bool: ...
    def __hash__(self) -> int: ...
    def is_composite(self) -> bool:
        """
        Returns true if the symbol string contains a period ('.').

        Returns
        -------
        str

        """
        ...
    def root(self) -> str:
        """
        Return the symbol root.

        The symbol root is the substring that appears before the first period ('.')
        in the full symbol string. It typically represents the underlying asset for
        futures and options contracts. If no period is found, the entire symbol
        string is considered the root.

        Returns
        -------
        str

        """
        ...
    def topic(self) -> str:
        """
        Return the symbol topic.

        The symbol topic is the root symbol with a wildcard '*' appended if the symbol has a root,
        otherwise returns the full symbol string.

        Returns
        -------
        str

        """
        ...


class Venue(Identifier):
    """
    Represents a valid trading venue ID.

    Parameters
    ----------
    name : str
        The venue ID value.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    """

    def __init__(self, name: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: Venue) -> bool: ...
    def __hash__(self) -> int: ...
    def is_synthetic(self) -> bool:
        """
        Return whether the venue is synthetic ('SYNTH').

        Returns
        -------
        bool

        """
        ...
    @staticmethod
    def from_code(code: str) -> Venue | None:
        """
        Return the venue with the given `code` from the built-in internal map (if found).

        Currency only supports CME Globex exchange ISO 10383 MIC codes.

        Parameters
        ----------
        code : str
            The code of the venue.

        Returns
        -------
        Venue or ``None``

        """
        ...


class InstrumentId(Identifier):
    """
    Represents a valid instrument ID.

    The symbol and venue combination should uniquely identify the instrument.

    Parameters
    ----------
    symbol : Symbol
        The instruments ticker symbol.
    venue : Venue
        The instruments trading venue.
    """

    def __init__(self, symbol: Symbol, venue: Venue) -> None: ...
    @property
    def symbol(self) -> Symbol:
        """
        Returns the instrument ticker symbol.

        Returns
        -------
        Symbol

        """
        ...
    @property
    def venue(self) -> Venue:
        """
        Returns the instrument trading venue.

        Returns
        -------
        Venue

        """
        ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: InstrumentId) -> bool: ...
    def __hash__(self) -> int: ...
    @staticmethod
    def from_str(value: str) -> InstrumentId:
        """
        Return an instrument ID parsed from the given string value.
        Must be correctly formatted including symbol and venue components either side of a single
        period.

        Examples: 'AUD/USD.IDEALPRO', 'BTCUSDT.BINANCE'

        Parameters
        ----------
        value : str
            The instrument ID string value to parse.

        Returns
        -------
        InstrumentId

        Raises
        ------
        ValueError
            If `value` is not a valid instrument ID string.

        """
        ...
    def is_synthetic(self) -> bool:
        """
        Return whether the instrument ID is a synthetic instrument (with venue of 'SYNTH').

        Returns
        -------
        bool

        """
        ...
    @staticmethod
    def from_pyo3(pyo3_instrument_id: InstrumentId) -> InstrumentId:
        """
        Return an instrument ID from the given PyO3 instance.

        Parameters
        ----------
        value : nautilus_pyo3.InstrumentId
            The PyO3 instrument ID instance.

        Returns
        -------
        InstrumentId

        """
        ...
    def to_pyo3(self) -> InstrumentId:
        """
        Return a pyo3 object from this legacy Cython instance.

        Returns
        -------
        nautilus_pyo3.InstrumentId

        """
        ...


class ComponentId(Identifier):
    """
    Represents a valid component ID.

    Parameters
    ----------
    value : str
        The component ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.

    Warnings
    --------
    The ID value must be unique at the trader level.
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: ComponentId) -> bool: ...
    def __hash__(self) -> int: ...


class ClientId(Identifier):
    """
    Represents a system client ID.

    Parameters
    ----------
    value : str
        The client ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.

    Warnings
    --------
    The ID value must be unique at the trader level.
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: ClientId) -> bool: ...
    def __hash__(self) -> int: ...


class TraderId(Identifier):
    """
    Represents a valid trader ID.

    Must be correctly formatted with two valid strings either side of a hyphen.
    It is expected a trader ID is the abbreviated name of the trader
    with an order ID tag number separated by a hyphen.

    Example: "TESTER-001".

    The reason for the numerical component of the ID is so that order and position IDs
    do not collide with those from another node instance.

    Parameters
    ----------
    value : str
        The trader ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string containing a hyphen.

    Warnings
    --------
    The name and tag combination ID value must be unique at the firm level.
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: TraderId) -> bool: ...
    def __hash__(self) -> int: ...
    def get_tag(self) -> str:
        """
        Return the order ID tag value for this ID.

        Returns
        -------
        str

        """
        ...


class StrategyId(Identifier):
    """
    Represents a valid strategy ID.

    Must be correctly formatted with two valid strings either side of a hyphen.
    It is expected a strategy ID is the class name of the strategy,
    with an order ID tag number separated by a hyphen.

    Example: "EMACross-001".

    The reason for the numerical component of the ID is so that order and position IDs
    do not collide with those from another strategy within the node instance.

    Parameters
    ----------
    value : str
        The strategy ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string containing a hyphen.

    Warnings
    --------
    The name and tag combination must be unique at the trader level.
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: StrategyId) -> bool: ...
    def __hash__(self) -> int: ...
    def get_tag(self) -> str:
        """
        Return the order ID tag value for this ID.

        Returns
        -------
        str

        """
        ...
    def is_external(self) -> bool:
        """
        If the strategy ID is the global 'external' strategy. This represents
        the strategy for all orders interacting with this instance of the system
        which did not originate from any strategy being managed by the system.

        Returns
        -------
        bool

        """
        ...


class ExecAlgorithmId(Identifier):
    """
    Represents a valid execution algorithm ID.

    Parameters
    ----------
    value : str
        The execution algorithm ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: ExecAlgorithmId) -> bool: ...
    def __hash__(self) -> int: ...


class AccountId(Identifier):
    """
    Represents a valid account ID.

    Must be correctly formatted with two valid strings either side of a hyphen.
    It is expected an account ID is the name of the issuer with an account number
    separated by a hyphen.

    Example: "IB-D02851908".

    Parameters
    ----------
    value : str
        The account ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string containing a hyphen.

    Warnings
    --------
    The issuer and number ID combination must be unique at the firm level.
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: AccountId) -> bool: ...
    def __hash__(self) -> int: ...
    def get_issuer(self) -> str:
        """
        Return the account issuer for this ID.

        Returns
        -------
        str

        """
        ...
    def get_id(self) -> str:
        """
        Return the account ID without issuer name.

        Returns
        -------
        str

        """
        ...


class ClientOrderId(Identifier):
    """
    Represents a valid client order ID (assigned by the Nautilus system).

    Parameters
    ----------
    value : str
        The client order ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.

    Warnings
    --------
    The ID value must be unique at the firm level.
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: ClientOrderId) -> bool: ...
    def __hash__(self) -> int: ...


class VenueOrderId(Identifier):
    """
    Represents a valid venue order ID (assigned by a trading venue).

    Parameters
    ----------
    value : str
        The venue assigned order ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: VenueOrderId) -> bool: ...
    def __hash__(self) -> int: ...


class OrderListId(Identifier):
    """
    Represents a valid order list ID (assigned by the Nautilus system).

    Parameters
    ----------
    value : str
        The order list ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: OrderListId) -> bool: ...
    def __hash__(self) -> int: ...


class PositionId(Identifier):
    """
    Represents a valid position ID.

    Parameters
    ----------
    value : str
        The position ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string containing a hyphen.
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: PositionId) -> bool: ...
    def __hash__(self) -> int: ...


class TradeId(Identifier):
    """
    Represents a valid trade match ID (assigned by a trading venue).

    Maximum length is 36 characters.
    Can correspond to the `TradeID <1003> field` of the FIX protocol.

    The unique ID assigned to the trade entity once it is received or matched by
    the exchange or central counterparty.

    Parameters
    ----------
    value : str
        The trade match ID value.

    Raises
    ------
    ValueError
        If `value` is not a valid string.
    ValueError
        If `value` length exceeds maximum 36 characters.

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0/tagnum_1003.html
    """

    def __init__(self, value: str) -> None: ...
    def __getstate__(self): ...
    def __setstate__(self, state) -> None: ...
    def __eq__(self, other: TradeId) -> bool: ...
    def __hash__(self) -> int: ...
