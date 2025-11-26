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

import math

import pytest

from nautilus_trader.model.objects import Price
from nautilus_trader.model.tick_scheme import FixedTickScheme
from nautilus_trader.model.tick_scheme import TieredTickScheme
from nautilus_trader.model.tick_scheme import get_tick_scheme
from nautilus_trader.model.tick_scheme.base import round_down
from nautilus_trader.model.tick_scheme.base import round_up
from nautilus_trader.test_kit.providers import TestInstrumentProvider


AUDUSD = TestInstrumentProvider.default_fx_ccy("AUD/USD")
JPYUSD = TestInstrumentProvider.default_fx_ccy("JPY/USD")


@pytest.mark.parametrize(
    ("value", "precision", "expected"),
    [
        (0.727775, 4, "0.7277"),
        (0.72777, 4, "0.7277"),
        (0.727741111, 4, "0.7277"),
        (0.799999, 2, "0.79"),
    ],
)
def test_round_down(value, precision, expected):
    base = 1 * 10**-precision
    assert round_down(value, base=base) == Price.from_str(expected).as_double()


@pytest.mark.parametrize(
    ("value", "precision", "expected"),
    [
        (0.72775, 4, "0.7278"),
        (0.7277, 4, "0.7277"),
        (0.727741111, 4, "0.7278"),
        (0.799999, 2, "0.80"),
    ],
)
def test_round_up(value, precision, expected):
    base = 1 * 10**-precision
    assert round_up(value, base) == Price.from_str(expected).as_double()


def test_fixed_tick_scheme_attrs():
    tick_scheme = get_tick_scheme("FOREX_3DECIMAL")
    assert tick_scheme.price_precision == 3
    assert tick_scheme.min_price == Price.from_str("0.001")
    assert tick_scheme.max_price == Price.from_str("999.999")
    assert tick_scheme.increment == Price.from_str("0.001")


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (0.727, "0.727"),
        (0.99999, "1.0000"),
        (0.72775, "0.728"),
        (10000, None),
        (0.0005, None),
        (0.7271, "0.728"),
    ],
)
def test_fixed_tick_scheme_next_ask_price(value, expected):
    tick_scheme = get_tick_scheme("FOREX_3DECIMAL")
    result = tick_scheme.next_ask_price(value)
    expected = expected if expected is None else Price.from_str(expected)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (0.7271, "0.727"),
        (0.001, "0.001"),
        (0.72750, "0.727"),
        (0.00001, None),
        (0.7271, "0.727"),
    ],
)
def test_fixed_tick_scheme_next_bid_price(value, expected):
    tick_scheme = get_tick_scheme("FOREX_3DECIMAL")
    result = tick_scheme.next_bid_price(value)
    expected = expected if expected is None else Price.from_str(expected)
    assert result == expected


def test_topix100_tick_scheme_attrs():
    tick_scheme = get_tick_scheme("TOPIX100")
    assert tick_scheme.min_price == Price.from_str("0.1")
    assert tick_scheme.max_price == Price.from_int(130_000_000)


@pytest.mark.parametrize(
    ("value", "n", "expected"),
    [
        (1000, 0, "1000"),
        (1000.25, 0, "1000.50"),
        (10_001, 0, "10_005"),
        (10_000_001, 0, "10_005_000"),
        (9999, 2, "10_005"),
    ],
)
def test_topix100_tick_scheme_next_ask_price(value, n, expected):
    tick_scheme = get_tick_scheme("TOPIX100")
    result = tick_scheme.next_ask_price(value, n=n)
    expected = Price.from_str(expected)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "n", "expected"),
    [
        (1000.75, 0, "1000.50"),
        (10_007, 0, "10_005"),
        (10_000_001, 0, "10_000_000"),
        (10_006, 2, "9999"),
    ],
)
def test_topix100_tick_scheme_next_bid_price(value, n, expected):
    tick_scheme = get_tick_scheme("TOPIX100")
    result = tick_scheme.next_bid_price(value=value, n=n)
    expected = Price.from_str(expected)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "n", "expected"),
    [
        (10.1, 0, "10.5"),
    ],
)
def test_bitmex_spot_tick_scheme_next_ask_price(value, n, expected):
    tick_scheme = FixedTickScheme(
        name="BitmexSpot",
        price_precision=1,
        increment=0.50,
        min_tick=Price.from_str("0.001"),
        max_tick=Price.from_str("999.999"),
    )
    result = tick_scheme.next_ask_price(value, n=n)
    expected = Price.from_str(expected)
    assert result == expected


@pytest.mark.parametrize(
    ("value", "n", "expected"),
    [
        (10.1, 0, "10.0"),
    ],
)
def test_bitmex_spot_tick_scheme_next_bid_price(value, n, expected):
    tick_scheme = FixedTickScheme(
        name="BitmexSpot",
        price_precision=1,
        increment=0.50,
        min_tick=Price.from_str("0.001"),
        max_tick=Price.from_str("999.999"),
    )
    result = tick_scheme.next_bid_price(value=value, n=n)
    expected = Price.from_str(expected)
    assert result == expected


def test_fixed_tick_scheme_negative_n_raises_error():
    scheme = get_tick_scheme("FOREX_3DECIMAL")
    with pytest.raises(ValueError, match="n must be >= 0"):
        scheme.next_ask_price(1.0, n=-1)
    with pytest.raises(ValueError, match="n must be >= 0"):
        scheme.next_bid_price(1.0, n=-1)


def test_fixed_tick_scheme_out_of_bounds_returns_none():
    scheme = get_tick_scheme("FOREX_3DECIMAL")
    assert scheme.next_ask_price(999.999, n=1) is None
    assert scheme.next_ask_price(1000.0, n=0) is None
    assert scheme.next_ask_price(0.0005, n=0) is None
    assert scheme.next_ask_price(0.0, n=0) is None
    assert scheme.next_bid_price(0.001, n=1) is None
    assert scheme.next_bid_price(0.0005, n=0) is None


def test_fixed_tick_scheme_max_price_comparison_bug():
    """
    Test that comparing value against max_price works correctly (float vs Price bug).
    """
    scheme = get_tick_scheme("FOREX_5DECIMAL")
    result = scheme.next_ask_price(9.99999, n=0)
    assert result == Price.from_str("9.99999")
    result = scheme.next_ask_price(9.99999, n=1)
    assert result is None


def test_fixed_tick_scheme_precision_zero():
    """
    Test that precision=0 schemes work correctly.
    """
    scheme = get_tick_scheme("FIXED_PRECISION_0")
    assert scheme.increment == Price.from_int(1)
    result = scheme.next_ask_price(1.2, n=0)
    assert result == Price.from_int(2)
    result = scheme.next_bid_price(1.8, n=0)
    assert result == Price.from_int(1)


def test_fixed_tick_scheme_bounds_enforced_after_rounding():
    """
    Test that bounds are checked after applying n steps.
    """
    scheme = FixedTickScheme(
        name="TEST",
        price_precision=2,
        min_tick=Price.from_str("1.00"),
        max_tick=Price.from_str("10.00"),
    )
    result = scheme.next_ask_price(0.0, n=0)
    assert result is None
    result = scheme.next_bid_price(1.05, n=10)
    assert result is None


def test_round_up_down_with_small_base():
    """
    Test round_up/round_down with very small base values.
    """
    result = round_up(0.001, base=1e-11)
    assert result >= 0.001


def test_round_does_not_accept_values_half_tick_off():
    """
    Test that values 0.5 ticks off-grid are NOT treated as on-grid.
    """
    value = 10_000_000.0000015
    base = 1e-6

    result = round_down(value, base)
    expected = 10_000_000.000001
    assert abs(result - expected) < 1e-10, f"Expected {expected}, was {result}"

    value2 = 10_000_000.0000005
    result2 = round_up(value2, base)
    expected2 = 10_000_000.000001
    assert abs(result2 - expected2) < 1e-10, f"Expected {expected2}, was {result2}"


def test_round_base_validation():
    """
    Test that round_up/round_down validate base > 0.
    """
    with pytest.raises(ValueError, match="base must be positive"):
        round_up(1.0, base=0.0)
    with pytest.raises(ValueError, match="base must be positive"):
        round_down(1.0, base=-0.1)


def test_tiered_tick_scheme_minimum_tick_bid_price():
    """
    Test that next_bid_price works at minimum tick (idx=0 bug).
    """
    scheme = get_tick_scheme("TOPIX100")
    result = scheme.next_bid_price(0.1, n=0)
    assert result == Price.from_str("0.1")


def test_tiered_tick_scheme_boundary_tick_equality():
    """
    Test that exact tick boundaries work correctly.
    """
    scheme = get_tick_scheme("TOPIX100")
    result = scheme.next_bid_price(1000, n=0)
    assert result == Price.from_str("1000")


def test_tiered_tick_scheme_out_of_bounds_returns_none():
    """
    Test that out of bounds values return None instead of IndexError.
    """
    scheme = get_tick_scheme("TOPIX100")
    result = scheme.next_ask_price(999_999_999, n=0)
    assert result is None
    max_tick = scheme.max_price.as_double()
    result = scheme.next_ask_price(max_tick, n=1)
    assert result is None
    result = scheme.next_bid_price(0.05, n=0)
    assert result is None


def test_tiered_tick_scheme_negative_n_raises_error():
    scheme = get_tick_scheme("TOPIX100")
    with pytest.raises(ValueError, match="n must be >= 0"):
        scheme.next_ask_price(1000, n=-1)
    with pytest.raises(ValueError, match="n must be >= 0"):
        scheme.next_bid_price(1000, n=-1)


def test_round_extreme_magnitudes():
    """
    Test rounding at extreme magnitudes to catch FP precision issues.
    """
    result = round_up(1e12 + 0.5e-6, 1e-6)
    assert abs(result - (1e12 + 1e-6)) < 1e-10

    result = round_down(1e12 + 0.5e-6, 1e-6)
    assert abs(result - 1e12) < 1e-10

    result = round_up(1e15, 1.0)
    assert result == 1e15

    result = round_down(1e15, 1.0)
    assert result == 1e15

    result = round_up(1e-8 + 5e-14, 1e-13)
    assert result > 1e-8

    result = round_down(1e-8 - 5e-14, 1e-13)
    assert result < 1e-8


def test_fixed_tick_scheme_extreme_magnitude():
    """
    Test FixedTickScheme with extreme price ranges.
    """
    scheme = FixedTickScheme(
        name="BTCUSD",
        price_precision=2,
        min_tick=Price.from_str("0.01"),
        max_tick=Price.from_str("1000000.00"),
    )

    result = scheme.next_ask_price(99999.995, n=0)
    assert result == Price.from_str("100000.00")

    result = scheme.next_bid_price(100000.005, n=0)
    assert result == Price.from_str("100000.00")

    result = scheme.next_ask_price(999999.99, n=0)
    assert result == Price.from_str("999999.99")

    result = scheme.next_ask_price(999999.99, n=1)
    assert result == Price.from_str("1000000.00")

    result = scheme.next_ask_price(999999.99, n=2)
    assert result is None


def test_fixed_tick_scheme_micro_tick_precision():
    """
    Test FixedTickScheme with very small tick sizes.
    """
    scheme = FixedTickScheme(
        name="MICRO",
        price_precision=8,
        min_tick=Price.from_str("0.00000001"),
        max_tick=Price.from_str("100000.00000000"),
    )

    result = scheme.next_ask_price(50000.123456785, n=0)
    assert result == Price.from_str("50000.12345679")

    result = scheme.next_bid_price(50000.123456785, n=0)
    assert result == Price.from_str("50000.12345678")


def test_tiered_tick_scheme_tier_boundaries():
    """
    Test behavior exactly at tier transition points.
    """
    scheme = get_tick_scheme("TOPIX100")

    assert scheme.next_ask_price(1000.0, n=0) == Price.from_str("1000.0")
    assert scheme.next_bid_price(1000.0, n=0) == Price.from_str("1000.0")

    result = scheme.next_ask_price(999.9, n=0)
    assert result == Price.from_str("999.9")

    result = scheme.next_ask_price(1000.0, n=1)
    assert result == Price.from_str("1000.5")

    result = scheme.next_bid_price(1000.5, n=1)
    assert result == Price.from_str("1000.0")


def test_tiered_tick_scheme_multiple_tier_transitions():
    """
    Test jumps across multiple tiers.
    """
    scheme = get_tick_scheme("TOPIX100")

    start = 100.0
    result = scheme.next_ask_price(start, n=50)
    assert result is not None
    assert result > Price.from_str("100.0")
    assert result <= Price.from_str("110.0")


def test_fixed_tick_scheme_large_n_overflow():
    """
    Test that very large n values don't cause overflow or garbage results.
    """
    scheme = get_tick_scheme("FOREX_5DECIMAL")

    result = scheme.next_ask_price(1.0, n=1_000_000)
    assert result is None

    result = scheme.next_bid_price(1.0, n=1_000_000)
    assert result is None

    result = scheme.next_ask_price(0.00001, n=100)
    assert result == Price.from_str("0.00101")


def test_tiered_tick_scheme_large_n_beyond_bounds():
    """
    Test TieredTickScheme with n that exceeds tick count.
    """
    scheme = get_tick_scheme("TOPIX100")

    max_price = scheme.max_price.as_double()
    result = scheme.next_ask_price(max_price, n=1)
    assert result is None

    result = scheme.next_ask_price(max_price - 1000, n=100000)
    assert result is None

    min_price = scheme.min_price.as_double()
    result = scheme.next_bid_price(min_price, n=1)
    assert result is None

    result = scheme.next_bid_price(min_price + 1000, n=100000)
    assert result is None


def test_fixed_tick_scheme_n_boundary_exact():
    """
    Test n values that land exactly on boundaries.
    """
    scheme = FixedTickScheme(
        name="TEST",
        price_precision=2,
        min_tick=Price.from_str("1.00"),
        max_tick=Price.from_str("10.00"),
    )

    result = scheme.next_ask_price(1.00, n=900)
    assert result == Price.from_str("10.00")

    result = scheme.next_ask_price(1.00, n=901)
    assert result is None

    result = scheme.next_bid_price(10.00, n=900)
    assert result == Price.from_str("1.00")

    result = scheme.next_bid_price(10.00, n=901)
    assert result is None


def test_tiered_tick_scheme_invalid_negative_increment():
    """
    Test that negative increments are rejected.
    """
    with pytest.raises(ValueError, match="Increment must be positive"):
        TieredTickScheme(
            name="BAD",
            tiers=[(1.0, 2.0, -0.01)],
            price_precision=2,
        )


def test_tiered_tick_scheme_invalid_zero_increment():
    """
    Test that zero increments are rejected.
    """
    with pytest.raises(ValueError, match="Increment must be positive"):
        TieredTickScheme(
            name="BAD",
            tiers=[(1.0, 2.0, 0.0)],
            price_precision=2,
        )


def test_tiered_tick_scheme_invalid_increment_larger_than_range():
    """
    Test that increment larger than tier range is rejected.
    """
    with pytest.raises(ValueError, match="Increment should be less than tier range"):
        TieredTickScheme(
            name="BAD",
            tiers=[(1.0, 2.0, 5.0)],
            price_precision=2,
        )


def test_tiered_tick_scheme_invalid_start_equals_stop():
    """
    Test that start >= stop is rejected.
    """
    with pytest.raises(ValueError, match="Start should be less than stop"):
        TieredTickScheme(
            name="BAD",
            tiers=[(2.0, 2.0, 0.1)],
            price_precision=2,
        )


def test_tiered_tick_scheme_invalid_start_greater_than_stop():
    """
    Test that start > stop is rejected.
    """
    with pytest.raises(ValueError, match="Start should be less than stop"):
        TieredTickScheme(
            name="BAD",
            tiers=[(2.0, 1.0, 0.1)],
            price_precision=2,
        )


def test_round_infinity():
    """
    Test round_up/round_down with infinity.
    """
    result = round_up(math.inf, 1.0)
    assert math.isinf(result)

    result = round_down(math.inf, 1.0)
    assert math.isinf(result)

    result = round_up(-math.inf, 1.0)
    assert math.isinf(result)
    assert result < 0

    result = round_down(-math.inf, 1.0)
    assert math.isinf(result)
    assert result < 0


def test_round_nan():
    """
    Test round_up/round_down with NaN.
    """
    result = round_up(math.nan, 1.0)
    assert math.isnan(result)

    result = round_down(math.nan, 1.0)
    assert math.isnan(result)


def test_fixed_tick_scheme_infinity():
    """
    Test FixedTickScheme with infinity.
    """
    scheme = get_tick_scheme("FOREX_5DECIMAL")

    result = scheme.next_ask_price(math.inf, n=0)
    assert result is None

    result = scheme.next_bid_price(math.inf, n=0)
    assert result is None

    result = scheme.next_ask_price(-math.inf, n=0)
    assert result is None

    result = scheme.next_bid_price(-math.inf, n=0)
    assert result is None


def test_fixed_tick_scheme_nan():
    """
    Test FixedTickScheme with NaN.
    """
    scheme = get_tick_scheme("FOREX_5DECIMAL")

    with pytest.raises(ValueError, match="invalid `value`, was nan"):
        scheme.next_ask_price(math.nan, n=0)

    with pytest.raises(ValueError, match="invalid `value`, was nan"):
        scheme.next_bid_price(math.nan, n=0)


def test_fixed_tick_scheme_idempotent_on_grid():
    """
    Test that applying next_ask/bid to on-grid price with n=0 is idempotent.
    """
    scheme = get_tick_scheme("FOREX_5DECIMAL")

    price = 1.23450
    result1 = scheme.next_ask_price(price, n=0)
    assert result1.as_double() == price

    result2 = scheme.next_ask_price(result1.as_double(), n=0)
    assert result2 == result1

    result1 = scheme.next_bid_price(price, n=0)
    assert result1.as_double() == price

    result2 = scheme.next_bid_price(result1.as_double(), n=0)
    assert result2 == result1


def test_fixed_tick_scheme_ask_bid_symmetry():
    """
    Test that ask/bid operations are symmetric.
    """
    scheme = get_tick_scheme("FOREX_5DECIMAL")

    price = 1.234567

    ask = scheme.next_ask_price(price, n=0)
    assert ask >= Price.from_str("1.23457")

    prev_bid = scheme.next_bid_price(ask.as_double(), n=1)
    assert prev_bid < ask

    bid = scheme.next_bid_price(price, n=0)
    assert bid <= Price.from_str("1.23456")

    next_ask = scheme.next_ask_price(bid.as_double(), n=1)
    assert next_ask > bid


def test_tiered_tick_scheme_consistency():
    """
    Test TieredTickScheme consistency across operations.
    """
    scheme = get_tick_scheme("TOPIX100")

    start = 5000.0
    forward = scheme.next_ask_price(start, n=10)
    back = scheme.next_bid_price(forward.as_double(), n=10)

    assert back.as_double() <= start

    price = 1000.0
    cumulative = price
    for _ in range(5):
        result = scheme.next_ask_price(cumulative, n=1)
        if result:
            cumulative = result.as_double()

    direct = scheme.next_ask_price(price, n=5)
    assert abs(cumulative - direct.as_double()) < 1e-10


def test_fixed_tick_scheme_deterministic():
    """
    Test that FixedTickScheme produces deterministic results.
    """
    scheme = get_tick_scheme("FOREX_5DECIMAL")

    price = 1.23456789
    results = [scheme.next_ask_price(price, n=0) for _ in range(10)]
    assert all(r == results[0] for r in results)

    results = [scheme.next_bid_price(price, n=0) for _ in range(10)]
    assert all(r == results[0] for r in results)
