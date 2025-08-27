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

from nautilus_trader.live.reconciliation import calculate_reconciliation_price
from nautilus_trader.model.currencies import EUR
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


@pytest.fixture
def eurusd_instrument():
    """
    Create a test EUR/USD instrument for price precision testing.
    """
    return CurrencyPair(
        instrument_id=InstrumentId(Symbol("EUR/USD"), Venue("TEST")),
        raw_symbol=Symbol("EUR/USD"),
        base_currency=EUR,
        quote_currency=USD,
        price_precision=5,
        size_precision=2,
        price_increment=Price(1e-05, precision=5),
        size_increment=Quantity(0.01, precision=2),
        lot_size=Quantity(10000, precision=2),
        max_quantity=Quantity(1000000, precision=2),
        min_quantity=Quantity(0.01, precision=2),
        max_notional=None,
        min_notional=None,
        max_price=Price(10.0, precision=5),
        min_price=Price(0.0001, precision=5),
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=Decimal("0.0002"),
        taker_fee=Decimal("0.0002"),
        ts_event=0,
        ts_init=0,
    )


@pytest.mark.parametrize(
    "current_qty,current_avg_px,target_qty,target_avg_px,expected_price,description",
    [
        # Flat position scenarios
        (
            Decimal("0"),
            None,
            Decimal("100"),
            Decimal("1.25000"),
            Price(1.25000, precision=5),
            "Flat to long position",
        ),
        (
            Decimal("0"),
            None,
            Decimal("-100"),
            Decimal("1.25000"),
            Price(1.25000, precision=5),
            "Flat to short position",
        ),
        # Position increases
        (
            Decimal("100"),
            Decimal("1.20000"),
            Decimal("200"),
            Decimal("1.22000"),
            Price(1.24000, precision=5),
            "Long position increase",
        ),
        (
            Decimal("-100"),
            Decimal("1.30000"),
            Decimal("-200"),
            Decimal("1.28000"),
            Price(1.26000, precision=5),
            "Short position increase",
        ),
        # Position decreases
        (
            Decimal("200"),
            Decimal("1.20000"),
            Decimal("100"),
            Decimal("1.20000"),
            Price(1.20000, precision=5),
            "Long position decrease",
        ),
        # Position flips
        (
            Decimal("100"),
            Decimal("1.20000"),
            Decimal("-100"),
            Decimal("1.25000"),
            Price(1.22500, precision=5),
            "Long to short flip",
        ),
        (
            Decimal("-100"),
            Decimal("1.30000"),
            Decimal("100"),
            Decimal("1.25000"),
            Price(1.27500, precision=5),
            "Short to long flip",
        ),
        # Complex scenario
        (
            Decimal("150"),
            Decimal("1.23456"),
            Decimal("250"),
            Decimal("1.24567"),
            Price(1.26233, precision=5),
            "Complex partial fill scenario",
        ),
    ],
)
def test_reconciliation_price_calculations(
    eurusd_instrument,
    current_qty,
    current_avg_px,
    target_qty,
    target_avg_px,
    expected_price,
    description,
):
    """
    Test reconciliation price calculations for various scenarios.
    """
    result = calculate_reconciliation_price(
        current_position_qty=current_qty,
        current_position_avg_px=current_avg_px,
        target_position_qty=target_qty,
        target_position_avg_px=target_avg_px,
        instrument=eurusd_instrument,
    )

    assert result is not None, f"Failed for scenario: {description}"
    assert result == expected_price, f"Failed for scenario: {description}"


@pytest.mark.parametrize(
    "current_qty,current_avg_px,target_qty,target_avg_px,description",
    [
        # No target average price
        (
            Decimal("100"),
            Decimal("1.20000"),
            Decimal("200"),
            None,
            "No target avg price",
        ),
        # Zero target average price
        (
            Decimal("100"),
            Decimal("1.20000"),
            Decimal("200"),
            Decimal("0"),
            "Zero target avg price",
        ),
        # No quantity change
        (
            Decimal("100"),
            Decimal("1.20000"),
            Decimal("100"),
            Decimal("1.20000"),
            "No quantity change",
        ),
        # Negative price scenario
        (
            Decimal("100"),
            Decimal("2.00000"),
            Decimal("200"),
            Decimal("1.00000"),
            "Negative price calculation",
        ),
    ],
)
def test_reconciliation_price_returns_none(
    eurusd_instrument,
    current_qty,
    current_avg_px,
    target_qty,
    target_avg_px,
    description,
):
    """
    Test scenarios where reconciliation price calculation should return None.
    """
    result = calculate_reconciliation_price(
        current_position_qty=current_qty,
        current_position_avg_px=current_avg_px,
        target_position_qty=target_qty,
        target_position_avg_px=target_avg_px,
        instrument=eurusd_instrument,
    )

    assert result is None, f"Expected None for scenario: {description}"


def test_reconciliation_price_flat_position_logic(eurusd_instrument):
    """
    Test that flat position logic works correctly.
    """
    # When current position is flat, reconciliation price should equal target avg price
    result = calculate_reconciliation_price(
        current_position_qty=Decimal("0"),
        current_position_avg_px=None,
        target_position_qty=Decimal("100"),
        target_position_avg_px=Decimal("1.25000"),
        instrument=eurusd_instrument,
    )

    assert result == Price(1.25000, precision=5)


def test_reconciliation_price_precision_handling(eurusd_instrument):
    """
    Test that price precision is handled correctly by the instrument.
    """
    # Test with high precision input that should be rounded
    result = calculate_reconciliation_price(
        current_position_qty=Decimal("100"),
        current_position_avg_px=Decimal("1.123456789"),
        target_position_qty=Decimal("200"),
        target_position_avg_px=Decimal("1.234567890"),
        instrument=eurusd_instrument,
    )

    assert result is not None
    # Should be rounded to instrument precision (5 decimal places)
    assert result.precision == 5
    assert str(result) == "1.34568"  # Expected calculation result rounded to 5 decimals


def test_reconciliation_price_zero_quantity_difference_after_precision():
    """
    Test handling when quantity difference rounds to zero after precision.
    """
    # This scenario is mainly handled at the engine level, but we can test
    # that the function handles very small differences correctly
    instrument = CurrencyPair(
        instrument_id=InstrumentId(Symbol("EUR/USD"), Venue("TEST")),
        raw_symbol=Symbol("EUR/USD"),
        base_currency=EUR,
        quote_currency=USD,
        price_precision=5,
        size_precision=0,  # No decimal places for quantity
        price_increment=Price(1e-05, precision=5),
        size_increment=Quantity(1, precision=0),
        lot_size=Quantity(1, precision=0),
        max_quantity=Quantity(1000000, precision=0),
        min_quantity=Quantity(1, precision=0),
        max_notional=None,
        min_notional=None,
        max_price=Price(10.0, precision=5),
        min_price=Price(0.0001, precision=5),
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=Decimal("0.0002"),
        taker_fee=Decimal("0.0002"),
        ts_event=0,
        ts_init=0,
    )

    # Very small difference that would round to zero at instrument precision
    result = calculate_reconciliation_price(
        current_position_qty=Decimal("100.4"),
        current_position_avg_px=Decimal("1.20000"),
        target_position_qty=Decimal("100.6"),
        target_position_avg_px=Decimal("1.21000"),
        instrument=instrument,
    )

    # Should still calculate since we're working with the raw decimals
    assert result is not None
