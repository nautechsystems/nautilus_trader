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
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
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


cdef class PortfolioReadOnly:
    """
    Provides a read-only facade for a trading portfolio.
    """
    cpdef Account account(self, Venue venue):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money order_margin(self, Venue venue):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money position_margin(self, Venue venue):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money unrealized_pnl(self, Venue venue):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money open_value(self, Venue venue):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class Portfolio(PortfolioReadOnly):
    """
    Provides a trading portfolio.
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

        self._instruments = {}           # type: {Symbol, Instrument}
        self._ticks = {}                 # type: {Symbol, QuoteTick}
        self._bid_quotes = {}            # type: {Venue: {str: float}}
        self._ask_quotes = {}            # type: {Venue: {str: float}}
        self._accounts = {}              # type: {Venue: Account}
        self._orders_working = {}        # type: {Venue: {Order}}
        self._positions_open = {}        # type: {Venue: {Position}}
        self._positions_closed = {}      # type: {Venue: {Position}}

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
        account.register_portfolio(self)

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

        cdef Venue venue = tick.symbol.venue

        self._ticks[tick.symbol] = tick

        cdef Instrument instrument = self._instruments.get(tick.symbol)
        if not instrument:
            self._log.error(f"No instrument for received tick symbol {tick.symbol}.")
            return  # Cannot add quotes

        cdef dict bid_quotes
        cdef dict ask_quotes
        if self._is_crypto_spot_or_swap(instrument) or self._is_fx_spot(instrument):
            bid_quotes = self._bid_quotes.get(venue)
            ask_quotes = self._ask_quotes.get(venue)
            if not bid_quotes:
                self._bid_quotes[venue] = {instrument.symbol_base_quote: tick.bid.as_double()}
                self._ask_quotes[venue] = {instrument.symbol_base_quote: tick.ask.as_double()}
                return  # Quotes updated

            bid_quotes[instrument.symbol_base_quote] = tick.bid.as_double()
            ask_quotes[instrument.symbol_base_quote] = tick.ask.as_double()

    cpdef void update_orders_working(self, set orders) except *:
        """
        Update the portfolio with the given orders.

        Parameters
        ----------
        orders : Set[Order]

        """
        Condition.not_none(orders, "orders")

        # Blank slate
        self._orders_working.clear()

        cdef Order order
        for order in orders:
            if order.symbol.venue not in self._orders_working:
                self._orders_working[order.symbol.venue] = set()
            if order.is_working():
                self._orders_working[order.symbol.venue].add(order)
                self._log.debug(f"Added working {order}")

        self._log.info(f"Updated {len(orders)} order(s) working.")

        cdef Venue venue
        for venue in self._orders_working.keys():
            self._update_order_margin(venue)

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

        self._update_order_margin(venue)

    cpdef void update_positions(self, set positions) except *:
        """
        Update the portfolio with the given positions.

        Parameters
        ----------
        positions : Set[Position]
            The positions to update with.

        """
        Condition.not_none(positions, "positions")

        cdef Position position
        cdef set positions_open
        cdef set positions_closed
        cdef int open_count = 0
        cdef int closed_count = 0
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
                closed_count += 1

        self._log.info(f"Updated {open_count} position(s) open.")
        self._log.info(f"Updated {closed_count} position(s) closed.")

        cdef Venue venue
        for venue in self._positions_open.keys():
            self._update_position_margin(venue)

    cpdef void update_position(self, PositionEvent event) except *:
        """
        Update the portfolio with the given position event.

        Parameters
        ----------
        event : PositionEvent
            The event to update with.

        """
        Condition.not_none(event, "event")

        if isinstance(event, PositionOpened):
            self._handle_position_opened(event)
        elif isinstance(event, PositionModified):
            self._handle_position_modified(event)
        elif isinstance(event, PositionClosed):
            self._handle_position_closed(event)

        self._log.debug(f"Updated {event.position}.")

        self._update_position_margin(event.position.symbol.venue)

    cpdef void reset(self) except *:
        """
        Reset the portfolio by returning all stateful values to their initial
        value.
        """
        self._log.debug(f"Resetting...")

        self._instruments.clear()
        self._ticks.clear()
        self._bid_quotes.clear()
        self._ask_quotes.clear()
        self._accounts.clear()
        self._orders_working.clear()
        self._positions_open.clear()
        self._positions_closed.clear()

        self._log.info("Reset.")

    cpdef Account account(self, Venue venue):
        """
        Return the account for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the account.

        Returns
        -------
        AccountReadOnly or None

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._accounts.get(venue)
        if not account:
            self._log.error(f"Cannot get account (no account registered for {venue}).")
            return None
        return self._accounts.get(venue)

    cpdef Money order_margin(self, Venue venue):
        """
        Return the order margin for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the order margin.

        Returns
        -------
        Money or None

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._accounts.get(venue)
        if not account:
            self._log.error(f"Cannot get order margin (no account registered for {venue}).")
            return None
        return account.order_margin()

    cpdef Money position_margin(self, Venue venue):
        """
        Return the position margin for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the position margin.

        Returns
        -------
        Money or None

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._accounts.get(venue)
        if not account:
            self._log.error(f"Cannot get position margin (no account registered for {venue}).")
            return None
        return account.position_margin()

    cpdef Money unrealized_pnl(self, Venue venue):
        """
        Return the unrealized pnl for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the unrealized pnl.

        Returns
        -------
        Money or None

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._accounts.get(venue)
        if not account:
            self._log.error(f"Cannot calculate unrealized PNL (no account registered for {venue}).")
            return None

        cdef set positions_open = self._positions_open.get(venue)
        if not positions_open:
            return Money(0, account.currency)

        cdef dict bid_quotes = self._bid_quotes.get(venue, {})
        cdef dict ask_quotes = self._ask_quotes.get(venue, {})
        cdef double pnl = 0.
        cdef double xrate
        cdef Position position
        cdef QuoteTick last
        cdef Instrument instrument
        for position in positions_open:
            last = self._ticks.get(position.symbol)
            instrument = self._instruments.get(position.symbol)
            if not last:
                self._log.error(f"Cannot calculate unrealized PNL "
                                f"(no quotes for {position.symbol}).")
                return None  # Cannot calculate
            if not instrument:
                self._log.error(f"Cannot calculate unrealized PNL "
                                f"(no instrument for {position.symbol}).")
                return None  # Cannot calculate

            xrate = self._xrate_calculator.get_rate(
                from_currency=instrument.base_currency,
                to_currency=account.currency,
                price_type=PriceType.BID if position.entry == OrderSide.BUY else PriceType.ASK,
                bid_quotes=bid_quotes,
                ask_quotes=ask_quotes,
            )
            if xrate == 0:
                self._log.error(f"Cannot calculate unrealized PNL (insufficient data for "
                                f"{position.base_currency}/{account.currency}).")
                return None  # Cannot calculate

            pnl += position.unrealized_pnl(last) * instrument.multiplier * xrate

        return Money(pnl, account.currency)

    cpdef Money open_value(self, Venue venue):
        """
        Return the open value for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the open value.

        Returns
        -------
        Money or None

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._accounts.get(venue)
        if not account:
            self._log.error(f"Cannot calculate open value (no account registered for {venue}).")
            return None

        cdef set positions_open = self._positions_open.get(venue)
        if not positions_open:
            return Money(0, account.currency)

        cdef dict bid_quotes = self._bid_quotes.get(venue, {})
        cdef dict ask_quotes = self._ask_quotes.get(venue, {})
        cdef double open_value = 0.
        cdef double xrate
        cdef Position position
        cdef Instrument instrument
        for position in positions_open:
            instrument = self._instruments.get(position.symbol)
            if not instrument:
                self._log.error(f"Cannot calculate open value "
                                f"(no instrument for {position.symbol}).")
                return None  # Cannot calculate

            xrate = self._xrate_calculator.get_rate(
                from_currency=instrument.base_currency,
                to_currency=account.currency,
                price_type=PriceType.BID if position.entry == OrderSide.BUY else PriceType.ASK,
                bid_quotes=bid_quotes,
                ask_quotes=ask_quotes,
            )

            if xrate == 0:
                self._log.error(f"Cannot calculate open value (insufficient data for "
                                f"{position.base_currency}/{account.currency}).")
                return None  # Cannot calculate

            open_value += position.quantity * instrument.multiplier * xrate

        return Money(open_value, account.currency)

    cdef inline bint _is_crypto_spot_or_swap(self, Instrument instrument) except *:
        return instrument.asset_class == AssetClass.CRYPTO \
               and (instrument.asset_type == AssetType.SPOT or instrument.asset_type == AssetType.SWAP)

    cdef inline bint _is_fx_spot(self, Instrument instrument) except *:
        return instrument.asset_class == AssetClass.FX and instrument.asset_type == AssetType.SPOT

    cdef inline void _handle_position_opened(self, PositionOpened event) except *:
        cdef Venue venue = event.position.symbol.venue
        cdef Position position = event.position

        # Add to positions open
        cdef set positions_open = self._positions_open.get(venue, set())
        positions_open.add(position)
        self._positions_open[venue] = positions_open

    cdef inline void _handle_position_modified(self, PositionModified event) except *:
        cdef Venue venue = event.position.symbol.venue
        cdef set positions_open = self._positions_open.get(venue)

    cdef inline void _handle_position_closed(self, PositionClosed event) except *:
        cdef Venue venue = event.position.symbol.venue
        cdef Position position = event.position

        # Remove from positions open if found
        cdef set positions_open = self._positions_open.get(venue)
        if positions_open:
            positions_open.discard(position)

        # Add to positions closed
        cdef set positions_closed = self._positions_closed.get(venue, set())
        positions_closed.add(position)
        self._positions_closed[venue] = positions_closed

    cdef inline void _update_order_margin(self, Venue venue):
        cdef Account account = self._accounts.get(venue)
        if not account:
            self._log.error(f"Cannot update order initial margin "
                            f"(no account registered for {venue}).")
            return  # Cannot calculate

        cdef set working_orders = self._orders_working.get(venue)
        if not working_orders:
            return  # Nothing to calculate

        cdef dict bid_quotes = self._bid_quotes.get(venue, {})
        cdef dict ask_quotes = self._ask_quotes.get(venue, {})
        cdef double notional = 0
        cdef double margin = 0
        cdef double xrate = 1.
        cdef Order order
        cdef Instrument instrument
        for order in working_orders:
            instrument = self._instruments.get(order.symbol)
            if not instrument:
                self._log.error(f"Cannot calculate order initial margin "
                                f"(no instrument for {order.symbol}).")
                continue  # Cannot calculate

            if instrument.leverage == 1:
                continue  # No margin necessary

            xrate = self._xrate_calculator.get_rate(
                from_currency=instrument.base_currency,
                to_currency=account.currency,
                price_type=PriceType.BID if order.side == OrderSide.SELL else PriceType.ASK,
                bid_quotes=bid_quotes,
                ask_quotes=ask_quotes,
            )
            if xrate == 0:
                self._log.error(f"Cannot calculate order initial margin (insufficient data for "
                                f"{instrument.base_currency}/{account.currency}).")
                continue  # Cannot calculate

            # Calculate notional value
            notional = order.quantity * instrument.multiplier * xrate

            # Order margin
            margin += notional * instrument.margin_initial / instrument.leverage

            # Fees on notional
            margin += notional * instrument.taker_fee * 2

        cdef Money order_margin = Money(margin, account.currency)
        account.update_order_margin(order_margin)

        self._log.info(f"Updated {venue} order initial margin to "
                       f"{order_margin.to_string_formatted()}")

    cdef inline void _update_position_margin(self, Venue venue):
        cdef Account account = self._accounts.get(venue)
        if not account:
            self._log.error(f"Cannot update position maintenance margin "
                            f"(no account registered for {venue}).")
            return  # Cannot calculate

        cdef set open_positions = self._positions_open.get(venue)
        if not open_positions:
            return  # Nothing to calculate

        cdef dict bid_quotes = self._bid_quotes.get(venue, {})
        cdef dict ask_quotes = self._ask_quotes.get(venue, {})
        cdef double notional
        cdef double margin = 0
        cdef double xrate = 1.
        cdef Position position
        cdef Instrument instrument
        for position in open_positions:
            instrument = self._instruments.get(position.symbol)
            if not instrument:
                self._log.error(f"Cannot calculate position maintenance margin "
                                f"(no instrument for {position.symbol}).")
                continue  # Cannot calculate

            if instrument.leverage == 1:
                continue  # No margin necessary

            xrate = self._xrate_calculator.get_rate(
                from_currency=instrument.base_currency,
                to_currency=account.currency,
                price_type=PriceType.BID if position.entry == OrderSide.BUY else PriceType.ASK,
                bid_quotes=bid_quotes,
                ask_quotes=ask_quotes,
            )
            if xrate == 0:
                self._log.error(f"Cannot calculate position maintenance margin "
                                f"(insufficient data for {instrument.base_currency}/{account.currency}).")
                continue  # Cannot calculate

            # Calculate notional value
            notional = position.quantity * instrument.multiplier * xrate

            # Position margin
            margin += notional * instrument.margin_maintenance / instrument.leverage

            # Fees on notional
            margin += notional * instrument.taker_fee

        cdef Money position_margin = Money(margin, account.currency)
        account.update_position_margin(position_margin)

        self._log.info(f"Updated {venue} position maintenance margin to "
                       f"{position_margin.to_string_formatted()}")
