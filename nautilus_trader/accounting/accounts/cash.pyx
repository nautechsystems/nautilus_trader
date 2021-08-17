# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.currency cimport Currency
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
    """

    def __init__(
        self,
        AccountState event,
        bint calculate_account_state=False,
    ):
        """
        Initialize a new instance of the ``CashAccount`` class.

        Parameters
        ----------
        event : AccountState
            The initial account state event.
        calculate_account_state : bool, optional
            If the account state should be calculated from order fills.

        Raises
        ------
        ValueError
            If event.account_type is not equal to AccountType.CASH.

        """
        Condition.not_none(event, "event")
        Condition.equal(event.account_type, AccountType.CASH, "event.account_type", "account_type")

        super().__init__(event, calculate_account_state)

        self._balances_locked = {}  # type: dict[InstrumentId, Money]

    cpdef void update_margin_init(self, InstrumentId instrument_id, Money margin_init) except *:
        """
        Update the initial (order) margin.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the margin.
        margin_init : Money
            The current initial (order) margin for the currency.

        Raises
        ------
        ValueError
            If margin initial is negative (< 0).

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(margin_init, "margin_init")
        Condition.not_negative(margin_init.as_decimal(), "margin_init")

        self._balances_locked[instrument_id] = margin_init
        self._recalculate_balance(margin_init.currency)

    cpdef void clear_margin_init(self, InstrumentId instrument_id) except *:
        """
        Clear the locked balances for the given instrument ID.

        There may be more than one currency if the instrument is being traded
        in multiple directions.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the locked balance to clear.

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef Money locked = self._balances_locked.pop(instrument_id, None)
        if locked is not None:
            self._recalculate_balance(locked.currency)

    cdef void _recalculate_balance(self, Currency currency) except *:
        cdef AccountBalance current_balance = self._balances.get(currency)
        if current_balance is None:
            raise RuntimeError("Cannot recalculate balance when no current balance")

        total_locked: Decimal = Decimal(0)

        cdef Money locked
        for locked in self._balances_locked.values():
            if locked.currency != currency:
                continue
            total_locked += locked.as_decimal()

        cdef AccountBalance new_balance = AccountBalance(
            currency,
            current_balance.total,
            Money(total_locked, currency),
            Money(current_balance.total.as_decimal() - total_locked, currency),
        )

        self._balances[currency] = new_balance

# -- CALCULATIONS ----------------------------------------------------------------------------------

    cpdef Money calculate_commission(
        self,
        Instrument instrument,
        Quantity last_qty,
        last_px: Decimal,
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
        last_px : Decimal or Price
            The transaction price.
        liquidity_side : LiquiditySide
            The liquidity side for the transaction.
        inverse_as_quote : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        Raises
        ------
        ValueError
            If liquidity_side is NONE.

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(last_qty, "last_qty")
        Condition.type(last_px, (Decimal, Price), "last_px")
        Condition.not_equal(liquidity_side, LiquiditySide.NONE, "liquidity_side", "NONE")

        notional: Decimal = instrument.notional_value(
            quantity=last_qty,
            price=last_px,
            inverse_as_quote=inverse_as_quote,
        ).as_decimal()

        if liquidity_side == LiquiditySide.MAKER:
            commission: Decimal = notional * instrument.maker_fee
        elif liquidity_side == LiquiditySide.TAKER:
            commission: Decimal = notional * instrument.taker_fee
        else:
            raise ValueError(
                f"invalid LiquiditySide, was {LiquiditySideParser.to_str(liquidity_side)}"
            )

        if instrument.is_inverse and not inverse_as_quote:
            return Money(commission, instrument.base_currency)
        else:
            return Money(commission, instrument.quote_currency)

    cpdef Money calculate_margin_init(
        self,
        Instrument instrument,
        Quantity quantity,
        Price price,
        bint inverse_as_quote=False,
    ):
        """
        Calculate the initial (order) margin from the given parameters.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
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

        notional: Decimal = instrument.notional_value(
            quantity=quantity,
            price=price.as_decimal(),
            inverse_as_quote=inverse_as_quote,
        ).as_decimal()

        margin: Decimal = notional
        margin += (notional * instrument.taker_fee * 2)

        if instrument.is_inverse and not inverse_as_quote:
            return Money(margin, instrument.base_currency)
        else:
            return Money(margin, instrument.quote_currency)

    cpdef list calculate_pnls(
        self,
        Instrument instrument,
        Position position,  # Can be None
        OrderFilled fill,
    ):
        """
        Return the calculated immediate PnL.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        position : Position, optional
            The position for the calculation (can be None).
        fill : OrderFilled
            The fill for the calculation.

        Returns
        -------
        list[Money] or None

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(fill, "fill")

        cdef list pnls = []

        cdef Currency quote_currency = instrument.quote_currency
        cdef Currency base_currency = instrument.get_base_currency()

        fill_qty: Decimal = fill.last_qty.as_decimal()
        fill_px: Decimal = fill.last_px.as_decimal()

        if fill.side == OrderSide.BUY:
            if base_currency:
                pnls.append(Money(fill_qty, base_currency))
            pnls.append(Money(-(fill_px * fill_qty), quote_currency))
        else:  # OrderSide.SELL
            if base_currency:
                pnls.append(Money(-fill_qty, base_currency))
            pnls.append(Money(fill_px * fill_qty, quote_currency))

        return pnls
