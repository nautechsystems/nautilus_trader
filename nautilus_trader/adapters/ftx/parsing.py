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

from datetime import datetime
from decimal import Decimal
from typing import Any, Dict, List, Optional

import pandas as pd

from nautilus_trader.adapters.ftx.common import FTX_VENUE
from nautilus_trader.adapters.ftx.data_types import FTXTicker
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.core.text import precision_from_str
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.crypto_perp import CryptoPerpetual
from nautilus_trader.model.instruments.currency import CurrencySpot
from nautilus_trader.model.instruments.future import Future
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import Order
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot


def parse_order_status(
    account_id: AccountId,
    instrument: Instrument,
    data: Dict[str, Any],
    report_id: UUID4,
    ts_init: int,
) -> OrderStatusReport:
    client_id_str = data.get("clientId")
    price = data.get("price")
    avg_px = data["avgFillPrice"]
    created_at = int(pd.to_datetime(data["createdAt"]).to_datetime64())
    return OrderStatusReport(
        account_id=account_id,
        instrument_id=InstrumentId(Symbol(data["market"]), FTX_VENUE),
        client_order_id=ClientOrderId(client_id_str) if client_id_str is not None else None,
        order_list_id=None,
        venue_order_id=VenueOrderId(str(data["id"])),
        order_side=OrderSide.BUY if data["side"] == "buy" else OrderSide.SELL,
        order_type=parse_order_type(data=data, price_str="price"),
        contingency_type=ContingencyType.NONE,
        time_in_force=TimeInForce.IOC if data["ioc"] else TimeInForce.GTC,
        expire_time=None,
        order_status=parse_status(data),
        price=instrument.make_price(price) if price is not None else None,
        trigger_price=None,
        trigger_type=TriggerType.DEFAULT,
        limit_offset=None,
        trailing_offset=None,
        offset_type=TrailingOffsetType.PRICE,
        quantity=instrument.make_qty(str(data["size"])),
        filled_qty=instrument.make_qty(str(data["filledSize"])),
        display_qty=None,
        avg_px=Decimal(str(avg_px)) if avg_px is not None else None,
        post_only=data["postOnly"],
        reduce_only=data["reduceOnly"],
        reject_reason=None,
        report_id=report_id,
        ts_accepted=created_at,
        ts_triggered=0,
        ts_last=created_at,
        ts_init=ts_init,
    )


def parse_trigger_order_status(
    account_id: AccountId,
    instrument: Instrument,
    data: Dict[str, Any],
    report_id: UUID4,
    ts_init: int,
) -> OrderStatusReport:
    client_id_str = data.get("clientId")
    trigger_price = data.get("triggerPrice")
    order_price = data.get("orderPrice")
    avg_px = data["avgFillPrice"]
    triggered_at = data["triggeredAt"]
    trail_value = data["trailValue"]
    created_at = int(pd.to_datetime(data["createdAt"]).to_datetime64())
    return OrderStatusReport(
        account_id=account_id,
        instrument_id=InstrumentId(Symbol(data["market"]), FTX_VENUE),
        client_order_id=ClientOrderId(client_id_str) if client_id_str is not None else None,
        order_list_id=None,
        venue_order_id=VenueOrderId(str(data["id"])),
        order_side=OrderSide.BUY if data["side"] == "buy" else OrderSide.SELL,
        order_type=parse_order_type(data=data),
        contingency_type=ContingencyType.NONE,
        time_in_force=TimeInForce.GTC,
        expire_time=None,
        order_status=parse_status(data),
        price=instrument.make_price(order_price) if order_price is not None else None,
        trigger_price=instrument.make_price(trigger_price) if trigger_price is not None else None,
        trigger_type=TriggerType.DEFAULT,
        limit_offset=None,
        trailing_offset=Decimal(str(trail_value)) if trail_value is not None else None,
        offset_type=TrailingOffsetType.PRICE,
        quantity=instrument.make_qty(str(data["size"])),
        filled_qty=instrument.make_qty(str(data["filledSize"])),
        display_qty=None,
        avg_px=Decimal(str(avg_px)) if avg_px is not None else None,
        post_only=False,
        reduce_only=data["reduceOnly"],
        reject_reason=None,
        report_id=report_id,
        ts_accepted=created_at,
        ts_triggered=int(pd.to_datetime(triggered_at, utc=True).to_datetime64())
        if triggered_at is not None
        else 0,
        ts_last=created_at,
        ts_init=ts_init,
    )


def parse_status(result: Dict[str, Any]) -> OrderStatus:
    status: Optional[str] = result.get("status")
    if status in ("new", "open"):
        if result["filledSize"] == 0:
            if result["triggeredAt"] is not None:
                return OrderStatus.TRIGGERED
            else:
                return OrderStatus.ACCEPTED
        else:
            return OrderStatus.PARTIALLY_FILLED
    elif status in ("closed", "cancelled"):
        if result["filledSize"] == 0:
            return OrderStatus.CANCELED
        return OrderStatus.FILLED
    elif status == "triggered":
        if result["filledSize"] == 0:
            return OrderStatus.TRIGGERED
        elif result["filledSize"] == result["size"]:
            return OrderStatus.FILLED
        else:
            return OrderStatus.PARTIALLY_FILLED
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"Cannot parse order status, was {status}")


def parse_order_type(data: Dict[str, Any], price_str: str = "orderPrice") -> OrderType:
    order_type: str = data["type"]
    if order_type == "limit":
        return OrderType.LIMIT
    elif order_type == "market":
        return OrderType.MARKET
    elif order_type in ("stop", "trailing_stop", "take_profit"):
        if data.get(price_str):
            return OrderType.STOP_LIMIT
        else:
            return OrderType.STOP_MARKET
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"Cannot parse order type, was {order_type}")


def parse_book_partial_ws(
    instrument_id: InstrumentId,
    data: Dict[str, Any],
    ts_init: int,
) -> OrderBookSnapshot:
    return OrderBookSnapshot(
        instrument_id=instrument_id,
        book_type=BookType.L2_MBP,
        bids=[[o[0], o[1]] for o in data.get("bids")],
        asks=[[o[0], o[1]] for o in data.get("asks")],
        ts_event=secs_to_nanos(data["time"]),
        ts_init=ts_init,
        update_id=data["checksum"],
    )


def parse_book_update_ws(
    instrument_id: InstrumentId,
    data: Dict[str, Any],
    ts_init: int,
) -> OrderBookDeltas:
    ts_event: int = secs_to_nanos(data["time"])
    update_id: int = data["checksum"]

    bid_deltas: List[OrderBookDelta] = [
        parse_book_delta_ws(instrument_id, OrderSide.BUY, d, ts_event, ts_init, update_id)
        for d in data["bids"]
    ]
    ask_deltas: List[OrderBookDelta] = [
        parse_book_delta_ws(instrument_id, OrderSide.SELL, d, ts_event, ts_init, update_id)
        for d in data["asks"]
    ]

    return OrderBookDeltas(
        instrument_id=instrument_id,
        book_type=BookType.L2_MBP,
        deltas=bid_deltas + ask_deltas,
        ts_event=ts_event,
        ts_init=ts_init,
        update_id=update_id,
    )


def parse_book_delta_ws(
    instrument_id: InstrumentId,
    side: OrderSide,
    delta: List[float],
    ts_event: int,
    ts_init: int,
    update_id: int,
) -> OrderBookDelta:
    price: float = delta[0]
    size: float = delta[1]

    order = Order(
        price=price,
        size=size,
        side=side,
    )

    return OrderBookDelta(
        instrument_id=instrument_id,
        book_type=BookType.L2_MBP,
        action=BookAction.UPDATE if size > 0.0 else BookAction.DELETE,
        order=order,
        ts_event=ts_event,
        ts_init=ts_init,
        update_id=update_id,
    )


def parse_ticker_ws(
    instrument: Instrument,
    data: Dict[str, Any],
    ts_init: int,
) -> FTXTicker:
    return FTXTicker(
        instrument_id=instrument.id,
        bid=Price(data["bid"], instrument.price_precision),
        ask=Price(data["ask"], instrument.price_precision),
        bid_size=Quantity(data["bidSize"], instrument.size_precision),
        ask_size=Quantity(data["askSize"], instrument.size_precision),
        last=Price(data["last"], instrument.price_precision),
        ts_event=secs_to_nanos(data["time"]),
        ts_init=ts_init,
    )


def parse_quote_tick_ws(
    instrument: Instrument,
    data: Dict[str, Any],
    ts_init: int,
) -> QuoteTick:
    return QuoteTick(
        instrument_id=instrument.id,
        bid=Price(data["bid"], instrument.price_precision),
        ask=Price(data["ask"], instrument.price_precision),
        bid_size=Quantity(data["bidSize"], instrument.size_precision),
        ask_size=Quantity(data["askSize"], instrument.size_precision),
        ts_event=secs_to_nanos(data["time"]),
        ts_init=ts_init,
    )


def parse_trade_ticks_ws(
    instrument: Instrument,
    data: List[Dict[str, Any]],
    ts_init: int,
) -> List[TradeTick]:
    ticks: List[TradeTick] = []
    for trade in data:
        tick: TradeTick = TradeTick(
            instrument_id=instrument.id,
            price=Price(trade["price"], instrument.price_precision),
            size=Quantity(trade["size"], instrument.size_precision),
            aggressor_side=AggressorSide.BUY if trade["side"] == "buy" else AggressorSide.SELL,
            trade_id=TradeId(str(trade["id"])),
            ts_event=pd.to_datetime(trade["time"], utc=True).to_datetime64(),
            ts_init=ts_init,
        )
        ticks.append(tick)

    return ticks


def parse_bars(
    instrument: Instrument,
    bar_type: BarType,
    data: List[Dict[str, Any]],
    ts_event_delta: int,
    ts_init: int,
) -> List[Bar]:
    bars: List[Bar] = []
    for row in data:
        bar: Bar = Bar(
            bar_type=bar_type,
            open=Price(row["open"], instrument.price_precision),
            high=Price(row["high"], instrument.price_precision),
            low=Price(row["low"], instrument.price_precision),
            close=Price(row["close"], instrument.price_precision),
            volume=Quantity(row["volume"], instrument.size_precision),
            check=True,
            ts_event=secs_to_nanos(row["time"]) + ts_event_delta,
            ts_init=ts_init,
        )
        bars.append(bar)

    return bars


def parse_market(
    account_info: Dict[str, Any],
    data: Dict[str, Any],
    ts_init: int,
) -> Instrument:
    native_symbol = Symbol(data["name"])
    asset_type = data["type"]

    # Create base asset
    if asset_type == "future":
        base_asset: str = data["underlying"]
        base_currency = Currency(
            code=base_asset,
            precision=8,
            iso4217=0,  # Currently undetermined for crypto assets
            name=base_asset,
            currency_type=CurrencyType.CRYPTO,
        )

        quote_currency: Currency = USD
    elif asset_type == "spot":
        base_asset = data["baseCurrency"]
        base_currency = Currency(
            code=base_asset,
            precision=8,
            iso4217=0,  # Currently undetermined for crypto assets
            name=base_asset,
            currency_type=CurrencyType.CRYPTO,
        )

        # Create quote asset
        quote_asset: str = data["quoteCurrency"]
        quote_currency = Currency.from_str(quote_asset)
        if quote_currency is None:
            quote_currency = Currency(
                code=quote_asset,
                precision=precision_from_str(str(data["priceIncrement"])),
                iso4217=0,  # Currently undetermined for crypto assets
                name=quote_asset,
                currency_type=CurrencyType.CRYPTO,
            )
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"unknown asset type, was {asset_type}")

    # symbol = Symbol(base_currency.code + "/" + quote_currency.code)
    instrument_id = InstrumentId(symbol=native_symbol, venue=FTX_VENUE)

    price_precision = precision_from_str(str(data["priceIncrement"]))
    size_precision = precision_from_str(str(data["sizeIncrement"]))
    price_increment = Price.from_str(str(data["priceIncrement"]))
    size_increment = Quantity.from_str(str(data["sizeIncrement"]))
    min_provide_size = data.get("minProvideSize")
    lot_size = (
        Quantity.from_str(str(min_provide_size)) if min_provide_size else Quantity.from_int(1)
    )
    margin_init = Decimal(str(account_info["initialMarginRequirement"]))
    margin_maint = Decimal(str(account_info["maintenanceMarginRequirement"]))
    maker_fee = Decimal(str(account_info.get("makerFee")))
    taker_fee = Decimal(str(account_info.get("takerFee")))

    if asset_type == "spot":
        # Create instrument
        return CurrencySpot(
            instrument_id=instrument_id,
            native_symbol=native_symbol,
            base_currency=base_currency,
            quote_currency=quote_currency,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=price_increment,
            size_increment=size_increment,
            lot_size=lot_size,
            max_quantity=None,
            min_quantity=None,
            max_notional=None,
            min_notional=None,
            max_price=None,
            min_price=None,
            margin_init=margin_init,
            margin_maint=margin_maint,
            maker_fee=maker_fee,
            taker_fee=taker_fee,
            ts_event=ts_init,
            ts_init=ts_init,
            info=data,
        )
    elif asset_type == "future":
        # Create instrument
        if native_symbol.value.endswith("-PERP"):
            return CryptoPerpetual(
                instrument_id=instrument_id,
                native_symbol=native_symbol,
                base_currency=base_currency,
                quote_currency=quote_currency,
                settlement_currency=USD,
                is_inverse=False,
                price_precision=price_precision,
                size_precision=size_precision,
                price_increment=price_increment,
                size_increment=size_increment,
                max_quantity=None,
                min_quantity=None,
                max_notional=None,
                min_notional=None,
                max_price=None,
                min_price=None,
                margin_init=margin_init,
                margin_maint=margin_maint,
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event=ts_init,
                ts_init=ts_init,
                info=data,
            )
        else:
            return Future(
                instrument_id=instrument_id,
                native_symbol=native_symbol,
                asset_class=AssetClass.CRYPTO,
                currency=USD,
                price_precision=price_precision,
                price_increment=price_increment,
                multiplier=Quantity.from_int(1),
                lot_size=Quantity.from_int(1),
                underlying=data["underlying"],
                expiry_date=datetime.utcnow().date(),  # TODO(cs): Implement
                # margin_init=margin_init,  # TODO(cs): Implement
                # margin_maint=margin_maint,  # TODO(cs): Implement
                # maker_fee=maker_fee,  # TODO(cs): Implement
                # taker_fee=taker_fee,  # TODO(cs): Implement
                ts_event=ts_init,
                ts_init=ts_init,
            )
    else:  # pragma: no cover (design-time error)
        raise ValueError(f"Cannot parse market instrument: unknown asset type {asset_type}")
