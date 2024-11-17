from typing import Dict, List, Optional

from nautilus_trader.core.message import Event
from nautilus_trader.core.model import AccountType
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.objects import AccountBalance, Currency, MarginBalance

class AccountState(Event):
    account_id: AccountId
    account_type: AccountType
    base_currency: Optional[Currency]
    balances: List[AccountBalance]
    margins: List[MarginBalance]
    is_reported: bool
    info: Dict[str, object]

    def __init__(
        self,
        account_id: AccountId,
        account_type: AccountType,
        base_currency: Optional[Currency],
        reported: bool,
        balances: List[AccountBalance],
        margins: List[MarginBalance],
        info: Dict[str, object],
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
    def from_dict(values: Dict[str, object]) -> AccountState: ...
    @staticmethod
    def to_dict(obj: AccountState) -> Dict[str, object]: ...
