# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Any

from ibapi.commission_report import CommissionReport
from ibapi.const import UNSET_DECIMAL
from ibapi.const import UNSET_DOUBLE
from ibapi.execution import Execution
from ibapi.order import Order as IBOrder
from ibapi.order_state import OrderState as IBOrderState

# fmt: off
from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.client.common import IBPosition
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.parsing.execution import MAP_ORDER_ACTION
from nautilus_trader.adapters.interactive_brokers.parsing.execution import MAP_ORDER_FIELDS
from nautilus_trader.adapters.interactive_brokers.parsing.execution import MAP_ORDER_STATUS
from nautilus_trader.adapters.interactive_brokers.parsing.execution import MAP_ORDER_TYPE
from nautilus_trader.adapters.interactive_brokers.parsing.execution import MAP_TIME_IN_FORCE
from nautilus_trader.adapters.interactive_brokers.parsing.execution import MAP_TRIGGER_METHOD
from nautilus_trader.adapters.interactive_brokers.parsing.execution import ORDER_SIDE_TO_ORDER_ACTION
from nautilus_trader.adapters.interactive_brokers.parsing.execution import timestring_to_timestamp
from nautilus_trader.adapters.interactive_brokers.parsing.price_conversion import ib_price_to_nautilus_price
from nautilus_trader.adapters.interactive_brokers.parsing.price_conversion import nautilus_price_to_ib_price
from nautilus_trader.adapters.interactive_brokers.providers import InteractiveBrokersInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.rust.common import LogColor
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import trailing_offset_type_to_str
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
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


# fmt: on

ib_to_nautilus_trigger_method = dict(
    zip(MAP_TRIGGER_METHOD.values(), MAP_TRIGGER_METHOD.keys(), strict=False),
)
ib_to_nautilus_time_in_force = dict(
    zip(MAP_TIME_IN_FORCE.values(), MAP_TIME_IN_FORCE.keys(), strict=False),
)
ib_to_nautilus_order_side = dict(
    zip(MAP_ORDER_ACTION.values(), MAP_ORDER_ACTION.keys(), strict=False),
)
ib_to_nautilus_order_type = dict(zip(MAP_ORDER_TYPE.values(), MAP_ORDER_TYPE.keys(), strict=False))


class InteractiveBrokersExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Interactive Brokers TWS API, allowing for the
    retrieval of account information and execution of orders.

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
    instrument_provider : InteractiveBrokersInstrumentProvider
        The instrument provider.
    config : InteractiveBrokersExecClientConfig, optional
        The configuration for the instance.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: InteractiveBrokersClient,
        account_id: AccountId,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: InteractiveBrokersInstrumentProvider,
        config: InteractiveBrokersExecClientConfig,
        name: str | None = None,
        connection_timeout: int = 300,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or f"{IB_VENUE.value}"),
            venue=IB_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.MARGIN,
            base_currency=None,  # IB accounts are multi-currency
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        # Track known positions to detect external changes (like option exercises)
        self._known_positions: dict[int, Decimal] = {}  # conId -> quantity
        self._connection_timeout = connection_timeout
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

        # Track processed fill IDs
        self._spread_fill_tracking: dict[ClientOrderId, set[str]] = {}

        # Track average fill prices for orders
        self._order_avg_prices: dict[ClientOrderId, Price] = {}

    @property
    def instrument_provider(self) -> InteractiveBrokersInstrumentProvider:
        return self._instrument_provider  # type: ignore

    async def _connect(self):
        # Connect client
        await self._client.wait_until_ready(self._connection_timeout)
        await self.instrument_provider.initialize()

        # Set instrument provider on client for price magnifier access
        self._client._instrument_provider = self._instrument_provider

        # Validate if connected to expected TWS/Gateway using Account
        if self.account_id.get_id() in self._client.accounts():
            self._log.info(
                f"Account `{self.account_id.get_id()}` found in the connected TWS/Gateway",
                LogColor.GREEN,
            )
        else:
            self.fault()
            raise ValueError(
                f"Account `{self.account_id.get_id()}` not found in the connected TWS/Gateway: "
                f"available accounts are {self._client.accounts()}",
            )

        # Event hooks
        account = self.account_id.get_id()
        self._client.registered_nautilus_clients.add(self.id)
        self._client.subscribe_event(f"accountSummary-{account}", self._on_account_summary)
        self._client.subscribe_event(f"openOrder-{account}", self._on_open_order)
        self._client.subscribe_event(f"orderStatus-{account}", self._on_order_status)
        self._client.subscribe_event(f"execDetails-{account}", self._on_exec_details)
        self._client.subscribe_event(f"positionUpdate-{account}", self._on_position_update)

        # Load account balance
        self._client.subscribe_account_summary()
        await self._account_summary_loaded.wait()

        # Initialize known positions tracking to avoid duplicates from execDetails
        await self._initialize_position_tracking()

        # Subscribe to real-time position updates for external changes (option exercises)
        self._client.subscribe_positions()

        self._set_connected(True)

    async def _disconnect(self):
        self._client.registered_nautilus_clients.discard(self.id)

        # Unsubscribe from position updates when disconnecting
        if self._client.is_running:
            self._client.unsubscribe_positions()

        if self._client.is_running and self._client.registered_nautilus_clients == set():
            self._client.stop()

        self._set_connected(False)

    async def _initialize_position_tracking(self) -> None:
        """
        Initialize position tracking to avoid processing duplicates from execDetails.
        """
        try:
            positions = await self._client.get_positions(self.account_id.get_id())

            if positions:
                for position in positions:
                    self._known_positions[position.contract.conId] = position.quantity

                self._log.info(f"Initialized tracking for {len(positions)} existing positions")
        except Exception as e:
            self._log.warning(f"Failed to initialize position tracking: {e}")

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        PyCondition.type_or_none(command.client_order_id, ClientOrderId, "client_order_id")
        PyCondition.type_or_none(command.venue_order_id, VenueOrderId, "venue_order_id")

        if not (command.client_order_id or command.venue_order_id):
            self._log.debug("Both `client_order_id` and `venue_order_id` cannot be None")
            return None

        report = None
        ib_orders = await self._client.get_open_orders(self.account_id.get_id())

        for ib_order in ib_orders:
            if (command.client_order_id and command.client_order_id.value == ib_order.orderRef) or (
                command.venue_order_id
                and command.venue_order_id.value
                == str(
                    ib_order.orderId,
                )
            ):
                report = await self._parse_ib_order_to_order_status_report(ib_order)
                break

        if report is None:
            self._log.warning(
                f"Order {command.client_order_id=}, {command.venue_order_id} not found, canceling",
            )
            self._on_order_status(
                order_ref=command.client_order_id.value,
                order_status="Cancelled",
                reason="Not found in query",
            )

        return report

    async def _parse_ib_order_to_order_status_report(self, ib_order: IBOrder) -> OrderStatusReport:
        self._log.debug(f"Trying OrderStatusReport for {ib_order.__dict__}")
        instrument = await self.instrument_provider.get_instrument(ib_order.contract)
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
            order_status = MAP_ORDER_STATUS[ib_order.order_state.status]

        ts_init = self._clock.timestamp_ns()

        price_magnifier = self.instrument_provider.get_price_magnifier(instrument.id)
        price = None

        if ib_order.lmtPrice != UNSET_DOUBLE:
            converted_price = ib_price_to_nautilus_price(ib_order.lmtPrice, price_magnifier)
            price = instrument.make_price(converted_price)

        expire_time = (
            timestring_to_timestamp(ib_order.goodTillDate) if ib_order.tif == "GTD" else None
        )
        mapped_order_type_info = ib_to_nautilus_order_type[ib_order.orderType]

        if isinstance(mapped_order_type_info, tuple):
            order_type, time_in_force = mapped_order_type_info
        else:
            order_type = mapped_order_type_info
            time_in_force = ib_to_nautilus_time_in_force[ib_order.tif]

        order_status = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=instrument.id,
            venue_order_id=VenueOrderId(str(ib_order.orderId)),
            order_side=ib_to_nautilus_order_side[ib_order.action],
            order_type=order_type,
            time_in_force=time_in_force,
            order_status=order_status,
            quantity=total_qty,
            filled_qty=Quantity.from_int(0),
            avg_px=Decimal(0),
            report_id=UUID4(),
            ts_accepted=ts_init,
            ts_last=ts_init,
            ts_init=ts_init,
            client_order_id=ClientOrderId(ib_order.orderRef),
            # order_list_id=,
            # contingency_type=,
            expire_time=expire_time,
            price=price,
            trigger_price=(
                instrument.make_price(
                    ib_price_to_nautilus_price(ib_order.auxPrice, price_magnifier),
                )
                if ib_order.auxPrice != UNSET_DOUBLE
                else None
            ),
            trigger_type=TriggerType.BID_ASK,
            # limit_offset=,
            # trailing_offset=,
        )
        self._log.debug(f"Received {order_status!r}")

        return order_status

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        report = []

        # Create the Filled OrderStatusReport from Open Positions
        positions: list[IBPosition] = await self._client.get_positions(
            self.account_id.get_id(),
        )

        if not positions:
            return []

        ts_init = self._clock.timestamp_ns()

        for position in positions:
            self._log.debug(
                f"Infer OrderStatusReport from open position {position.contract}",
            )

            if position.quantity > 0:
                order_side = OrderSide.BUY
            elif position.quantity < 0:
                order_side = OrderSide.SELL
            else:
                continue  # Skip, IB may continue to display closed positions

            instrument = await self.instrument_provider.get_instrument(position.contract)

            if instrument is None:
                self._log.error(
                    f"Cannot generate report: instrument not found for contract ID {position.contract.conId}",
                )
                continue

            contract_details = self.instrument_provider.contract_details[instrument.id]
            avg_px = instrument.make_price(
                position.avg_cost
                / (instrument.multiplier.as_double() * contract_details.priceMagnifier),
            ).as_decimal()
            quantity = Quantity.from_str(str(position.quantity.copy_abs()))
            order_status = OrderStatusReport(
                account_id=self.account_id,
                instrument_id=instrument.id,
                venue_order_id=VenueOrderId(instrument.id.value),
                order_side=order_side,
                order_type=OrderType.MARKET,
                time_in_force=TimeInForce.FOK,
                order_status=OrderStatus.FILLED,
                quantity=quantity,
                filled_qty=quantity,
                avg_px=avg_px,
                report_id=UUID4(),
                ts_accepted=ts_init,
                ts_last=ts_init,
                ts_init=ts_init,
                client_order_id=ClientOrderId(instrument.id.value),
            )
            self._log.debug(f"Received {order_status!r}")
            report.append(order_status)

        # Create the Open OrderStatusReport from Open Orders
        ib_orders: list[IBOrder] = await self._client.get_open_orders(
            self.account_id.get_id(),
        )

        for ib_order in ib_orders:
            order_status = await self._parse_ib_order_to_order_status_report(ib_order)
            report.append(order_status)

        return report

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        self._log.warning("Cannot generate `list[FillReport]`: not yet implemented")
        return []  # TODO: Implement

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        report = []
        positions: list[IBPosition] | None = await self._client.get_positions(
            self.account_id.get_id(),
        )

        if not positions:
            return []

        for position in positions:
            self._log.debug(f"Trying PositionStatusReport for {position.contract.conId}")

            if position.quantity > 0:
                side = PositionSide.LONG
            elif position.quantity < 0:
                side = PositionSide.SHORT
            else:
                continue  # Skip, IB may continue to display closed positions

            instrument = await self.instrument_provider.get_instrument(position.contract)

            if instrument is None:
                self._log.error(
                    f"Cannot generate report: instrument not found for contract ID {position.contract.conId}",
                )
                continue

            if not self._cache.instrument(instrument.id):
                self._handle_data(instrument)

            # Convert avg_cost to Price if available
            avg_px_open = None
            if position.avg_cost and position.avg_cost > 0:
                avg_px_open = instrument.make_price(position.avg_cost)

            position_status = PositionStatusReport(
                account_id=self.account_id,
                instrument_id=instrument.id,
                position_side=side,
                quantity=Quantity.from_str(str(abs(position.quantity))),
                avg_px_open=avg_px_open,
                report_id=UUID4(),
                ts_last=self._clock.timestamp_ns(),
                ts_init=self._clock.timestamp_ns(),
            )
            self._log.debug(f"Received {position_status!r}")
            report.append(position_status)

        return report

    def _transform_order_to_ib_order(self, order: Order) -> IBOrder:  # noqa: C901 11 > 10
        if order.is_post_only:
            raise ValueError("`post_only` not supported by Interactive Brokers")

        ib_order = IBOrder()
        time_in_force = order.time_in_force
        price_magnifier = self.instrument_provider.get_price_magnifier(order.instrument_id)

        for key, field, fn in MAP_ORDER_FIELDS:
            if value := getattr(order, key, None):
                if key == "order_type" and time_in_force == TimeInForce.AT_THE_CLOSE:
                    setattr(ib_order, field, fn((value, time_in_force)))
                elif key == "price" and value is not None:
                    converted_price = nautilus_price_to_ib_price(value.as_double(), price_magnifier)
                    setattr(ib_order, field, converted_price)
                else:
                    setattr(ib_order, field, fn(value))

        if self.instrument_provider.find(order.instrument_id).is_inverse:
            ib_order.cashQty = int(ib_order.totalQuantity)
            ib_order.totalQuantity = 0

        if isinstance(order, TrailingStopLimitOrder | TrailingStopMarketOrder):
            if order.trailing_offset_type != TrailingOffsetType.PRICE:
                raise ValueError(
                    f"`TrailingOffsetType` {trailing_offset_type_to_str(order.trailing_offset_type)} is not supported",
                )

            ib_order.auxPrice = float(order.trailing_offset)

            if order.trigger_price:
                converted_trigger_price = nautilus_price_to_ib_price(
                    order.trigger_price.as_double(),
                    price_magnifier,
                )
                ib_order.trailStopPrice = converted_trigger_price
                ib_order.triggerMethod = MAP_TRIGGER_METHOD[order.trigger_type]
        elif (
            isinstance(
                order,
                MarketIfTouchedOrder | LimitIfTouchedOrder | StopLimitOrder | StopMarketOrder,
            )
        ) and order.trigger_price:
            converted_aux_price = nautilus_price_to_ib_price(
                order.trigger_price.as_double(),
                price_magnifier,
            )
            ib_order.auxPrice = converted_aux_price

        if order.instrument_id.is_spread():
            bag_contract = self.instrument_provider.contract.get(order.instrument_id)

            if not bag_contract:
                raise ValueError(
                    f"No BAG contract found for spread instrument {order.instrument_id}",
                )

            ib_order.contract = bag_contract
        else:
            details = self.instrument_provider.contract_details[order.instrument_id]
            ib_order.contract = details.contract

        ib_order.account = self.account_id.get_id()
        ib_order.clearingAccount = self.account_id.get_id()

        # Handle OCA (One-Cancels-All) settings - check order tags first, then contingency types
        oca_group_from_tags = None
        oca_type_from_tags = None

        # Check if OCA settings are specified in order tags
        if order.tags:
            for tag in order.tags:
                if tag.startswith("IBOrderTags:"):
                    try:
                        tags_dict = IBOrderTags.parse(tag.replace("IBOrderTags:", "")).dict()

                        if tags_dict.get("ocaGroup"):
                            oca_group_from_tags = tags_dict["ocaGroup"]

                        # Get ocaType from tags, including 0 as a valid value
                        if "ocaType" in tags_dict:
                            oca_type_from_tags = tags_dict["ocaType"]
                    except Exception as e:
                        self._log.warning(f"Failed to parse IBOrderTags: {e}")

        # Apply OCA settings from tags if specified
        if oca_group_from_tags:
            ib_order.ocaGroup = oca_group_from_tags

            # If ocaType is explicitly set in tags (even to 0), use it; otherwise default to 1
            if oca_type_from_tags is not None and oca_type_from_tags > 0:
                ib_order.ocaType = oca_type_from_tags
            else:
                ib_order.ocaType = 1  # Default to type 1 for safety

            self._log.info(
                f"Setting OCA from tags - Group: {oca_group_from_tags}, Type: {ib_order.ocaType}",
            )
        # Otherwise, handle OCO/OUO contingency types automatically for bracket orders
        elif (
            order.contingency_type in (ContingencyType.OCO, ContingencyType.OUO)
            and order.linked_order_ids
            and order.parent_order_id
        ):  # Child orders in bracket

            # Generate unique OCA group identifier using order list ID
            if order.order_list_id:
                oca_group = f"OCA_{order.order_list_id.value}"
            else:
                # Fallback to using parent order ID for consistency
                oca_group = f"OCA_{order.parent_order_id.value}"

            ib_order.ocaGroup = oca_group
            # Use OCA type 1 (cancel all remaining orders with block) for safety
            ib_order.ocaType = 1

            self._log.info(
                f"Setting {order.contingency_type.name} order {order.client_order_id} to OCA group: {oca_group}",
            )

        if order.tags:
            return self._attach_order_tags(ib_order, order)
        else:
            return ib_order

    def _attach_order_tags(self, ib_order: IBOrder, order: Order) -> IBOrder:
        tags: dict = {}

        for ot in order.tags:
            if ot.startswith("IBOrderTags:"):
                tags = IBOrderTags.parse(ot.replace("IBOrderTags:", "")).dict()
                break

        for tag in tags:
            if tag == "conditions":
                for condition in tags[tag]:
                    pass  # TODO:
            elif tag in ("ocaGroup", "ocaType"):
                # Skip OCA tags as they're handled in the main transformation logic
                continue
            else:
                setattr(ib_order, tag, tags[tag])

        return ib_order

    async def _submit_order(self, command: SubmitOrder) -> None:
        PyCondition.type(command, SubmitOrder, "command")

        try:
            ib_order: IBOrder = self._transform_order_to_ib_order(command.order)
            ib_order.orderId = self._client.next_order_id()
            self._client.place_order(ib_order)
            self._handle_order_event(status=OrderStatus.SUBMITTED, order=command.order)
        except ValueError as e:
            self._handle_order_event(
                status=OrderStatus.REJECTED,
                order=command.order,
                reason=str(e),
            )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        PyCondition.type(command, SubmitOrderList, "command")

        order_id_map = {}
        client_id_to_orders = {}
        ib_orders = []

        # Translate orders
        for order in command.order_list.orders:
            order_id_map[order.client_order_id.value] = self._client.next_order_id()
            client_id_to_orders[order.client_order_id.value] = order

            try:
                ib_order = self._transform_order_to_ib_order(order)
                ib_order.transmit = False
                ib_order.orderId = order_id_map[order.client_order_id.value]
                ib_orders.append(ib_order)
            except ValueError as e:
                # All orders in the list are declined to prevent unintended side effects
                for o in command.order_list.orders:
                    if o == order:
                        self._handle_order_event(
                            status=OrderStatus.REJECTED,
                            order=o,
                            reason=str(e),
                        )
                    else:
                        self._handle_order_event(
                            status=OrderStatus.REJECTED,
                            order=o,
                            reason=f"The order has been rejected due to the rejection of the order with "
                            f"{order.client_order_id!r} in the list",
                        )

                return

        # Mark last order to transmit
        ib_orders[-1].transmit = True

        for ib_order in ib_orders:
            # Map the Parent Order Ids
            if parent_id := order_id_map.get(ib_order.parentId):
                ib_order.parentId = parent_id

            # Place orders
            order_ref = ib_order.orderRef
            self._client.place_order(ib_order)
            self._handle_order_event(
                status=OrderStatus.SUBMITTED,
                order=client_id_to_orders[order_ref],
            )

    async def _modify_order(self, command: ModifyOrder) -> None:
        PyCondition.not_none(command, "command")
        if not (command.quantity or command.price or command.trigger_price):
            return

        nautilus_order: Order = self._cache.order(command.client_order_id)
        self._log.info(f"Nautilus order status is {nautilus_order.status_string()}")

        try:
            ib_order: IBOrder = self._transform_order_to_ib_order(nautilus_order)
        except ValueError as e:
            self._handle_order_event(
                status=OrderStatus.REJECTED,
                order=command.order,
                reason=str(e),
            )
            return

        ib_order.orderId = int(command.venue_order_id.value)

        if ib_order.parentId:
            parent_nautilus_order = self._cache.order(ClientOrderId(ib_order.parentId))

            if parent_nautilus_order:
                ib_order.parentId = int(parent_nautilus_order.venue_order_id.value)
            else:
                ib_order.parentId = 0

        if command.quantity and command.quantity != ib_order.totalQuantity:
            ib_order.totalQuantity = command.quantity.as_double()

        price_magnifier = self.instrument_provider.get_price_magnifier(command.instrument_id)

        if command.price and command.price.as_double() != getattr(ib_order, "lmtPrice", None):
            converted_price = nautilus_price_to_ib_price(command.price.as_double(), price_magnifier)
            ib_order.lmtPrice = converted_price

        if command.trigger_price and command.trigger_price.as_double() != getattr(
            ib_order,
            "auxPrice",
            None,
        ):
            converted_trigger_price = nautilus_price_to_ib_price(
                command.trigger_price.as_double(),
                price_magnifier,
            )
            ib_order.auxPrice = converted_trigger_price

        self._log.info(f"Placing {ib_order!r}")
        self._client.place_order(ib_order)

    async def _cancel_order(self, command: CancelOrder) -> None:
        PyCondition.not_none(command, "command")

        venue_order_id = command.venue_order_id

        if venue_order_id:
            self._client.cancel_order(int(venue_order_id.value))
        else:
            self._log.error(f"VenueOrderId not found for {command.client_order_id}")

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        for order in self._cache.orders_open(
            instrument_id=command.instrument_id,
        ):
            venue_order_id = order.venue_order_id

            if venue_order_id:
                self._client.cancel_order(int(venue_order_id.value))
            else:
                self._log.error(f"VenueOrderId not found for {order.client_order_id}")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        for order in command.cancels:
            await self._cancel_order(order)

    def _on_account_summary(self, tag: str, value: str, currency: str) -> None:
        if not self._account_summary.get(currency):
            self._account_summary[currency] = {}

        try:
            self._account_summary[currency][tag] = float(value)
        except ValueError:
            self._account_summary[currency][tag] = value

        for currency in self._account_summary:
            if not currency:
                continue

            if self._account_summary_tags - set(self._account_summary[currency].keys()) == set():
                self._log.info(f"{self._account_summary}", LogColor.GREEN)
                # free = self._account_summary[currency]["FullAvailableFunds"]
                locked = self._account_summary[currency]["FullMaintMarginReq"]
                total = self._account_summary[currency]["NetLiquidation"]

                if total - locked < locked:
                    total = 400000  # TODO: Bug; Cannot recalculate balance when no current balance

                free = total - locked
                account_balance = AccountBalance(
                    total=Money(total, Currency.from_str(currency)),
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

    def _handle_order_event(  # noqa: C901
        self,
        status: OrderStatus,
        order: Order,
        order_id: int | None = None,
        reason: str = "",
    ) -> None:
        if status == OrderStatus.SUBMITTED:
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
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
                self._log.debug(f"Order {order.client_order_id} already accepted")
        elif status == OrderStatus.FILLED:
            if order.status != OrderStatus.FILLED:
                # TODO: self.generate_order_filled
                self._log.debug(f"Order {order.client_order_id} is filled")
        elif status == OrderStatus.PENDING_CANCEL:
            # TODO: self.generate_order_pending_cancel
            self._log.warning(f"Order {order.client_order_id} is {status.name}")
        elif status == OrderStatus.CANCELED:
            if order.status != OrderStatus.CANCELED:
                self.generate_order_canceled(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    ts_event=self._clock.timestamp_ns(),
                )
        elif status == OrderStatus.REJECTED:
            if order.status != OrderStatus.REJECTED:
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=reason,
                    ts_event=self._clock.timestamp_ns(),
                )
        else:
            self._log.warning(
                f"Order {order.client_order_id} with status={status.name} is unknown or "
                "not yet implemented",
            )

    async def handle_order_status_report(self, ib_order: IBOrder) -> None:
        report = await self._parse_ib_order_to_order_status_report(ib_order)
        self._send_order_status_report(report)

    def _on_open_order(self, order_ref: str, order: IBOrder, order_state: IBOrderState) -> None:
        if not order.orderRef:
            self._log.warning(
                f"ClientOrderId not available, order={order.__dict__}, state={order_state.__dict__}",
            )
            return

        if not (nautilus_order := self._cache.order(ClientOrderId(order_ref))):
            self.create_task(self.handle_order_status_report(order))
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
        elif order_state.status in [
            "PreSubmitted",
            "Submitted",
        ]:
            instrument = self.instrument_provider.find(nautilus_order.instrument_id)
            total_qty = (
                Quantity.from_int(0)
                if order.totalQuantity == UNSET_DECIMAL
                else Quantity.from_str(str(order.totalQuantity))
            )

            if total_qty <= 0.0:
                # This can be caused by a partially filled entry bracket order and SL triggered.
                self._log.warning(f"IB order with totalQuantity <= 0, skipping: {order.__dict__}")
                return

            price_magnifier = self.instrument_provider.get_price_magnifier(
                nautilus_order.instrument_id,
            )
            price = None

            if order.lmtPrice != UNSET_DOUBLE:
                converted_price = ib_price_to_nautilus_price(order.lmtPrice, price_magnifier)
                price = instrument.make_price(converted_price)

            trigger_price = None

            if order.auxPrice != UNSET_DOUBLE:
                converted_trigger_price = ib_price_to_nautilus_price(
                    order.auxPrice,
                    price_magnifier,
                )
                trigger_price = instrument.make_price(converted_trigger_price)

            venue_order_id_modified = bool(
                nautilus_order.venue_order_id is None
                or nautilus_order.venue_order_id != VenueOrderId(str(order.orderId)),
            )

            if total_qty != nautilus_order.quantity or price or trigger_price:
                self.generate_order_updated(
                    strategy_id=nautilus_order.strategy_id,
                    instrument_id=nautilus_order.instrument_id,
                    client_order_id=nautilus_order.client_order_id,
                    venue_order_id=VenueOrderId(str(order.orderId)),
                    quantity=total_qty,
                    price=price,
                    trigger_price=trigger_price,
                    ts_event=self._clock.timestamp_ns(),
                    venue_order_id_modified=venue_order_id_modified,
                )
            self._handle_order_event(
                status=OrderStatus.ACCEPTED,
                order=nautilus_order,
                order_id=order.orderId,
            )

    def _on_order_status(
        self,
        order_ref: str,
        order_status: str,
        avg_fill_price: float = 0.0,
        filled: Decimal = Decimal(0),
        remaining: Decimal = Decimal(0),
        reason: str = "",
    ) -> None:
        if order_status in ["ApiCancelled", "Cancelled"]:
            status = OrderStatus.CANCELED
        elif order_status == "PendingCancel":
            status = OrderStatus.PENDING_CANCEL
        elif order_status == "Rejected":
            status = OrderStatus.REJECTED
        elif order_status == "Filled":
            status = OrderStatus.FILLED
        elif order_status == "Inactive":
            self._log.warning(
                f"Order status is 'Inactive' because it is invalid or triggered an error for {order_ref=}",
            )
            return
        elif order_status in ["PreSubmitted", "Submitted"]:
            self._log.debug(
                f"Ignoring `_on_order_status` event for {order_status=} is handled in `_on_open_order`",
            )
            return
        else:
            self._log.warning(
                f"Unknown {order_status=} received on `_on_order_status` for {order_ref=}",
            )
            return

        nautilus_order = self._cache.order(ClientOrderId(order_ref))

        if nautilus_order:
            # Update order with average fill price if provided and order is filled/partially filled
            if avg_fill_price and avg_fill_price > 0 and status == OrderStatus.FILLED:
                # Generate an order updated event with the average fill price
                instrument = self._cache.instrument(nautilus_order.instrument_id)
                if instrument:
                    price_magnifier = self.instrument_provider.get_price_magnifier(
                        nautilus_order.instrument_id,
                    )
                    converted_avg_price = ib_price_to_nautilus_price(
                        avg_fill_price,
                        price_magnifier,
                    )
                    avg_px = instrument.make_price(converted_avg_price)

                    # Store the average price for later use in fill events
                    self._order_avg_prices[nautilus_order.client_order_id] = avg_px

                    self._log.debug(
                        f"Updated order {nautilus_order.client_order_id} with avg_px={avg_px}",
                    )

            self._handle_order_event(
                status=status,
                order=nautilus_order,
                reason=reason,
            )
        else:
            self._log.warning(f"ClientOrderId {order_ref} not found in Cache")

    def _on_exec_details(
        self,
        order_ref: str,
        execution: Execution,
        commission_report: CommissionReport,
        contract: IBContract,
    ) -> None:
        if not execution.orderRef:
            self._log.warning(f"ClientOrderId not available, execution={execution.__dict__}")
            return

        if not (nautilus_order := self._cache.order(ClientOrderId(order_ref))):
            self._log.warning(f"ClientOrderId not found in Cache, execution={execution.__dict__}")
            return

        instrument = self.instrument_provider.find(nautilus_order.instrument_id)

        if not instrument:
            self._log.error(
                f"Cannot process execution details for {nautilus_order.instrument_id}: instrument not found",
            )
            return

        # Check if this is a spread order and handle accordingly
        if nautilus_order.instrument_id.is_spread():
            self._handle_spread_execution(
                nautilus_order,
                execution,
                contract,
                commission_report,
            )
            return

        # Regular single instrument order - prepare fill data
        price_magnifier = self.instrument_provider.get_price_magnifier(
            nautilus_order.instrument_id,
        )
        converted_execution_price = ib_price_to_nautilus_price(
            execution.price,
            price_magnifier,
        )

        # Include avg_px in info if we have it stored
        info = {}
        if nautilus_order.client_order_id in self._order_avg_prices:
            info["avg_px"] = self._order_avg_prices[nautilus_order.client_order_id]

        self.generate_order_filled(
            strategy_id=nautilus_order.strategy_id,
            instrument_id=nautilus_order.instrument_id,
            client_order_id=nautilus_order.client_order_id,
            venue_order_id=VenueOrderId(str(execution.orderId)),
            venue_position_id=None,
            trade_id=TradeId(execution.execId),
            order_side=OrderSide[ORDER_SIDE_TO_ORDER_ACTION[execution.side]],
            order_type=nautilus_order.order_type,
            last_qty=Quantity(execution.shares, precision=instrument.size_precision),
            last_px=Price(converted_execution_price, precision=instrument.price_precision),
            quote_currency=instrument.quote_currency,
            commission=Money(
                commission_report.commission,
                Currency.from_str(commission_report.currency),
            ),
            liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
            ts_event=timestring_to_timestamp(execution.time).value,
            info=info if info else None,
        )

        # Update position tracking to avoid duplicate processing
        self._update_position_tracking_from_execution(contract, execution)

    def _handle_spread_execution(
        self,
        nautilus_order: Order,
        execution: Execution,
        contract: IBContract,
        commission_report: CommissionReport,
    ) -> None:
        """
        Handle spread execution by translating leg fills to combo progress and
        individual leg fills.
        """
        try:
            trade_id = TradeId(execution.execId)
            fill_id = str(trade_id)
            client_order_id = nautilus_order.client_order_id
            self._log.info(
                f"Handling spread execution: client_order_id={client_order_id}, trade_id={trade_id}",
            )

            if client_order_id not in self._spread_fill_tracking:
                self._spread_fill_tracking[client_order_id] = set()

            if fill_id in self._spread_fill_tracking[client_order_id]:
                self._log.info(f"Fill {fill_id} already processed, skipping")
                return

            self._spread_fill_tracking[client_order_id].add(fill_id)

            if len(self._spread_fill_tracking[client_order_id]) == 1:
                # Combo fill for order management, generated only once per combo
                self._generate_combo_fill(
                    nautilus_order,
                    execution,
                    contract,
                    commission_report,
                )

            # Leg fill to update leg position in nautilus
            self._generate_leg_fill(
                nautilus_order,
                execution,
                contract,
                commission_report,
            )
        except Exception as e:
            self._log.error(f"Error handling spread execution: {e}")

    def _generate_combo_fill(
        self,
        nautilus_order: Order,
        execution: Execution,
        contract: IBContract,
        commission_report: CommissionReport,
    ) -> None:
        """
        Generate combo fill from leg fill for order management.
        """
        try:
            spread_instrument = self._cache.instrument(nautilus_order.instrument_id)

            # Extract leg instrument ID and ratio to calculate proper combo quantity
            leg_instrument_id, ratio = self._get_leg_instrument_id_and_ratio(
                nautilus_order.instrument_id,
                contract,
            )

            # Price
            price_magnifier = self.instrument_provider.get_price_magnifier(
                nautilus_order.instrument_id,
            )
            converted_execution_price = ib_price_to_nautilus_price(execution.price, price_magnifier)
            combo_price = Price(
                converted_execution_price,
                precision=spread_instrument.price_precision,
            )

            # Combo quantity
            combo_quantity_value = execution.shares / abs(ratio)
            combo_quantity = Quantity(
                combo_quantity_value,
                precision=spread_instrument.size_precision,
            )

            # Order side based on execution side and ratio
            execution_side_numeric = (
                1 if ORDER_SIDE_TO_ORDER_ACTION[execution.side] == "BUY" else -1
            )
            leg_side_numeric = 1 if ratio >= 0 else -1
            combo_order_side = (
                OrderSide.BUY if execution_side_numeric == leg_side_numeric else OrderSide.SELL
            )

            # Combo commission scaled to the number of legs of the combo
            combo_commission = (
                commission_report.commission * nautilus_order.instrument_id.n_legs() / abs(ratio)
            )
            commission = Money(combo_commission, Currency.from_str(commission_report.currency))

            # Generate combo fill with spread instrument ID
            self._log.info(
                f"Generating combo fill: instrument_id={nautilus_order.instrument_id}, client_order_id={nautilus_order.client_order_id}, "
                f"execution_side={execution.side}, ratio={ratio}, combo_side={combo_order_side}",
            )

            # Include avg_px in info if we have it stored
            info = {}
            if nautilus_order.client_order_id in self._order_avg_prices:
                info["avg_px"] = self._order_avg_prices[nautilus_order.client_order_id]

            self.generate_order_filled(
                strategy_id=nautilus_order.strategy_id,
                instrument_id=nautilus_order.instrument_id,  # Keep spread ID
                client_order_id=nautilus_order.client_order_id,
                venue_order_id=VenueOrderId(str(execution.orderId)),
                venue_position_id=None,
                trade_id=TradeId(execution.execId),
                order_side=combo_order_side,
                order_type=nautilus_order.order_type,
                last_qty=combo_quantity,
                last_px=combo_price,
                quote_currency=spread_instrument.quote_currency,
                commission=commission,
                liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
                ts_event=timestring_to_timestamp(execution.time).value,
                info=info if info else None,
            )
        except Exception as e:
            self._log.error(f"Error generating combo fill: {e}")

    def _generate_leg_fill(
        self,
        nautilus_order: Order,
        execution: Execution,
        contract: IBContract,
        commission_report: CommissionReport,
    ) -> None:
        """
        Generate individual leg fill for portfolio updates.
        """
        try:
            leg_instrument_id, ratio = self._get_leg_instrument_id_and_ratio(
                nautilus_order.instrument_id,
                contract,
            )

            if not leg_instrument_id:
                self._log.warning(f"No leg instrument ID found for contract {contract}")
                return

            leg_instrument = self._cache.instrument(leg_instrument_id)

            if not leg_instrument:
                self._log.warning(f"Leg instrument not found in cache: {leg_instrument_id}")
                return

            # Unique client_order_id for leg fill so it doesn't conflict with spread order
            leg_client_order_id = ClientOrderId(
                f"{nautilus_order.client_order_id.value}-LEG-{leg_instrument_id.symbol}",
            )

            # Unique trade ID for leg fills to avoid conflicts with combo fills
            spread_legs = nautilus_order.instrument_id.to_list()  # [(instrument_id, ratio), ...]
            spread_instrument_ids = [leg[0] for leg in spread_legs]
            leg_position = (
                spread_instrument_ids.index(leg_instrument_id)
                if leg_instrument_id in spread_instrument_ids
                else 0
            )
            leg_trade_id_str = f"{execution.execId}-{leg_position}"
            leg_trade_id = TradeId(leg_trade_id_str)

            # Unique venue_order_id
            leg_venue_order_id = VenueOrderId(f"{execution.orderId}-LEG-{leg_position}")

            price_magnifier = self.instrument_provider.get_price_magnifier(leg_instrument_id)
            converted_execution_price = ib_price_to_nautilus_price(execution.price, price_magnifier)
            price = Price(converted_execution_price, precision=leg_instrument.price_precision)

            quantity = Quantity(execution.shares, precision=leg_instrument.size_precision)

            order_side = order_side = OrderSide[ORDER_SIDE_TO_ORDER_ACTION[execution.side]]

            commission = Money(
                commission_report.commission,
                Currency.from_str(commission_report.currency),
            )

            # Include avg_px in info if we have it stored for the parent order
            info = {}
            if nautilus_order.client_order_id in self._order_avg_prices:
                info["avg_px"] = self._order_avg_prices[nautilus_order.client_order_id]

            self.generate_order_filled(
                strategy_id=nautilus_order.strategy_id,
                instrument_id=leg_instrument_id,
                client_order_id=leg_client_order_id,
                venue_order_id=leg_venue_order_id,
                venue_position_id=None,
                trade_id=leg_trade_id,
                order_side=order_side,
                order_type=nautilus_order.order_type,
                last_qty=quantity,
                last_px=price,
                quote_currency=leg_instrument.quote_currency,
                commission=commission,
                liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
                ts_event=timestring_to_timestamp(execution.time).value,
                info=info if info else None,
            )

            # Update position tracking to avoid duplicate processing
            self._update_position_tracking_from_execution(contract, execution)
        except Exception as e:
            self._log.error(f"Error generating leg fill: {e}")

    def _get_leg_instrument_id_and_ratio(
        self,
        spread_instrument_id: InstrumentId,
        contract: IBContract,
    ) -> tuple[InstrumentId | None, int]:
        leg_instrument_id = self.instrument_provider.contract_id_to_instrument_id.get(
            contract.conId,
        )

        if leg_instrument_id:
            leg_tuples = spread_instrument_id.to_list()

            for leg_id, ratio in leg_tuples:
                if leg_id == leg_instrument_id:
                    return leg_instrument_id, ratio

        return None, 1

    def _update_position_tracking_from_execution(self, contract: IBContract, execution) -> None:
        """
        Update position tracking based on execution to avoid duplicate processing.
        """
        try:
            contract_id = contract.conId

            if contract_id in self._known_positions:
                # Update the tracked quantity based on the execution
                side_multiplier = 1 if execution.side == "BOT" else -1
                quantity_change = Decimal(execution.shares) * side_multiplier
                self._known_positions[contract_id] += quantity_change
        except Exception as e:
            self._log.warning(f"Failed to update position tracking: {e}")

    def _on_position_update(self, ib_position) -> None:
        """
        Handle real-time position updates from IB.

        This is triggered when positions change due to option exercises, assignments, or
        other external events.

        """
        self.create_task(self._handle_position_update(ib_position))

    async def _handle_position_update(self, ib_position) -> None:
        """
        Process position update and generate position status report only for external
        changes.

        This filters out position updates that result from normal trading (execDetails)
        and only processes external position changes like option exercises.

        """
        try:
            contract_id = ib_position.contract.conId
            new_quantity = ib_position.quantity

            # Skip zero positions (IB may send these for closed positions)
            if new_quantity == 0:
                # Remove from tracking if position is closed
                self._known_positions.pop(contract_id, None)
                return

            # Check if this is an external position change
            known_quantity = self._known_positions.get(contract_id, Decimal(0))

            # If quantities match, this is likely from normal trading - skip
            if known_quantity == new_quantity:
                return

            # This is an external position change (likely option exercise)
            self._log.info(
                f"External position change detected (likely option exercise): "
                f"Contract {contract_id} ({ib_position.contract.secType}), quantity change: {known_quantity} -> {new_quantity}",
                LogColor.YELLOW,
            )

            # Get instrument for this position
            instrument = await self.instrument_provider.get_instrument(ib_position.contract)

            if instrument is None:
                self._log.warning(
                    f"Cannot process position update: instrument not found for contract ID {contract_id}",
                )
                return

            # Ensure instrument is in cache
            if not self._cache.instrument(instrument.id):
                self._handle_data(instrument)

            # Determine position side
            side = PositionSide.LONG if new_quantity > 0 else PositionSide.SHORT

            # Convert avg_cost to Price if available
            avg_px_open = None
            if ib_position.avg_cost and ib_position.avg_cost > 0:
                avg_px_open = instrument.make_price(ib_position.avg_cost)

            # Create position status report
            position_report = PositionStatusReport(
                account_id=self.account_id,
                instrument_id=instrument.id,
                position_side=side,
                quantity=instrument.make_qty(new_quantity),
                avg_px_open=avg_px_open,
                report_id=UUID4(),
                ts_last=self._clock.timestamp_ns(),
                ts_init=self._clock.timestamp_ns(),
            )

            self._log.info(
                f"Option exercise position created: {instrument.id} {side} {abs(new_quantity)} @ {ib_position.avg_cost}",
                LogColor.CYAN,
            )

            # Send position status report to execution engine
            self._send_position_status_report(position_report)

            # Update tracking
            self._known_positions[contract_id] = new_quantity
        except Exception as e:
            self._log.error(f"Error handling position update: {e}")
