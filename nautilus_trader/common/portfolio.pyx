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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.events cimport PositionEvent, PositionOpened, PositionModified
from nautilus_trader.model.events cimport PositionClosed, OrderFillEvent
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.identifiers cimport Symbol, PositionId
from nautilus_trader.model.position cimport Position
from nautilus_trader.common.logging cimport Logger, LoggerAdapter


cdef class Portfolio:
    """
    Provides a trading portfolio of positions.
    """

    def __init__(self,
                 Clock clock not None,
                 UUIDFactory uuid_factory not None,
                 Logger logger=None):
        """
        Initialize a new instance of the Portfolio class.

        :param clock: The clock for the component.
        :param uuid_factory: The uuid factory for the component.
        :param logger: The logger for the component.
        """
        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)

        self._positions_open = {}    # type: [Symbol, {PositionId, Position}]
        self._positions_closed = {}  # type: [Symbol, {PositionId, Position}]

        self.currency = Currency.USD  # Default
        self.daily_pnl_realized = Money(0, self.currency)
        self.total_pnl_realized = Money(0, self.currency)
        self.date_now = self._clock.time_now().date()

    cpdef void set_base_currency(self, Currency currency) except *:
        """
        Set the portfolios base currency.

        :param currency: The base currency to set.
        """
        self.currency = currency

    cpdef void update(self, PositionEvent event) except *:
        """
        Update the portfolio with the given event.

        :param event: The event to update with.
        """
        Condition.not_none(event, "event")

        if event.timestamp.date() != self.date_now:
            self.date_now = event.timestamp.date()
            self.daily_pnl_realized = Money(0, event.position.quote_currency)

        if isinstance(event, PositionOpened):
            self._handle_position_opened(event)
        elif isinstance(event, PositionModified):
            self._handle_position_modified(event)
        else:
            self._handle_position_closed(event)

    cpdef void reset(self) except *:
        """
        Reset the portfolio by returning all stateful values to their initial value.
        """
        self._log.debug(f"Resetting...")

        self._positions_open.clear()
        self._positions_closed.clear()
        self.currency = Currency.USD  # Default
        self.daily_pnl_realized = Money(0, self.currency)
        self.total_pnl_realized = Money(0, self.currency)
        self.date_now = self._clock.time_now().date()

        self._log.info("Reset.")

    cpdef set symbols_open(self):
        """
        Return the open symbols in the portfolio.

        :return: Set[Symbol].
        """
        return set(self._positions_open.keys())

    cpdef set symbols_closed(self):
        """
        Return the closed symbols in the portfolio.

        :return: Set[Symbol].
        """
        return set(self._positions_closed.keys())

    cpdef set symbols_all(self):
        """
        Return the symbols in the portfolio.

        :return: Set[Symbol].
        """
        return self.symbols_open().union(self.symbols_closed())

    cpdef dict positions_open(self, Symbol symbol=None):
        """
        Return the open positions in the portfolio.

        :param symbol: The optional symbol query filter.
        :return: Dict[PositionId, Position].
        """
        cdef dict positions_open
        if symbol is None:
            positions_open = {}
            for symbol, positions in self._positions_open.items():
                positions_open = {**positions_open, **positions}
            return positions_open

        positions_open = self._positions_open.get(symbol)
        if positions_open is None:
            return {}
        return positions_open.copy()

    cpdef dict positions_closed(self, Symbol symbol=None):
        """
        Return the closed positions in the portfolio.

        :param symbol: The optional symbol query filter.
        :return: Dict[PositionId, Position].
        """
        cdef dict positions_closed
        if symbol is None:
            positions_closed = {}
            for symbol, positions in self._positions_closed.items():
                positions_closed = {**positions_closed, **positions}
            return positions_closed

        positions_closed = self._positions_closed.get(symbol)
        if positions_closed is None:
            return {}
        return positions_closed.copy()

    cpdef dict positions_all(self, Symbol symbol=None):
        """
        Return all positions in the portfolio.

        :param symbol: The optional symbol query filter.
        :return: Dict[PositionId, Position].
        """
        return {**self.positions_open(symbol), **self.positions_closed(symbol)}

    cdef void _handle_position_opened(self, PositionOpened event) except *:
        cdef Position position = event.position

        # Remove from positions closed if found
        cdef dict positions_closed = self._positions_closed.get(position.symbol)
        if positions_closed is not None:
            if positions_closed.pop(position.id, None) is not None:
                self._log.warning(f"{position.id} already found in closed positions).")
            # Remove symbol from positions closed if empty
            if not self._positions_closed[position.symbol]:
                del self._positions_closed[position.symbol]

        # Add to positions open
        cdef dict positions_open = self._positions_open.get(position.symbol)
        if positions_open is None:
            positions_open = {}
            self._positions_open[position.symbol] = positions_open

        if position.id in positions_open:
            self._log.warning(f"The opened {position.id} already found in open positions.")
        else:
            positions_open[position.id] = position

    cdef void _handle_position_modified(self, PositionModified event) except *:
        cdef Position position = event.position
        cdef OrderFillEvent fill_event = position.last_event

        if position.entry_direction != fill_event.order_side:
            # Increment PNL
            self.daily_pnl_realized = self.daily_pnl_realized.add(position.realized_pnl_last)
            self.total_pnl_realized = self.total_pnl_realized.add(position.realized_pnl_last)

    cdef void _handle_position_closed(self, PositionClosed event) except *:
        cdef Position position = event.position

        # Remove from positions open if found
        cdef dict positions_open = self._positions_open.get(position.symbol)
        if positions_open is None:
            self._log.error(f"Cannot find {position.symbol.value} in positions open.")
        else:
            if positions_open.pop(position.id, None) is None:
                self._log.error(f"The closed {position.id} was not not found in open positions.")
            else:
                # Remove symbol dictionary from positions open if empty
                if not self._positions_open[position.symbol]:
                    del self._positions_open[position.symbol]

        # Add to positions closed
        cdef dict positions_closed = self._positions_closed.get(position.symbol)
        if positions_closed is None:
            positions_closed = {}
            self._positions_closed[position.symbol] = positions_closed

        if position.id in positions_closed:
            self._log.warning(f"The closed {position.id} already found in closed positions.")
        else:
            positions_closed[position.id] = position

        # Increment PNL
        self.daily_pnl_realized = self.daily_pnl_realized.add(position.realized_pnl)
        self.total_pnl_realized = self.total_pnl_realized.add(position.realized_pnl)
