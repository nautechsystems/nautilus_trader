from typing import ClassVar, Literal

from nautilus_trader.model.enums import AccountType, LiquiditySide
from nautilus_trader.model.enums import OrderSide
from stubs.accounting.accounts.base import Account
from stubs.model.events.account import AccountState
from stubs.model.events.order import OrderFilled
from stubs.model.identifiers import InstrumentId
from stubs.model.instruments.base import Instrument
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.position import Position

class CashAccount(Account):

    ACCOUNT_TYPE: ClassVar[AccountType] = ...

    _balances_locked: dict[InstrumentId, Money]

    def __init__(
        self,
        event: AccountState,
        calculate_account_state: bool = False,
    ) -> None: ...
    @staticmethod
    def to_dict(obj: CashAccount) -> dict: ...
    @staticmethod
    def from_dict(values: dict) -> CashAccount: ...
    def update_balance_locked(self, instrument_id: InstrumentId, locked: Money) -> None: ...
    def clear_balance_locked(self, instrument_id: InstrumentId) -> None: ...
    def is_unleveraged(self, instrument_id: InstrumentId) -> bool: ...
    def calculate_commission(
        self,
        instrument: Instrument,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: bool = False,
    ) -> Money: ...
    def calculate_balance_locked(
        self,
        instrument: Instrument,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: bool = False,
    ) -> Money: ...
    def calculate_pnls(
        self,
        instrument: Instrument,
        fill: OrderFilled,
        position: Position | None = None,
    ) -> list[Money]: ...
    def balance_impact(
        self,
        instrument: Instrument,
        quantity: Quantity,
        price: Price,
        order_side: OrderSide,
    ) -> Money: ...

