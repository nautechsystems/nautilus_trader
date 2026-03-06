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
