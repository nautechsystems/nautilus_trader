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
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance
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
        bint calculate_account_state=False,
    ):
        Condition.not_none(event, "event")
        Condition.equal(event.account_type, AccountType.MARGIN, "event.account_type", "account_type")

        super().__init__(event, calculate_account_state)

        cdef dict margins_init = event.info.get("margins_init", {})
        cdef dict margins_maint = event.info.get("margins_maint", {})

        self.default_leverage = Decimal(1)
        self._leverages = {}                 # type: dict[InstrumentId, Decimal]
        self._margins_init = margins_init    # type: dict[InstrumentId, Money]
        self._margins_maint = margins_maint  # type: dict[InstrumentId, Money]

# -- QUERIES ---------------------------------------------------------------------------------------

    cpdef dict leverages(self):
        """
        Return the account leverages.

        Returns
        -------
        dict[InstrumentId, Decimal]

        """
        return self._leverages.copy()

    cpdef dict margins_init(self):
        """
        Return the initial (order) margins for the account.

        Returns
        -------
        dict[InstrumentId, Money]

        """
        return self._margins_init.copy()

    cpdef dict margins_maint(self):
        """
        Return the maintenance (position) margins for the account.

        Returns
        -------
        dict[InstrumentId, Money]

        """
        return self._margins_maint.copy()

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

        For multi-currency accounts, specify the currency for the query.

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

        return self._margins_init.get(instrument_id)

    cpdef Money margin_maint(self, InstrumentId instrument_id):
        """
        Return the current maintenance (position) margin.

        For multi-currency accounts, specify the currency for the query.

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

        return self._margins_maint.get(instrument_id)

# -- COMMANDS --------------------------------------------------------------------------------------

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
        Condition.not_negative(margin_init.as_decimal(), "margin_init")

        self._margins_init[instrument_id] = margin_init
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
        Condition.not_negative(margin_maint.as_decimal(), "margin_maint")

        self._margins_maint[instrument_id] = margin_maint
        self._recalculate_balance(margin_maint.currency)

    cpdef void clear_margin_init(self, InstrumentId instrument_id) except *:
        """
        Clear the initial (order) margins for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the initial margin to clear.

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef Money margin_init = self._margins_init.pop(instrument_id, None)
        if margin_init is not None:
            self._recalculate_balance(margin_init.currency)

    cpdef void clear_margin_maint(self, InstrumentId instrument_id) except *:
        """
        Clear the maintenance (position) margins for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the maintenance margin to clear.

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef Money margin_maint = self._margins_maint.pop(instrument_id, None)
        if margin_maint is not None:
            self._recalculate_balance(margin_maint.currency)

# -- CALCULATIONS ----------------------------------------------------------------------------------

    cdef void _recalculate_balance(self, Currency currency) except *:
        cdef AccountBalance current_balance = self._balances.get(currency)
        if current_balance is None:
            raise RuntimeError("cannot recalculate balance when no current balance")

        total_margin: Decimal = Decimal(0)

        cdef Money margin_init
        for margin_init in self._margins_init.values():
            if margin_init.currency != currency:
                continue
            total_margin += margin_init.as_decimal()

        cdef Money margin_maint
        for margin_maint in self._margins_maint.values():
            if margin_maint.currency != currency:
                continue
            total_margin += margin_maint.as_decimal()

        cdef AccountBalance new_balance = AccountBalance(
            currency,
            current_balance.total,
            Money(total_margin, currency),
            Money(current_balance.total.as_decimal() - total_margin, currency),
        )

        self._balances[currency] = new_balance

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
            If `liquidity_side` is NONE.

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

        leverage: Decimal = self._leverages.get(instrument.id)
        if leverage is None:
            leverage = self.default_leverage
            self._leverages[instrument.id] = leverage

        adjusted_notional: Decimal = notional / leverage

        margin: Decimal = adjusted_notional * instrument.margin_init
        margin += (adjusted_notional * instrument.taker_fee * 2)

        if instrument.is_inverse and not inverse_as_quote:
            return Money(margin, instrument.base_currency)
        else:
            return Money(margin, instrument.quote_currency)

    cpdef Money calculate_margin_maint(
        self,
        Instrument instrument,
        PositionSide side,
        Quantity quantity,
        avg_open_px: Decimal,
        bint inverse_as_quote=False,
    ):
        """
        Calculate the maintenance (position) margin from the given parameters.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        side : PositionSide
            The currency position side.
        quantity : Quantity
            The currency position quantity.
        avg_open_px : Decimal or Price
            The positions average open price.
        inverse_as_quote : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(quantity, "quantity")
        Condition.not_none(avg_open_px, "avg_open_px")

        notional: Decimal = instrument.notional_value(
            quantity=quantity,
            price=avg_open_px,
            inverse_as_quote=inverse_as_quote
        ).as_decimal()

        leverage: Decimal = self._leverages.get(instrument.id)
        if leverage is None:
            leverage = self.default_leverage
            self._leverages[instrument.id] = leverage

        adjusted_notional: Decimal = notional / leverage

        margin: Decimal = adjusted_notional * instrument.margin_maint
        margin += adjusted_notional * instrument.taker_fee

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
        Return the calculated PnL.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        position : Position, optional
            The position for the calculation.
        fill : OrderFilled
            The fill for the calculation.

        Returns
        -------
        list[Money] or ``None``

        """
        Condition.not_none(instrument, "instrument")
        Condition.not_none(fill, "fill")

        self.update_commissions(fill.commission)

        cdef dict pnls = {}  # type: dict[Currency, Money]

        cdef Money pnl
        if position and position.entry != fill.order_side:
            # Calculate and add PnL
            pnl = position.calculate_pnl(
                avg_px_open=position.avg_px_open,
                avg_px_close=fill.last_px,
                quantity=fill.last_qty,
            )
            pnls[pnl.currency] = pnl

        # Add commission PnL
        cdef Currency currency = fill.commission.currency
        pnl_existing = pnls.get(currency, Decimal(0))
        pnls[currency] = Money(pnl_existing - fill.commission, currency)

        return list(pnls.values())
