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
Test sizing behavior.
"""

from decimal import Decimal

import pytest

from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.risk import FixedRiskSizer
from nautilus_trader.risk import PositionSizer
from tests.providers import TestInstrumentProvider


USD = Currency.from_str("USD")
GBPUSD = TestInstrumentProvider.gbpusd_sim()
ENTRY = Price.from_str("1.00100")
STOP_LOSS = Price.from_str("1.00000")
EQUITY = Money.from_str("1000000 USD")
RISK = Decimal("0.001")


def test_position_sizer_exposes_instrument() -> None:
    """
    Test position sizer exposes instrument.
    """
    sizer = PositionSizer(GBPUSD)

    assert sizer.instrument is GBPUSD


def test_position_sizer_updates_matching_instrument() -> None:
    """
    Test position sizer updates matching instrument.
    """
    replacement = TestInstrumentProvider.gbpusd_sim()
    sizer = PositionSizer(GBPUSD)

    sizer.update_instrument(replacement)

    assert sizer.instrument is replacement


def test_position_sizer_rejects_mismatched_instrument_update() -> None:
    """
    Test position sizer rejects mismatched instrument update.
    """
    sizer = PositionSizer(GBPUSD)

    with pytest.raises(ValueError, match=r"instrument\.id"):
        sizer.update_instrument(TestInstrumentProvider.audusd_sim())

    assert sizer.instrument is GBPUSD


def test_position_sizer_rejects_invalid_instrument_update() -> None:
    """
    Test position sizer rejects invalid instrument update.
    """
    sizer = PositionSizer(GBPUSD)

    with pytest.raises(TypeError, match="must be an `Instrument`"):
        sizer.update_instrument(None)

    assert sizer.instrument is GBPUSD


@pytest.mark.parametrize("sizer_type", [PositionSizer, FixedRiskSizer])
@pytest.mark.parametrize("instrument", [None, "GBP/USD.SIM", 1])
def test_sizer_rejects_invalid_instrument(sizer_type: object, instrument: object) -> None:
    """
    Test sizer rejects invalid instrument.
    """
    with pytest.raises(TypeError, match="must be an `Instrument`"):
        sizer_type(instrument)


def test_position_sizer_calculate_is_abstract() -> None:
    """
    Test position sizer calculate is abstract.
    """
    sizer = PositionSizer(GBPUSD)

    with pytest.raises(NotImplementedError, match="subclasses must implement"):
        sizer.calculate(ENTRY, STOP_LOSS, EQUITY, RISK)


def test_fixed_risk_sizer_inherits_position_sizer() -> None:
    """
    Test fixed risk sizer inherits position sizer.
    """
    sizer = FixedRiskSizer(GBPUSD)

    assert isinstance(sizer, PositionSizer)
    assert sizer.instrument is GBPUSD


@pytest.mark.parametrize(
    ("entry", "stop_loss", "equity", "exchange_rate"),
    [
        (ENTRY, STOP_LOSS, Money.zero(USD), Decimal(1)),
        (ENTRY, STOP_LOSS, EQUITY, Decimal(0)),
        (ENTRY, ENTRY, EQUITY, Decimal(1)),
    ],
)
def test_calculate_returns_zero_without_riskable_size(
    entry: object,
    stop_loss: object,
    equity: object,
    exchange_rate: object,
) -> None:
    """
    Test calculate returns zero without riskable size.
    """
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=entry,
        stop_loss=stop_loss,
        equity=equity,
        risk=RISK,
        exchange_rate=exchange_rate,
    )

    assert result == Quantity.zero()


def test_calculate_uses_decimal_defaults() -> None:
    """
    Test calculate uses decimal defaults.
    """
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=ENTRY,
        stop_loss=STOP_LOSS,
        equity=EQUITY,
        risk=RISK,
    )

    assert result == Quantity.from_int(1_000_000)


def test_calculate_accepts_zero_unit_batch_size() -> None:
    """
    Test calculate accepts zero unit batch size.
    """
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=ENTRY,
        stop_loss=STOP_LOSS,
        equity=EQUITY,
        risk=RISK,
        unit_batch_size=Decimal(0),
    )

    assert result == Quantity.from_int(1_000_000)


def test_calculate_rejects_positive_size_rounded_to_zero() -> None:
    """
    Test calculate rejects positive size rounded to zero.
    """
    sizer = FixedRiskSizer(GBPUSD)

    with pytest.raises(ValueError, match="value rounded to zero for quantity"):
        sizer.calculate(
            entry=ENTRY,
            stop_loss=STOP_LOSS,
            equity=Money.from_str("1000 USD"),
            risk=Decimal("0.0000001"),
            unit_batch_size=Decimal(0),
        )


def test_calculate_applies_commission() -> None:
    """
    Test calculate applies commission.
    """
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=ENTRY,
        stop_loss=STOP_LOSS,
        equity=EQUITY,
        risk=RISK,
        commission_rate=Decimal("0.0002"),
    )

    assert result == Quantity.from_int(999_600)


def test_calculate_applies_hard_limit() -> None:
    """
    Test calculate applies hard limit.
    """
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=Price.from_str("1.00010"),
        stop_loss=STOP_LOSS,
        equity=EQUITY,
        risk=Decimal("0.01"),
        hard_limit=Decimal(500_000),
        unit_batch_size=Decimal(1_000),
    )

    assert result == Quantity.from_int(500_000)


def test_calculate_batches_multiple_units() -> None:
    """
    Test calculate batches multiple units.
    """
    sizer = FixedRiskSizer(GBPUSD)

    result = sizer.calculate(
        entry=Price.from_str("1.00010"),
        stop_loss=STOP_LOSS,
        equity=EQUITY,
        risk=RISK,
        unit_batch_size=Decimal(1_000),
        units=3,
    )

    assert result == Quantity.from_int(3_333_000)


@pytest.mark.parametrize(
    ("parameter", "value", "message"),
    [
        ("risk", Decimal(0), "risk"),
        ("risk", Decimal("-0.001"), "risk"),
        ("commission_rate", Decimal("-0.001"), "commission_rate must be non-negative"),
        ("exchange_rate", Decimal(-1), "exchange_rate must be non-negative"),
        ("hard_limit", Decimal(0), "hard_limit"),
        ("hard_limit", Decimal(-1), "hard_limit"),
        ("unit_batch_size", Decimal(-1), "unit_batch_size must be non-negative"),
        ("units", 0, "units"),
        ("units", -1, "units"),
    ],
)
def test_calculate_rejects_invalid_values(
    parameter: object,
    value: object,
    message: object,
) -> None:
    """
    Test calculate rejects invalid values.
    """
    sizer = FixedRiskSizer(GBPUSD)
    arguments = {
        "entry": ENTRY,
        "stop_loss": STOP_LOSS,
        "equity": EQUITY,
        "risk": RISK,
        parameter: value,
    }

    with pytest.raises(ValueError, match=message):
        sizer.calculate(**arguments)


@pytest.mark.parametrize(
    "parameter",
    [
        "risk",
        "commission_rate",
        "exchange_rate",
        "hard_limit",
        "unit_batch_size",
    ],
)
@pytest.mark.parametrize("value", ["0.001", 1, 0.001, True])
def test_calculate_rejects_non_decimal_numeric_parameters(parameter: object, value: object) -> None:
    """
    Test calculate rejects non decimal numeric parameters.
    """
    sizer = FixedRiskSizer(GBPUSD)
    arguments = {
        "entry": ENTRY,
        "stop_loss": STOP_LOSS,
        "equity": EQUITY,
        "risk": RISK,
        parameter: value,
    }

    with pytest.raises(TypeError, match=r"expected decimal\.Decimal"):
        sizer.calculate(**arguments)


@pytest.mark.parametrize(
    "parameter",
    ["risk", "commission_rate", "exchange_rate", "unit_batch_size"],
)
def test_calculate_rejects_none_for_required_decimal_parameters(parameter: object) -> None:
    """
    Test calculate rejects none for required decimal parameters.
    """
    sizer = FixedRiskSizer(GBPUSD)
    arguments = {
        "entry": ENTRY,
        "stop_loss": STOP_LOSS,
        "equity": EQUITY,
        "risk": RISK,
        parameter: None,
    }

    with pytest.raises(TypeError, match=r"expected decimal\.Decimal"):
        sizer.calculate(**arguments)


@pytest.mark.parametrize(
    ("risk", "commission_rate", "exchange_rate"),
    [
        (Decimal("1e23"), Decimal(0), Decimal(1)),
        (RISK, Decimal("1e26"), Decimal(1)),
        (RISK, Decimal(0), Decimal("1e-28")),
    ],
)
def test_calculate_returns_value_error_on_decimal_overflow(
    risk: object,
    commission_rate: object,
    exchange_rate: object,
) -> None:
    """
    Test calculate returns value error on decimal overflow.
    """
    sizer = FixedRiskSizer(GBPUSD)

    with pytest.raises(
        ValueError,
        match="arithmetic overflow calculating fixed-risk position size",
    ):
        sizer.calculate(
            entry=ENTRY,
            stop_loss=STOP_LOSS,
            equity=EQUITY,
            risk=risk,
            commission_rate=commission_rate,
            exchange_rate=exchange_rate,
        )
