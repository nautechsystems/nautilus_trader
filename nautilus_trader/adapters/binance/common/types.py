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

from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BinanceBar(Bar):
    """
    Represents an aggregated `Binance` bar.

    This data type includes the raw data provided by `Binance`.

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
    quote_volume : Quantity
        The bars quote asset volume.
    count : int
        The number of trades for the bar.
    taker_buy_base_volume : Quantity
        The liquidity taker volume on the buy side for the base asset.
    taker_buy_quote_volume : Quantity
        The liquidity taker volume on the buy side for the quote asset.
    ts_event : int64
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init: int64
        The UNIX timestamp (nanoseconds) when the data object was initialized.

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#kline-candlestick-data
    """

    def __init__(
        self,
        bar_type: BarType,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
        volume: Quantity,
        quote_volume: Quantity,
        count: int,
        taker_buy_base_volume: Quantity,
        taker_buy_quote_volume: Quantity,
        ts_event: int,
        ts_init: int,
    ):
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
        taker_sell_base_volume: Decimal = self.volume - self.taker_buy_base_volume
        taker_sell_quote_volume: Decimal = self.quote_volume - self.taker_buy_quote_volume
        self.taker_sell_base_volume = Quantity.from_str(str(taker_sell_base_volume))
        self.taker_sell_quote_volume = Quantity.from_str(str(taker_sell_quote_volume))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"bar_type={self.type}, "
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
            f"ts_event={self.ts_event},"
            f"ts_init={self.ts_init})"
        )

    @staticmethod
    def from_dict(values: Dict[str, Any]) -> "BinanceBar":
        """
        Return a `Binance` bar parsed from the given values.

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
            quote_volume=Quantity.from_str(values["quote_volume"]),
            count=values["count"],
            taker_buy_base_volume=Quantity.from_str(values["taker_buy_base_volume"]),
            taker_buy_quote_volume=Quantity.from_str(values["taker_buy_quote_volume"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    def to_dict(obj: "BinanceBar") -> Dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, Any]

        """
        return {
            "type": type(obj).__name__,
            "bar_type": str(obj.type),
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
