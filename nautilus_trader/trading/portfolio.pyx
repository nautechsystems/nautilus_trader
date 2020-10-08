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
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionModified
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.account cimport Account


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

        self._accounts = {}          # type: {Venue: Account}
        self._positions_open = {}    # type: {Venue: [Position]}
        self._positions_closed = {}  # type: {Venue: [Position]}

        self.date_now = self._clock.utc_now().date()

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

        self._accounts[account.id.issuer] = account
        account.register_portfolio(self)

    cpdef void handle_tick(self, QuoteTick tick) except *:
        """
        TBD.
        Parameters
        ----------
        tick : QuoteTick
            The tick to handle

        """
        pass

    cpdef void handle_event(self, PositionEvent event) except *:
        """
        Update the portfolio with the given event.

        Parameters
        ----------
        event : PositionEvent
            The event to update with.

        """
        Condition.not_none(event, "event")

        if event.timestamp.date() != self.date_now:
            self.date_now = event.timestamp.date()

        if isinstance(event, PositionOpened):
            self._handle_position_opened(event)
        elif isinstance(event, PositionModified):
            self._handle_position_modified(event)
        else:
            self._handle_position_closed(event)

    cpdef void reset(self) except *:
        """
        Reset the portfolio by returning all stateful values to their initial
        value.
        """
        self._log.debug(f"Resetting...")

        self._accounts.clear()
        self._positions_open.clear()
        self._positions_closed.clear()
        self.date_now = self._clock.utc_now().date()

        self._log.info("Reset.")

    cdef void _handle_position_opened(self, PositionOpened event) except *:
        cdef Position position = event.position
        cdef Venue venue = event.position.symbol.venue
        cdef Account account = self._accounts.get(venue)

        if account is None:
            self._accounts[venue] = account
            # TODO: Other protections for single account per venue

        # Add to positions open
        cdef list positions_open = self._positions_open.get(venue)
        if not positions_open:
            self._positions_open[venue] = [position]
            return

        if position in positions_open:
            self._log.warning(f"The opened {position.id} already found in open positions.")
        else:
            positions_open.append(position)

    cdef void _handle_position_modified(self, PositionModified event) except *:
        pass  # TODO: Implement

    cdef void _handle_position_closed(self, PositionClosed event) except *:
        cdef Venue venue = event.position.symbol.venue
        cdef Position position = event.position

        # Remove from positions open if found
        cdef list positions_open = self._positions_open.get(venue)
        if not positions_open:
            self._log.error(f"Cannot find open positions for {venue}.")
        else:
            try:
                positions_open.remove(position)
            except ValueError as ex:
                self._log.error(f"The closed {position} was not not found in open positions.")

        # Add to positions closed
        cdef list positions_closed = self._positions_closed.get(venue)
        if not positions_closed:
            self._positions_closed[venue] = [position]
        else:
            if position in positions_closed:
                self._log.error(f"The closed {position} already found in closed positions.")
            else:
                positions_closed.append(position)

        # Increment PNLs
        # TODO: Implement
