import decimal

QUANTITY_MAX: float = ...
QUANTITY_MIN: float = ...
PRICE_MAX: float = ...
PRICE_MIN: float = ...
MONEY_MAX: float = ...
MONEY_MIN: float = ...
HIGH_PRECISION: bool = ...
FIXED_PRECISION: int = ...
FIXED_SCALAR: float = ...
FIXED_PRECISION_BYTES: int = ...

class Quantity:
    """
    Represents a quantity with a non-negative value.

    Capable of storing either a whole number (no decimal places) of 'contracts'
    or 'shares' (instruments denominated in whole units) or a decimal value
    containing decimal places for instruments denominated in fractional units.

    Handles up to 16 decimals of precision (in high-precision mode).

    - ``QUANTITY_MAX`` = 34_028_236_692_093
    - ``QUANTITY_MIN`` = 0

    Parameters
    ----------
    value : integer, float, string, Decimal
        The value of the quantity.
    precision : uint8_t
        The precision for the quantity. Use a precision of 0 for whole numbers
        (no fractional units).

    Raises
    ------
    ValueError
        If `value` is greater than 34_028_236_692_093.
    ValueError
        If `value` is negative (< 0).
    ValueError
        If `precision` is greater than 16.
    OverflowError
        If `precision` is negative (< 0).

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/index.html#Qty
    """

    def __init__(self, value: float, precision: int) -> None: ...
    def __getstate__(self) -> tuple[int, int]: ...
    def __setstate__(self, state: tuple[int, int]) -> None: ...
    def __eq__(self, other: object) -> bool: ...
    def __lt__(self, other: object) -> bool: ...
    def __le__(self, other: object) -> bool: ...
    def __gt__(self, other: object) -> bool: ...
    def __ge__(self, other: object) -> bool: ...
    def __add__(a: object, b: object) -> decimal.Decimal | float: ...
    def __radd__(b: object, a: object) -> decimal.Decimal | float: ...
    def __sub__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rsub__(b: object, a: object) -> decimal.Decimal | float: ...
    def __mul__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rmul__(b: object, a: object) -> decimal.Decimal | float: ...
    def __truediv__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rtruediv__(b: object, a: object) -> decimal.Decimal | float: ...
    def __floordiv__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rfloordiv__(b: object, a: object) -> decimal.Decimal | float: ...
    def __mod__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rmod__(b: object, a: object) -> decimal.Decimal | float: ...
    def __neg__(self) -> decimal.Decimal: ...
    def __pos__(self) -> decimal.Decimal: ...
    def __abs__(self) -> decimal.Decimal: ...
    def __round__(self, ndigits: int | None = None) -> decimal.Decimal: ...
    def __float__(self) -> float: ...
    def __int__(self) -> int: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def raw(self) -> QuantityRaw:
        """
        Return the raw memory representation of the quantity value.

        Returns
        -------
        int

        """
        ...
    @property
    def precision(self) -> int:
        """
        Return the precision for the quantity.

        Returns
        -------
        uint8_t

        """
        ...
    @staticmethod
    def raw_to_f64(raw: int) -> float: ...
    @staticmethod
    def zero(precision: int = 0) -> Quantity:
        """
        Return a quantity with a value of zero.

        precision : uint8_t, default 0
            The precision for the quantity.

        Returns
        -------
        Quantity

        Raises
        ------
        ValueError
            If `precision` is greater than 16.
        OverflowError
            If `precision` is negative (< 0).

        Warnings
        --------
        The default precision is zero.

        """
        ...
    @staticmethod
    def from_raw(raw: int, precision: int) -> Quantity:
        """
        Return a quantity from the given `raw` fixed-point integer and `precision`.

        Handles up to 16 decimals of precision (in high-precision mode).

        Parameters
        ----------
        raw : int
            The raw fixed-point quantity value.
        precision : uint8_t
            The precision for the quantity. Use a precision of 0 for whole numbers
            (no fractional units).

        Returns
        -------
        Quantity

        Raises
        ------
        ValueError
            If `precision` is greater than 16.
        OverflowError
            If `precision` is negative (< 0).

        Warnings
        --------
        Small `raw` values can produce a zero quantity depending on the `precision`.

        """
        ...
    @staticmethod
    def from_str(value: str) -> Quantity:
        """
        Return a quantity parsed from the given string.

        Handles up to 16 decimals of precision (in high-precision mode).

        Parameters
        ----------
        value : str
            The value to parse.

        Returns
        -------
        Quantity

        Raises
        ------
        ValueError
            If inferred precision is greater than 16.
        OverflowError
            If inferred precision is negative (< 0).

        Warnings
        --------
        The decimal precision will be inferred from the number of digits
        following the '.' point (if no point then precision zero).

        """
        ...
    @staticmethod
    def from_int(value: int) -> Quantity:
        """
        Return a quantity from the given integer value.

        A precision of zero will be inferred.

        Parameters
        ----------
        value : int
            The value for the quantity.

        Returns
        -------
        Quantity

        """
        ...
    def to_formatted_str(self) -> str:
        """
        Return the formatted string representation of the quantity.

        Returns
        -------
        str

        """
        ...
    def as_decimal(self) -> decimal.Decimal:
        """
        Return the value as a built-in `Decimal`.

        Returns
        -------
        Decimal

        """
        ...
    def as_double(self) -> float:
        """
        Return the value as a `double`.

        Returns
        -------
        double

        """
        ...

class Price:
    """
    Represents a price in a market.

    The number of decimal places may vary. For certain asset classes, prices may
    have negative values. For example, prices for options instruments can be
    negative under certain conditions.

    Handles up to 16 decimals of precision (in high-precision mode).

    - ``PRICE_MAX`` = 17_014_118_346_046
    - ``PRICE_MIN`` = -17_014_118_346_046

    Parameters
    ----------
    value : integer, float, string or Decimal
        The value of the price.
    precision : uint8_t
        The precision for the price. Use a precision of 0 for whole numbers
        (no fractional units).

    Raises
    ------
    ValueError
        If `value` is greater than 17_014_118_346_046.
    ValueError
        If `value` is less than -17_014_118_346_046.
    ValueError
        If `precision` is greater than 16.
    OverflowError
        If `precision` is negative (< 0).

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/index.html#Price
    """

    def __init__(self, value: float, precision: int) -> None: ...
    def __getstate__(self) -> tuple[int, int]: ...
    def __setstate__(self, state: tuple[int, int]) -> None: ...
    def __eq__(self, other: object) -> bool: ...
    def __lt__(self, other: object) -> bool: ...
    def __le__(self, other: object) -> bool: ...
    def __gt__(self, other: object) -> bool: ...
    def __ge__(self, other: object) -> bool: ...
    def __add__(a: object, b: object) -> decimal.Decimal | float: ...
    def __radd__(b: object, a: object) -> decimal.Decimal | float: ...
    def __sub__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rsub__(b: object, a: object) -> decimal.Decimal | float: ...
    def __mul__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rmul__(b: object, a: object) -> decimal.Decimal | float: ...
    def __truediv__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rtruediv__(b: object, a: object) -> decimal.Decimal | float: ...
    def __floordiv__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rfloordiv__(b: object, a: object) -> decimal.Decimal | float: ...
    def __mod__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rmod__(b: object, a: object) -> decimal.Decimal | float: ...
    def __neg__(self) -> decimal.Decimal: ...
    def __pos__(self) -> decimal.Decimal: ...
    def __abs__(self) -> decimal.Decimal: ...
    def __round__(self, ndigits: int | None = None) -> decimal.Decimal: ...
    def __float__(self) -> float: ...
    def __int__(self) -> int: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def raw(self) -> PriceRaw:
        """
        Return the raw memory representation of the price value.

        Returns
        -------
        int

        """
        ...
    @property
    def precision(self) -> int:
        """
        Return the precision for the price.

        Returns
        -------
        uint8_t

        """
        ...
    @staticmethod
    def from_raw(raw: int, precision: int) -> Price:
        """
        Return a price from the given `raw` fixed-point integer and `precision`.

        Handles up to 16 decimals of precision (in high-precision mode).

        Parameters
        ----------
        raw : int
            The raw fixed-point price value.
        precision : uint8_t
            The precision for the price. Use a precision of 0 for whole numbers
            (no fractional units).

        Returns
        -------
        Price

        Raises
        ------
        ValueError
            If `precision` is greater than 16.
        OverflowError
            If `precision` is negative (< 0).

        Warnings
        --------
        Small `raw` values can produce a zero price depending on the `precision`.

        """
        ...
    @staticmethod
    def from_str(value: str) -> Price:
        """
        Return a price parsed from the given string.

        Handles up to 16 decimals of precision (in high-precision mode).

        Parameters
        ----------
        value : str
            The value to parse.

        Returns
        -------
        Price

        Warnings
        --------
        The decimal precision will be inferred from the number of digits
        following the '.' point (if no point then precision zero).

        Raises
        ------
        ValueError
            If inferred precision is greater than 16.
        OverflowError
            If inferred precision is negative (< 0).

        """
        ...
    @staticmethod
    def from_int(value: int) -> Price:
        """
        Return a price from the given integer value.

        A precision of zero will be inferred.

        Parameters
        ----------
        value : int
            The value for the price.

        Returns
        -------
        Price

        """
        ...
    def to_formatted_str(self) -> str:
        """
        Return the formatted string representation of the price.

        Returns
        -------
        str

        """
        ...
    def as_decimal(self) -> decimal.Decimal:
        """
        Return the value as a built-in `Decimal`.

        Returns
        -------
        Decimal

        """
        ...
    def as_double(self) -> float:
        """
        Return the value as a `double`.

        Returns
        -------
        double

        """
        ...

class Money:
    """
    Represents an amount of money in a specified currency denomination.

    - ``MONEY_MAX`` = 17_014_118_346_046
    - ``MONEY_MIN`` = -17_014_118_346_046

    Parameters
    ----------
    value : integer, float, string or Decimal
        The amount of money in the currency denomination.
    currency : Currency
        The currency of the money.

    Raises
    ------
    ValueError
        If `value` is greater than 17_014_118_346_046.
    ValueError
        If `value` is less than -17_014_118_346_046.
    """

    def __init__(self, value: object, currency: Currency) -> None: ...
    def __getstate__(self) -> tuple[int, str]: ...
    def __setstate__(self, state: tuple[int, str]) -> None: ...
    def __eq__(self, other: Money) -> bool: ...
    def __lt__(self, other: Money) -> bool: ...
    def __le__(self, other: Money) -> bool: ...
    def __gt__(self, other: Money) -> bool: ...
    def __ge__(self, other: Money) -> bool: ...
    def __add__(a: object, b: object) -> decimal.Decimal | float: ...
    def __radd__(b: object, a: object) -> decimal.Decimal | float: ...
    def __sub__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rsub__(b: object, a: object) -> decimal.Decimal | float: ...
    def __mul__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rmul__(b: object, a: object) -> decimal.Decimal | float: ...
    def __truediv__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rtruediv__(b: object, a: object) -> decimal.Decimal | float: ...
    def __floordiv__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rfloordiv__(b: object, a: object) -> decimal.Decimal | float: ...
    def __mod__(a: object, b: object) -> decimal.Decimal | float: ...
    def __rmod__(b: object, a: object) -> decimal.Decimal | float: ...
    def __neg__(self) -> decimal.Decimal: ...
    def __pos__(self) -> decimal.Decimal: ...
    def __abs__(self) -> decimal.Decimal: ...
    def __round__(self, ndigits: int | None = None) -> decimal.Decimal: ...
    def __float__(self) -> float: ...
    def __int__(self) -> int: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def raw(self) -> MoneyRaw:
        """
        Return the raw memory representation of the money amount.

        Returns
        -------
        int

        """
        ...
    @property
    def currency(self) -> Currency:
        """
        Return the currency for the money.

        Returns
        -------
        Currency

        """
        ...
    @staticmethod
    def from_raw(raw: int, currency: Currency) -> Money:
        """
        Return money from the given `raw` fixed-point integer and `currency`.

        Parameters
        ----------
        raw : int
            The raw fixed-point money amount.
        currency : Currency
            The currency of the money.

        Returns
        -------
        Money

        Warnings
        --------
        Small `raw` values can produce a zero money amount depending on the precision of the currency.

        """
        ...
    @staticmethod
    def from_str(value: str) -> Money:
        """
        Return money parsed from the given string.

        Must be correctly formatted with a value and currency separated by a
        whitespace delimiter.

        Example: "1000000.00 USD".

        Parameters
        ----------
        value : str
            The value to parse.

        Returns
        -------
        Money

        Raises
        ------
        ValueError
            If inferred currency precision is greater than 16.
        OverflowError
            If inferred currency precision is negative (< 0).

        """
        ...
    def to_formatted_str(self) -> str:
        """
        Return the formatted string representation of the money.

        Returns
        -------
        str

        """
        ...
    def as_decimal(self) -> decimal.Decimal:
        """
        Return the value as a built-in `Decimal`.

        Returns
        -------
        Decimal

        """
        ...
    def as_double(self) -> float:
        """
        Return the value as a `double`.

        Returns
        -------
        double

        """
        ...

class Currency:
    """
    Represents a medium of exchange in a specified denomination with a fixed
    decimal precision.

    Handles up to 16 decimals of precision (in high-precision mode).

    Parameters
    ----------
    code : str
        The currency code.
    precision : uint8_t
        The currency decimal precision.
    iso4217 : uint16
        The currency ISO 4217 code.
    name : str
        The currency name.
    currency_type : CurrencyType
        The currency type.

    Raises
    ------
    ValueError
        If `code` is not a valid string.
    OverflowError
        If `precision` is negative (< 0).
    ValueError
        If `precision` greater than 16.
    ValueError
        If `name` is not a valid string.
    """

    def __init__(
        self,
        code: str,
        precision: int,
        iso4217: int,
        name: str,
        currency_type: CurrencyType,
    ) -> None: ...
    def __getstate__(self) -> tuple[str, int, int, str, CurrencyType]: ...
    def __setstate__(self, state: tuple[str, int, int, str, CurrencyType]) -> None: ...
    def __eq__(self, other: Currency) -> bool: ...
    def __hash__(self) -> int: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @property
    def code(self) -> str:
        """
        Return the currency code.

        Returns
        -------
        str

        """
        ...
    @property
    def name(self) -> str:
        """
        Return the currency name.

        Returns
        -------
        str

        """
        ...
    @property
    def precision(self) -> int:
        """
        Return the currency decimal precision.

        Returns
        -------
        uint8

        """
        ...
    @property
    def iso4217(self) -> int:
        """
        Return the currency ISO 4217 code.

        Returns
        -------
        str

        """
        ...
    @property
    def currency_type(self) -> CurrencyType:
        """
        Return the currency type.

        Returns
        -------
        CurrencyType

        """
        ...
    @staticmethod
    def register(currency: Currency, overwrite: bool = False) -> None:
        """
        Register the given `currency`.

        Will override the internal currency map.

        Parameters
        ----------
        currency : Currency
            The currency to register
        overwrite : bool
            If the currency in the internal currency map should be overwritten.

        """
        ...
    @staticmethod
    def from_internal_map(code: str) -> Currency | None:
        """
        Return the currency with the given `code` from the built-in internal map (if found).

        Parameters
        ----------
        code : str
            The code of the currency.

        Returns
        -------
        Currency or ``None``

        """
        ...
    @staticmethod
    def from_str(code: str, strict: bool = False) -> Currency | None:
        """
        Parse a currency from the given string (if found).

        Parameters
        ----------
        code : str
            The code of the currency.
        strict : bool, default False
            If not `strict` mode then an unknown currency will very likely
            be a Cryptocurrency, so for robustness will then return a new
            `Currency` object using the given `code` with a default `precision` of 8.

        Returns
        -------
        Currency or ``None``

        """
        ...
    @staticmethod
    def is_fiat(code: str) -> bool:
        """
        Return whether a currency with the given code is ``FIAT``.

        Parameters
        ----------
        code : str
            The code of the currency.

        Returns
        -------
        bool
            True if ``FIAT``, else False.

        Raises
        ------
        ValueError
            If `code` is not a valid string.

        """
        ...
    @staticmethod
    def is_crypto(code: str) -> bool:
        """
        Return whether a currency with the given code is ``CRYPTO``.

        Parameters
        ----------
        code : str
            The code of the currency.

        Returns
        -------
        bool
            True if ``CRYPTO``, else False.

        Raises
        ------
        ValueError
            If `code` is not a valid string.

        """
        ...

class AccountBalance:
    """
    Represents an account balance denominated in a particular currency.

    Parameters
    ----------
    total : Money
        The total account balance.
    locked : Money
        The account balance locked (assigned to pending orders).
    free : Money
        The account balance free for trading.

    Raises
    ------
    ValueError
        If money currencies are not equal.
    ValueError
        If `total` - `locked` != `free`.
    """

    total: Money
    locked: Money
    free: Money
    currency: Currency

    def __init__(self, total: Money, locked: Money, free: Money) -> None: ...
    def __eq__(self, other: AccountBalance) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, object]) -> AccountBalance:
        """
        Return an account balance from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        AccountBalance

        """
        ...
    def copy(self) -> AccountBalance:
        """
        Return a copy of this account balance.

        Returns
        -------
        AccountBalance

        """
        ...
    def to_dict(self) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...

class MarginBalance:
    """
    Represents a margin balance optionally associated with a particular instrument.

    Parameters
    ----------
    initial : Money
        The initial (order) margin requirement for the instrument.
    maintenance : Money
        The maintenance (position) margin requirement for the instrument.
    instrument_id : InstrumentId, optional
        The instrument ID associated with the margin.

    Raises
    ------
    ValueError
        If `margin_init` currency does not equal `currency`.
    ValueError
        If `margin_maint` currency does not equal `currency`.
    ValueError
        If any margin is negative (< 0).
    """

    initial: Money
    maintenance: Money
    currency: Currency
    instrument_id: InstrumentId | None

    def __init__(
        self,
        initial: Money,
        maintenance: Money,
        instrument_id: InstrumentId | None = None,
    ) -> None: ...
    def __eq__(self, other: MarginBalance) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, object]) -> MarginBalance:
        """
        Return a margin balance from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        MarginAccountBalance

        """
        ...
    def copy(self) -> MarginBalance:
        """
        Return a copy of this margin balance.

        Returns
        -------
        MarginBalance

        """
        ...
    def to_dict(self) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
