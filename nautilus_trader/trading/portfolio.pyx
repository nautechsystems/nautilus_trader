# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.currency cimport USD
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionModified
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.trading.account cimport Account
from nautilus_trader.trading.calculators cimport ExchangeRateCalculator


cdef class Portfolio:
    """
    Provides a trading portfolio of positions.
    """

    def __init__(
            self,
            Clock clock not None,
            UUIDFactory uuid_factory not None,
            Logger logger=None,
    ):
        """
        Initialize a new instance of the Portfolio class.

        Parameters
        ----------
        clock : Clock
            The clock for the component.
        uuid_factory : UUIDFactory
            The uuid factory for the component.
        logger : Logger
            The logger for the component.

        """
        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._xrate_calculator = ExchangeRateCalculator()

        self.date_now = self._clock.utc_now().date()
        self.base_currency = USD

        self._bid_quotes = {}            # type: {str: float}
        self._ask_quotes = {}            # type: {str: float}
        self._instruments = {}           # type: {Symbol, Instrument}
        self._accounts = {}              # type: {Venue: Account}
        self._orders_working = {}        # type: {Venue: {Order}}
        self._positions_open = {}        # type: {Venue: {Position}}
        self._positions_closed = {}      # type: {Venue: {Position}}
        self._unrealized_pnls = {}       # type: {Venue: Money}
        self._open_values = {}           # type: {Venue: Money}

        self._unrealized_pnl = self._money_zero()
        self._open_value = self._money_zero()
        self._calculated_latest_totals = False

    cpdef void set_base_currency(self, Currency currency) except *:
        """
        Set the base currency for the portfolio.

        Parameters
        ----------
        currency : Currency
            The base currency to set.

        """
        Condition.not_none(currency, "currency")

        self.base_currency = currency
        self._unrealized_pnl = self._money_zero()
        self._open_value = self._money_zero()
        self._calculated_latest_totals = False

    cpdef void register_account(self, Account account) except *:
        """
        Register the given account with the portfolio.

        Parameters
        ----------
        account : Account
            The account to register.

        Raises
        ------
        KeyError
            If issuer is already registered with the portfolio.

        """
        Condition.not_none(account, "account")
        Condition.not_in(account.id.issuer, self._accounts, "venue", "_accounts")

        self._accounts[account.id.issuer_as_venue()] = account

    cpdef void update_instrument(self, Instrument instrument) except *:
        """
        Update the portfolio with the given instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument to update.

        """
        Condition.not_none(instrument, "instrument")

        self._instruments[instrument.symbol] = instrument

    cpdef void update_tick(self, QuoteTick tick) except *:
        """
        Update the portfolio with the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick to update with.

        """
        Condition.not_none(tick, "tick")

        self._calculated_latest_totals = False
        # TODO: Handle the case of same symbol over different venues
        self._bid_quotes[tick.symbol.code] = tick.bid.as_double()
        self._ask_quotes[tick.symbol.code] = tick.ask.as_double()

        cdef Venue venue = tick.symbol.venue
        cdef set positions_open = self._positions_open.get(venue)

        if not positions_open:
            return

        cdef dict unrealized_pnls = {}  # type: {(Currency, PositionSide), float}
        cdef tuple currency_side        # type: (Currency, PositionSide)

        cdef double pnl
        # Total all venue position unrealized pnls in position base currencies
        cdef Position position
        for position in positions_open:
            if position.symbol == tick.symbol:
                position.update(tick)
                currency_side = (position.base_currency, position.side)
                pnl = unrealized_pnls.get(currency_side, 0.)
                unrealized_pnls[currency_side] = pnl + position.unrealized_pnl.as_double()

        cdef double total_unrealized_pnl = 0.
        cdef double xrate
        cdef Currency currency
        cdef PositionSide side
        for currency_side, pnl in unrealized_pnls.items():
            currency = currency_side[0]
            if currency == self.base_currency:
                total_unrealized_pnl += pnl
            else:
                xrate = self._get_xrate(currency, currency_side[1])
                total_unrealized_pnl += pnl * xrate

        self._unrealized_pnls[venue] = Money(total_unrealized_pnl, self.base_currency)

    cpdef void update_orders_working(self, set orders) except *:
        """
        TBD.

        Parameters
        ----------
        orders

        """
        Condition.not_none(orders, "orders")

        # Blank slate
        self._orders_working.clear()

        cdef Order order
        for order in orders:
            self.update_order(order)

        self._log.info(f"Updated {len(orders)} order(s) working.")

    cpdef void update_order(self, Order order) except *:
        """
        Update the portfolio with the given order.

        Parameters
        ----------
        order : Order
            The order to update with.

        """
        Condition.not_none(order, "order")

        cdef Venue venue = order.symbol.venue
        cdef set orders_working = self._orders_working.get(venue)

        if order.is_working():
            if orders_working:
                orders_working.add(order)
                self._log.debug(f"Added working {order}")
            else:
                self._orders_working[venue] = {order}
        elif order.is_completed() and orders_working:
            orders_working.discard(order)
            self._log.debug(f"Discarded working {order}")

    cpdef void update_positions(self, set positions) except *:
        """
        Update the portfolio with the given positions.

        Parameters
        ----------
        positions : Set[Position]
            The positions to update with.

        """
        Condition.not_none(positions, "positions")

        cdef int open_count = 0
        cdef int closed_count = 0

        cdef Position position
        for position in positions:
            if position.is_open():
                positions_open = self._positions_open.get(position.symbol.venue, set())
                positions_open.add(position)
                self._positions_open[position.symbol.venue].add(position)
                self._log.debug(f"Added open {position}")
                open_count += 1
            elif position.is_closed():
                positions_closed = self._positions_closed.get(position.symbol.venue, set())
                positions_closed.add(position)
                self._positions_closed[position.symbol.venue].add(position)
                self._log.debug(f"Added closed {position}")
                closed_count += 1

        self._log.info(f"Updated {open_count} position(s) open.")
        self._log.info(f"Updated {closed_count} position(s) closed.")

    cpdef void update_position(self, PositionEvent event) except *:
        """
        Update the portfolio with the given position event.

        Parameters
        ----------
        event : PositionEvent
            The event to update with.

        """
        Condition.not_none(event, "event")

        self._log.debug(f"Updating {event.position}...")

        if event.timestamp.date() != self.date_now:
            self.date_now = event.timestamp.date()

        if isinstance(event, PositionOpened):
            self._handle_position_opened(event)
        elif isinstance(event, PositionModified):
            self._handle_position_modified(event)
        elif isinstance(event, PositionClosed):
            self._handle_position_closed(event)

    cpdef void reset(self) except *:
        """
        Reset the portfolio by returning all stateful values to their initial
        value.
        """
        self._log.debug(f"Resetting...")

        self._instruments.clear()
        self._bid_quotes.clear()
        self._ask_quotes.clear()
        self._accounts.clear()
        self._orders_working.clear()
        self._positions_open.clear()
        self._positions_closed.clear()
        self._unrealized_pnls.clear()
        self._open_values.clear()
        self._unrealized_pnl = self._money_zero()
        self._open_value = self._money_zero()
        self._calculated_latest_totals = False

        self._log.info("Reset.")

    cpdef Money unrealized_pnl(self, Venue venue=None):
        """
        Return the unrealized pnl for the portfolio or a specific venue.

        Parameters
        ----------
        venue : Venue, optional
            The venue filter for the unrealized pnl.

        Returns
        -------
        Money

        """
        if venue is not None:
            return self._unrealized_pnls.get(venue, self._money_zero())

        if self._calculated_latest:
            return self._unrealized_pnl

        # Recalculate
        self._calculate_unrealized_pnl()
        return self._unrealized_pnl

    cpdef Money position_value(self, Venue venue=None):
        """
        Return the value at risk for the portfolio or a specific venue.

        Parameters
        ----------
        venue : Venue, optional.
            The venue filter for the value at risk.

        Returns
        -------
        Money

        """
        Condition.not_none(venue, "venue")

        if venue is None:
            return self._open_value

        return self._open_values.get(venue, self._money_zero())

    cpdef Money position_margin(self, Venue venue):
        return self._money_zero()

    cpdef Money order_margin(self, Venue venue):
        return self._money_zero()

    cdef inline Money _money_zero(self):
        return Money(0, self.base_currency)

    cdef inline double _get_xrate(self, Currency currency, PositionSide side):
        cdef PriceType price_type = PriceType.BID if side == PositionSide.LONG else PriceType.ASK
        # TODO: Handle exceptions
        return self._xrate_calculator.get_rate(
            from_currency=currency,
            to_currency=self.base_currency,
            price_type=price_type,
            bid_quotes=self._bid_quotes,
            ask_quotes=self._ask_quotes,
        )

    cdef inline void _calculate_unrealized_pnl(self) except *:
        cdef Money new_unrealized_pnl = self._money_zero()
        cdef Money unrealized_pnl
        for unrealized_pnl in self._unrealized_pnls.values():
            new_unrealized_pnl.add(unrealized_pnl)

        self._unrealized_pnl = new_unrealized_pnl
        self._calculated_latest = True

    cdef inline void _calculate_position_value(self, Position position) except *:
        cdef OrderFilled fill = position.last_event()
        cdef double xrate = 1.
        if fill.base_currency != self.base_currency:
            xrate = self._get_xrate(fill.base_currency, position.side)

        # TODO: Add multiplier
        cdef Money change = Money(fill.filled_qty.as_double() * xrate, self.base_currency)

        if position.entry == OrderSide.BUY:
            self._calculate_long_position_value_change(position.symbol.venue, fill.order_side, change)
        elif position.entry == OrderSide.SELL:
            self._calculate_short_position_value_change(position.symbol.venue, fill.order_side, change)
        # TODO: Handle invalid order side

    cdef inline void _calculate_long_position_value_change(
            self,
            Venue venue,
            OrderSide fill_side,
            Money change,
    ) except *:
        cdef Money previous_value = self._open_values.get(venue, self._money_zero())

        if fill_side == OrderSide.BUY:
            self._open_value = self._open_value.add(change)
            self._open_values[venue] = previous_value.add(change)
        else:
            self._open_value = self._open_value.sub(change)
            self._open_values[venue] = previous_value.add(change)

    cdef inline void _calculate_short_position_value_change(
            self,
            Venue venue,
            OrderSide fill_side,
            Money change,
    ) except *:
        cdef Money previous_value = self._open_values.get(venue, self._money_zero())

        if fill_side == OrderSide.SELL:
            self._open_value = self._open_value.add(change)
            self._open_values[venue] = previous_value.add(change)
        else:
            self._open_value = self._open_value.sub(change)
            self._open_values[venue] = previous_value.add(change)

    cdef inline void _handle_position_opened(self, PositionOpened event) except *:
        cdef Position position = event.position
        cdef Venue venue = event.position.symbol.venue
        cdef Account account = self._accounts.get(venue)

        if account is None:
            self._accounts[venue] = account

        # Add to positions open
        cdef set positions_open = self._positions_open.get(venue)
        if positions_open:
            positions_open.add(position)
        else:
            self._positions_open[venue] = {position}

        self._calculate_position_value(position)

    cdef inline void _handle_position_modified(self, PositionModified event) except *:
        self._calculate_position_value(event.position)

    cdef inline void _handle_position_closed(self, PositionClosed event) except *:
        cdef Venue venue = event.position.symbol.venue
        cdef Position position = event.position

        # Remove from positions open if found
        cdef set positions_open = self._positions_open.get(venue)
        if positions_open:
            positions_open.discard(position)

        # Add to positions closed
        cdef set positions_closed = self._positions_closed.get(venue)
        if positions_closed:
            positions_closed.add(position)
        else:
            self._positions_closed[venue] = {position}

        self._calculate_position_value(position)
