# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.functions cimport liquidity_side_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.position cimport Position


cdef class CashAccount(Account):
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
    ACCOUNT_TYPE = AccountType.CASH  # required for BettingAccount subclass

    def __init__(
        self,
        AccountState event,
        bint calculate_account_state = False,
    ):
        Condition.not_none(event, "event")
        Condition.equal(event.account_type, self.ACCOUNT_TYPE, "event.account_type", "account_type")

        super().__init__(event, calculate_account_state)

        self._balances_locked: dict[InstrumentId, Money] = {}

    @staticmethod
    cdef dict to_dict_c(CashAccount obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "CashAccount",
            "calculate_account_state":obj.calculate_account_state,
            "events": [AccountState.to_dict_c(event) for event in obj.events_c()]
        }

    @staticmethod
    def to_dict(CashAccount obj):
        return CashAccount.to_dict_c(obj)


    @staticmethod
    cdef CashAccount from_dict_c(dict values):
        Condition.not_none(values, "values")
        calculate_account_state = values["calculate_account_state"]
        events = values["events"]
        if len(events) == 0:
            return None
        init_event = events[0]
        other_events = events[1:]
        account = CashAccount(
            event=AccountState.from_dict_c(init_event),
            calculate_account_state=calculate_account_state
        )
        for event in other_events:
            account.apply(AccountState.from_dict_c(event))
        return account

    @staticmethod
    def from_dict(dict values):
        return CashAccount.from_dict_c(values)

    cpdef void update_balance_locked(self, InstrumentId instrument_id, Money locked):
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(locked, "locked")
        Condition.is_true(locked.raw_int_c() >= 0, "locked was negative")

        self._balances_locked[instrument_id] = locked
        self._recalculate_balance(locked.currency)

    cpdef void clear_balance_locked(self, InstrumentId instrument_id):
        """
        Clear the balance locked for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the locked balance to clear.

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef Money locked = self._balances_locked.pop(instrument_id, None)
        if locked is not None:
            self._recalculate_balance(locked.currency)

# -- CALCULATIONS ---------------------------------------------------------------------------------

    cpdef bint is_unleveraged(self, InstrumentId instrument_id):
        return True

    cdef void _recalculate_balance(self, Currency currency):
        cdef AccountBalance current_balance = self._balances.get(currency)
        if current_balance is None:
            # TODO: Temporary pending reimplementation of accounting
            print("Cannot recalculate balance when no current balance")
            return

        total_locked = Decimal(0)

        cdef Money locked
        for locked in self._balances_locked.values():
            if locked.currency != currency:
                continue
            total_locked += locked.as_decimal()

        cdef AccountBalance new_balance = AccountBalance(
            current_balance.total,
            Money(total_locked, currency),
            Money(current_balance.total.as_decimal() - total_locked, currency),
        )

        self._balances[currency] = new_balance

    cpdef Money calculate_commission(
        self,
        Instrument instrument,
        Quantity last_qty,
        Price last_px,
        LiquiditySide liquidity_side,
        bint use_quote_for_inverse=False,
    ):
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
        Condition.not_none(instrument, "instrument")
        Condition.not_none(last_qty, "last_qty")
        Condition.not_equal(liquidity_side, LiquiditySide.NO_LIQUIDITY_SIDE, "liquidity_side", "NO_LIQUIDITY_SIDE")

        notional = instrument.notional_value(
            quantity=last_qty,
            price=last_px,
            use_quote_for_inverse=use_quote_for_inverse,
        ).as_decimal()

        if liquidity_side == LiquiditySide.MAKER:
            commission = notional * instrument.maker_fee
        elif liquidity_side == LiquiditySide.TAKER:
            commission = notional * instrument.taker_fee
        else:
            raise ValueError(
                f"invalid LiquiditySide, was {liquidity_side_to_str(liquidity_side)}"
            )

        if instrument.is_inverse and not use_quote_for_inverse:
            return Money(commission, instrument.base_currency)
        else:
            return Money(commission, instrument.quote_currency)

    cpdef Money calculate_balance_locked(
        self,
        Instrument instrument,
        OrderSide side,
        Quantity quantity,
        Price price,
        bint use_quote_for_inverse=False,
    ):
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
        Condition.not_none(instrument, "instrument")
        Condition.not_none(quantity, "quantity")
        Condition.not_none(price, "price")

        cdef Currency quote_currency = instrument.quote_currency
        cdef Currency base_currency = instrument.get_base_currency() or instrument.quote_currency

        # Determine notional value
        if side == OrderSide.BUY:
            notional = instrument.notional_value(
                quantity=quantity,
                price=price,
                use_quote_for_inverse=use_quote_for_inverse,
            ).as_decimal()
        elif side == OrderSide.SELL:
            if base_currency is not None:
                notional = quantity.as_decimal()
            else:
                return None  # No balance to lock
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

        # Add expected commission
        locked = notional
        locked += notional * instrument.taker_fee * Decimal(2)

        # Handle inverse
        if instrument.is_inverse and not use_quote_for_inverse:
            return Money(locked, base_currency)

        if side == OrderSide.BUY:
            return Money(locked, quote_currency)
        elif side == OrderSide.SELL:
            return Money(locked, base_currency)
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    cpdef list calculate_pnls(
        self,
        Instrument instrument,
        OrderFilled fill,
        Position position: Position | None = None,
    ):
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
        Condition.not_none(instrument, "instrument")
        Condition.not_none(fill, "fill")

        cdef dict pnls = {}  # type: dict[Currency, Money]

        cdef Currency quote_currency = instrument.quote_currency
        cdef Currency base_currency = instrument.get_base_currency()

        fill_px = fill.last_px.as_decimal()
        fill_qty = fill.last_qty.as_decimal()
        last_qty = fill_qty

        if position is not None and position.quantity._mem.raw != 0 and position.entry != fill.order_side:
            # Only book open quantity towards realized PnL
            fill_qty = min(fill_qty, position.quantity.as_decimal())

        # Below we are using the original `last_qty` to adjust the base currency,
        # this is to avoid a desync in account balance vs filled quantities later.
        if fill.order_side == OrderSide.BUY:
            if base_currency and not self.base_currency:
                pnls[base_currency] = Money(last_qty, base_currency)
            pnls[quote_currency] = Money(-(fill_px * fill_qty), quote_currency)
        elif fill.order_side == OrderSide.SELL:
            if base_currency and not self.base_currency:
                pnls[base_currency] = Money(-last_qty, base_currency)
            pnls[quote_currency] = Money(fill_px * fill_qty, quote_currency)
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"invalid `OrderSide`, was {fill.order_side}")  # pragma: no cover (design-time error)

        return list(pnls.values())

    cpdef Money balance_impact(
        self,
        Instrument instrument,
        Quantity quantity,
        Price price,
        OrderSide order_side,
    ):
        cdef Money notional = instrument.notional_value(quantity, price)
        if order_side == OrderSide.BUY:
            return Money.from_raw_c(-notional._mem.raw, notional.currency)
        elif order_side == OrderSide.SELL:
            return Money.from_raw_c(notional._mem.raw, notional.currency)
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(f"invalid `OrderSide`, was {order_side}")  # pragma: no cover (design-time error)
