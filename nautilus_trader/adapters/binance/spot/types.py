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
from typing import Any, Dict

from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.identifiers import InstrumentId


class BinanceSpotTicker(Ticker):
    """
    Represents a `Binance Spot/Margin` 24hr statistics ticker.

    This data type includes the raw data provided by `Binance`.

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
    prev_close_price : Decimal
        The previous close price.
    last_price : Decimal
        The last price.
    last_qty : Decimal
        The last quantity.
    bid_price : Decimal
        The bid price.
    ask_price : Decimal
        The ask price.
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
        The UNIX timestamp (milliseconds) when the ticker opened.
    close_time_ms : int
        The UNIX timestamp (milliseconds) when the ticker closed.
    close_time_ms : int
        The UNIX timestamp (milliseconds) when the ticker closed.
    first_id : int
        The first trade match ID (assigned by the venue) for the ticker.
    last_id : int
        The last trade match ID (assigned by the venue) for the ticker.
    count : int
        The count of trades over the tickers time range.
    ts_event : int64
        The UNIX timestamp (nanoseconds) when the ticker event occurred.
    ts_init : int64
        The UNIX timestamp (nanoseconds) when the object was initialized.

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#24hr-ticker-price-change-statistics
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        price_change: Decimal,
        price_change_percent: Decimal,
        weighted_avg_price: Decimal,
        prev_close_price: Decimal,
        last_price: Decimal,
        last_qty: Decimal,
        bid_price: Decimal,
        ask_price: Decimal,
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
    ):
        super().__init__(
            instrument_id=instrument_id,
            ts_event=ts_event,
            ts_init=ts_init,
        )

        self.price_change = price_change
        self.price_change_percent = price_change_percent
        self.weighted_avg_price = weighted_avg_price
        self.prev_close_price = prev_close_price
        self.last_price = last_price
        self.last_qty = last_qty
        self.bid_price = bid_price
        self.ask_price = ask_price
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
            f"ask_price={self.ask_price}, "
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

    @staticmethod
    def from_dict(values: Dict[str, Any]) -> "BinanceSpotTicker":
        """
        Return a `Binance Spot/Margin` ticker parsed from the given values.

        Parameters
        ----------
        values : dict[str, Any]
            The values for initialization.

        Returns
        -------
        BinanceSpotTicker

        """
        return BinanceSpotTicker(
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            price_change=Decimal(values["price_change"]),
            price_change_percent=Decimal(values["price_change_percent"]),
            weighted_avg_price=Decimal(values["weighted_avg_price"]),
            prev_close_price=Decimal(values["prev_close_price"]),
            last_price=Decimal(values["last_price"]),
            last_qty=Decimal(values["last_qty"]),
            bid_price=Decimal(values["bid_price"]),
            ask_price=Decimal(values["ask_price"]),
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
    def to_dict(obj: "BinanceSpotTicker") -> Dict[str, Any]:
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
            "prev_close_price": str(obj.prev_close_price),
            "last_price": str(obj.last_price),
            "last_qty": str(obj.last_qty),
            "bid_price": str(obj.bid_price),
            "ask_price": str(obj.ask_price),
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
