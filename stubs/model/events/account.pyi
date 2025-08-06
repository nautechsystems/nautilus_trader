from typing import Any

from nautilus_trader.model.enums import AccountType
from stubs.core.message import Event
from stubs.core.uuid import UUID4
from stubs.model.identifiers import AccountId
from stubs.model.objects import AccountBalance
from stubs.model.objects import Currency
from stubs.model.objects import MarginBalance

class AccountState(Event):

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
    def id(self) -> UUID4: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> AccountState: ...
    @staticmethod
    def to_dict(obj: AccountState) -> dict[str, Any]: ...