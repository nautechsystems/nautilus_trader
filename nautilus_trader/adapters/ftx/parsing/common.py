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

import datetime
from decimal import Decimal
from typing import Any, Dict, Optional

import pandas as pd

from nautilus_trader.adapters.ftx.core.constants import FTX_VENUE
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.string import precision_from_str
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.crypto_future import CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual import CryptoPerpetual
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.objects import PRICE_MAX
from nautilus_trader.model.objects import PRICE_MIN
from nautilus_trader.model.objects import QUANTITY_MAX
from nautilus_trader.model.objects import QUANTITY_MIN
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def parse_order_status(result: Dict[str, Any]) -> OrderStatus:
    status: Optional[str] = result.get("status")
    if status in ("new", "open"):
        if result["filledSize"] == 0:
            if result.get("triggeredAt") is not None:
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
        raise RuntimeError(f"cannot parse order status, was {status}")


def parse_order_type(data: Dict[str, Any], price_str: str = "orderPrice") -> OrderType:
    order_type: str = data["type"]
    if order_type == "limit":
        return OrderType.LIMIT
    elif order_type == "market":
        return OrderType.MARKET
    elif order_type in ("stop", "trailing_stop", "trailingStop", "take_profit", "takeProfit"):
        if data.get(price_str):
            return OrderType.STOP_LIMIT
        else:
            return OrderType.STOP_MARKET
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"cannot parse order type, was {order_type}")


def parse_trade_report(
    account_id: AccountId,
    instrument: Instrument,
    data: Dict[str, Any],
    report_id: UUID4,
    ts_init: int,
) -> TradeReport:
    return TradeReport(
        account_id=account_id,
        instrument_id=instrument.id,
        venue_order_id=VenueOrderId(str(data["orderId"])),
        trade_id=TradeId(str(data["tradeId"])),
        order_side=OrderSide.BUY if data["side"] == "buy" else OrderSide.SELL,
        last_qty=instrument.make_qty(data["size"]),
        last_px=instrument.make_price(data["price"]),
        commission=Money(data["fee"], Currency.from_str(data["feeCurrency"])),
        liquidity_side=LiquiditySide.TAKER if data["liquidity"] == "taker" else LiquiditySide.MAKER,
        report_id=report_id,
        ts_event=pd.to_datetime(data["time"], utc=True).value,
        ts_init=ts_init,
    )


def parse_position_report(
    account_id: AccountId,
    instrument: Instrument,
    data: Dict[str, Any],
    report_id: UUID4,
    ts_init: int,
) -> PositionStatusReport:
    net_size = data["netSize"]
    return PositionStatusReport(
        account_id=account_id,
        instrument_id=instrument.id,
        position_side=PositionSide.LONG if net_size > 0 else PositionSide.SHORT,
        quantity=instrument.make_qty(abs(net_size)),
        report_id=report_id,
        ts_last=ts_init,
        ts_init=ts_init,
    )


def parse_instrument(
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

    PyCondition.in_range(float(data["priceIncrement"]), PRICE_MIN, PRICE_MAX, "priceIncrement")
    PyCondition.in_range(float(data["sizeIncrement"]), QUANTITY_MIN, QUANTITY_MAX, "sizeIncrement")
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
        return CurrencyPair(
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
            expiry_str = data["name"].rsplit("-", maxsplit=1)[1]
            expiry_date = datetime.datetime.strptime(
                f"{expiry_str}{datetime.date.today().year}", "%m%d%Y"
            ).date()
            return CryptoFuture(
                instrument_id=instrument_id,
                native_symbol=native_symbol,
                underlying=base_currency,
                quote_currency=quote_currency,
                settlement_currency=base_currency,
                price_precision=price_precision,
                size_precision=size_precision,
                price_increment=price_increment,
                size_increment=size_increment,
                multiplier=Quantity.from_int(1),
                lot_size=Quantity.from_int(1),
                expiry_date=expiry_date,
                margin_init=margin_init,
                margin_maint=margin_maint,
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event=ts_init,
                ts_init=ts_init,
            )
    else:  # pragma: no cover (design-time error)
        raise ValueError(f"Cannot parse market instrument: unknown asset type {asset_type}")
