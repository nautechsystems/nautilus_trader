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

from decimal import Decimal
from typing import Any, Dict, List

import pandas as pd

from nautilus_trader.adapters.ftx.core.constants import FTX_VENUE
from nautilus_trader.adapters.ftx.parsing.common import parse_order_status
from nautilus_trader.adapters.ftx.parsing.common import parse_order_type
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def parse_order_status_http(
    account_id: AccountId,
    instrument: Instrument,
    data: Dict[str, Any],
    report_id: UUID4,
    ts_init: int,
) -> OrderStatusReport:
    client_id_str = data.get("clientId")
    price = data.get("price")
    avg_px = data["avgFillPrice"]
    created_at = pd.to_datetime(data["createdAt"], utc=True).value
    return OrderStatusReport(
        account_id=account_id,
        instrument_id=InstrumentId(Symbol(data["market"]), FTX_VENUE),
        client_order_id=ClientOrderId(client_id_str) if client_id_str is not None else None,
        venue_order_id=VenueOrderId(str(data["id"])),
        order_side=OrderSide.BUY if data["side"] == "buy" else OrderSide.SELL,
        order_type=parse_order_type(data=data, price_str="price"),
        time_in_force=TimeInForce.IOC if data["ioc"] else TimeInForce.GTC,
        order_status=parse_order_status(data),
        price=instrument.make_price(price) if price is not None else None,
        quantity=instrument.make_qty(data["size"]),
        filled_qty=instrument.make_qty(data["filledSize"]),
        avg_px=Decimal(str(avg_px)) if avg_px is not None else None,
        post_only=data["postOnly"],
        reduce_only=data["reduceOnly"],
        report_id=report_id,
        ts_accepted=created_at,
        ts_last=created_at,
        ts_init=ts_init,
    )


def parse_trigger_order_status_http(
    account_id: AccountId,
    instrument: Instrument,
    triggers: Dict[str, ClientOrderId],
    data: Dict[str, Any],
    report_id: UUID4,
    ts_init: int,
) -> OrderStatusReport:
    trigger_id = str(data["id"])
    parent_order_id = triggers.get(trigger_id)  # Map trigger to parent
    trigger_price = data.get("triggerPrice")
    order_price = data.get("orderPrice")
    avg_px = data["avgFillPrice"]
    triggered_at = data["triggeredAt"]
    trail_value = data["trailValue"]
    created_at = pd.to_datetime(data["createdAt"], utc=True).value
    return OrderStatusReport(
        account_id=account_id,
        instrument_id=instrument.id,
        client_order_id=ClientOrderId(parent_order_id) if parent_order_id is not None else None,
        venue_order_id=VenueOrderId(trigger_id),
        order_side=OrderSide.BUY if data["side"] == "buy" else OrderSide.SELL,
        order_type=parse_order_type(data=data),
        time_in_force=TimeInForce.GTC,
        order_status=parse_order_status(data),
        price=instrument.make_price(order_price) if order_price is not None else None,
        trigger_price=instrument.make_price(trigger_price) if trigger_price is not None else None,
        trigger_type=TriggerType.LAST,
        trailing_offset=Decimal(str(trail_value)) if trail_value is not None else None,
        trailing_offset_type=TrailingOffsetType.PRICE,
        quantity=instrument.make_qty(data["size"]),
        filled_qty=instrument.make_qty(data["filledSize"]),
        avg_px=Decimal(str(avg_px)) if avg_px is not None else None,
        post_only=False,
        reduce_only=data["reduceOnly"],
        report_id=report_id,
        ts_accepted=created_at,
        ts_triggered=pd.to_datetime(triggered_at, utc=True).value
        if triggered_at is not None
        else 0,
        ts_last=created_at,
        ts_init=ts_init,
    )


def parse_bars_http(
    instrument: Instrument,
    bar_type: BarType,
    data: List[Dict[str, Any]],
    ts_event_delta: int,
    ts_init: int,
) -> List[Bar]:
    bars: List[Bar] = []
    for row in data:
        ts_event = millis_to_nanos(row["time"]) + ts_event_delta
        bar: Bar = Bar(
            bar_type=bar_type,
            open=Price(row["open"], instrument.price_precision),
            high=Price(row["high"], instrument.price_precision),
            low=Price(row["low"], instrument.price_precision),
            close=Price(row["close"], instrument.price_precision),
            volume=Quantity(row["volume"], instrument.size_precision),
            check=True,
            ts_event=ts_event,
            ts_init=max(ts_init, ts_event),
        )
        bars.append(bar)

    return bars
