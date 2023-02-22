# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
import json
from decimal import Decimal
from typing import Any, Optional

import pandas as pd
from ibapi.commission_report import CommissionReport
from ibapi.common import UNSET_DECIMAL
from ibapi.common import UNSET_DOUBLE
from ibapi.execution import Execution
from ibapi.order import Order as IBOrder
from ibapi.order_state import OrderState as IBOrderState

from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.client.common import IBPosition
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.parsing.execution import map_order_action
from nautilus_trader.adapters.interactive_brokers.parsing.execution import map_order_fields
from nautilus_trader.adapters.interactive_brokers.parsing.execution import map_order_status
from nautilus_trader.adapters.interactive_brokers.parsing.execution import map_order_type
from nautilus_trader.adapters.interactive_brokers.parsing.execution import map_time_in_force
from nautilus_trader.adapters.interactive_brokers.parsing.execution import map_trigger_method
from nautilus_trader.adapters.interactive_brokers.parsing.execution import (
    order_side_to_order_action,
)
from nautilus_trader.adapters.interactive_brokers.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.rust.common import LogColor
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.limit_if_touched import LimitIfTouchedOrder
from nautilus_trader.model.orders.market_if_touched import MarketIfTouchedOrder
from nautilus_trader.model.orders.stop_limit import StopLimitOrder
from nautilus_trader.model.orders.stop_market import StopMarketOrder
from nautilus_trader.model.orders.trailing_stop_limit import TrailingStopLimitOrder
from nautilus_trader.model.orders.trailing_stop_market import TrailingStopMarketOrder
from nautilus_trader.msgbus.bus import MessageBus


ib_to_nautilus_trigger_method = dict(zip(map_trigger_method.values(), map_trigger_method.keys()))
ib_to_nautilus_time_in_force = dict(zip(map_time_in_force.values(), map_time_in_force.keys()))
ib_to_nautilus_order_side = dict(zip(map_order_action.values(), map_order_action.keys()))
ib_to_nautilus_order_type = dict(zip(map_order_type.values(), map_order_type.keys()))


class InteractiveBrokersExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Interactive Brokers TWS API.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : InteractiveBrokersClient
        The nautilus InteractiveBrokersClient using ibapi.
    account_id: AccountId
        Account ID associated with this client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    instrument_provider : InteractiveBrokersInstrumentProvider
        The instrument provider.
    ibg_client_id : int
        Client ID used to connect TWS/Gateway.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: InteractiveBrokersClient,
        account_id: AccountId,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: InteractiveBrokersInstrumentProvider,
        ibg_client_id: int,
    ):
        super().__init__(
            loop=loop,
            # client_id=ClientId(f"{IB_VENUE.value}-{ibg_client_id:03d}"), # TODO: Fix account_id.get_id()
            client_id=ClientId(f"{IB_VENUE.value}"),
            venue=IB_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.MARGIN,
            base_currency=None,  # IB accounts are multi-currency
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config={
                "name": f"{type(self).__name__}-{ibg_client_id:03d}",
                "client_id": ibg_client_id,
            },
        )
        self._client: InteractiveBrokersClient = client
        self._set_account_id(account_id)
        self._account_summary_tags = {
            "NetLiquidation",
            "FullAvailableFunds",
            "FullInitMarginReq",
            "FullMaintMarginReq",
        }

        self._account_summary_loaded: asyncio.Event = asyncio.Event()

        # Hot caches
        self._account_summary: dict[str, dict[str, Any]] = {}
        self._client_order_id_to_ib_order_id: dict[ClientOrderId, int] = {}

    @property
    def instrument_provider(self) -> InteractiveBrokersInstrumentProvider:
        return self._instrument_provider  # type: ignore

    async def _connect(self):
        # Connect client
        await self._client.is_running_async()
        await self.instrument_provider.initialize()

        # Validate if connected to expected TWS/Gateway using Account
        if self.account_id.get_id() in self._client.accounts():
            self._log.info(
                f"Account `{self.account_id.get_id()}` found in the connected TWS/Gateway.",
                LogColor.GREEN,
            )
        else:
            self.fault()
            raise ValueError(
                f"Account `{self.account_id.get_id()}` not found in the connected TWS/Gateway. "
                f"Available accounts are {self._client.accounts()}",
            )

        # Event hooks
        account = self.account_id.get_id()
        self._client.registered_nautilus_clients.add(account)
        self._client.subscribe_event(f"accountSummary-{account}", self._on_account_summary)
        self._client.subscribe_event(f"openOrder-{account}", self._on_open_order)
        self._client.subscribe_event(f"orderStatus-{account}", self._on_order_status)
        self._client.subscribe_event(f"execDetails-{account}", self._on_exec_details)

        # Load account balance
        self._client.subscribe_account_summary()
        await self._account_summary_loaded.wait()

    async def _disconnect(self):
        self._client.registered_nautilus_clients.remove(self.id)
        if self._client.is_running() and self._client.registered_nautilus_clients == set():
            self._client.stop()

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
    ) -> Optional[OrderStatusReport]:
        self._log.warning("Cannot generate `IBOrderStatusReport`: not yet implemented.")

        return None  # TODO: I see single order can return. What if multiple orders for same Instrument?

    async def generate_order_status_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        report = []
        # Create the Filled OrderStatusReport from Open Positions
        positions: list[IBPosition] = await self._client.get_positions(self.account_id.get_id())
        ts_init = self._clock.timestamp_ns()
        for position in positions:
            self._log.debug(f"Trying OrderStatusReport for {position.contract.conId}")
            if position.quantity > 0:
                order_side = OrderSide.BUY
            elif position.quantity < 0:
                order_side = OrderSide.SELL
            else:
                continue  # Skip, IB may continue to display closed positions

            instrument_id = await self.instrument_provider.find_with_contract_id(
                position.contract.conId,
            )
            quantity = Quantity.from_str(str(abs(position.quantity)))
            order_status = OrderStatusReport(
                account_id=self.account_id,
                instrument_id=instrument_id,
                venue_order_id=VenueOrderId(str(instrument_id.value)),
                order_side=order_side,
                order_type=OrderType.MARKET,
                time_in_force=TimeInForce.FOK,
                order_status=OrderStatus.FILLED,
                quantity=quantity,
                filled_qty=quantity,
                avg_px=Decimal(position.avg_cost),
                report_id=UUID4(),
                ts_accepted=ts_init,
                ts_last=ts_init,
                ts_init=ts_init,
                client_order_id=ClientOrderId(instrument_id.value),
            )
            self._log.debug(f"Received {repr(order_status)}")
            report.append(order_status)

        # Create the Open OrderStatusReport from Open Orders
        ib_orders = await self._client.get_open_orders(self.account_id.get_id())
        for ib_order in ib_orders:
            self._log.debug(f"Trying OrderStatusReport for {ib_order}")
            instrument_id = await self.instrument_provider.find_with_contract_id(
                ib_order.contract.conId,
            )

            # Use quantity ignoring precision
            total_qty = (
                Quantity.from_int(0)
                if ib_order.totalQuantity == UNSET_DECIMAL
                else Quantity.from_str(str(ib_order.totalQuantity))
            )
            filled_qty = (
                Quantity.from_int(0)
                if ib_order.filledQuantity == UNSET_DECIMAL
                else Quantity.from_str(str(ib_order.filledQuantity))
            )
            if total_qty.as_double() > filled_qty.as_double() > 0:
                order_status = OrderStatus.PARTIALLY_FILLED
            else:
                order_status = map_order_status[ib_order.order_state.status]
            ts_init = self._clock.timestamp_ns()
            price = (
                None
                if ib_order.lmtPrice == UNSET_DOUBLE
                else Price.from_str(str(ib_order.lmtPrice))
            )
            # TODO: Testing for advanced Open orders
            order_status = OrderStatusReport(
                account_id=self.account_id,
                instrument_id=instrument_id,
                venue_order_id=VenueOrderId(str(ib_order.orderId)),
                order_side=ib_to_nautilus_order_side[ib_order.action],
                order_type=ib_to_nautilus_order_type[ib_order.orderType],
                time_in_force=ib_to_nautilus_time_in_force[ib_order.tif],
                order_status=order_status,
                quantity=total_qty,
                filled_qty=filled_qty,
                report_id=UUID4(),
                ts_accepted=ts_init,
                ts_last=ts_init,
                ts_init=ts_init,
                client_order_id=ClientOrderId(ib_order.orderRef),
                # order_list_id=,
                # contingency_type=,
                # expire_time=,
                price=price,
                trigger_price=Price.from_str(str(ib_order.auxPrice)),
                trigger_type=TriggerType.BID_ASK,
                # limit_offset=,
                # trailing_offset=,
            )
            self._log.debug(f"Received {repr(order_status)}")
            report.append(order_status)
        return report

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
        report = []
        positions: list[IBPosition] = await self._client.get_positions(self.account_id.get_id())
        for position in positions:
            self._log.debug(f"Trying PositionStatusReport for {position.contract.conId}")
            if position.quantity > 0:
                side = PositionSide.LONG
            elif position.quantity < 0:
                side = PositionSide.SHORT
            else:
                continue  # Skip, IB may continue to display closed positions

            instrument_id = await self.instrument_provider.find_with_contract_id(
                position.contract.conId,
            )
            position_status = PositionStatusReport(
                account_id=self.account_id,
                instrument_id=instrument_id,
                position_side=side,
                quantity=Quantity.from_str(str(abs(position.quantity))),
                report_id=UUID4(),
                ts_last=self._clock.timestamp_ns(),
                ts_init=self._clock.timestamp_ns(),
            )
            self._log.debug(f"Received {repr(position_status)}")
            report.append(position_status)
        return report

    def _transform_order(self, order: Order) -> IBOrder:
        ib_order = IBOrder()
        for key, field, fn in map_order_fields:
            if value := getattr(order, key, None):
                setattr(ib_order, field, fn(value))

        if isinstance(order, TrailingStopLimitOrder) or isinstance(order, TrailingStopMarketOrder):
            ib_order.auxPrice = float(order.trailing_offset)
            if order.trigger_price:
                ib_order.trailStopPrice = order.trigger_price.as_double()
                ib_order.triggerMethod = map_trigger_method[order.trigger_type]
        elif (
            isinstance(order, MarketIfTouchedOrder)
            or isinstance(order, LimitIfTouchedOrder)
            or isinstance(order, StopLimitOrder)
            or isinstance(order, StopMarketOrder)
        ):
            if order.trigger_price:
                ib_order.auxPrice = order.trigger_price.as_double()

        details = self.instrument_provider.contract_details[order.instrument_id.value]
        ib_order.contract = details.contract
        ib_order.account = self.account_id.get_id()
        ib_order.clearingAccount = self.account_id.get_id()

        if order.tags:
            return self._attach_order_tags(ib_order, order)
        else:
            return ib_order

    def _attach_order_tags(self, ib_order: IBOrder, order: Order) -> IBOrder:
        try:
            tags: dict = json.loads(order.tags)
            for tag in tags:
                if tag == "conditions":
                    for condition in tags[tag]:
                        pass  # TODO:
                else:
                    setattr(ib_order, tag, tags[tag])
            return ib_order
        except (json.JSONDecodeError, TypeError):
            self._log.warning(
                f"{order.client_order_id} {order.tags=} ignored, must be valid IBOrderTags.value",
            )

    async def _submit_order(self, command: SubmitOrder) -> None:
        PyCondition.type(command, SubmitOrder, "command")

        # Reject the non-compliant orders.
        # These conditions are based on available info and can be relaxed if there is use case.
        reject_reason = None
        if getattr(command.order, "trailing_offset_type", None) not in [
            TrailingOffsetType.PRICE,
            None,
        ]:
            reject_reason = f"{repr(command.order.trailing_offset_type)} not implemented"
        elif getattr(command.order, "is_post_only", None) is True:
            reject_reason = (
                "post_only=True, `Marketing making` not supported by InteractiveBrokers."
            )
        if reject_reason:
            self._handle_order_event(
                status=OrderStatus.REJECTED,
                order=command.order,
                reason=reject_reason,
            )
            return

        ib_order: IBOrder = self._transform_order(command.order)
        ib_order.orderId = self._client.next_order_id()
        self._client_order_id_to_ib_order_id[ib_order.orderRef] = ib_order.orderId
        self._client.place_order(ib_order)
        self._handle_order_event(status=OrderStatus.SUBMITTED, order=command.order)

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        PyCondition.type(command, SubmitOrderList, "command")

        order_id_map = {}
        client_id_to_orders = {}
        ib_orders = []

        # Translate orders
        for order in command.order_list.orders:
            order_id_map[order.client_order_id.value] = self._client.next_order_id()
            client_id_to_orders[order.client_order_id.value] = order

            ib_order = self._transform_order(order)
            ib_order.transmit = False
            ib_order.orderId = order_id_map[order.client_order_id.value]
            ib_orders.append(ib_order)

        # Mark last order to transmit
        ib_orders[-1].transmit = True

        for ib_order in ib_orders:
            # Map the Parent Order Ids
            if parent_id := order_id_map.get(ib_order.parentId):
                ib_order.parentId = parent_id
            self._client_order_id_to_ib_order_id[ib_order.orderRef] = ib_order.orderId
            # Place orders
            order_ref = ib_order.orderRef
            self._client.place_order(ib_order)
            self._handle_order_event(
                status=OrderStatus.SUBMITTED,
                order=client_id_to_orders[order_ref],
            )

    async def _modify_order(self, command: ModifyOrder) -> None:
        PyCondition.not_none(command, "command")
        if not (command.quantity or command.price):
            return

        nautilus_order: Order = self._cache.order(command.client_order_id)
        ib_order: IBOrder = self._transform_order(nautilus_order)
        ib_order.orderId = int(command.venue_order_id.value)
        if command.quantity and command.quantity != ib_order.totalQuantity:
            ib_order.totalQuantity = command.quantity.as_double()
        if command.price and command.price.as_double() != getattr(ib_order, "lmtPrice", None):
            ib_order.lmtPrice = command.price.as_double()
        self._client.place_order(ib_order)

    async def _cancel_order(self, command: CancelOrder) -> None:
        PyCondition.not_none(command, "command")

        venue_order_id = (
            command.venue_order_id
        )  # or self._client_order_id_to_ib_order_id.get(command.client_order_id.value)
        self._client.cancel_order(venue_order_id)

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        for odrder in self._cache.orders_open(
            instrument_id=command.instrument_id,
        ):  # TODO: Query orders from IB
            venue_order_id = (
                odrder.venue_order_id
            )  # or self._client_order_id_to_ib_order_id.get(command.client_order_id.value)
            self._client.cancel_order(venue_order_id)

    def _on_account_summary(self, tag: str, value: str, currency: str):
        if not self._account_summary.get(currency):
            self._account_summary[currency] = {}
        try:
            self._account_summary[currency][tag] = float(value)
        except ValueError:
            self._account_summary[currency][tag] = value

        for currency in self._account_summary.keys():
            if not currency:
                continue
            if self._account_summary_tags - set(self._account_summary[currency].keys()) == set():
                free = max(
                    self._account_summary[currency]["FullAvailableFunds"],
                    0,
                )  # Maybe negative
                locked = self._account_summary[currency]["FullMaintMarginReq"]
                self._log.info(f"{free=}, {locked=}", LogColor.GREEN)
                account_balance = AccountBalance(
                    total=Money(free + locked, Currency.from_str(currency)),
                    free=Money(free, Currency.from_str(currency)),
                    locked=Money(locked, Currency.from_str(currency)),
                )

                margin_balance = MarginBalance(
                    initial=Money(
                        self._account_summary[currency]["FullInitMarginReq"],
                        currency=Currency.from_str(currency),
                    ),
                    maintenance=Money(
                        self._account_summary[currency]["FullMaintMarginReq"],
                        currency=Currency.from_str(currency),
                    ),
                )

                self.generate_account_state(
                    balances=[account_balance],
                    margins=[margin_balance],
                    reported=True,
                    ts_event=self._clock.timestamp_ns(),
                )

                # Store all available fields to Cache (for now until permanent solution)
                self._cache.add(
                    f"accountSummary:{self.account_id.get_id()}",
                    json.dumps(self._account_summary).encode("utf-8"),
                )

        self._account_summary_loaded.set()

    def _handle_order_event(
        self,
        status: OrderStatus,
        order: Order,
        order_id: int = None,
        reason: str = "",
    ):
        if status == OrderStatus.SUBMITTED:
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                # TODO: We should have VenueOrderId here to record as soon submit event
                # venue_order_id=VenueOrderId(str(order_id)),
                ts_event=self._clock.timestamp_ns(),
            )
        elif status == OrderStatus.ACCEPTED:
            if order.status != OrderStatus.ACCEPTED:
                self.generate_order_accepted(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=VenueOrderId(str(order_id)),
                    ts_event=self._clock.timestamp_ns(),
                )
            else:
                self._log.debug(f"{order.client_order_id} already accepted.")
        elif status == OrderStatus.CANCELED:
            self.generate_order_canceled(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                ts_event=self._clock.timestamp_ns(),
            )
        elif status == OrderStatus.REJECTED:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=reason,
                ts_event=self._clock.timestamp_ns(),
            )

    def _on_open_order(self, order_ref: str, order: IBOrder, order_state: IBOrderState):
        if not order.orderRef:
            self._log.warning(
                f"ClientOrderId not available, order={order.__dict__}, state={order_state.__dict__}",
            )
            return
        if not (nautilus_order := self._cache.order(ClientOrderId(order_ref))):
            self._log.warning(
                f"ClientOrderId not found in Cache, order={order.__dict__}, state={order_state.__dict__}",
            )
            return

        if order.whatIf and order_state.status == "PreSubmitted":
            # TODO: Is there more better approach for this use case?
            # This tells the details about Pre and Post margin changes, user can request by setting whatIf flag
            # order will not be placed by IB and instead returns simulation.
            # example={'status': 'PreSubmitted', 'initMarginBefore': '52.88', 'maintMarginBefore': '52.88', 'equityWithLoanBefore': '23337.31', 'initMarginChange': '2517.5099999999998', 'maintMarginChange': '2517.5099999999998', 'equityWithLoanChange': '-0.6200000000026193', 'initMarginAfter': '2570.39', 'maintMarginAfter': '2570.39', 'equityWithLoanAfter': '23336.69', 'commission': 2.12362, 'minCommission': 1.7976931348623157e+308, 'maxCommission': 1.7976931348623157e+308, 'commissionCurrency': 'USD', 'warningText': '', 'completedTime': '', 'completedStatus': ''}  # noqa
            self._handle_order_event(
                status=OrderStatus.REJECTED,
                order=nautilus_order,
                reason=json.dumps({"whatIf": order_state.__dict__}),
            )
        elif nautilus_order.status != OrderStatus.ACCEPTED and order_state.status in [
            "PreSubmitted",
            "Submitted",
        ]:
            self._handle_order_event(
                status=OrderStatus.ACCEPTED,
                order=nautilus_order,
                order_id=order.orderId,
            )

    def _on_order_status(self, order_ref: str, order_status: str, reason: str = ""):
        if order_status in ["ApiCancelled", "Cancelled"]:
            status = OrderStatus.CANCELED
        elif order_status == "Rejected":
            status = OrderStatus.REJECTED
        else:
            return

        nautilus_order = self._cache.order(ClientOrderId(order_ref))
        if nautilus_order:
            self._handle_order_event(
                status=status,
                order=nautilus_order,
                reason=reason,
            )

    def _on_exec_details(
        self,
        order_ref: str,
        execution: Execution,
        commission_report: CommissionReport,
    ):
        if not execution.orderRef:
            self._log.warning(f"ClientOrderId not available, order={execution.__dict__}")
            return
        if not (nautilus_order := self._cache.order(ClientOrderId(order_ref))):
            self._log.warning(f"ClientOrderId not found in Cache, order={execution.__dict__}")
            return

        instrument = self.instrument_provider.find(nautilus_order.instrument_id)
        dt, tz = execution.time.rsplit(" ", 1)  # 20230223 00:43:36 America/New_York TWS >= v10.19

        self.generate_order_filled(
            strategy_id=nautilus_order.strategy_id,
            instrument_id=nautilus_order.instrument_id,
            client_order_id=nautilus_order.client_order_id,
            venue_order_id=VenueOrderId(str(execution.orderId)),
            venue_position_id=None,
            trade_id=TradeId(execution.execId),
            order_side=OrderSide[order_side_to_order_action[execution.side]],
            order_type=nautilus_order.order_type,
            last_qty=Quantity(execution.shares, precision=instrument.size_precision),
            last_px=Price(execution.price, precision=instrument.price_precision),
            quote_currency=instrument.quote_currency,
            commission=Money(
                commission_report.commission,
                Currency.from_str(commission_report.currency),
            ),
            liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
            ts_event=pd.Timestamp(dt, tz=tz).value,
        )
