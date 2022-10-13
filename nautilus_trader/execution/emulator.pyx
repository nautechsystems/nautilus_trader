# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import List, Optional

from nautilus_trader.config.common import OrderEmulatorConfig

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.matching_core cimport MatchingCore
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.model.c_enums.trigger_type cimport TriggerType
from nautilus_trader.model.c_enums.trigger_type cimport TriggerTypeParser
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.msgbus.bus cimport MessageBus


cdef tuple SUPPORTED_TRIGGERS = (TriggerType.DEFAULT, TriggerType.BID_ASK, TriggerType.LAST)


cdef class OrderEmulator(Actor):
    """
    Provides order emulation for specified trigger types.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        config: Optional[OrderEmulatorConfig] = None,
    ):
        super().__init__()

        self.register_base(
            trader_id=trader_id,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._commands: dict[ClientOrderId, SubmitOrder] = {}
        self._matching_cores: dict[InstrumentId, MatchingCore]  = {}

        self._subscribed_quotes: set[InstrumentId] = set()
        self._subscribed_trades: set[InstrumentId] = set()

        # Register endpoints
        self._msgbus.register(endpoint="OrderEmulator.emulate", handler=self.emulate)

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _reset(self) except *:
        self._commands.clear()
        self._matching_cores.clear()

# -------------------------------------------------------------------------------------------------

    @property
    def subscribed_quotes(self) -> List[InstrumentId]:
        """
        Return the subscribed quote feeds for the emulator.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscribed_quotes))

    @property
    def subscribed_trades(self) -> List[InstrumentId]:
        """
        Return the subscribed trade feeds for the emulator.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscribed_trades))

    def get_commands(self) -> dict[ClientOrderId, SubmitOrder]:
        """
        Return the emulators cached commands.

        Returns
        -------
        dict[ClientOrderId, SubmitOrder]

        """
        return self._commands.copy()

    def get_matching_core(self, InstrumentId instrument_id) -> Optional[MatchingCore]:
        """
        Return the emulators matching core for the given instrument ID.

        Returns
        -------
        MatchingCore or ``None``

        """
        return self._matching_cores.get(instrument_id)

    cpdef void emulate(self, SubmitOrder command) except *:
        """
        Process the command by emulating its contained order.

        Parameters
        ----------
        command : SubmitOrder
            The command to process.

        """
        Condition.not_none(command, "command")
        Condition.not_in(command.order.client_order_id, self._commands, "command.order.client_order_id", "self._commands")

        if command.emulation_trigger not in SUPPORTED_TRIGGERS:
            raise RuntimeError(
                f"cannot emulate order: `TriggerType` {TriggerTypeParser.to_str(command.emulation_trigger)} "
                f"not supported."
            )

        # Cache command
        self._commands[command.order.client_order_id] = command

        # Add to matching core
        cdef MatchingCore matching_core = self._matching_cores.get(command.instrument_id)

        if matching_core is None:
            instrument = self.cache.instrument(command.instrument_id)
            if instrument is None:
                raise RuntimeError(f"cannot find instrument for {instrument.id}")

            matching_core = MatchingCore(
                instrument=instrument,
                trigger_stop_order=self.trigger_stop_order,
                fill_market_order=self.fill_market_order,
                fill_limit_order=self.fill_limit_order,
            )
            self._matching_cores[instrument.id] = matching_core

        matching_core.add_order(command.order)

        # Check data subscription
        if command.emulation_trigger == TriggerType.DEFAULT or command.emulation_trigger == TriggerType.BID_ASK:
            if command.instrument_id not in self._subscribed_quotes:
                self.subscribe_quote_ticks(command.instrument_id)
                self._subscribed_quotes.add(command.instrument_id)
        elif command.emulation_trigger == TriggerType.LAST:
            if command.instrument_id not in self._subscribed_trades:
                self.subscribe_trade_ticks(command.instrument_id)
                self._subscribed_trades.add(command.instrument_id)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void trigger_stop_order(self, Order order) except *:
        pass

    cpdef void fill_market_order(self, Order order, LiquiditySide liquidity_side) except *:
        pass

    cpdef void fill_limit_order(self, Order order, LiquiditySide liquidity_side) except *:
        pass

    cpdef void on_quote_tick(self, QuoteTick tick) except *:
        cdef MatchingCore matching_core = self._matching_cores.get(tick.instrument_id)
        if matching_core is None:
            self._log.error(f"Cannot handle `QuoteTick`: no matching core for {tick.instrument_id}.")
            return

        matching_core.bid = tick.bid
        matching_core.ask = tick.ask
        matching_core.iterate(self._clock.timestamp_ns())

    cpdef void on_trade_tick(self, TradeTick tick) except *:
        cdef MatchingCore matching_core = self._matching_cores.get(tick.instrument_id)
        if matching_core is None:
            self._log.error(f"Cannot handle `TradeTick`: no matching core for {tick.instrument_id}.")
            return

        matching_core.last = tick.last
        if tick.instrument_id not in self._subscribed_quotes:
            matching_core.bid = tick.last
            matching_core.ask = tick.last
        matching_core.iterate(self._clock.timestamp_ns())
