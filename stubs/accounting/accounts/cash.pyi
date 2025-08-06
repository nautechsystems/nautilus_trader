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
    """
    Provides a cash account.

    Parameters
    ----------
    event : AccountState
        The initial account state event.
    calculate_account_state : bool, optional
        If the account state should be calculated from order fills.

    Raises
    ------
    ValueError
        If `event.account_type` is not equal to ``CASH``.
    """

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
    def update_balance_locked(self, instrument_id: InstrumentId, locked: Money) -> None:
        """
        Update the balance locked for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the update.
        locked : Money
            The locked balance for the instrument.

        Raises
        ------
        ValueError
            If `margin_init` is negative (< 0).

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        ...
    def clear_balance_locked(self, instrument_id: InstrumentId) -> None:
        """
        Clear the balance locked for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the locked balance to clear.

        """
        ...
    def is_unleveraged(self, instrument_id: InstrumentId) -> bool: ...
    def calculate_commission(
        self,
        instrument: Instrument,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: bool = False,
    ) -> Money:
        """
        Calculate the commission generated from a transaction with the given
        parameters.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        last_qty : Quantity
            The transaction quantity.
        last_px : Price
            The transaction price.
        liquidity_side : LiquiditySide {``MAKER``, ``TAKER``}
            The liquidity side for the transaction.
        use_quote_for_inverse : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        Raises
        ------
        ValueError
            If `liquidity_side` is ``NO_LIQUIDITY_SIDE``.

        """
        ...
    def calculate_balance_locked(
        self,
        instrument: Instrument,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: bool = False,
    ) -> Money:
        """
        Calculate the locked balance.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        side : OrderSide {``BUY``, ``SELL``}
            The order side.
        quantity : Quantity
            The order quantity.
        price : Price
            The order price.
        use_quote_for_inverse : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        """
        ...
    def calculate_pnls(
        self,
        instrument: Instrument,
        fill: OrderFilled,
        position: Position | None = None,
    ) -> list[Money]:
        """
        Return the calculated PnL.

        The calculation does not include any commissions.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        fill : OrderFilled
            The fill for the calculation.
        position : Position, optional
            The position for the calculation (can be None).

        Returns
        -------
        list[Money]

        """
        ...
    def balance_impact(
        self,
        instrument: Instrument,
        quantity: Quantity,
        price: Price,
        order_side: OrderSide,
    ) -> Money: ...

