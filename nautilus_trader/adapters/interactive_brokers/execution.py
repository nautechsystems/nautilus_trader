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
import asyncio
from typing import Dict

import ib_insync
from ib_insync import Trade

from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.parsing.execution import (
    nautilus_order_to_ib_order,
)
from nautilus_trader.adapters.interactive_brokers.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.correctness import PyCondition

# TODO - Investigate `updateEvent`:  "Is emitted after a network packet has been handled."
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.msgbus.bus import MessageBus


class InteractiveBrokersExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Interactive Brokers TWS API.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : IB
        The ib_insync IB client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    instrument_provider : BinanceInstrumentProvider
        The instrument provider.
    instrument_provider : InteractiveBrokersInstrumentProvider
        The instrument provider.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: ib_insync.IB,
        account_id: AccountId,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: InteractiveBrokersInstrumentProvider,
    ):
        super().__init__(
            loop=loop,
            client_id=ClientId(IB_VENUE.value),
            venue=IB_VENUE,
            oms_type=OMSType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.CASH,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._client = client
        self._set_account_id(account_id)

        # Hot caches
        self._instrument_ids: Dict[str, InstrumentId] = {}
        self._venue_order_id_to_client_order_id: Dict[VenueOrderId, ClientOrderId] = {}
        self._venue_order_id_to_venue_perm_id: Dict[VenueOrderId, ClientOrderId] = {}
        self._client_order_id_to_strategy_id: Dict[ClientOrderId, StrategyId] = {}

        # Event hooks
        self._client.newOrderEvent += self._on_new_order
        # self._client.orderModifyEvent += self.on_modified_order
        # self._client.cancelOrderEvent += self.on_cancel_order
        self._client.openOrderEvent += self._on_open_order
        # self._client.orderStatusEvent += self.on_order_status
        # self._client.execDetailsEvent += self.on_order_execution

    def connect(self):
        """
        Connect the client to InteractiveBrokers.
        """
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    async def _connect(self):
        # Connect client
        if not self._client.isConnected():
            await self._client.connect()

        # Load instruments based on config
        # try:
        await self._instrument_provider.initialize()
        # except Exception as ex:
        #     self._log.exception(ex)
        #     return
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)
        self._set_connected(True)
        self._log.info("Connected.")

    def disconnect(self):
        """
        Disconnect the client from Interactive Brokers.
        """
        self._log.info("Disconnecting...")
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        # Disconnect clients
        if self._client.isConnected():
            self._client.disconnect()

        self._set_connected(False)
        self._log.info("Disconnected.")

    def create_task(self, coro):
        self._loop.create_task(self._check_task(coro))

    async def _check_task(self, coro):
        try:
            awaitable = await coro
            return awaitable
        except Exception as ex:
            self._log.exception("Unhandled exception", ex)

    def submit_order(self, command: SubmitOrder) -> None:
        PyCondition.not_none(command, "command")

        contract_details = self._instrument_provider.contract_details[command.instrument_id]
        trade: Trade = self._client.placeOrder(
            contract=contract_details.contract,
            order=nautilus_order_to_ib_order(order=command.order),
        )
        self._venue_order_id_to_client_order_id[trade.order.orderId] = command.order.client_order_id
        self._client_order_id_to_strategy_id[command.order.client_order_id] = command.strategy_id

    def _on_new_order(self, trade: Trade):
        self._log.debug(f"new_order: {Trade}")
        instrument_id = self._instrument_provider.contract_id_to_instrument_id[trade.contract.conId]
        client_order_id = self._venue_order_id_to_client_order_id[trade.order.orderId]
        strategy_id = self._client_order_id_to_strategy_id[client_order_id]
        assert trade.log
        self.generate_order_submitted(
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            ts_event=dt_to_unix_nanos(trade.log[-1].time),
        )

    def _on_open_order(self, trade: Trade):
        self.generate_order_accepted()
