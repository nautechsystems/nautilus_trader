# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Test the VPIN indicator Python bindings.
"""

import pytest

from nautilus_trader.indicators import Vpin
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick


INSTRUMENT_ID = InstrumentId.from_str("AUD/USD.SIM")


def _trade(price: str, size: str, side: AggressorSide) -> TradeTick:
    return TradeTick(
        instrument_id=INSTRUMENT_ID,
        price=Price.from_str(price),
        size=Quantity.from_str(size),
        aggressor_side=side,
        trade_id=TradeId("T-1"),
        ts_event=0,
        ts_init=0,
    )


def test_vpin_from_aggressor_trades() -> None:
    """
    Test buy 6 and sell 4, then sell 10, give buckets of 0.2 and 1.0 and VPIN 0.6.
    """
    indicator = Vpin(10.0, 2)

    indicator.handle_trade_tick(_trade("1.00", "6", AggressorSide.BUY))
    indicator.handle_trade_tick(_trade("1.00", "4", AggressorSide.SELL))
    indicator.handle_trade_tick(_trade("1.00", "10", AggressorSide.SELL))

    assert indicator.count == 2
    assert indicator.initialized is True
    assert indicator.value == pytest.approx(0.6, abs=1e-12)
    assert indicator.bucket_volume == 10.0
    assert indicator.window == 2
    assert indicator.sigma_window == 50


@pytest.mark.parametrize(
    ("closes", "volume"),
    [
        (("100.00", "100.01", "100.02"), "10"),
        (("1", "2364", "4727.000000000000000000"), "10.000000000000000000"),
    ],
)
def test_vpin_equal_decimal_bar_changes_split_evenly(
    closes: tuple[str, ...],
    volume: str,
) -> None:
    """
    Test that equal fixed-point close changes split volume evenly.
    """
    indicator = Vpin(10.0, 1, sigma_window=2)
    bar_type = BarType.from_str(f"{INSTRUMENT_ID}-1-MINUTE-LAST-EXTERNAL")

    for close in closes:
        price = Price.from_str(close)
        indicator.handle_bar(
            Bar(bar_type, price, price, price, price, Quantity.from_str(volume), 0, 0),
        )

    assert indicator.count == 1
    assert indicator.initialized is True
    assert indicator.value == 0.0


def test_vpin_accepts_native_precision_trade() -> None:
    """
    Test that trade prices and quantities can use native precision.
    """
    indicator = Vpin(1.0, 1)
    trade = _trade("1.000000000000000000", "1.000000000000000000", AggressorSide.BUY)
    indicator.handle_trade_tick(trade)

    assert indicator.count == 1
    assert indicator.value == 1.0


def test_vpin_update_raw_and_reset() -> None:
    """
    Test pre-classified volume keeps its buy share across buckets and reset clears it.
    """
    indicator = Vpin(10.0, 2, sigma_window=5)

    indicator.update_raw(15.0, 5.0)

    assert indicator.sigma_window == 5
    assert indicator.value == pytest.approx(0.5, abs=1e-12)

    indicator.reset()

    assert indicator.count == 0
    assert indicator.value == 0.0
    assert indicator.initialized is False
    assert indicator.has_inputs is False


@pytest.mark.parametrize("bucket_volume", [0.0, -1.0, float("nan"), float("inf")])
def test_vpin_invalid_bucket_volume_raises_value_error(bucket_volume: float) -> None:
    """
    Test a bucket volume that is not finite and positive raises ValueError.
    """
    with pytest.raises(ValueError, match="bucket_volume must be finite and > 0"):
        Vpin(bucket_volume, 2)
