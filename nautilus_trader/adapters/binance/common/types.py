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

from decimal import Decimal
from typing import Any

from nautilus_trader.core.data import Data
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BinanceBar(Bar):
    """
    Represents an aggregated Binance bar.

    This data type includes the raw data provided by Binance.

    Parameters
    ----------
    bar_type : BarType
        The bar type for this bar.
    open : Price
        The bars open price.
    high : Price
        The bars high price.
    low : Price
        The bars low price.
    close : Price
        The bars close price.
    volume : Quantity
        The bars volume.
    quote_volume : Decimal
        The bars quote asset volume.
    count : int
        The number of trades for the bar.
    taker_buy_base_volume : Decimal
        The liquidity taker volume on the buy side for the base asset.
    taker_buy_quote_volume : Decimal
        The liquidity taker volume on the buy side for the quote asset.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#kline-candlestick-data
    https://binance-docs.github.io/apidocs/futures/en/#kline-candlestick-data

    """

    def __init__(
        self,
        bar_type: BarType,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
        volume: Quantity,
        quote_volume: Decimal,
        count: int,
        taker_buy_base_volume: Decimal,
        taker_buy_quote_volume: Decimal,
        ts_event: int,
        ts_init: int,
    ) -> None:
        super().__init__(
            bar_type=bar_type,
            open=open,
            high=high,
            low=low,
            close=close,
            volume=volume,
            ts_event=ts_event,
            ts_init=ts_init,
        )

        self.quote_volume = quote_volume
        self.count = count
        self.taker_buy_base_volume = taker_buy_base_volume
        self.taker_buy_quote_volume = taker_buy_quote_volume
        self.taker_sell_base_volume = self.volume - self.taker_buy_base_volume
        self.taker_sell_quote_volume = self.quote_volume - self.taker_buy_quote_volume

    def __getstate__(self):
        return (
            *super().__getstate__(),
            str(self.quote_volume),
            self.count,
            str(self.taker_buy_base_volume),
            str(self.taker_buy_quote_volume),
            str(self.taker_sell_base_volume),
            str(self.taker_sell_quote_volume),
        )

    def __setstate__(self, state):
        super().__setstate__(state[:14])
        self.quote_volume = Decimal(state[14])
        self.count = state[15]
        self.taker_buy_base_volume = Decimal(state[16])
        self.taker_buy_quote_volume = Decimal(state[17])
        self.taker_sell_base_volume = Decimal(state[18])
        self.taker_sell_quote_volume = Decimal(state[19])

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"bar_type={self.bar_type}, "
            f"open={self.open}, "
            f"high={self.high}, "
            f"low={self.low}, "
            f"close={self.close}, "
            f"volume={self.volume}, "
            f"quote_volume={self.quote_volume}, "
            f"count={self.count}, "
            f"taker_buy_base_volume={self.taker_buy_base_volume}, "
            f"taker_buy_quote_volume={self.taker_buy_quote_volume}, "
            f"taker_sell_base_volume={self.taker_sell_base_volume}, "
            f"taker_sell_quote_volume={self.taker_sell_quote_volume}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    def from_dict(values: dict[str, Any]) -> BinanceBar:
        """
        Return a Binance bar parsed from the given values.

        Parameters
        ----------
        values : dict[str, Any]
            The values for initialization.

        Returns
        -------
        BinanceBar

        """
        return BinanceBar(
            bar_type=BarType.from_str(values["bar_type"]),
            open=Price.from_str(values["open"]),
            high=Price.from_str(values["high"]),
            low=Price.from_str(values["low"]),
            close=Price.from_str(values["close"]),
            volume=Quantity.from_str(values["volume"]),
            quote_volume=Decimal(values["quote_volume"]),
            count=values["count"],
            taker_buy_base_volume=Decimal(values["taker_buy_base_volume"]),
            taker_buy_quote_volume=Decimal(values["taker_buy_quote_volume"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    def to_dict(obj: BinanceBar) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, Any]

        """
        return {
            "type": type(obj).__name__,
            "bar_type": str(obj.bar_type),
            "open": str(obj.open),
            "high": str(obj.high),
            "low": str(obj.low),
            "close": str(obj.close),
            "volume": str(obj.volume),
            "quote_volume": str(obj.quote_volume),
            "count": obj.count,
            "taker_buy_base_volume": str(obj.taker_buy_base_volume),
            "taker_buy_quote_volume": str(obj.taker_buy_quote_volume),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }


class BinanceTicker(Data):
    """
    Represents a Binance 24hr statistics ticker.

    This data type includes the raw data provided by Binance.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    price_change : Decimal
        The price change.
    price_change_percent : Decimal
        The price change percent.
    weighted_avg_price : Decimal
        The weighted average price.
    prev_close_price : Decimal, optional
        The previous close price.
    last_price : Decimal
        The last price.
    last_qty : Decimal
        The last quantity.
    bid_price : Decimal, optional
        The bid price.
    bid_qty : Decimal, optional
        The bid quantity.
    ask_price : Decimal, optional
        The ask price.
    ask_qty : Decimal, optional
        The ask quantity.
    open_price : Decimal
        The open price.
    high_price : Decimal
        The high price.
    low_price : Decimal
        The low price.
    volume : Decimal
        The volume.
    quote_volume : Decimal
        The quote volume.
    open_time_ms : int
        UNIX timestamp (milliseconds) when the ticker opened.
    close_time_ms : int
        UNIX timestamp (milliseconds) when the ticker closed.
    first_id : int
        The first trade match ID (assigned by the venue) for the ticker.
    last_id : int
        The last trade match ID (assigned by the venue) for the ticker.
    count : int
        The count of trades over the tickers time range.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the ticker event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#24hr-ticker-price-change-statistics
    https://binance-docs.github.io/apidocs/futures/en/#24hr-ticker-price-change-statistics

    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        price_change: Decimal,
        price_change_percent: Decimal,
        weighted_avg_price: Decimal,
        last_price: Decimal,
        last_qty: Decimal,
        open_price: Decimal,
        high_price: Decimal,
        low_price: Decimal,
        volume: Decimal,
        quote_volume: Decimal,
        open_time_ms: int,
        close_time_ms: int,
        first_id: int,
        last_id: int,
        count: int,
        ts_event: int,
        ts_init: int,
        prev_close_price: Decimal | None = None,
        bid_price: Decimal | None = None,
        bid_qty: Decimal | None = None,
        ask_price: Decimal | None = None,
        ask_qty: Decimal | None = None,
    ) -> None:
        self.instrument_id = instrument_id
        self.price_change = price_change
        self.price_change_percent = price_change_percent
        self.weighted_avg_price = weighted_avg_price
        self.prev_close_price = prev_close_price
        self.last_price = last_price
        self.last_qty = last_qty
        self.bid_price = bid_price
        self.bid_qty = bid_qty
        self.ask_price = ask_price
        self.ask_qty = ask_qty
        self.open_price = open_price
        self.high_price = high_price
        self.low_price = low_price
        self.volume = volume
        self.quote_volume = quote_volume
        self.open_time_ms = open_time_ms
        self.close_time_ms = close_time_ms
        self.first_id = first_id
        self.last_id = last_id
        self.count = count
        self._ts_event = ts_event
        self._ts_init = ts_init

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, BinanceTicker):
            return False
        return self.instrument_id == other.instrument_id

    def __hash__(self) -> int:
        return hash(self.instrument_id)

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.value}, "
            f"price_change={self.price_change}, "
            f"price_change_percent={self.price_change_percent}, "
            f"weighted_avg_price={self.weighted_avg_price}, "
            f"prev_close_price={self.prev_close_price}, "
            f"last_price={self.last_price}, "
            f"last_qty={self.last_qty}, "
            f"bid_price={self.bid_price}, "
            f"bid_qty={self.bid_qty}, "
            f"ask_price={self.ask_price}, "
            f"ask_qty={self.ask_qty}, "
            f"open_price={self.open_price}, "
            f"high_price={self.high_price}, "
            f"low_price={self.low_price}, "
            f"volume={self.volume}, "
            f"quote_volume={self.quote_volume}, "
            f"open_time_ms={self.open_time_ms}, "
            f"close_time_ms={self.close_time_ms}, "
            f"first_id={self.first_id}, "
            f"last_id={self.last_id}, "
            f"count={self.count}, "
            f"ts_event={self.ts_event}, "
            f"ts_init={self.ts_init})"
        )

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._ts_init

    @staticmethod
    def from_dict(values: dict[str, Any]) -> BinanceTicker:
        """
        Return a Binance Spot/Margin ticker parsed from the given values.

        Parameters
        ----------
        values : dict[str, Any]
            The values for initialization.

        Returns
        -------
        BinanceTicker

        """
        prev_close_str: str | None = values.get("prev_close")
        bid_price_str: str | None = values.get("bid_price")
        bid_qty_str: str | None = values.get("bid_qty")
        ask_price_str: str | None = values.get("ask_price")
        ask_qty_str: str | None = values.get("ask_qty")
        return BinanceTicker(
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            price_change=Decimal(values["price_change"]),
            price_change_percent=Decimal(values["price_change_percent"]),
            weighted_avg_price=Decimal(values["weighted_avg_price"]),
            prev_close_price=Decimal(prev_close_str) if prev_close_str is not None else None,
            last_price=Decimal(values["last_price"]),
            last_qty=Decimal(values["last_qty"]),
            bid_price=Decimal(bid_price_str) if bid_price_str is not None else None,
            bid_qty=Decimal(bid_qty_str) if bid_qty_str is not None else None,
            ask_price=Decimal(ask_price_str) if ask_price_str is not None else None,
            ask_qty=Decimal(ask_qty_str) if ask_qty_str is not None else None,
            open_price=Decimal(values["open_price"]),
            high_price=Decimal(values["high_price"]),
            low_price=Decimal(values["low_price"]),
            volume=Decimal(values["volume"]),
            quote_volume=Decimal(values["quote_volume"]),
            open_time_ms=values["open_time_ms"],
            close_time_ms=values["close_time_ms"],
            first_id=values["first_id"],
            last_id=values["last_id"],
            count=values["count"],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    def to_dict(obj: BinanceTicker) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, Any]

        """
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.value,
            "price_change": str(obj.price_change),
            "price_change_percent": str(obj.price_change_percent),
            "weighted_avg_price": str(obj.weighted_avg_price),
            "prev_close_price": (
                str(obj.prev_close_price) if obj.prev_close_price is not None else None
            ),
            "last_price": str(obj.last_price),
            "last_qty": str(obj.last_qty),
            "bid_price": str(obj.bid_price),
            "bid_qty": str(obj.bid_qty) if obj.bid_qty is not None else None,
            "ask_price": str(obj.ask_price),
            "ask_qty": str(obj.ask_qty) if obj.ask_qty is not None else None,
            "open_price": str(obj.open_price),
            "high_price": str(obj.high_price),
            "low_price": str(obj.low_price),
            "volume": str(obj.volume),
            "quote_volume": str(obj.quote_volume),
            "open_time_ms": obj.open_time_ms,
            "close_time_ms": obj.close_time_ms,
            "first_id": obj.first_id,
            "last_id": obj.last_id,
            "count": obj.count,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }
