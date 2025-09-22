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

from __future__ import annotations

from datetime import datetime
from decimal import Decimal
from typing import Any

import pandas as pd

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.message import Document
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import contingency_type_to_str
from nautilus_trader.model.enums import liquidity_side_to_str
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.enums import order_status_to_str
from nautilus_trader.model.enums import order_type_to_str
from nautilus_trader.model.enums import position_side_to_str
from nautilus_trader.model.enums import time_in_force_to_str
from nautilus_trader.model.enums import trailing_offset_type_to_str
from nautilus_trader.model.enums import trigger_type_to_str
from nautilus_trader.model.functions import contingency_type_from_pyo3
from nautilus_trader.model.functions import liquidity_side_from_pyo3
from nautilus_trader.model.functions import order_side_from_pyo3
from nautilus_trader.model.functions import order_status_from_pyo3
from nautilus_trader.model.functions import order_type_from_pyo3
from nautilus_trader.model.functions import position_side_from_pyo3
from nautilus_trader.model.functions import time_in_force_from_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class ExecutionReport(Document):
    """
    The base class for all execution reports.
    """

    def __init__(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        ts_init: int,
    ) -> None:
        super().__init__(
            report_id,
            ts_init,
        )
        self.account_id = account_id
        self.instrument_id = instrument_id


class OrderStatusReport(ExecutionReport):
    """
    Represents an order status at a point in time.

    Reporting is best-effort; if filled exceeds quantity due to venue anomalies,
    avoid negative leaves_qty by clamping to zero.

    Parameters
    ----------
    account_id : AccountId
        The account ID for the report.
    instrument_id : InstrumentId
        The instrument ID for the report.
    venue_order_id : VenueOrderId
        The reported order ID (assigned by the venue).
    order_side : OrderSide {``BUY``, ``SELL``}
        The reported order side.
    order_type : OrderType
        The reported order type.
    time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}
        The reported order time in force.
    order_status : OrderStatus
        The reported order status at the exchange.
    quantity : Quantity
        The reported order original quantity.
    filled_qty : Quantity
        The reported filled quantity at the exchange.
    report_id : UUID4
        The report ID.
    ts_accepted : int
        UNIX timestamp (nanoseconds) when the reported order was accepted.
    ts_last : int
        UNIX timestamp (nanoseconds) of the last order status change.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.
    client_order_id : ClientOrderId, optional
        The reported client order ID.
    order_list_id : OrderListId, optional
        The reported order list ID associated with the order.
    venue_position_id : PositionId, optional
        The reported venue position ID for the order. If the trading venue has
        associated a position ID / ticket with the order then pass that here,
        otherwise pass ``None`` and the execution engine OMS will handle
        position ID resolution.
    contingency_type : ContingencyType, default ``NO_CONTINGENCY``
        The reported order contingency type.
    expire_time : datetime, optional
        The order expiration.
    price : Price, optional
        The reported order price (LIMIT).
    trigger_price : Price, optional
        The reported order trigger price (STOP).
    trigger_type : TriggerType, default ``NO_TRIGGER``
        The reported order trigger type.
    limit_offset : Decimal, optional
        The trailing offset for the order price (LIMIT).
    trailing_offset : Decimal, optional
        The trailing offset for the trigger price (STOP).
    trailing_offset_type : TrailingOffsetType, default ``NO_TRAILING_OFFSET``
        The order trailing offset type.
    avg_px : Decimal, optional
        The reported order average fill price.
    display_qty : Quantity, optional
        The reported order quantity displayed on the public book (iceberg).
    post_only : bool, default False
        If the reported order will only provide liquidity (make a market).
    reduce_only : bool, default False
        If the reported order carries the 'reduce-only' execution instruction.
    cancel_reason : str, optional
        The reported reason for order cancellation.
    ts_triggered : int, optional
        UNIX timestamp (nanoseconds) when the object was initialized.

    Raises
    ------
    ValueError
        If `quantity` is not positive (> 0).
    ValueError
        If `filled_qty` is negative (< 0).
    ValueError
        If `trigger_price` is not ``None`` and `trigger_price` is equal to ``NO_TRIGGER``.
    ValueError
        If `limit_offset` or `trailing_offset` is not ``None`` and trailing_offset_type is equal to ``NO_TRAILING_OFFSET``.

    """

    def __init__(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        time_in_force: TimeInForce,
        order_status: OrderStatus,
        quantity: Quantity,
        filled_qty: Quantity,
        report_id: UUID4,
        ts_accepted: int,
        ts_last: int,
        ts_init: int,
        client_order_id: ClientOrderId | None = None,  # (None if external order)
        order_list_id: OrderListId | None = None,
        venue_position_id: PositionId | None = None,  # (None if not assigned by venue)
        linked_order_ids: list[ClientOrderId] | None = None,
        parent_order_id: ClientOrderId | None = None,
        contingency_type: ContingencyType = ContingencyType.NO_CONTINGENCY,
        expire_time: datetime | None = None,
        price: Price | None = None,
        trigger_price: Price | None = None,
        trigger_type: TriggerType = TriggerType.NO_TRIGGER,
        limit_offset: Decimal | None = None,
        trailing_offset: Decimal | None = None,
        trailing_offset_type: TrailingOffsetType = TrailingOffsetType.NO_TRAILING_OFFSET,
        avg_px: Decimal | None = None,
        display_qty: Quantity | None = None,
        post_only: bool = False,
        reduce_only: bool = False,
        cancel_reason: str | None = None,
        ts_triggered: int | None = None,
    ) -> None:
        PyCondition.positive(quantity, "quantity")
        PyCondition.not_negative(filled_qty, "filled_qty")
        if trigger_price is not None and trigger_price > 0:
            PyCondition.not_equal(trigger_type, TriggerType.NO_TRIGGER, "trigger_type", "NONE")
        if limit_offset is not None or trailing_offset is not None:
            PyCondition.not_equal(
                trailing_offset_type,
                TrailingOffsetType.NO_TRAILING_OFFSET,
                "trailing_offset_type",
                "NO_TRAILING_OFFSET",
            )

        super().__init__(
            account_id,
            instrument_id,
            report_id,
            ts_init,
        )
        self.client_order_id = client_order_id
        self.order_list_id = order_list_id
        self.venue_order_id = venue_order_id
        self.venue_position_id = venue_position_id
        self.linked_order_ids = linked_order_ids
        self.parent_order_id = parent_order_id
        self.order_side = order_side
        self.order_type = order_type
        self.contingency_type = contingency_type
        self.time_in_force = time_in_force
        self.expire_time = expire_time
        self.order_status = order_status
        self.price = price
        self.trigger_price = trigger_price
        self.trigger_type = trigger_type
        self.limit_offset = limit_offset
        self.trailing_offset = trailing_offset
        self.trailing_offset_type = trailing_offset_type
        self.quantity = quantity
        self.filled_qty = filled_qty

        # Clamp to minimum zero for robustness
        raw_leaves_qty = max(self.quantity.raw - self.filled_qty.raw, 0)
        self.leaves_qty = Quantity.from_raw(raw_leaves_qty, self.quantity.precision)

        self.display_qty = display_qty
        self.avg_px = avg_px
        self.post_only = post_only
        self.reduce_only = reduce_only
        self.cancel_reason = cancel_reason
        self.ts_accepted = ts_accepted
        self.ts_triggered = ts_triggered or 0
        self.ts_last = ts_last

    @property
    def is_open(self) -> bool:
        """
        Return whether the reported order status is 'open'.

        Returns
        -------
        bool

        """
        return self.order_status in (
            OrderStatus.ACCEPTED,
            OrderStatus.TRIGGERED,
            OrderStatus.PENDING_CANCEL,
            OrderStatus.PENDING_UPDATE,
            OrderStatus.PARTIALLY_FILLED,
        )

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, OrderStatusReport):
            return False
        return (
            self.account_id == other.account_id
            and self.instrument_id == other.instrument_id
            and self.venue_order_id == other.venue_order_id
            and self.ts_accepted == other.ts_accepted
        )

    def __repr__(self) -> str:
        linked_ids = [o.value for o in self.linked_order_ids] if self.linked_order_ids else None
        return (
            f"{type(self).__name__}("
            f"account_id={self.account_id}, "
            f"instrument_id={self.instrument_id}, "
            f"client_order_id={self.client_order_id}, "
            f"order_list_id={self.order_list_id}, "  # Can be None
            f"venue_order_id={self.venue_order_id}, "  # Can be None
            f"venue_position_id={self.venue_position_id}, "  # Can be None
            f"linked_order_ids={linked_ids}, "
            f"parent_order_id={self.parent_order_id}, "
            f"order_side={order_side_to_str(self.order_side)}, "
            f"order_type={order_type_to_str(self.order_type)}, "
            f"contingency_type={contingency_type_to_str(self.contingency_type)}, "
            f"time_in_force={time_in_force_to_str(self.time_in_force)}, "
            f"expire_time={self.expire_time}, "
            f"order_status={order_status_to_str(self.order_status)}, "
            f"price={self.price}, "
            f"trigger_price={self.trigger_price}, "
            f"trigger_type={trigger_type_to_str(self.trigger_type)}, "
            f"limit_offset={self.limit_offset}, "
            f"trailing_offset={self.trailing_offset}, "
            f"trailing_offset_type={trailing_offset_type_to_str(self.trailing_offset_type)}, "
            f"quantity={self.quantity.to_formatted_str()}, "
            f"filled_qty={self.filled_qty.to_formatted_str()}, "
            f"leaves_qty={self.leaves_qty.to_formatted_str()}, "
            f"display_qty={self.display_qty}, "
            f"avg_px={self.avg_px}, "
            f"post_only={self.post_only}, "
            f"reduce_only={self.reduce_only}, "
            f"cancel_reason={self.cancel_reason}, "
            f"report_id={self.id}, "
            f"ts_accepted={self.ts_accepted}, "
            f"ts_triggered={self.ts_triggered}, "
            f"ts_last={self.ts_last}, "
            f"ts_init={self.ts_init})"
        )

    def to_dict(self) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, Any]

        """
        return {
            "type": type(self).__name__,
            "account_id": self.account_id.value,
            "instrument_id": self.instrument_id.value,
            "venue_order_id": self.venue_order_id.value,
            "order_side": self.order_side.value,
            "order_type": self.order_type.value,
            "time_in_force": self.time_in_force.value,
            "order_status": self.order_status.value,
            "quantity": str(self.quantity),
            "filled_qty": str(self.filled_qty),
            "report_id": self.id.value,
            "ts_accepted": self.ts_accepted,
            "ts_last": self.ts_last,
            "ts_init": self.ts_init,
            "client_order_id": self.client_order_id.value if self.client_order_id else None,
            "order_list_id": self.order_list_id.value if self.order_list_id else None,
            "venue_position_id": self.venue_position_id.value if self.venue_position_id else None,
            "linked_order_ids": (
                [o.value for o in self.linked_order_ids] if self.linked_order_ids else None
            ),
            "parent_order_id": self.parent_order_id.value if self.parent_order_id else None,
            "contingency_type": self.contingency_type.value,
            "expire_time": self.expire_time.isoformat() if self.expire_time else None,
            "price": str(self.price) if self.price else None,
            "trigger_price": str(self.trigger_price) if self.trigger_price else None,
            "trigger_type": self.trigger_type.value,
            "limit_offset": str(self.limit_offset) if self.limit_offset else None,
            "trailing_offset": str(self.trailing_offset) if self.trailing_offset else None,
            "trailing_offset_type": self.trailing_offset_type.value,
            "avg_px": str(self.avg_px) if self.avg_px else None,
            "display_qty": str(self.display_qty) if self.display_qty else None,
            "post_only": self.post_only,
            "reduce_only": self.reduce_only,
            "cancel_reason": self.cancel_reason,
            "ts_triggered": self.ts_triggered,
        }

    @classmethod
    def from_dict(cls, values: dict[str, Any]) -> OrderStatusReport:
        """
        Return an order status report from the given dict values.

        Parameters
        ----------
        values : dict[str, Any]
            The values for initialization.

        Returns
        -------
        OrderStatusReport

        """
        return cls(
            account_id=AccountId(values["account_id"]),
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            order_side=OrderSide(values["order_side"]),
            order_type=OrderType(values["order_type"]),
            time_in_force=TimeInForce(values["time_in_force"]),
            order_status=OrderStatus(values["order_status"]),
            quantity=Quantity.from_str(values["quantity"]),
            filled_qty=Quantity.from_str(values["filled_qty"]),
            report_id=UUID4.from_str(values["report_id"]),
            ts_accepted=values["ts_accepted"],
            ts_last=values["ts_last"],
            ts_init=values["ts_init"],
            client_order_id=(
                ClientOrderId(values["client_order_id"]) if values["client_order_id"] else None
            ),
            order_list_id=OrderListId(values["order_list_id"]) if values["order_list_id"] else None,
            venue_position_id=(
                PositionId(values["venue_position_id"]) if values["venue_position_id"] else None
            ),
            linked_order_ids=(
                [ClientOrderId(value) for value in values.get("linked_order_ids") or []] or None
            ),
            parent_order_id=(
                ClientOrderId(values["parent_order_id"]) if values.get("parent_order_id") else None
            ),
            contingency_type=ContingencyType(values["contingency_type"]),
            expire_time=(
                datetime.fromisoformat(values["expire_time"]) if values["expire_time"] else None
            ),
            price=Price.from_str(values["price"]) if values["price"] else None,
            trigger_price=(
                Price.from_str(values["trigger_price"]) if values["trigger_price"] else None
            ),
            trigger_type=TriggerType(values["trigger_type"]),
            limit_offset=Decimal(values["limit_offset"]) if values["limit_offset"] else None,
            trailing_offset=(
                Decimal(values["trailing_offset"]) if values["trailing_offset"] else None
            ),
            trailing_offset_type=TrailingOffsetType(values["trailing_offset_type"]),
            avg_px=Decimal(values["avg_px"]) if values["avg_px"] else None,
            display_qty=Quantity.from_str(values["display_qty"]) if values["display_qty"] else None,
            post_only=values["post_only"],
            reduce_only=values["reduce_only"],
            cancel_reason=values["cancel_reason"],
            ts_triggered=values["ts_triggered"],
        )

    @staticmethod
    def from_pyo3(pyo3_report: nautilus_pyo3.OrderStatusReport) -> OrderStatusReport:
        return OrderStatusReport(
            account_id=AccountId(pyo3_report.account_id.value),
            instrument_id=InstrumentId.from_str(pyo3_report.instrument_id.value),
            venue_order_id=VenueOrderId(pyo3_report.venue_order_id.value),
            order_side=order_side_from_pyo3(pyo3_report.order_side),
            order_type=order_type_from_pyo3(pyo3_report.order_type),
            time_in_force=time_in_force_from_pyo3(pyo3_report.time_in_force),
            order_status=order_status_from_pyo3(pyo3_report.order_status),
            quantity=Quantity.from_str(str(pyo3_report.quantity)),
            filled_qty=Quantity.from_str(str(pyo3_report.filled_qty)),
            ts_accepted=pyo3_report.ts_accepted,
            ts_last=pyo3_report.ts_last,
            ts_init=pyo3_report.ts_init,
            client_order_id=(
                ClientOrderId(pyo3_report.client_order_id.value)
                if pyo3_report.client_order_id
                else None
            ),
            report_id=UUID4.from_str(pyo3_report.report_id.value),
            order_list_id=(
                OrderListId(pyo3_report.order_list_id.value) if pyo3_report.order_list_id else None
            ),
            venue_position_id=(
                PositionId(pyo3_report.venue_position_id.value)
                if pyo3_report.venue_position_id
                else None
            ),
            linked_order_ids=(
                [ClientOrderId(str(oid.value)) for oid in pyo3_report.linked_order_ids]
                if pyo3_report.linked_order_ids
                else None
            ),
            parent_order_id=(
                ClientOrderId(str(pyo3_report.parent_order_id.value))
                if pyo3_report.parent_order_id
                else None
            ),
            contingency_type=contingency_type_from_pyo3(pyo3_report.contingency_type),
            expire_time=pd.Timestamp(pyo3_report.expire_time) if pyo3_report.expire_time else None,
            price=Price.from_str(str(pyo3_report.price)) if pyo3_report.price else None,
            trigger_price=(
                Price.from_str(str(pyo3_report.trigger_price))
                if pyo3_report.trigger_price
                else None
            ),
            trigger_type=(
                TriggerType(pyo3_report.trigger_type.value)
                if pyo3_report.trigger_type
                else TriggerType.NO_TRIGGER
            ),
            limit_offset=pyo3_report.limit_offset,
            trailing_offset=pyo3_report.trailing_offset,
            trailing_offset_type=TrailingOffsetType(pyo3_report.trailing_offset_type.value),
            avg_px=pyo3_report.avg_px,
            display_qty=(
                Quantity.from_str(str(pyo3_report.display_qty)) if pyo3_report.display_qty else None
            ),
            post_only=pyo3_report.post_only,
            reduce_only=pyo3_report.reduce_only,
            cancel_reason=pyo3_report.cancel_reason,
            ts_triggered=pyo3_report.ts_triggered,
        )


class FillReport(ExecutionReport):
    """
    Represents a report of a single order fill.

    Parameters
    ----------
    account_id : AccountId
        The account ID for the report.
    instrument_id : InstrumentId
        The reported instrument ID for the trade.
    venue_order_id : VenueOrderId
        The reported venue order ID (assigned by the venue) for the trade.
    trade_id : TradeId
        The reported trade match ID (assigned by the venue).
    order_side : OrderSide {``BUY``, ``SELL``}
        The reported order side for the trade.
    last_qty : Quantity
        The reported quantity of the trade.
    last_px : Price
        The reported price of the trade.
    commission : Money
        The reported commission for the trade.
        If no commission then use a zero `Money` amount of the commission currency.
    liquidity_side : LiquiditySide {``NO_LIQUIDITY_SIDE``, ``MAKER``, ``TAKER``}
        The reported liquidity side for the trade.
    report_id : UUID4
        The report ID.
    ts_event : int
        UNIX timestamp (nanoseconds) when the trade occurred.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.
    client_order_id : ClientOrderId, optional
        The reported client order ID for the trade.
    venue_position_id : PositionId, optional
        The reported venue position ID for the trade. If the trading venue has
        assigned a position ID / ticket for the trade then pass that here,
        otherwise pass ``None`` and the execution engine OMS will handle
        position ID resolution.

    Raises
    ------
    ValueError
        If `last_qty` is not positive (> 0).

    """

    def __init__(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        trade_id: TradeId,
        order_side: OrderSide,
        last_qty: Quantity,
        last_px: Price,
        commission: Money,
        liquidity_side: LiquiditySide,
        report_id: UUID4,
        ts_event: int,
        ts_init: int,
        client_order_id: ClientOrderId | None = None,  # (None if external order)
        venue_position_id: PositionId | None = None,
    ) -> None:
        PyCondition.positive(last_qty, "last_qty")

        super().__init__(
            account_id,
            instrument_id,
            report_id,
            ts_init,
        )
        self.client_order_id = client_order_id
        self.venue_order_id = venue_order_id
        self.venue_position_id = venue_position_id
        self.trade_id = trade_id
        self.order_side = order_side
        self.last_qty = last_qty
        self.last_px = last_px
        self.commission = commission
        self.liquidity_side = liquidity_side
        self.ts_event = ts_event

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, FillReport):
            return False
        return (
            self.account_id == other.account_id
            and self.instrument_id == other.instrument_id
            and self.venue_order_id == other.venue_order_id
            and self.trade_id == other.trade_id
            and self.ts_event == other.ts_event
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"account_id={self.account_id}, "
            f"instrument_id={self.instrument_id}, "
            f"client_order_id={self.client_order_id}, "
            f"venue_order_id={self.venue_order_id}, "
            f"venue_position_id={self.venue_position_id}, "
            f"trade_id={self.trade_id}, "
            f"order_side={order_side_to_str(self.order_side)}, "
            f"last_qty={self.last_qty.to_formatted_str()}, "
            f"last_px={self.last_px.to_formatted_str()}, "
            f"commission={self.commission.to_formatted_str() if self.commission is not None else None}, "
            f"liquidity_side={liquidity_side_to_str(self.liquidity_side)}, "
            f"report_id={self.id}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    def to_dict(self) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, Any]

        """
        return {
            "type": type(self).__name__,
            "account_id": self.account_id.value,
            "instrument_id": self.instrument_id.value,
            "venue_order_id": self.venue_order_id.value,
            "trade_id": self.trade_id.value,
            "order_side": self.order_side.value,
            "last_qty": str(self.last_qty),
            "last_px": str(self.last_px),
            "commission": str(self.commission),
            "liquidity_side": self.liquidity_side.value,
            "report_id": self.id.value,
            "ts_event": self.ts_event,
            "ts_init": self.ts_init,
            "client_order_id": self.client_order_id.value if self.client_order_id else None,
            "venue_position_id": self.venue_position_id.value if self.venue_position_id else None,
        }

    @classmethod
    def from_dict(cls, values: dict[str, Any]) -> FillReport:
        """
        Return a fill report from the given dict values.

        Parameters
        ----------
        values : dict[str, Any]
            The values for initialization.

        Returns
        -------
        FillReport

        """
        return cls(
            account_id=AccountId(values["account_id"]),
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            venue_order_id=VenueOrderId(values["venue_order_id"]),
            trade_id=TradeId(values["trade_id"]),
            order_side=OrderSide(values["order_side"]),
            last_qty=Quantity.from_str(values["last_qty"]),
            last_px=Price.from_str(values["last_px"]),
            commission=Money.from_str(values["commission"]),
            liquidity_side=LiquiditySide(values["liquidity_side"]),
            report_id=UUID4.from_str(values["report_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            client_order_id=(
                ClientOrderId(values["client_order_id"]) if values["client_order_id"] else None
            ),
            venue_position_id=(
                PositionId(values["venue_position_id"]) if values["venue_position_id"] else None
            ),
        )

    @staticmethod
    def from_pyo3(pyo3_report: nautilus_pyo3.FillReport) -> FillReport:
        return FillReport(
            account_id=AccountId(pyo3_report.account_id.value),
            instrument_id=InstrumentId.from_str(pyo3_report.instrument_id.value),
            venue_order_id=VenueOrderId(pyo3_report.venue_order_id.value),
            trade_id=TradeId(pyo3_report.trade_id.value),
            order_side=order_side_from_pyo3(pyo3_report.order_side),
            last_qty=Quantity.from_str(str(pyo3_report.last_qty)),
            last_px=Price.from_str(str(pyo3_report.last_px)),
            commission=Money.from_str(str(pyo3_report.commission)),
            liquidity_side=liquidity_side_from_pyo3(pyo3_report.liquidity_side),
            report_id=UUID4.from_str(pyo3_report.report_id.value),
            ts_event=pyo3_report.ts_event,
            ts_init=pyo3_report.ts_init,
            client_order_id=(
                ClientOrderId(pyo3_report.client_order_id.value)
                if pyo3_report.client_order_id
                else None
            ),
            venue_position_id=(
                PositionId(pyo3_report.venue_position_id.value)
                if pyo3_report.venue_position_id
                else None
            ),
        )


class PositionStatusReport(ExecutionReport):
    """
    Represents a position status at a point in time.

    Parameters
    ----------
    account_id : AccountId
        The account ID for the report.
    instrument_id : InstrumentId
        The reported instrument ID for the position.
    position_side : PositionSide {``FLAT``, ``LONG``, ``SHORT``}
        The reported position side at the exchange.
    quantity : Quantity
        The reported position quantity at the exchange.
    report_id : UUID4
        The report ID.
    ts_last : int
        UNIX timestamp (nanoseconds) of the last position change.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.
    venue_position_id : PositionId, optional
        The reported venue position ID (assigned by the venue). If the trading
        venue has assigned a position ID / ticket for the trade then pass that
        here, otherwise pass ``None`` and the execution engine OMS will handle
        position ID resolution.
    avg_px_open : Decimal, optional
        The reported position average open price.

    """

    def __init__(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        position_side: PositionSide,
        quantity: Quantity,
        report_id: UUID4,
        ts_last: int,
        ts_init: int,
        venue_position_id: PositionId | None = None,
        avg_px_open: Decimal | None = None,
    ) -> None:
        super().__init__(
            account_id,
            instrument_id,
            report_id,
            ts_init,
        )
        self.venue_position_id = venue_position_id
        self.position_side = position_side
        self.quantity = quantity
        self.avg_px_open = avg_px_open
        self.signed_decimal_qty = (
            -self.quantity.as_decimal()
            if position_side == PositionSide.SHORT
            else self.quantity.as_decimal()
        )
        self.ts_last = ts_last

    @staticmethod
    def create_flat(
        account_id: AccountId,
        instrument_id: InstrumentId,
        size_precision: int,
        ts_init: int,
        report_id: UUID4 | None = None,
    ) -> PositionStatusReport:
        return PositionStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.zero(size_precision),
            report_id=report_id or UUID4(),
            ts_last=ts_init,
            ts_init=ts_init,
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"account_id={self.account_id}, "
            f"instrument_id={self.instrument_id}, "
            f"venue_position_id={self.venue_position_id}, "
            f"position_side={position_side_to_str(self.position_side)}, "
            f"quantity={self.quantity.to_formatted_str()}, "
            f"avg_px_open={self.avg_px_open}, "
            f"signed_decimal_qty={self.signed_decimal_qty}, "
            f"report_id={self.id}, "
            f"ts_last={self.ts_last}, "
            f"ts_init={self.ts_init})"
        )

    def to_dict(self) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, Any]

        """
        return {
            "type": type(self).__name__,
            "account_id": self.account_id.value,
            "instrument_id": self.instrument_id.value,
            "position_side": self.position_side.value,
            "quantity": str(self.quantity),
            "report_id": self.id.value,
            "ts_last": self.ts_last,
            "ts_init": self.ts_init,
            "venue_position_id": self.venue_position_id.value if self.venue_position_id else None,
            "avg_px_open": str(self.avg_px_open) if self.avg_px_open else None,
        }

    @classmethod
    def from_dict(cls, values: dict[str, Any]) -> PositionStatusReport:
        """
        Return a position status report from the given dict values.

        Parameters
        ----------
        values : dict[str, Any]
            The values for initialization.

        Returns
        -------
        PositionStatusReport

        """
        return cls(
            account_id=AccountId(values["account_id"]),
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            position_side=PositionSide(values["position_side"]),
            quantity=Quantity.from_str(values["quantity"]),
            report_id=UUID4.from_str(values["report_id"]),
            ts_last=values["ts_last"],
            ts_init=values["ts_init"],
            venue_position_id=(
                PositionId(values["venue_position_id"]) if values["venue_position_id"] else None
            ),
            avg_px_open=(Decimal(values["avg_px_open"]) if values.get("avg_px_open") else None),
        )

    @staticmethod
    def from_pyo3(pyo3_report: nautilus_pyo3.PositionStatusReport) -> PositionStatusReport:
        return PositionStatusReport(
            account_id=AccountId(pyo3_report.account_id.value),
            instrument_id=InstrumentId.from_str(pyo3_report.instrument_id.value),
            position_side=position_side_from_pyo3(pyo3_report.position_side),
            quantity=Quantity.from_str(str(pyo3_report.quantity)),
            report_id=UUID4.from_str(pyo3_report.report_id.value),
            ts_last=pyo3_report.ts_last,
            ts_init=pyo3_report.ts_init,
            venue_position_id=(
                PositionId(pyo3_report.venue_position_id.value)
                if pyo3_report.venue_position_id
                else None
            ),
            avg_px_open=pyo3_report.avg_px_open,
        )


class ExecutionMassStatus(Document):
    """
    Represents an execution mass status report for an execution client - including
    status of all orders, trades for those orders and open positions.

    Parameters
    ----------
    venue : Venue
        The venue for the report.
    client_id : ClientId
        The client ID for the report.
    account_id : AccountId
        The account ID for the report.
    report_id : UUID4
        The report ID.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        report_id: UUID4,
        ts_init: int,
    ) -> None:
        super().__init__(
            report_id,
            ts_init,
        )
        self.client_id = client_id
        self.account_id = account_id
        self.venue = venue

        self._order_reports: dict[VenueOrderId, OrderStatusReport] = {}
        self._fill_reports: dict[VenueOrderId, list[FillReport]] = {}
        self._position_reports: dict[InstrumentId, list[PositionStatusReport]] = {}

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"client_id={self.client_id}, "
            f"account_id={self.account_id}, "
            f"venue={self.venue}, "
            f"order_reports={self._order_reports}, "
            f"fill_reports={self._fill_reports}, "
            f"position_reports={self._position_reports}, "
            f"report_id={self.id}, "
            f"ts_init={self.ts_init})"
        )

    @property
    def order_reports(self) -> dict[VenueOrderId, OrderStatusReport]:
        """
        The order status reports.

        Returns
        -------
        dict[VenueOrderId, OrderStatusReport]

        """
        return self._order_reports.copy()

    @property
    def fill_reports(self) -> dict[VenueOrderId, list[FillReport]]:
        """
        The fill reports.

        Returns
        -------
        dict[VenueOrderId, list[FillReport]

        """
        return self._fill_reports.copy()

    @property
    def position_reports(self) -> dict[InstrumentId, list[PositionStatusReport]]:
        """
        The position status reports.

        Returns
        -------
        dict[InstrumentId, list[PositionStatusReport]]

        """
        return self._position_reports.copy()

    def add_order_reports(self, reports: list[OrderStatusReport]) -> None:
        """
        Add the order reports to the mass status.

        Parameters
        ----------
        reports : list[OrderStatusReport]
            The list of reports to add.

        Raises
        ------
        TypeError
            If `reports` contains a type other than `OrderStatusReport`.

        """
        PyCondition.not_none(reports, "reports")

        for report in reports:
            self._order_reports[report.venue_order_id] = report

    def add_fill_reports(self, reports: list[FillReport]) -> None:
        """
        Add the fill reports to the mass status.

        Parameters
        ----------
        reports : list[FillReport]
            The list of reports to add.

        Raises
        ------
        TypeError
            If `reports` contains a type other than `FillReport`.

        """
        PyCondition.not_none(reports, "reports")

        # Sort reports by venue order ID
        for report in reports:
            if report.venue_order_id not in self._fill_reports:
                self._fill_reports[report.venue_order_id] = []
            self._fill_reports[report.venue_order_id].append(report)

    def add_position_reports(self, reports: list[PositionStatusReport]) -> None:
        """
        Add the position status reports to the mass status.

        Parameters
        ----------
        reports : list[PositionStatusReport]
            The reports to add.

        """
        PyCondition.not_none(reports, "reports")

        # Sort reports by instrument ID
        for report in reports:
            if report.instrument_id not in self._position_reports:
                self._position_reports[report.instrument_id] = []
            self._position_reports[report.instrument_id].append(report)

    def to_dict(self) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, Any]

        """
        return {
            "type": type(self).__name__,
            "client_id": self.client_id.value,
            "account_id": self.account_id.value,
            "venue": self.venue.value,
            "report_id": self.id.value,
            "ts_init": self.ts_init,
            "order_reports": {
                venue_order_id.value: report.to_dict()
                for venue_order_id, report in self._order_reports.items()
            },
            "fill_reports": {
                venue_order_id.value: [report.to_dict() for report in reports]
                for venue_order_id, reports in self._fill_reports.items()
            },
            "position_reports": {
                instrument_id.value: [report.to_dict() for report in reports]
                for instrument_id, reports in self._position_reports.items()
            },
        }

    @classmethod
    def from_dict(cls, values: dict[str, Any]) -> ExecutionMassStatus:
        """
        Return an execution mass status from the given dict values.

        Parameters
        ----------
        values : dict[str, Any]
            The values for initialization.

        Returns
        -------
        ExecutionMassStatus

        """
        mass_status = cls(
            client_id=ClientId(values["client_id"]),
            account_id=AccountId(values["account_id"]),
            venue=Venue(values["venue"]),
            report_id=UUID4.from_str(values["report_id"]),
            ts_init=values["ts_init"],
        )

        # Restore order reports
        for venue_order_id_str, report_dict in values["order_reports"].items():
            venue_order_id = VenueOrderId(venue_order_id_str)
            report = OrderStatusReport.from_dict(report_dict)
            mass_status._order_reports[venue_order_id] = report

        # Restore fill reports
        for venue_order_id_str, report_dicts in values["fill_reports"].items():
            venue_order_id = VenueOrderId(venue_order_id_str)
            reports = [FillReport.from_dict(report_dict) for report_dict in report_dicts]
            mass_status._fill_reports[venue_order_id] = reports

        # Restore position reports
        for instrument_id_str, report_dicts in values["position_reports"].items():
            instrument_id = InstrumentId.from_str(instrument_id_str)
            reports = [PositionStatusReport.from_dict(report_dict) for report_dict in report_dicts]
            mass_status._position_reports[instrument_id] = reports

        return mass_status
