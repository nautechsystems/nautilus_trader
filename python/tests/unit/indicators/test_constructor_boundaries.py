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
Test the exported indicator constructor size boundaries.
"""

import struct
import subprocess
import sys
from collections.abc import Iterator
from types import MappingProxyType

import pytest

from nautilus_trader import indicators
from nautilus_trader.model import InstrumentId


MAX_PERIOD = 1 << 24
MAX_USIZE = 2 * sys.maxsize + 1
INSTRUMENT_ID = InstrumentId.from_str("ETHUSDT-PERP.BINANCE")
MOVING_AVERAGE_TYPES = (
    indicators.MovingAverageType.Simple,
    indicators.MovingAverageType.Exponential,
    indicators.MovingAverageType.DoubleExponential,
    indicators.MovingAverageType.Wilder,
    indicators.MovingAverageType.Hull,
)

BOUNDED_CONSTRUCTORS = [
    ("ExponentialMovingAverage", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("SimpleMovingAverage", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("DoubleExponentialMovingAverage", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("HullMovingAverage", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("WilderMovingAverage", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("LinearRegression", {"period": 2}, {"period": (2, MAX_PERIOD)}),
    ("WeightedMovingAverage", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("ZScore", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("RelativeStrengthIndex", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("AroonOscillator", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("Bias", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("ChandeMomentumOscillator", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("VerticalHorizontalFilter", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    (
        "KlingerVolumeOscillator",
        {"fast_period": 2, "slow_period": 3},
        {"fast_period": (1, MAX_PERIOD - 1), "slow_period": (1, MAX_PERIOD)},
    ),
    ("DirectionalMovement", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    (
        "IchimokuCloud",
        {"tenkan_period": 2, "kijun_period": 3, "senkou_period": 4, "displacement": 5},
        {
            "tenkan_period": (1, MAX_PERIOD),
            "kijun_period": (1, MAX_PERIOD),
            "senkou_period": (1, MAX_PERIOD),
            "displacement": (1, MAX_PERIOD),
        },
    ),
    (
        "ArcherMovingAveragesTrends",
        {"fast_period": 2, "slow_period": 3, "signal_period": 4},
        {
            "fast_period": (1, MAX_PERIOD - 1),
            "slow_period": (1, MAX_PERIOD),
            "signal_period": (1, 1024),
        },
    ),
    ("Swings", {"period": 2}, {"period": (1, 1024)}),
    ("BollingerBands", {"period": 2, "k": 1.5}, {"period": (1, MAX_PERIOD)}),
    (
        "Stochastics",
        {"period_k": 2, "period_d": 3, "slowing": 4},
        {"period_k": (1, MAX_PERIOD), "period_d": (1, MAX_PERIOD), "slowing": (1, MAX_PERIOD)},
    ),
    ("PsychologicalLine", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("Pressure", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("CommodityChannelIndex", {"period": 2, "scalar": 0.015}, {"period": (1, MAX_PERIOD)}),
    ("RateOfChange", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    (
        "MovingAverageConvergenceDivergence",
        {"fast_period": 2, "slow_period": 3, "signal_period": 4},
        {
            "fast_period": (1, MAX_PERIOD - 1),
            "slow_period": (1, MAX_PERIOD),
            "signal_period": (1, MAX_PERIOD),
        },
    ),
    ("AverageTrueRange", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("VolatilityRatio", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("DonchianChannel", {"period": 2}, {"period": (1, MAX_PERIOD)}),
    ("RelativeVolatilityIndex", {"period": 2}, {"period": (2, MAX_PERIOD)}),
    (
        "KeltnerChannel",
        {"period": 2, "k_multiplier": 1.5, "atr_period": 3},
        {"period": (1, MAX_PERIOD), "atr_period": (1, MAX_PERIOD)},
    ),
    ("KeltnerPosition", {"period": 2, "k_multiplier": 1.5}, {"period": (1, MAX_PERIOD)}),
    (
        "FuzzyCandlesticks",
        {"period": 2, "threshold1": 0.1, "threshold2": 0.2, "threshold3": 0.3, "threshold4": 0.4},
        {"period": (0, 1024)},
    ),
]


@pytest.mark.parametrize(
    ("name", "kwargs", "field", "invalid"),
    [
        (name, kwargs, field, invalid)
        for name, kwargs, bounds in BOUNDED_CONSTRUCTORS
        for field, (minimum, maximum) in bounds.items()
        for invalid in ([0] if minimum > 0 else []) + [maximum + 1, MAX_USIZE]
    ],
)
def test_bounded_constructor_rejects_invalid_sizes(
    name: str,
    kwargs: dict[str, object],
    field: str,
    invalid: int,
) -> None:
    """
    Reject invalid sizes through ordinary Python exceptions.
    """
    expected = OverflowError if name == "ZScore" and invalid > sys.maxsize else ValueError
    pattern = "too large" if expected is OverflowError else r"period|displacement|slowing"
    with pytest.raises(expected, match=pattern):
        getattr(indicators, name)(**(kwargs | {field: invalid}))


@pytest.mark.parametrize(("name", "kwargs", "bounds"), BOUNDED_CONSTRUCTORS)
def test_bounded_constructor_accepts_supported_maximum(
    name: str,
    kwargs: dict[str, object],
    bounds: dict[str, tuple[int, int]],
) -> None:
    """
    Accept each bounded constructor at its supported maximum.
    """
    maximum_kwargs = kwargs | {field: maximum for field, (_, maximum) in bounds.items()}

    # Construct sequentially so large, bounded windows are released between cases
    indicator = getattr(indicators, name)(**maximum_kwargs)
    for field in bounds:
        assert getattr(indicator, field) == maximum_kwargs[field]
    assert indicator.initialized is False
    assert indicator.has_inputs is False


@pytest.mark.parametrize(("name", "kwargs", "bounds"), BOUNDED_CONSTRUCTORS)
def test_bounded_constructor_preserves_valid_initial_state(
    name: str,
    kwargs: dict[str, object],
    bounds: dict[str, tuple[int, int]],
) -> None:
    """
    Preserve supplied sizes and initial state for valid construction.
    """
    indicator = getattr(indicators, name)(**kwargs)
    for field in bounds:
        assert getattr(indicator, field) == kwargs[field]
    assert indicator.initialized is False
    assert indicator.has_inputs is False


@pytest.mark.parametrize(
    "ma_type",
    MOVING_AVERAGE_TYPES,
)
@pytest.mark.parametrize("period", [0, MAX_PERIOD + 1, MAX_USIZE])
def test_psychological_line_checks_period_before_factory(
    period: int,
    ma_type: indicators.MovingAverageType,
) -> None:
    """
    Reject invalid periods for every moving average variant.
    """
    with pytest.raises(ValueError, match=r"period must be in \[1, 16777216\]"):
        indicators.PsychologicalLine(period, ma_type)


@pytest.mark.parametrize(
    "ma_type",
    MOVING_AVERAGE_TYPES,
)
@pytest.mark.parametrize(
    ("fast", "slow", "signal"),
    [(0, 2, 1), (1, 0, 1), (1, 2, 0), (2, 2, 1), (1, MAX_PERIOD + 1, 1), (1, 2, 1025)],
)
def test_archer_trends_checks_periods_before_factory(
    fast: int,
    slow: int,
    signal: int,
    ma_type: indicators.MovingAverageType,
) -> None:
    """
    Reject invalid trends periods before constructing nested averages.
    """
    with pytest.raises(ValueError, match="period"):
        indicators.ArcherMovingAveragesTrends(fast, slow, signal, ma_type)


@pytest.mark.parametrize("period", [0, 1024])
def test_fuzzy_candlesticks_preserves_accepted_bounds(period: int) -> None:
    """
    Preserve the accepted zero and maximum fuzzy periods.
    """
    indicator = indicators.FuzzyCandlesticks(period, 0.1, 0.2, 0.3, 0.4)
    assert indicator.period == period
    assert indicator.initialized is False
    assert indicator.has_inputs is False


@pytest.mark.parametrize("capacity", [0, 10])
def test_spread_analyzer_preserves_capacity(capacity: int) -> None:
    """
    Preserve spread capacity, instrument, and initial values.
    """
    indicator = indicators.SpreadAnalyzer(INSTRUMENT_ID, capacity)
    assert indicator.capacity == capacity
    assert indicator.instrument_id == INSTRUMENT_ID
    assert indicator.current == 0.0
    assert indicator.average == 0.0
    assert indicator.initialized is False
    assert indicator.has_inputs is False


@pytest.mark.parametrize("capacity", [MAX_USIZE, sys.maxsize // struct.calcsize("d") + 1])
def test_spread_analyzer_rejects_capacity_overflow(capacity: int) -> None:
    """
    Report deterministic capacity overflow as ValueError.
    """
    with pytest.raises(ValueError, match="computed capacity exceeded"):
        indicators.SpreadAnalyzer(INSTRUMENT_ID, capacity)


@pytest.mark.parametrize("period", [0, MAX_USIZE, sys.maxsize // struct.calcsize("d")])
def test_efficiency_ratio_checked_allocation_and_arithmetic(period: int) -> None:
    """
    Reject zero and overflow before allocating efficiency windows.
    """
    with pytest.raises(ValueError, match=r"period|failed to reserve efficiency ratio input window"):
        indicators.EfficiencyRatio(period)


@pytest.mark.parametrize(
    ("name", "kwargs", "fields"),
    [
        (
            "AdaptiveMovingAverage",
            {
                "period_efficiency_ratio": 2,
                "period_fast": MAX_USIZE - 2,
                "period_slow": MAX_USIZE - 1,
            },
            {"period_fast": MAX_USIZE - 2, "period_slow": MAX_USIZE - 1},
        ),
        (
            "VariableIndexDynamicAverage",
            {"period": MAX_USIZE, "cmo_period": 2},
            {"period": MAX_USIZE, "cmo_period": 2},
        ),
    ],
)
def test_scalar_periods_preserve_large_accepted_values(
    name: str,
    kwargs: dict[str, object],
    fields: dict[str, int],
) -> None:
    """
    Preserve scalar periods beyond the windowed indicator limit.
    """
    indicator = getattr(indicators, name)(**kwargs)
    for field, expected in fields.items():
        assert getattr(indicator, field) == expected
    assert indicator.initialized is False
    assert indicator.has_inputs is False


@pytest.mark.parametrize(
    ("name", "kwargs"),
    [
        (
            "AdaptiveMovingAverage",
            {"period_efficiency_ratio": 0, "period_fast": 2, "period_slow": 3},
        ),
        (
            "AdaptiveMovingAverage",
            {"period_efficiency_ratio": 2, "period_fast": 0, "period_slow": 3},
        ),
        (
            "AdaptiveMovingAverage",
            {"period_efficiency_ratio": 2, "period_fast": 2, "period_slow": MAX_USIZE},
        ),
        ("VariableIndexDynamicAverage", {"period": 0}),
        ("VariableIndexDynamicAverage", {"period": 2, "cmo_period": 0}),
        ("VariableIndexDynamicAverage", {"period": 2, "cmo_period": MAX_PERIOD + 1}),
    ],
)
def test_composed_checked_constructor_rejects_invalid_sizes(
    name: str,
    kwargs: dict[str, object],
) -> None:
    """
    Reject invalid sizes in composed checked constructors.
    """
    with pytest.raises(ValueError, match="period"):
        getattr(indicators, name)(**kwargs)


@pytest.mark.parametrize(
    "name",
    ["VolumeWeightedAveragePrice", "BookImbalanceRatio", "OnBalanceVolume"],
)
def test_parameterless_constructor_initial_state(name: str) -> None:
    """
    Preserve initial state for constructors without size parameters.
    """
    indicator = getattr(indicators, name)()
    assert indicator.initialized is False
    assert indicator.has_inputs is False


def test_fuzzy_candle_constructor_preserves_distinct_fields() -> None:
    """
    Preserve every distinct fuzzy candle classification.
    """
    candle = indicators.FuzzyCandle(
        indicators.CandleDirection.Bull,
        indicators.CandleSize.Large,
        indicators.CandleBodySize.Medium,
        indicators.CandleWickSize.Small,
        indicators.CandleWickSize.Large,
    )
    assert candle.direction == indicators.CandleDirection.Bull
    assert candle.size == indicators.CandleSize.Large
    assert candle.body_size == indicators.CandleBodySize.Medium
    assert candle.upper_wick_size == indicators.CandleWickSize.Small
    assert candle.lower_wick_size == indicators.CandleWickSize.Large


@pytest.mark.parametrize("period", [0, MAX_PERIOD + 1, MAX_USIZE])
def test_weighted_average_validates_period_before_weights(period: int) -> None:
    """
    Reject invalid periods before invoking any sequence conversion.
    """

    class UnreadableWeights:
        def __len__(self) -> int:
            raise AssertionError("weights length accessed")

        def __getitem__(self, index: int) -> float:
            raise AssertionError("weights item accessed")

        def __iter__(self) -> Iterator[float]:
            raise AssertionError("weights iterator accessed")

    with pytest.raises(ValueError, match="period"):
        indicators.WeightedMovingAverage(period, UnreadableWeights())


@pytest.mark.parametrize("reported_length", [0, sys.maxsize])
def test_weighted_average_preserves_sequence_values(reported_length: int) -> None:
    """
    Convert the actual weights without trusting a reported sequence length.
    """

    class Weights:
        def __len__(self) -> int:
            return reported_length

        def __getitem__(self, index: int) -> float:
            return (1.0, 3.0)[index]

    indicator = indicators.WeightedMovingAverage(2, Weights())
    indicator.update_raw(10.0)
    indicator.update_raw(20.0)
    assert indicator.period == 2
    assert indicator.weights == [1.0, 3.0]
    assert indicator.value == 17.5
    assert indicator.count == 2
    assert indicator.initialized is True
    assert indicator.has_inputs is True


def test_weighted_average_bounds_sequence_consumption() -> None:
    """
    Reject an overlong sequence after one item beyond the valid window.
    """

    class OverlongWeights:
        def __len__(self) -> int:
            return sys.maxsize

        def __getitem__(self, index: int) -> float:
            if index > 2:
                raise AssertionError("weights consumed past rejection boundary")
            return 1.0

    with pytest.raises(ValueError, match=r"`period` must equal `weights.len\(\)`"):
        indicators.WeightedMovingAverage(2, OverlongWeights())


@pytest.mark.parametrize(
    "weights",
    ["", "12", iter([1.0, 3.0]), 2.0, {1.0: 2.0}, MappingProxyType({1.0: 2.0})],
)
def test_weighted_average_preserves_non_sequence_rejection(weights: object) -> None:
    """
    Preserve type rejection for inputs outside the sequence contract.
    """
    with pytest.raises(TypeError):
        indicators.WeightedMovingAverage(2, weights)


@pytest.mark.parametrize("period", [0, 2])
def test_weighted_average_oversized_length_raises_in_subprocess(period: int) -> None:
    """
    Catch an ordinary exception for forged lengths without aborting the process.
    """
    script = (
        "import sys\n"
        "from nautilus_trader import indicators\n"
        "class OversizedWeights:\n"
        "    def __len__(self): return sys.maxsize\n"
        "    def __getitem__(self, index): raise IndexError\n"
        "try:\n"
        f"    indicators.WeightedMovingAverage({period}, OversizedWeights())\n"
        "except ValueError as e:\n"
        "    print(type(e).__name__)\n"
        "else:\n"
        "    raise AssertionError('invalid weights accepted')\n"
    )
    result = subprocess.run(
        [sys.executable, "-c", script],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout == "ValueError\n"
    assert result.stderr == ""


@pytest.mark.parametrize(
    "call",
    [
        "indicators.ArcherMovingAveragesTrends(0, 2, 1)",
        "indicators.PsychologicalLine(0)",
        "indicators.Swings(0)",
        "indicators.FuzzyCandlesticks(1025, 0.1, 0.2, 0.3, 0.4)",
        "indicators.SpreadAnalyzer(InstrumentId.from_str('ETHUSDT-PERP.BINANCE'), 2 * sys.maxsize + 1)",
    ],
)
def test_invalid_constructor_raises_ordinary_exception_in_subprocess(call: str) -> None:
    """
    Exit normally after catching ValueError at the Python boundary.
    """
    script = (
        "import sys\n"
        "from nautilus_trader import indicators\n"
        "from nautilus_trader.model import InstrumentId\n"
        "try:\n"
        f"    {call}\n"
        "except ValueError as e:\n"
        "    print(type(e).__name__)\n"
        "else:\n"
        "    raise AssertionError('invalid size accepted')\n"
    )
    result = subprocess.run(
        [sys.executable, "-c", script],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout == "ValueError\n"
    assert result.stderr == ""
