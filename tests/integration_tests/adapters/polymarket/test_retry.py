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

import random

import pytest

from nautilus_trader.adapters.polymarket.common.retry import auto_load_retry_delay


@pytest.mark.parametrize(
    ("attempt", "expected_base"),
    [
        (0, 5.0),
        (1, 10.0),
        (2, 15.0),  # 5 * 2**2 = 20 -> capped at 15
        (3, 15.0),
        (10, 15.0),
    ],
)
def test_auto_load_retry_delay_no_jitter(attempt: int, expected_base: float) -> None:
    # With random_fn=0.0 the jitter term collapses to 0; we get the capped exp delay.
    result = auto_load_retry_delay(
        attempt,
        base_secs=5.0,
        max_secs=15.0,
        random_fn=lambda: 0.0,
    )
    assert result == expected_base


@pytest.mark.parametrize(
    ("attempt", "expected_base"),
    [
        (0, 5.0),
        (1, 10.0),
        (2, 15.0),
        (10, 15.0),
    ],
)
def test_auto_load_retry_delay_max_jitter(attempt: int, expected_base: float) -> None:
    # random_fn=1.0 sits at the open upper bound; the actual function uses
    # random.random() which never returns 1.0, but verifying the math here
    # pins the +25% jitter scaling.
    result = auto_load_retry_delay(
        attempt,
        base_secs=5.0,
        max_secs=15.0,
        random_fn=lambda: 1.0,
    )
    assert result == pytest.approx(expected_base * 1.25)


def test_auto_load_retry_delay_jitter_bounds_with_real_random() -> None:
    # Drive the helper across many samples per attempt to verify the
    # [capped, capped * 1.25) bound holds under the production random source.
    rng = random.Random(42)  # noqa: S311  (jitter sampling, not cryptographic)
    base, cap = 5.0, 15.0

    for attempt in range(6):
        capped = min(cap, base * (2**attempt))

        for _ in range(200):
            delay = auto_load_retry_delay(
                attempt,
                base_secs=base,
                max_secs=cap,
                random_fn=rng.random,
            )
            assert capped <= delay < capped * 1.25


def test_auto_load_retry_delay_zero_attempt_returns_base_with_zero_jitter() -> None:
    # Smallest valid attempt, smallest valid jitter: this guards against a
    # future regression that adds an off-by-one to the exponent.
    assert (
        auto_load_retry_delay(
            0,
            base_secs=1.0,
            max_secs=60.0,
            random_fn=lambda: 0.0,
        )
        == 1.0
    )
