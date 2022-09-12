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
from typing import Dict, List, Optional

import ib_insync
import pandas as pd
from ib_insync import Order as IBOrder
from ib_insync import Trade as IBTrade

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
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
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
        self._ib_insync_orders: Dict[ClientOrderId, IBTrade] = {}

        # Event hooks
        # self._client.orderStatusEvent += self.on_order_status # TODO - Does this capture everything?
        self._client.newOrderEvent += self._on_new_order
        self._client.openOrderEvent += self._on_open_order
        self._client.orderModifyEvent += self._on_order_modify
        self._client.cancelOrderEvent += self._on_order_cancel
        self._client.execDetailsEvent += self._on_execution_detail

    @property
    def instrument_provider(self) -> InteractiveBrokersInstrumentProvider:
        return self._instrument_provider  # type: ignore

    def connect(self):
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    async def _connect(self):
        # Connect client
        if not self._client.isConnected():
            await self._client.connect()

        # Load instruments based on config
        # try:
        await self.instrument_provider.initialize()
        # except Exception as e:
        #     self._log.exception(e)
        #     return
        for instrument in self.instrument_provider.get_all().values():
            self._handle_data(instrument)
        self._set_connected(True)
        self._log.info("Connected.")

    def disconnect(self):
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
        except Exception as e:
            self._log.exception("Unhandled exception", e)

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
    ) -> OrderStatusReport:
        pass  # TODO: Implement

    async def generate_order_status_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
        open_only: bool = False,
    ) -> List[OrderStatusReport]:
        self._log.warning("Cannot generate `List[OrderStatusReport]`: not yet implemented.")

        return []  # TODO: Implement

    async def generate_trade_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> List[TradeReport]:
        self._log.warning("Cannot generate `List[TradeReport]`: not yet implemented.")

        return []  # TODO: Implement

    async def generate_position_status_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> List[PositionStatusReport]:
        self._log.warning("Cannot generate `List[PositionStatusReport]`: not yet implemented.")

        return []  # TODO: Implement

    def submit_order(self, command: SubmitOrder) -> None:
        PyCondition.not_none(command, "command")

        contract_details = self.instrument_provider.contract_details[command.instrument_id.value]
        order: IBOrder = nautilus_order_to_ib_order(order=command.order)
        trade: IBTrade = self._client.placeOrder(contract=contract_details.contract, order=order)
        venue_order_id = VenueOrderId(str(trade.order.orderId))
        self._venue_order_id_to_client_order_id[venue_order_id] = command.order.client_order_id
        self._client_order_id_to_strategy_id[command.order.client_order_id] = command.strategy_id
        self._ib_insync_orders[command.order.client_order_id] = trade

    def modify_order(self, command: ModifyOrder) -> None:
        # ib_insync modifies orders by modifying the original order object and
        # calling placeOrder again.
        PyCondition.not_none(command, "command")
        # TODO - Can we just reconstruct the IBOrder object from the `command` ?
        trade: IBTrade = self._ib_insync_orders[command.client_order_id]
        order = trade.order
        if order.totalQuantity != command.quantity:
            order.totalQuantity = command.quantity.as_double()
        if getattr(order, "lmtPrice", None) != command.price:
            order.lmtPrice = command.price.as_double()
        new_trade: IBTrade = self._client.placeOrder(contract=trade.contract, order=order)
        self._ib_insync_orders[command.client_order_id] = new_trade

    def cancel_order(self, command: CancelOrder) -> None:
        # ib_insync modifies orders by modifying the original order object and
        # calling placeOrder again.
        PyCondition.not_none(command, "command")
        # TODO - Can we just reconstruct the IBOrder object from the `command` ?
        trade: IBTrade = self._ib_insync_orders[command.client_order_id]
        order = trade.order
        new_trade: IBTrade = self._client.cancelOrder(order=order)
        self._ib_insync_orders[command.client_order_id] = new_trade

    def _on_new_order(self, trade: IBTrade):
        self._log.debug(f"new_order: {IBTrade}")
        instrument_id = self.instrument_provider.contract_id_to_instrument_id[trade.contract.conId]
        venue_order_id = VenueOrderId(str(trade.order.permId))
        client_order_id = self._venue_order_id_to_client_order_id[venue_order_id]
        strategy_id = self._client_order_id_to_strategy_id[client_order_id]
        assert trade.log
        self.generate_order_submitted(
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            ts_event=dt_to_unix_nanos(trade.log[-1].time),
        )

    def _on_open_order(self, trade: IBTrade):
        venue_order_id = VenueOrderId(str(trade.order.permId))
        instrument_id = self.instrument_provider.contract_id_to_instrument_id[trade.contract.conId]
        client_order_id = self._venue_order_id_to_client_order_id[venue_order_id]
        strategy_id = self._client_order_id_to_strategy_id[client_order_id]
        self.generate_order_accepted(
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            ts_event=dt_to_unix_nanos(trade.log[-1].time),
        )
        # We can remove the local `_venue_order_id_to_client_order_id` now, we have a permId
        self._venue_order_id_to_client_order_id.pop(venue_order_id)

    def _on_order_modify(self, trade: IBTrade):
        venue_order_id = VenueOrderId(str(trade.orderStatus.permId))
        instrument_id = self.instrument_provider.contract_id_to_instrument_id[trade.contract.conId]
        instrument: Instrument = self._cache.instrument(instrument_id)
        client_order_id = self._venue_order_id_to_client_order_id[venue_order_id]
        strategy_id = self._client_order_id_to_strategy_id[client_order_id]
        self.generate_order_updated(
            strategy_id=strategy_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            quantity=Quantity(trade.order.totalQuantity, precision=instrument.size_precision),
            price=Price(trade.order.lmtPrice, precision=instrument.price_precision),
            trigger_price=None,
            ts_event=dt_to_unix_nanos(trade.log[-1].time),
            venue_order_id_modified=False,  # TODO - does this happen?
        )

    def _on_order_cancel(self, trade: IBTrade):
        if trade.orderStatus.status not in ("PendingCancel", "Cancelled"):
            self._log.warning("Called `_on_order_cancel` without order cancel status")
        instrument_id = self.instrument_provider.contract_id_to_instrument_id[trade.contract.conId]
        venue_order_id = VenueOrderId(str(trade.order.permId))
        client_order_id = self._venue_order_id_to_client_order_id[venue_order_id]
        strategy_id = self._client_order_id_to_strategy_id[client_order_id]
        if trade.orderStatus.status == "PendingCancel":
            self.generate_order_pending_cancel(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=dt_to_unix_nanos(trade.log[-1].time),
            )
        elif trade.orderStatus.status == "Cancelled":
            self.generate_order_canceled(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=dt_to_unix_nanos(trade.log[-1].time),
            )

    def _on_execution_detail(self, trade: IBTrade):
        raise NotImplementedError
