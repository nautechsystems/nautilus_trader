# -------------------------------------------------------------------------------------------------
# <copyright file="portfolio.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from typing import Dict

from nautilus_trader.model.events cimport (
    PositionEvent,
    PositionOpened,
    PositionModified,
    PositionClosed,
    OrderFillEvent)
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.identifiers cimport Symbol, PositionId
from nautilus_trader.model.position cimport Position
from nautilus_trader.common.logger cimport Logger, LoggerAdapter


cdef class Portfolio:
    """
    Provides a trading portfolio of positions.
    """

    def __init__(self,
                 Clock clock,
                 GuidFactory guid_factory,
                 Logger logger=None):
        """
        Initializes a new instance of the Portfolio class.

        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        """
        self._clock = clock
        self._guid_factory = guid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)

        self._positions_open = {}    # type: Dict[Symbol, Dict[PositionId, Position]]
        self._positions_closed = {}  # type: Dict[Symbol, Dict[PositionId, Position]]

        self.daily_pnl_realized = Money.zero()
        self.total_pnl_realized = Money.zero()
        self.date_now = self._clock.time_now().date()

    cpdef void update(self, PositionEvent event):
        """
        Update the portfolio with the given event.
        
        :param event: The event to update with.
        """
        if event.timestamp.date() != self.date_now:
            self.date_now = event.timestamp.date()
            self.daily_pnl_realized = Money.zero()

        if isinstance(event, PositionOpened):
            self._handle_position_opened(event)
        elif isinstance(event, PositionModified):
            self._handle_position_modified(event)
        else:
            self._handle_position_closed(event)

    cpdef void reset(self):
        """
        Reset the portfolio by returning all stateful values to their initial value.
        """
        self._log.info(f"Resetting...")

        self._positions_open.clear()
        self._positions_closed.clear()
        self.daily_pnl_realized = Money.zero()
        self.total_pnl_realized = Money.zero()
        self.date_now = self._clock.date_now()

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
        
        :param symbol: The symbol positions query filter (optional can be None).
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
        
        :param symbol: The symbol positions query filter (optional can be None).
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
        
        :param symbol: The symbol positions query filter (optional can be None).
        :return: Dict[PositionId, Position].
        """
        return {**self.positions_open(symbol), **self.positions_closed(symbol)}

    cdef void _handle_position_opened(self, PositionOpened event):
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

    cdef void _handle_position_modified(self, PositionModified event):
        cdef Position position = event.position
        cdef OrderFillEvent fill_event = position.last_event

        if position.entry_direction != fill_event.order_side:
            # Increment PNL
            self.daily_pnl_realized += position.realized_pnl_last
            self.total_pnl_realized += position.realized_pnl_last

    cdef void _handle_position_closed(self, PositionClosed event):
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
        self.daily_pnl_realized += position.realized_pnl
        self.total_pnl_realized += position.realized_pnl
