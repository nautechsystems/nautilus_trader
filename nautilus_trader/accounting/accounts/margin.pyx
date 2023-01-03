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

from decimal import Decimal
from typing import Optional

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.enums_c cimport AccountType
from nautilus_trader.model.enums_c cimport LiquiditySide
from nautilus_trader.model.enums_c cimport liquidity_side_to_str
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport MarginBalance
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.position cimport Position


cdef class MarginAccount(Account):
    """
    Provides a margin account.

    Parameters
    ----------
    event : AccountState
        The initial account state event.
    calculate_account_state : bool, optional
        If the account state should be calculated from order fills.

    Raises
    ------
    ValueError
        If `event.account_type` is not equal to ``MARGIN``.
    """

    def __init__(
        self,
        AccountState event,
        bint calculate_account_state = False,
    ):
        Condition.not_none(event, "event")
        Condition.equal(event.account_type, AccountType.MARGIN, "event.account_type", "account_type")

        super().__init__(event, calculate_account_state)

        self.default_leverage = Decimal(1)
        self._leverages: dict[InstrumentId, Decimal] = {}
        self._margins: dict[InstrumentId, MarginBalance] = {m.instrument_id: m for m in event.margins}

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef dict margins(self):
        """
        Return the initial (order) margins for the account.

        Returns
        -------
        dict[InstrumentId, Money]

        """
        return self._margins.copy()

    cpdef dict margins_init(self):
        """
        Return the initial (order) margins for the account.

        Returns
        -------
        dict[InstrumentId, Money]

        """
        return {k: v.initial for k, v in self._margins.items()}

    cpdef dict margins_maint(self):
        """
        Return the maintenance (position) margins for the account.

        Returns
        -------
        dict[InstrumentId, Money]

        """
        return {k: v.maintenance for k, v in self._margins.items()}

    cpdef dict leverages(self):
        """
        Return the account leverages.

        Returns
        -------
        dict[InstrumentId, Decimal]

        """
        return self._leverages.copy()

    cpdef object leverage(self, InstrumentId instrument_id):
        """
        Return the leverage for the given instrument (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the leverage.

        Returns
        -------
        Decimal or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._leverages.get(instrument_id)

    cpdef Money margin_init(self, InstrumentId instrument_id):
        """
        Return the current initial (order) margin.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the query.

        Returns
        -------
        Money or ``None``

        Warnings
        --------
        Returns ``None`` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef MarginBalance margin = self._margins.get(instrument_id)
        return None if margin is None else margin.initial

    cpdef Money margin_maint(self, InstrumentId instrument_id):
        """
        Return the current maintenance (position) margin.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the query.

        Returns
        -------
        Money or ``None``

        Warnings
        --------
        Returns ``None`` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef MarginBalance margin = self._margins.get(instrument_id)
        return None if margin is None else margin.maintenance

    cpdef MarginBalance margin(self, InstrumentId instrument_id):
        """
        Return the current margin balance.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the query.

        Returns
        -------
        MarginBalance or ``None``

        Warnings
        --------
        Returns ``None`` if there is no applicable information for the query,
        rather than `MarginBalance` with zero amounts.

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._margins.get(instrument_id)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void set_default_leverage(self, leverage: Decimal) except *:
        """
        Set the default leverage for the account (if not specified by instrument).

        Parameters
        ----------
        leverage : Decimal
            The default leverage value

        Returns
        -------
        TypeError
            If leverage is not of type `Decimal`.
        ValueError
            If leverage is not >= 1.

        """
        Condition.type(leverage, Decimal, "leverage")
        Condition.true(leverage >= 1, "leverage was not >= 1")

        self.default_leverage = leverage

    cpdef void set_leverage(self, InstrumentId instrument_id, leverage: Decimal) except *:
        """
        Set the leverage for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the leverage.
        leverage : Decimal
            The leverage value

        Returns
        -------
        TypeError
            If leverage is not of type `Decimal`.
        ValueError
            If leverage is not >= 1.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.type(leverage, Decimal, "leverage")
        Condition.true(leverage >= 1, "leverage was not >= 1")

        self._leverages[instrument_id] = leverage

    cpdef void update_margin_init(self, InstrumentId instrument_id, Money margin_init) except *:
        """
        Update the initial (order) margin.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the update.
        margin_init : Money
            The current initial (order) margin for the instrument.

        Raises
        ------
        ValueError
            If `margin_init` is negative (< 0).

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(margin_init, "margin_init")

        cdef MarginBalance margin = self._margins.get(instrument_id)
        if margin is None:
            self._margins[instrument_id] = MarginBalance(
                initial=margin_init,
                maintenance=Money(0, margin_init.currency),
                instrument_id=instrument_id,
            )
        else:
            margin.initial = margin_init

        self._recalculate_balance(margin_init.currency)

    cpdef void update_margin_maint(self, InstrumentId instrument_id, Money margin_maint) except *:
        """
        Update the maintenance (position) margin.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the update.
        margin_maint : Money
            The current maintenance (position) margin for the instrument.

        Raises
        ------
        ValueError
            If `margin_maint` is negative (< 0).

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(margin_maint, "margin_maint")

        cdef MarginBalance margin = self._margins.get(instrument_id)
        if margin is None:
            self._margins[instrument_id] = MarginBalance(
                initial=Money(0, margin_maint.currency),
                maintenance=margin_maint,
                instrument_id=instrument_id,
            )
        else:
            margin.maintenance = margin_maint

        self._recalculate_balance(margin_maint.currency)

    cpdef void update_margin(self, MarginBalance margin) except *:
        """
        Update the margin balance.

        Parameters
        ----------
        margin : MarginBalance

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(margin, "margin")

        self._margins[margin.instrument_id] = margin
        self._recalculate_balance(margin.currency)

    cpdef void clear_margin_init(self, InstrumentId instrument_id) except *:
        """
        Clear the initial (order) margins for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the initial margin to clear.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef MarginBalance margin = self._margins.get(instrument_id)
        if margin is not None:
            if margin.maintenance._mem.raw == 0:
                self._margins.pop(instrument_id)
            else:
                margin.initial = Money(0, margin.currency)

            self._recalculate_balance(margin.currency)

    cpdef void clear_margin_maint(self, InstrumentId instrument_id) except *:
        """
        Clear the maintenance (position) margins for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the maintenance margin to clear.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef MarginBalance margin = self._margins.get(instrument_id)
        if margin is not None:
            if margin.initial._mem.raw == 0:
                self._margins.pop(instrument_id)
            else:
                margin.maintenance = Money(0, margin.currency)

            self._recalculate_balance(margin.currency)

    cpdef void clear_margin(self, InstrumentId instrument_id) except *:
        """
        Clear the maintenance (position) margins for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the maintenance margin to clear.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef MarginBalance margin = self._margins.pop(instrument_id, None)
        if margin is not None:
            self._recalculate_balance(margin.currency)

# -- CALCULATIONS ---------------------------------------------------------------------------------

    cpdef bint is_unleveraged(self, InstrumentId instrument_id) except *:
        Condition.not_none(instrument_id, "instrument_id")
        return self._leverages.get(instrument_id, self.default_leverage) == 1

    cdef void _recalculate_balance(self, Currency currency) except *:
        cdef AccountBalance current_balance = self._balances.get(currency)
        if current_balance is None:
            # TODO(cs): Temporary pending reimplementation of accounting
            print("Cannot recalculate balance when no current balance")
            return

        cdef double total_margin = 0.0

        cdef MarginBalance margin
        for margin in self._margins.values():
            if margin.currency != currency:
                continue
            total_margin += margin.initial.as_f64_c()
            total_margin += margin.maintenance.as_f64_c()

        cdef AccountBalance new_balance = AccountBalance(
            current_balance.total,
            Money(total_margin, currency),
            Money(current_balance.total.as_f64_c() - total_margin, currency),
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
        Condition.type(last_px, (Decimal, Price), "last_px")
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
                f"invalid `LiquiditySide`, was {liquidity_side_to_str(liquidity_side)}"
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
        Calculate the initial (order) margin.

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

        cdef double notional = instrument.notional_value(
            quantity=quantity,
            price=price,
            inverse_as_quote=inverse_as_quote,
        ).as_f64_c()

        cdef double leverage = self._leverages.get(instrument.id, 0.0)
        if leverage == 0.0:
            leverage = self.default_leverage
            self._leverages[instrument.id] = leverage

        cdef double adjusted_notional = notional / leverage
        cdef double margin = adjusted_notional * float(instrument.margin_init)
        margin += (adjusted_notional * float(instrument.taker_fee) * 2.0)

        if instrument.is_inverse and not inverse_as_quote:
            return Money(margin, instrument.base_currency)
        else:
            return Money(margin, instrument.quote_currency)

    cpdef Money calculate_margin_maint(
        self,
        Instrument instrument,
        PositionSide side,
        Quantity quantity,
        Price price,
        bint inverse_as_quote=False,
    ):
        """
        Calculate the maintenance (position) margin.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        side : PositionSide {``LONG``, ``SHORT``}
            The currency position side.
        quantity : Quantity
            The currency position quantity.
        price : Price
            The positions current price.
        inverse_as_quote : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(quantity, "quantity")

        cdef double notional = instrument.notional_value(
            quantity=quantity,
            price=price,
            inverse_as_quote=inverse_as_quote,
        ).as_f64_c()

        cdef double leverage = float(self._leverages.get(instrument.id, 0.0))
        if leverage == 0.0:
            leverage = self.default_leverage
            self._leverages[instrument.id] = leverage

        cdef double adjusted_notional = notional / leverage
        cdef double margin = adjusted_notional * float(instrument.margin_maint)
        margin += adjusted_notional * float(instrument.taker_fee)

        if instrument.is_inverse and not inverse_as_quote:
            return Money(margin, instrument.base_currency)
        else:
            return Money(margin, instrument.quote_currency)

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
            The position for the calculation.

        Returns
        -------
        list[Money] or ``None``

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(fill, "fill")

        cdef dict pnls = {}  # type: dict[Currency, Money]

        cdef Money pnl
        if position is not None and position.entry != fill.order_side:
            # Calculate and add PnL
            pnl = position.calculate_pnl(
                avg_px_open=position.avg_px_open,
                avg_px_close=fill.last_px.as_f64_c(),
                quantity=fill.last_qty,
            )
            pnls[pnl.currency] = pnl

        return list(pnls.values())
