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

from decimal import Decimal

import pytest

from nautilus_trader.adapters.polymarket.common.conversion import usdce_from_units
from nautilus_trader.adapters.polymarket.http.conversion import convert_tif_to_polymarket_order_type
from nautilus_trader.model.currencies import USDC_POS
from nautilus_trader.model.enums import TimeInForce


@pytest.mark.parametrize(
    ("units", "expected_amount"),
    [
        [1, Decimal("0.000001")],
        [1000000, Decimal("1.000000")],
    ],
)
def test_usdc_from_units(units: int, expected_amount: float) -> None:
    # Arrange, Act
    usdce = usdce_from_units(units)

    # Assert
    assert usdce.currency == USDC_POS
    assert usdce.as_decimal() == expected_amount


@pytest.mark.parametrize(
    ("time_in_force", "expected_order_type"),
    [
        [TimeInForce.GTC, "GTC"],
        [TimeInForce.GTD, "GTD"],
        [TimeInForce.FOK, "FOK"],
        [TimeInForce.IOC, "FAK"],  # IOC maps to FAK
    ],
)
def test_convert_tif_to_polymarket_order_type(
    time_in_force: TimeInForce,
    expected_order_type: str,
) -> None:
    # Arrange, Act
    result = convert_tif_to_polymarket_order_type(time_in_force)

    # Assert
    assert result == expected_order_type


def test_convert_tif_invalid_time_in_force() -> None:
    # Arrange, Act & Assert
    with pytest.raises(ValueError, match="invalid `TimeInForce` for conversion"):
        convert_tif_to_polymarket_order_type(TimeInForce.DAY)
