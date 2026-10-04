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
Test constructor validation for indicators composed from ATR and Keltner channels.
"""

import pytest

from nautilus_trader.indicators import KeltnerPosition
from nautilus_trader.indicators import Pressure


def test_pressure_negative_atr_floor_raises_value_error() -> None:
    """
    Test a negative ATR floor raises ValueError instead of panicking.
    """
    with pytest.raises(ValueError, match="value_floor"):
        Pressure(10, atr_floor=-1.0)


def test_pressure_zero_period_raises_value_error() -> None:
    """
    Test a zero period raises ValueError instead of panicking.
    """
    with pytest.raises(ValueError, match="period must be > 0"):
        Pressure(0)


@pytest.mark.parametrize("k_multiplier", [0.0, -1.0])
def test_keltner_position_non_positive_multiplier_raises_value_error(k_multiplier: float) -> None:
    """
    Test a non-positive multiplier raises ValueError instead of panicking.
    """
    with pytest.raises(ValueError, match="k_multiplier must be finite and positive"):
        KeltnerPosition(10, k_multiplier)


def test_keltner_position_negative_atr_floor_raises_value_error() -> None:
    """
    Test a negative ATR floor raises ValueError instead of panicking.
    """
    with pytest.raises(ValueError, match="value_floor"):
        KeltnerPosition(10, 2.0, atr_floor=-1.0)
