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
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


# TODO(cs): Add C @staticmethod(s)

cdef class Account:
    """
    The base class for all trading accounts.

    Represents Cash account type.
    """

    def __init__(self, AccountState event):
        """
        Initialize a new instance of the `Account` class.

        Parameters
        ----------
        event : AccountState
            The initial account state event.

        """
        Condition.not_none(event, "event")

        self.id = event.account_id

        default_currency_str = event.info.get("default_currency")
        if default_currency_str:
            self.default_currency = Currency.from_str_c(default_currency_str)
        else:
            self.default_currency = None

        initial_margins = event.info.get("initial_margins", {})
        maint_margins = event.info.get("maint_margins", {})

        self._events = [event]
        self._starting_balances = {b.currency: b.total for b in event.balances}
        self._balances = {}                      # type: dict[Currency, AccountBalance]
        self._initial_margins = initial_margins  # type: dict[Currency, Money]
        self._maint_margins = maint_margins      # type: dict[Currency, Money]
        self._portfolio = None  # Initialized when registered with portfolio

        self._update_balances(event.balances)

    def __eq__(self, Account other) -> bool:
        return self.id.value == other.id.value

    def __ne__(self, Account other) -> bool:
        return self.id.value != other.id.value

    def __hash__(self) -> int:
        return hash(self.id.value)

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id.value})"

    cdef AccountState last_event_c(self):
        return self._events[-1]  # Always at least one event

    cdef list events_c(self):
        return self._events.copy()

    cdef int event_count_c(self):
        return len(self._events)

    @property
    def last_event(self):
        """
        The accounts last state event.

        Returns
        -------
        AccountState

        """
        return self.last_event_c()

    @property
    def events(self):
        """
        All events received by the account.

        Returns
        -------
        list[AccountState]

        """
        return self.events_c()

    @property
    def event_count(self):
        """
        The count of events.

        Returns
        -------
        int

        """
        return self.event_count_c()

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void register_portfolio(self, PortfolioFacade portfolio):
        """
        Register the given portfolio with the account.

        Parameters
        ----------
        portfolio : PortfolioFacade
            The portfolio to register.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(portfolio, "portfolio")

        self._portfolio = portfolio

    cpdef void apply(self, AccountState event) except *:
        """
        Applies the given account event to the account.

        Parameters
        ----------
        event : AccountState
            The account event to apply.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(event, "event")
        Condition.equal(self.id, event.account_id, "id", "event.account_id")

        self._events.append(event)
        self._update_balances(event.balances)

    cpdef void update_initial_margin(self, Money margin) except *:
        """
        Update the initial margin.

        Parameters
        ----------
        margin : Money
            The current initial margin for the currency.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(margin, "money")

        self._initial_margins[margin.currency] = margin

    cpdef void update_maint_margin(self, Money margin) except *:
        """
        Update the maintenance margin.

        Parameters
        ----------
        margin : Decimal
            The current maintenance margin for the currency.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(margin, "money")

        self._maint_margins[margin.currency] = margin

# -- QUERIES-CASH ----------------------------------------------------------------------------------

    cpdef list currencies(self):
        """
        Return the account currencies.

        Returns
        -------
        list[Currency]

        """
        return list(self._balances.keys())

    cpdef dict starting_balances(self):
        """
        Return the account starting balances.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._starting_balances.copy()

    cpdef dict balances(self):
        """
        Return the account balances.

        Returns
        -------
        dict[Currency, Money]

        """
        return {c: b.total for c, b in self._balances.items()}

    cpdef dict balances_free(self):
        """
        Return the account balances free.

        Returns
        -------
        dict[Currency, Money]

        """
        return {c: b.free for c, b in self._balances.items()}

    cpdef dict balances_locked(self):
        """
        Return the account balances locked.

        Returns
        -------
        dict[Currency, Money]

        """
        return {c: b.locked for c, b in self._balances.items()}

    cpdef Money balance(self, Currency currency=None):
        """
        Return the current account balance total.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If None then will use the default
            currency (if set).

        Returns
        -------
        Money or None

        Raises
        ------
        ValueError
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        cdef AccountBalance balance = self._balances.get(currency)
        if balance is None:
            return None
        return balance.total

    cpdef Money balance_free(self, Currency currency=None):
        """
        Return the account balance free.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If None then will use the default
            currency (if set).

        Returns
        -------
        Money or None

        Raises
        ------
        ValueError
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        cdef AccountBalance balance = self._balances.get(currency)
        if balance is None:
            return None
        return balance.free

    cpdef Money balance_locked(self, Currency currency=None):
        """
        Return the account balance locked.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If None then will use the default
            currency (if set).

        Returns
        -------
        Money or None

        Raises
        ------
        ValueError
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        cdef AccountBalance balance = self._balances.get(currency)
        if balance is None:
            return None
        return balance.locked

    cpdef Money unrealized_pnl(self, Currency currency=None):
        """
        Return the current account unrealized PnL.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If None then will use the default
            currency (if set).

        Returns
        -------
        Money or None

        Raises
        ------
        ValueError
            If currency is None and default_currency is None.
        ValueError
            If portfolio is not registered.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")
        Condition.not_none(self._portfolio, "self._portfolio")

        # TODO: Assumption that issuer == venue
        cdef dict unrealized_pnls = self._portfolio.unrealized_pnls(Venue(self.id.issuer))
        if unrealized_pnls is None:
            return None

        return unrealized_pnls.get(currency, Money(0, currency))

    cpdef Money equity(self, Currency currency=None):
        """
        Return the account equity (`balance + unrealized_pnl`).

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If None then will use the default
            currency (if set).

        Returns
        -------
        Money or None

        Raises
        ------
        ValueError
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        balance: Decimal = self._balances.get(currency)
        if balance is None:
            return None

        cdef Money unrealized_pnl = self.unrealized_pnl(currency)
        if unrealized_pnl is None:
            return None

        return Money(balance.free + unrealized_pnl, currency)

    @staticmethod
    def market_value(
        Instrument instrument,
        Quantity quantity,
        close_price: Decimal,
    ):
        """
        Calculate the market value from the given parameters.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        quantity : Quantity
            The total quantity.
        close_price : Decimal or Price
            The closing price.

        Returns
        -------
        Money
            In the quote currency.

        """
        Condition.not_none(quantity, "quantity")
        Condition.type(close_price, (Decimal, Price), "close_price")
        Condition.not_none(close_price, "close_price")

        if instrument.is_inverse:
            close_price = 1 / close_price

        market_value: Decimal = (quantity * instrument.multiplier * close_price)
        return Money(market_value, instrument.cost_currency)

    @staticmethod
    def notional_value(Instrument instrument, Quantity quantity, close_price: Decimal):
        """
        Calculate the notional value from the given parameters.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        quantity : Quantity
            The total quantity.
        close_price : Decimal or Price
            The closing price.

        Returns
        -------
        Money
            In the settlement currency.

        """
        Condition.not_none(quantity, "quantity")
        Condition.type(close_price, (Decimal, Price), "close_price")
        Condition.not_none(close_price, "close_price")

        if instrument.is_inverse:
            return Money(quantity * instrument.multiplier, instrument.quote_currency)

        notional_value: Decimal = quantity * instrument.multiplier * close_price
        return Money(notional_value, instrument.quote_currency)

    @staticmethod
    def calculate_initial_margin(
        Instrument instrument,
        Quantity quantity,
        Price price,
    ):
        """
        Calculate the initial margin from the given parameters.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        quantity : Quantity
            The order quantity.
        price : Price
            The order price.

        Returns
        -------
        Money
            In the instruments PnL currency.

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(price, "price")

        # TODO: Temporarily no margin
        leverage = 1
        if leverage == 1:
            return Money(0, instrument.cost_currency)

        notional = Account.notional_value(quantity, price)
        margin = notional / leverage * instrument.margin_init
        margin += notional * instrument.taker_fee * 2

        return Money(margin, instrument.cost_currency)

    @staticmethod
    def calculate_maint_margin(
        Instrument instrument,
        PositionSide side,
        Quantity quantity,
        Price last,
    ):
        """
        Calculate the maintenance margin from the given parameters.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the calculation.
        side : PositionSide
            The currency position side.
        quantity : Quantity
            The currency position quantity.
        last : Price
            The position instruments last price.

        Returns
        -------
        Money
            In quote currency.

        """
        # side checked in _get_close_price
        Condition.not_none(quantity, "quantity")
        Condition.not_none(last, "last")

        # TODO: Temporarily no margin
        leverage = 1
        if leverage == 1:
            return Money(0, instrument.cost_currency)  # No margin necessary

        cdef Money notional = Account.notional_value(instrument, quantity, last)
        margin = (notional / leverage) * instrument.margin_maint
        margin += notional * instrument.taker_fee

        return Money(margin, instrument.cost_currency)

    @staticmethod
    def calculate_commission(
        Instrument instrument,
        Quantity last_qty,
        last_px: Decimal,
        LiquiditySide liquidity_side,
    ):
        """
        Calculate the commission generated from a transaction with the given
        parameters.

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

        Returns
        -------
        Money
            In quote currency.

        Raises
        ------
        ValueError
            If liquidity_side is NONE.

        """
        Condition.not_none(last_qty, "last_qty")
        Condition.type(last_px, (Decimal, Price), "last_px")
        Condition.not_equal(liquidity_side, LiquiditySide.NONE, "liquidity_side", "NONE")

        cdef Money notional = Account.notional_value(instrument, last_qty, last_px)

        if liquidity_side == LiquiditySide.MAKER:
            commission: Decimal = notional * instrument.maker_fee
        elif liquidity_side == LiquiditySide.TAKER:
            commission: Decimal = notional * instrument.taker_fee
        else:
            raise RuntimeError(
                f"invalid LiquiditySide, was {LiquiditySideParser.to_str(liquidity_side)}"
            )

        if instrument.is_inverse:
            commission *= 1 / last_px

        return Money(commission, instrument.cost_currency)

# -- QUERIES-MARGIN --------------------------------------------------------------------------------

    cpdef dict initial_margins(self):
        """
        Return the initial margins for the account.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._initial_margins.copy()

    cpdef dict maint_margins(self):
        """
        Return the maintenance margins for the account.

        Returns
        -------
        dict[Currency, Money]

        """
        return self._maint_margins.copy()

    cpdef Money initial_margin(self, Currency currency=None):
        """
        Return the current initial margin.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If None then will use the default
            currency (if set).

        Returns
        -------
        Money or None

        Raises
        ------
        ValueError
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        return self._initial_margins.get(currency)

    cpdef Money maint_margin(self, Currency currency=None):
        """
        Return the current maintenance margin.

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If None then will use the default
            currency (if set).

        Returns
        -------
        Money or None

        Raises
        ------
        ValueError
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        return self._maint_margins.get(currency)

    cpdef Money margin_available(self, Currency currency=None):
        """
        Return the current margin available.

        (`equity - initial_margin - maint_margin`).

        For multi-currency accounts, specify the currency for the query.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the query. If None then will use the default
            currency (if set).

        Returns
        -------
        Money or None

        Raises
        ------
        ValueError
            If currency is None and default_currency is None.

        Warnings
        --------
        Returns `None` if there is no applicable information for the query,
        rather than `Money` of zero amount.

        """
        if currency is None:
            currency = self.default_currency
        Condition.not_none(currency, "currency")

        cdef Money equity = self.equity(currency)
        if equity is None:
            return None

        initial_margin: Decimal = self._initial_margins.get(currency, Decimal())
        maint_margin: Decimal = self._maint_margins.get(currency, Decimal())

        return Money(equity - initial_margin - maint_margin, currency)

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline void _update_balances(
        self,
        list balances,
    ) except *:
        # Update the balances. Note that there is no guarantee that every
        # account currency is included in the event, which is why we don't just
        # assign a dict.
        cdef AccountBalance balance
        for balance in balances:
            self._balances[balance.currency] = balance
