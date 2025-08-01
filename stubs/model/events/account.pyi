from typing import Any

from stubs.core.message import Event

class AccountState(Event):
    """
    Represents an event which includes information on the state of the account.

    Parameters
    ----------
    account_id : AccountId
        The account ID (with the venue).
    account_type : AccountType
        The account type for the event.
    base_currency : Currency, optional
        The account base currency. Use None for multi-currency accounts.
    reported : bool
        If the state is reported from the exchange (otherwise system calculated).
    balances : list[AccountBalance]
        The account balances.
    margins : list[MarginBalance]
        The margin balances (can be empty).
    info : dict [str, object]
        The additional implementation specific account information.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the account state event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    Raises
    ------
    ValueError
        If `balances` is empty.
    """

    account_id: AccountId
    account_type: AccountType
    base_currency: Currency | None
    balances: list[AccountBalance]
    margins: list[MarginBalance]
    is_reported: bool
    info: dict[str, Any]

    _event_id: UUID4
    _ts_event: int
    _ts_init: int

    def __init__(
        self,
        account_id: AccountId,
        account_type: AccountType,
        base_currency: Currency | None,
        reported: bool,
        balances: list[AccountBalance],
        margins: list[MarginBalance],
        info: dict[str, Any],
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    def __eq__(self, other: Event) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @property
    def id(self) -> UUID4:
        """
        The event message identifier.

        Returns
        -------
        UUID4

        """
        ...
    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the event occurred.

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
    @staticmethod
    def from_dict(values: dict[str, Any]) -> AccountState:
        """
        Return an account state event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        AccountState

        """
        ...
    @staticmethod
    def to_dict(obj: AccountState) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...