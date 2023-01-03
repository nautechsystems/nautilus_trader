# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Optional

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.enums_c cimport AccountType
from nautilus_trader.model.enums_c cimport LiquiditySide
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport liquidity_side_to_str
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance
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

    cpdef void update_balance_locked(self, InstrumentId instrument_id, Money locked) except *:
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
        Condition.true(locked.raw_int64_c() >= 0, "locked was negative")

        self._balances_locked[instrument_id] = locked
        self._recalculate_balance(locked.currency)

    cpdef void clear_balance_locked(self, InstrumentId instrument_id) except *:
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

    cpdef bint is_unleveraged(self, InstrumentId instrument_id) except *:
        return True

    cdef void _recalculate_balance(self, Currency currency) except *:
        cdef AccountBalance current_balance = self._balances.get(currency)
        if current_balance is None:
            # TODO(cs): Temporary pending reimplementation of accounting
            print("Cannot recalculate balance when no current balance")
            return

        cdef double total_locked = 0.0

        cdef Money locked
        for locked in self._balances_locked.values():
            if locked.currency != currency:
                continue
            total_locked += locked.as_f64_c()

        cdef AccountBalance new_balance = AccountBalance(
            current_balance.total,
            Money(total_locked, currency),
            Money(current_balance.total.as_f64_c() - total_locked, currency),
        )

        self._balances[currency] = new_balance

    cpdef Money calculate_commission(
        self,
        Instrument instrument,
        Quantity last_qty,
        Price last_px,
        LiquiditySide liquidity_side,
        bint inverse_as_quote=False,
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
        inverse_as_quote : bool
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

        cdef double notional = instrument.notional_value(
            quantity=last_qty,
            price=last_px,
            inverse_as_quote=inverse_as_quote,
        ).as_f64_c()

        cdef double commission
        if liquidity_side == LiquiditySide.MAKER:
            commission = notional * float(instrument.maker_fee)
        elif liquidity_side == LiquiditySide.TAKER:
            commission = notional * float(instrument.taker_fee)
        else:
            raise ValueError(
                f"invalid LiquiditySide, was {liquidity_side_to_str(liquidity_side)}"
            )

        if instrument.is_inverse and not inverse_as_quote:
            return Money(commission, instrument.base_currency)
        else:
            return Money(commission, instrument.quote_currency)

    cpdef Money calculate_balance_locked(
        self,
        Instrument instrument,
        OrderSide side,
        Quantity quantity,
        Price price,
        bint inverse_as_quote=False,
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
        inverse_as_quote : bool
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

        cdef double notional
        # Determine notional value
        if side == OrderSide.BUY:
            notional = instrument.notional_value(
                quantity=quantity,
                price=price,
                inverse_as_quote=inverse_as_quote,
            ).as_f64_c()
        elif side == OrderSide.SELL:
            if base_currency is not None:
                notional = quantity.as_f64_c()
            else:
                return None  # No balance to lock
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

        # Add expected commission
        cdef double locked = notional
        locked += (notional * float(instrument.taker_fee) * 2.0)

        # Handle inverse
        if instrument.is_inverse and not inverse_as_quote:
            return Money(locked, base_currency)

        if side == OrderSide.BUY:
            return Money(locked, quote_currency)
        elif side == OrderSide.SELL:
            return Money(locked, base_currency)
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {side}")  # pragma: no cover (design-time error)

    cpdef list calculate_pnls(
        self,
        Instrument instrument,
        OrderFilled fill,
        Position position: Optional[Position] = None,
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
        list[Money] or ``None``

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(fill, "fill")

        cdef dict pnls = {}  # type: dict[Currency, Money]

        cdef Currency quote_currency = instrument.quote_currency
        cdef Currency base_currency = instrument.get_base_currency()

        cdef double fill_qty = fill.last_qty.as_f64_c()
        cdef double fill_px = fill.last_px.as_f64_c()

        if fill.order_side == OrderSide.BUY:
            if base_currency and not self.base_currency:
                pnls[base_currency] = Money(fill_qty, base_currency)
            pnls[quote_currency] = Money(-(fill_px * fill_qty), quote_currency)
        elif fill.order_side == OrderSide.SELL:
            if base_currency and not self.base_currency:
                pnls[base_currency] = Money(-fill_qty, base_currency)
            pnls[quote_currency] = Money(fill_px * fill_qty, quote_currency)
        else:
            raise RuntimeError(f"invalid `OrderSide`, was {fill.order_side}")  # pragma: no cover (design-time error)

        return list(pnls.values())
