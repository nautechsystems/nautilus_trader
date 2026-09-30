# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Test indicator handler float precision boundaries.
"""

from collections.abc import Callable
from typing import Any

import pytest

from nautilus_trader.indicators import AdaptiveMovingAverage
from nautilus_trader.indicators import ArcherMovingAveragesTrends
from nautilus_trader.indicators import AroonOscillator
from nautilus_trader.indicators import AverageTrueRange
from nautilus_trader.indicators import Bias
from nautilus_trader.indicators import BollingerBands
from nautilus_trader.indicators import BookImbalanceRatio
from nautilus_trader.indicators import ChandeMomentumOscillator
from nautilus_trader.indicators import CommodityChannelIndex
from nautilus_trader.indicators import DirectionalMovement
from nautilus_trader.indicators import DonchianChannel
from nautilus_trader.indicators import DoubleExponentialMovingAverage
from nautilus_trader.indicators import EfficiencyRatio
from nautilus_trader.indicators import ExponentialMovingAverage
from nautilus_trader.indicators import FuzzyCandlesticks
from nautilus_trader.indicators import HullMovingAverage
from nautilus_trader.indicators import IchimokuCloud
from nautilus_trader.indicators import KeltnerChannel
from nautilus_trader.indicators import KeltnerPosition
from nautilus_trader.indicators import KlingerVolumeOscillator
from nautilus_trader.indicators import LinearRegression
from nautilus_trader.indicators import MovingAverageConvergenceDivergence
from nautilus_trader.indicators import OnBalanceVolume
from nautilus_trader.indicators import Pressure
from nautilus_trader.indicators import PsychologicalLine
from nautilus_trader.indicators import RateOfChange
from nautilus_trader.indicators import RelativeStrengthIndex
from nautilus_trader.indicators import RelativeVolatilityIndex
from nautilus_trader.indicators import SimpleMovingAverage
from nautilus_trader.indicators import SpreadAnalyzer
from nautilus_trader.indicators import Stochastics
from nautilus_trader.indicators import Swings
from nautilus_trader.indicators import VariableIndexDynamicAverage
from nautilus_trader.indicators import VerticalHorizontalFilter
from nautilus_trader.indicators import VolatilityRatio
from nautilus_trader.indicators import VolumeWeightedAveragePrice
from nautilus_trader.indicators import WeightedMovingAverage
from nautilus_trader.indicators import WilderMovingAverage
from nautilus_trader.indicators import ZScore
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import PriceType
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick


INSTRUMENT_ID = InstrumentId.from_str("AUD/USD.SIM")
OTHER_INSTRUMENT_ID = InstrumentId.from_str("GBP/USD.SIM")
BAR_TYPE = BarType.from_str("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL")
FLOAT_PRECISION = 16
ABOVE_FLOAT_PRECISION_CASES = [
    pytest.param(17, id="precision-17"),
    pytest.param(18, id="precision-18"),
]


def _price(value: float, precision: int) -> Price:
    return Price.from_str(f"{value:.{precision}f}")


def _quantity(value: float, precision: int) -> Quantity:
    return Quantity.from_str(f"{value:.{precision}f}")


def _bar(price_precision: int, volume_precision: int) -> Bar:
    return Bar(
        bar_type=BAR_TYPE,
        open=_price(1.5, price_precision),
        high=_price(2.0, price_precision),
        low=_price(1.0, price_precision),
        close=_price(1.5, price_precision),
        volume=_quantity(100.0, volume_precision),
        ts_event=0,
        ts_init=0,
    )


def _quote(precision: int, instrument_id: InstrumentId = INSTRUMENT_ID) -> QuoteTick:
    return QuoteTick(
        instrument_id=instrument_id,
        bid_price=_price(1.5, precision),
        ask_price=_price(2.0, precision),
        bid_size=Quantity.from_int(100),
        ask_size=Quantity.from_int(100),
        ts_event=0,
        ts_init=0,
    )


def _trade(precision: int) -> TradeTick:
    return TradeTick(
        instrument_id=INSTRUMENT_ID,
        price=_price(1.5, precision),
        size=Quantity.from_int(100),
        aggressor_side=AggressorSide.BUY,
        trade_id=TradeId("T-1"),
        ts_event=0,
        ts_init=0,
    )


def _book(precision: int) -> OrderBook:
    book = OrderBook(instrument_id=INSTRUMENT_ID, book_type=BookType.L3_MBO)
    bid = BookOrder(OrderSide.BUY, _price(1.5, precision), _quantity(2.0, precision), 1)
    ask = BookOrder(OrderSide.SELL, _price(2.0, precision), _quantity(4.0, precision), 2)
    book.add(bid, flags=0, sequence=1, ts_event=1)
    book.add(ask, flags=0, sequence=2, ts_event=2)
    return book


def _book_sizes(precision: int) -> tuple[Quantity, Quantity]:
    return _quantity(2.0, precision), _quantity(4.0, precision)


QUOTE_TRADE_BAR_INDICATORS = [
    ("ama", lambda: AdaptiveMovingAverage(10, 2, 30, PriceType.MID)),
    ("dema", lambda: DoubleExponentialMovingAverage(10, PriceType.MID)),
    ("ema", lambda: ExponentialMovingAverage(10, PriceType.MID)),
    ("hma", lambda: HullMovingAverage(10, PriceType.MID)),
    ("rma", lambda: WilderMovingAverage(10, PriceType.MID)),
    ("sma", lambda: SimpleMovingAverage(10, PriceType.MID)),
    ("vidya", lambda: VariableIndexDynamicAverage(10, PriceType.MID)),
    ("wma", lambda: WeightedMovingAverage(3, [1.0, 2.0, 3.0], PriceType.MID)),
    ("zscore", lambda: ZScore(10, PriceType.MID)),
    ("aroon", lambda: AroonOscillator(10)),
    ("bb", lambda: BollingerBands(10, 2.0)),
    ("macd", lambda: MovingAverageConvergenceDivergence(3, 10, price_type=PriceType.MID)),
    ("rsi", lambda: RelativeStrengthIndex(10)),
]
PRICE_BAR_INDICATORS = [
    *QUOTE_TRADE_BAR_INDICATORS,
    ("lr", lambda: LinearRegression(10)),
    ("amat", lambda: ArcherMovingAveragesTrends(3, 10, 5)),
    ("bias", lambda: Bias(10)),
    ("cci", lambda: CommodityChannelIndex(10, 0.015)),
    ("cmo", lambda: ChandeMomentumOscillator(10)),
    ("dm", lambda: DirectionalMovement(10)),
    ("ichimoku", IchimokuCloud),
    ("psl", lambda: PsychologicalLine(10)),
    ("roc", lambda: RateOfChange(10)),
    ("stochastics", lambda: Stochastics(3, 3)),
    ("swings", lambda: Swings(3)),
    ("vhf", lambda: VerticalHorizontalFilter(10)),
    ("atr", lambda: AverageTrueRange(10)),
    ("dc", lambda: DonchianChannel(10)),
    ("fuzzy", lambda: FuzzyCandlesticks(10, 0.5, 1.0, 2.0, 3.0)),
    ("kc", lambda: KeltnerChannel(10, 2.0)),
    ("kp", lambda: KeltnerPosition(10, 2.0)),
    ("rvi", lambda: RelativeVolatilityIndex(10)),
    ("vr", lambda: VolatilityRatio(3, 10)),
    ("efficiency-ratio", lambda: EfficiencyRatio(10)),
]
VOLUME_BAR_INDICATORS = [
    ("vwap", VolumeWeightedAveragePrice),
    ("kvo", lambda: KlingerVolumeOscillator(3, 10, 5)),
    ("obv", lambda: OnBalanceVolume(10)),
    ("pressure", lambda: Pressure(10)),
]
HANDLER_CASES = [
    *(
        pytest.param(factory, "handle_quote_tick", _quote, id=f"{name}-quote")
        for name, factory in QUOTE_TRADE_BAR_INDICATORS
    ),
    *(
        pytest.param(factory, "handle_trade_tick", _trade, id=f"{name}-trade")
        for name, factory in QUOTE_TRADE_BAR_INDICATORS
    ),
    *(
        pytest.param(factory, "handle_bar", lambda p: _bar(p, p), id=f"{name}-bar")
        for name, factory in PRICE_BAR_INDICATORS + VOLUME_BAR_INDICATORS
    ),
    pytest.param(
        lambda: SpreadAnalyzer(INSTRUMENT_ID, 10),
        "handle_quote_tick",
        _quote,
        id="spread-analyzer-quote",
    ),
    pytest.param(BookImbalanceRatio, "handle_book", _book, id="book-imbalance-book"),
]


@pytest.mark.parametrize(("factory", "handler", "make_input"), HANDLER_CASES)
def test_indicator_handler_accepts_float_precision(
    factory: Callable[[], Any],
    handler: str,
    make_input: Callable[[int], object],
) -> None:
    """
    Test indicator handler accepts float precision.
    """
    indicator = factory()

    getattr(indicator, handler)(make_input(FLOAT_PRECISION))

    assert indicator.has_inputs is True


@pytest.mark.parametrize("precision", ABOVE_FLOAT_PRECISION_CASES)
@pytest.mark.parametrize(("factory", "handler", "make_input"), HANDLER_CASES)
def test_indicator_handler_rejects_precision_above_float_precision(
    factory: Callable[[], Any],
    handler: str,
    make_input: Callable[[int], object],
    precision: int,
) -> None:
    """
    Test indicator handler rejects precision above float precision.
    """
    indicator = factory()

    with pytest.raises(ValueError, match="maximum float precision") as exc_info:
        getattr(indicator, handler)(make_input(precision))

    assert str(exc_info.value) == (
        f"Fixed-point precision {precision} exceeds maximum float precision 16"
    )
    assert indicator.has_inputs is False


@pytest.mark.parametrize("precision", ABOVE_FLOAT_PRECISION_CASES)
@pytest.mark.parametrize(
    "factory",
    [pytest.param(factory, id=name) for name, factory in VOLUME_BAR_INDICATORS],
)
def test_volume_bar_handler_rejects_volume_above_float_precision(
    factory: Callable[[], Any],
    precision: int,
) -> None:
    """
    Test volume bar handler rejects volume above float precision.
    """
    indicator = factory()

    with pytest.raises(ValueError, match="maximum float precision") as exc_info:
        indicator.handle_bar(_bar(FLOAT_PRECISION, precision))

    assert str(exc_info.value) == (
        f"Fixed-point precision {precision} exceeds maximum float precision 16"
    )
    assert indicator.has_inputs is False


@pytest.mark.parametrize(
    "factory",
    [pytest.param(factory, id=name) for name, factory in PRICE_BAR_INDICATORS],
)
def test_price_bar_handler_accepts_volume_above_float_precision(
    factory: Callable[[], Any],
) -> None:
    """
    Test price bar handler accepts volume above float precision.
    """
    indicator = factory()

    indicator.handle_bar(_bar(FLOAT_PRECISION, 18))

    assert indicator.has_inputs is True


def test_book_imbalance_update_accepts_float_precision() -> None:
    """
    Test book imbalance update accepts float precision.
    """
    indicator = BookImbalanceRatio()

    indicator.update(*_book_sizes(FLOAT_PRECISION))

    assert indicator.value == 0.5


@pytest.mark.parametrize("precision", ABOVE_FLOAT_PRECISION_CASES)
def test_book_imbalance_update_rejects_precision_above_float_precision(precision: int) -> None:
    """
    Test book imbalance update rejects precision above float precision.
    """
    indicator = BookImbalanceRatio()

    with pytest.raises(ValueError, match="maximum float precision") as exc_info:
        indicator.update(*_book_sizes(precision))

    assert str(exc_info.value) == (
        f"Fixed-point precision {precision} exceeds maximum float precision 16"
    )
    assert indicator.has_inputs is False


def test_book_imbalance_update_accepts_one_sided_book_above_float_precision() -> None:
    """
    Test book imbalance update accepts one-sided book above float precision.
    """
    indicator = BookImbalanceRatio()

    indicator.update(_quantity(2.0, 18), None)

    assert indicator.has_inputs is True
    assert indicator.initialized is False


def test_spread_analyzer_ignores_other_instrument_above_float_precision() -> None:
    """
    Test spread analyzer ignores other instrument above float precision.
    """
    indicator = SpreadAnalyzer(INSTRUMENT_ID, 10)

    indicator.handle_quote_tick(_quote(18, OTHER_INSTRUMENT_ID))

    assert indicator.has_inputs is False
