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
from typing import Optional

import ib_insync
import pandas as pd
from ib_insync import AccountValue
from ib_insync import Fill as IBFill
from ib_insync import Order as IBOrder
from ib_insync import OrderStatus as IBOrderStatus
from ib_insync import Trade as IBTrade

from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.parsing.execution import (
    account_values_to_nautilus_account_info,
)
from nautilus_trader.adapters.interactive_brokers.parsing.execution import (
    ib_order_to_nautilus_order_type,
)
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
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.c_enums.order_side import OrderSideParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
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

        self._client: ib_insync.IB = client
        self._set_account_id(account_id)

        # Hot caches
        self._instrument_ids: dict[str, InstrumentId] = {}
        self._ib_insync_orders: dict[ClientOrderId, IBTrade] = {}

        # Event hooks
        self._client.newOrderEvent += self._on_order_update_event
        self._client.orderModifyEvent += self._on_order_update_event
        self._client.cancelOrderEvent += self._on_order_update_event
        self._client.openOrderEvent += self._on_order_update_event
        self._client.orderStatusEvent += self._on_order_update_event
        self._client.execDetailsEvent += self._on_execution_detail

    @property
    def instrument_provider(self) -> InteractiveBrokersInstrumentProvider:
        return self._instrument_provider  # type: ignore

    async def _connect(self):
        # Connect client
        if not self._client.isConnected():
            await self._client.connect()

        # Load account balance
        account_values: list[AccountValue] = self._client.accountValues()
        self.on_account_update(account_values)

    async def _disconnect(self):
        # Disconnect clients
        if self._client.isConnected():
            self._client.disconnect()

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
    ) -> Optional[OrderStatusReport]:
        self._log.warning("Cannot generate `IBOrderStatusReport`: not yet implemented.")

        return None  # TODO: Implement

    async def generate_order_status_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        self._log.warning("Cannot generate `list[IBOrderStatusReport]`: not yet implemented.")

        return []  # TODO: Implement

    async def generate_trade_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> list[TradeReport]:
        self._log.warning("Cannot generate `list[TradeReport]`: not yet implemented.")

        return []  # TODO: Implement

    async def generate_position_status_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> list[PositionStatusReport]:
        self._log.warning("Cannot generate `list[PositionStatusReport]`: not yet implemented.")

        return []  # TODO: Implement

    def submit_order(self, command: SubmitOrder) -> None:
        PyCondition.not_none(command, "command")

        contract_details = self.instrument_provider.contract_details[command.instrument_id.value]
        order: IBOrder = nautilus_order_to_ib_order(order=command.order)
        order.account = self.account_id.get_id()
        trade: IBTrade = self._client.placeOrder(contract=contract_details.contract, order=order)
        self._ib_insync_orders[command.order.client_order_id] = trade
        self.generate_order_submitted(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.order.client_order_id,
            ts_event=command.ts_init,
        )

    def modify_order(self, command: ModifyOrder) -> None:
        if not (command.quantity or command.price):
            return
        # ib_insync modifies orders by modifying the original order object and
        # calling placeOrder again.
        # TODO - NEEDS TESTING
        PyCondition.not_none(command, "command")
        # TODO - Can we just reconstruct the IBOrder object from the `command` ?
        trade: IBTrade = self._ib_insync_orders[command.client_order_id]
        order = trade.order

        if command.quantity and order.totalQuantity != command.quantity:
            order.totalQuantity = command.quantity.as_double()
        if getattr(order, "lmtPrice", None) != command.price:
            order.lmtPrice = command.price.as_double()
        order.account = self.account_id.get_id()
        new_trade: IBTrade = self._client.placeOrder(contract=trade.contract, order=order)
        self._ib_insync_orders[command.client_order_id] = new_trade
        trade.modifyEvent += self._on_order_modify
        new_trade.modifyEvent += self._on_order_modify

    def cancel_order(self, command: CancelOrder) -> None:
        PyCondition.not_none(command, "command")
        trade: IBTrade = self._ib_insync_orders[command.client_order_id]
        order = trade.order
        new_trade: IBTrade = self._client.cancelOrder(order=order)
        self._ib_insync_orders[command.client_order_id] = new_trade

    def _on_order_update_event(self, trade: IBTrade):
        self._log.debug(
            f"_on_order_update_event {trade.order.orderRef}: {trade.orderStatus.status=}",
        )
        status: str = trade.orderStatus.status
        if status == IBOrderStatus.PreSubmitted:
            self._on_pre_submitted_event(trade)
        elif status == IBOrderStatus.PendingSubmit:
            self._on_pending_submit_event(trade)
        elif status == IBOrderStatus.Submitted:
            self._on_submitted_event(trade)
        elif status == IBOrderStatus.PendingCancel:
            self._on_order_pending_cancel(trade)
        elif status in (IBOrderStatus.Cancelled, IBOrderStatus.ApiCancelled):
            self._on_order_cancelled(trade)
        elif status == IBOrderStatus.Filled:
            self._on_filled_event(trade)
        else:
            self._log.warning(
                f"UNHANDLED status {trade.order.orderRef}: {trade.orderStatus.status}",
            )

    def _on_pending_submit_event(self, trade: IBTrade):
        self._log.debug(f"order pending_submit {trade.order.orderRef}: {trade}")

    def _on_pre_submitted_event(self, trade: IBTrade):
        self._log.debug(f"order pre_submitted {trade.order.orderRef}: {trade}")
        client_order_id = ClientOrderId(trade.order.orderRef)
        order: Order = self._cache.order(client_order_id)
        if order.status in (OrderStatus.SUBMITTED,):
            self.generate_order_accepted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=client_order_id,
                venue_order_id=VenueOrderId(str(trade.order.permId)),
                ts_event=dt_to_unix_nanos(trade.log[-1].time),
            )

    def _on_submitted_event(self, trade: IBTrade):
        self._log.debug(f"order submitted {trade.order.orderRef}: {trade}")
        client_order_id = ClientOrderId(trade.order.orderRef)
        order: Order = self._cache.order(client_order_id)
        if order.status in (OrderStatus.SUBMITTED,):
            self.generate_order_accepted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=client_order_id,
                venue_order_id=VenueOrderId(str(trade.order.permId)),
                ts_event=dt_to_unix_nanos(trade.log[-1].time),
            )

    def _on_order_modify(self, trade: IBTrade):
        # TODO - NEEDS TESTING
        client_order_id = ClientOrderId(trade.order.orderRef)
        order: Order = self._cache.order(client_order_id)
        instrument: Instrument = self._cache.instrument(order.instrument_id)
        self.generate_order_updated(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=client_order_id,
            venue_order_id=order.venue_order_id,
            quantity=Quantity(trade.order.totalQuantity, precision=instrument.size_precision),
            price=Price(trade.order.lmtPrice, precision=instrument.price_precision),
            trigger_price=None,
            ts_event=dt_to_unix_nanos(trade.log[-1].time),
            venue_order_id_modified=False,  # TODO (bm) - does this happen?
        )

    def _on_order_pending_cancel(self, trade: IBTrade):
        assert trade.orderStatus.status == IBOrderStatus.PendingCancel
        client_order_id = ClientOrderId(trade.order.orderRef)
        order: Order = self._cache.order(client_order_id)
        if trade.orderStatus.status == IBOrderStatus.PendingCancel:
            self.generate_order_pending_cancel(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=client_order_id,
                venue_order_id=order.venue_order_id,
                ts_event=dt_to_unix_nanos(trade.log[-1].time),
            )

    def _on_order_cancelled(self, trade: IBTrade):
        assert trade.orderStatus.status in (IBOrderStatus.Cancelled, IBOrderStatus.ApiCancelled)
        client_order_id = ClientOrderId(trade.order.orderRef)
        order: Order = self._cache.order(client_order_id)
        self.generate_order_canceled(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=client_order_id,
            venue_order_id=order.venue_order_id,
            ts_event=dt_to_unix_nanos(trade.log[-1].time),
        )

    def _on_filled_event(self, trade: IBTrade):
        self._log.debug(f"order filled {trade.order.orderRef}: {trade}")
        self._log.warning(f"fill should be handled in _on_execution_detail {trade.order.orderRef}")

    def _on_execution_detail(self, trade: IBTrade, fill: IBFill):
        self._log.debug(f"_on_execution_detail {trade.order.orderRef}: {trade}")
        if trade.orderStatus.status not in ("Submitted", "Filled"):
            self._log.warning(
                f"Called `_on_execution_detail` without order filled status: {trade.orderStatus.status=}",
            )
            return

        client_order_id = ClientOrderId(trade.order.orderRef)
        order: Order = self._cache.order(client_order_id)
        instrument = self.instrument_provider.find(order.instrument_id)
        trade_id = TradeId(fill.execution.execId)
        venue_order_id = VenueOrderId(str(trade.order.permId))
        order_side = OrderSideParser.from_str_py(trade.order.action.upper())
        order_type = ib_order_to_nautilus_order_type(trade.order)
        last_qty = Quantity(fill.execution.shares, precision=instrument.size_precision)
        last_px = Price(fill.execution.price, precision=instrument.price_precision)
        currency = Currency.from_str(fill.contract.currency)
        commission = Money(fill.commissionReport.commission, currency)
        ts_event = dt_to_unix_nanos(fill.time)
        self.generate_order_filled(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            venue_position_id=None,
            trade_id=trade_id,
            order_side=order_side,
            order_type=order_type,
            last_qty=last_qty,
            last_px=last_px,
            quote_currency=currency,
            commission=commission,
            liquidity_side=LiquiditySide.NONE,
            ts_event=ts_event,
        )

    def on_account_update(self, account_values: list[AccountValue]):
        self._log.debug(str(account_values))
        balances, margins = account_values_to_nautilus_account_info(
            account_values,
            self.account_id.get_id(),
        )
        ts_event: int = self._clock.timestamp_ns()
        self.generate_account_state(
            balances=balances,
            margins=margins,
            reported=True,
            ts_event=ts_event,
        )
