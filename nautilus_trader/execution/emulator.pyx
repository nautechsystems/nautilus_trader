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
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.model.c_enums.trigger_type cimport TriggerType
from nautilus_trader.model.c_enums.trigger_type cimport TriggerTypeParser
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.msgbus.bus cimport MessageBus


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

        self._limit_buys: dict[InstrumentId, list[SubmitOrder]] = {}
        self._limit_sells: dict[InstrumentId, list[SubmitOrder]] = {}
        self._stop_buys: dict[InstrumentId, list[SubmitOrder]] = {}
        self._stop_sells: dict[InstrumentId, list[SubmitOrder]] = {}

        self._subscribed_quotes: set[InstrumentId] = set()
        self._subscribed_trades: set[InstrumentId] = set()

        # Register endpoints
        self._msgbus.register(endpoint="OrderEmulator.emulate", handler=self.emulate)

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _reset(self) except *:
        self._limit_buys.clear()
        self._limit_sells.clear()
        self._stop_buys.clear()
        self._stop_sells.clear()
        self._subscribed_quotes.clear()
        self._subscribed_trades.clear()

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

    def subscribed_trades(self) -> List[InstrumentId]:
        """
        Return the subscribed trade feeds for the emulator.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscribed_trades))

    cpdef void emulate(self, SubmitOrder command) except *:
        """
        Process the command by emulating its contained order.

        Parameters
        ----------
        command : SubmitOrder
            The command to process.

        """
        Condition.not_none(command, "command")

        if (
            command.emulation_trigger != TriggerType.DEFAULT
            or command.emulation_trigger != TriggerType.LAST
            or command.emulation_trigger != TriggerType.BID_ASK
        ):
            raise RuntimeError(
                f"cannot emulate order: `TriggerType` {TriggerTypeParser.to_str(command.emulation_trigger)} "
                f"not supported."
            )

        # Add emulated order
        # if command.order.side == OrderSide.BUY:
        #     buy_cmds = self._submit_buy.get(command.instrument_id)
        #     if buy_cmds is None:
        #         buy_cmds = []
        #         self._buy_cmds[command.instrument_id] = buy_cmds
        #     buy_cmds.append(command)
        # elif command.order.side == OrderSide.SELL:
        #     sell_cmds = self._submit_sell.get(command.instrument_id)
        #     if sell_cmds is None:
        #         sell_cmds = []
        #         self._sell_cmds[command.instrument_id] = sell_cmds
        # else:
        #     raise RuntimeError("invalid `OrderSide`")

        # Check data subscription
        if command.emulation_trigger == TriggerType.DEFAULT or command.emulation_trigger == TriggerType.BID_ASK:
            if command.instrument_id not in self._subscribed_quotes:
                self.subscribe_quote_ticks(command.instrument_id)
                self._subscribed_quotes.add(command.instrument_id)
        elif command.emulation_trigger == TriggerType.LAST:
            if command.instrument_id not in self._subscribed_trades:
                self.subscribe_trade_ticks(command.instrument_id)
                self._subscribed_trades.add(command.instrument_id)

    cpdef void on_quote_tick(self, QuoteTick tick) except *:
        pass  # Optionally override in subclass
    cpdef void on_trade_tick(self, TradeTick tick) except *:
        pass  # Optionally override in subclass
